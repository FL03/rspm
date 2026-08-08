use super::*;
use crate::ProtocolVersion;
use alloc::sync::Arc;
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use polymarket::clob::types::OrderStatusType;
use polymarket::types::Decimal;
use std::sync::atomic::AtomicUsize;

struct TransportPollProbe {
    first_poll: Option<tokio::sync::oneshot::Sender<()>>,
    release: tokio::sync::oneshot::Receiver<()>,
}

struct BlockingFirstPoll {
    started: Option<std::sync::mpsc::SyncSender<()>>,
    release: std::sync::mpsc::Receiver<()>,
}

struct PanickingFirstPoll {
    polls: Arc<AtomicUsize>,
}

impl Future for BlockingFirstPoll {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(started) = self.started.take() {
            started.send(()).expect("first-poll observer");
        }
        self.release
            .recv_timeout(core::time::Duration::from_secs(1))
            .expect("release blocked first poll");
        Poll::Ready(())
    }
}

impl Future for PanickingFirstPoll {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Self::Output> {
        self.polls.fetch_add(1, Ordering::AcqRel);
        panic!("hostile first transport poll");
    }
}

impl Future for TransportPollProbe {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(first_poll) = self.first_poll.take() {
            let _sent = first_poll.send(());
        }
        Pin::new(&mut self.release).poll(context).map(|_| ())
    }
}

fn post_response(success: bool, order_id: &str) -> PostOrderResponse {
    PostOrderResponse::builder()
        .making_amount(Decimal::ZERO)
        .taking_amount(Decimal::ZERO)
        .order_id(order_id)
        .status(OrderStatusType::Live)
        .success(success)
        .build()
}

#[test]
fn only_canonical_venue_identifiers_can_establish_acceptance() {
    assert_eq!(
        classify_post_order_response(post_response(true, "order_123-ABC")),
        Ok(PostOrderOutcome::Accepted("order_123-ABC".to_owned()))
    );
    assert_eq!(
        classify_post_order_response(post_response(false, "")),
        Ok(PostOrderOutcome::NotAccepted)
    );
    for hostile in ["", "订单", "order/id", "order:id", " order", "order\n"] {
        assert_eq!(
            classify_post_order_response(post_response(true, hostile)),
            Err(()),
            "success with hostile venue ID must be indeterminate: {hostile:?}"
        );
    }
    assert_eq!(
        classify_post_order_response(post_response(false, "order_123")),
        Err(()),
        "rejection carrying an order ID is contradictory"
    );
}

#[test]
fn controller_identity_exhaustion_is_sticky_and_never_wraps() {
    let counter = AtomicU64::new(u64::MAX - 1);
    assert_eq!(
        reserve_controller_identity(&counter).expect("last reservable controller identity"),
        u64::MAX - 1
    );
    assert_eq!(counter.load(Ordering::Relaxed), u64::MAX);
    assert!(matches!(
        reserve_controller_identity(&counter),
        Err(crate::Error::SubmissionControllerIdentityExhausted)
    ));
    assert!(matches!(
        reserve_controller_identity(&counter),
        Err(crate::Error::SubmissionControllerIdentityExhausted)
    ));
    assert_eq!(counter.load(Ordering::Relaxed), u64::MAX);
}

#[tokio::test]
async fn revocation_epoch_exhaustion_poison_is_sticky_and_fail_closed() {
    let (_binding, controller) = controller_pair().expect("controller identity");
    controller
        .state
        .revocation_epoch
        .store(u64::MAX, Ordering::Release);
    controller.request_revocation();
    assert!(controller.is_poisoned());
    assert_eq!(controller.revocation_epoch(), u64::MAX);
    controller.request_revocation();
    assert!(controller.is_poisoned());
    assert_eq!(controller.revocation_epoch(), u64::MAX);

    let authority = AuthenticatedProtocolAuthority::for_test(ProtocolVersion::V2, 1);
    let exclusive = controller.acquire_exclusive().await;
    assert!(!exclusive.activate(u64::MAX, authority));
    drop(exclusive);
    assert!(controller.authorize_if(authority, || true).await.is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn revocation_cannot_publish_between_validation_and_first_transport_poll() {
    let (binding, controller) = controller_pair().expect("controller identity");
    let authority = AuthenticatedProtocolAuthority::for_test(ProtocolVersion::V2, 2);
    let epoch = controller.revocation_epoch();
    let exclusive = controller.acquire_exclusive().await;
    assert!(exclusive.activate(epoch, authority));
    drop(exclusive);
    let attempt = controller
        .authorize_if(authority, || true)
        .await
        .expect("active exact authority");

    let (started_tx, started_rx) = std::sync::mpsc::sync_channel(1);
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
    let in_flight = tokio::spawn(async move {
        poll_transport_after_begin(
            &attempt,
            binding,
            authority,
            BlockingFirstPoll {
                started: Some(started_tx),
                release: release_rx,
            },
        )
        .await
    });
    started_rx
        .recv_timeout(core::time::Duration::from_secs(1))
        .expect("first poll started");

    let revoker = controller.clone();
    let (revoked_tx, revoked_rx) = std::sync::mpsc::sync_channel(1);
    let revoker_thread = std::thread::spawn(move || {
        revoker.request_revocation();
        revoked_tx.send(()).expect("revocation observer");
    });
    assert!(
        revoked_rx
            .recv_timeout(core::time::Duration::from_millis(50))
            .is_err(),
        "revocation must wait while the validated first poll holds authority"
    );
    release_tx.send(()).expect("release first poll");
    assert_eq!(in_flight.await.expect("transport task"), Ok(()));
    revoked_rx
        .recv_timeout(core::time::Duration::from_secs(1))
        .expect("revocation published after first poll");
    revoker_thread.join().expect("revoker thread joined");
}

#[tokio::test]
async fn revocation_invalidates_minted_capability_before_exclusive_drain() {
    let (binding, controller) = controller_pair().expect("controller identity");
    let authority = AuthenticatedProtocolAuthority::for_test(ProtocolVersion::V2, 1);
    let epoch = controller.revocation_epoch();
    let exclusive = controller.acquire_exclusive().await;
    assert!(exclusive.activate(epoch, authority));
    drop(exclusive);

    let capability = controller
        .authorize_if(authority, || true)
        .await
        .expect("active exact authority");
    assert!(capability.revalidate(binding, authority));
    controller.request_revocation();
    assert!(!capability.revalidate(binding, authority));
}

#[tokio::test]
async fn capability_is_bound_to_controller_and_full_protocol_authority() {
    let (binding, controller) = controller_pair().expect("controller identity");
    let (foreign_binding, _) = controller_pair().expect("foreign controller identity");
    let authority = AuthenticatedProtocolAuthority::for_test(ProtocolVersion::V2, 7);
    let different = AuthenticatedProtocolAuthority::for_test(ProtocolVersion::V2, 8);
    let epoch = controller.revocation_epoch();
    let exclusive = controller.acquire_exclusive().await;
    assert!(exclusive.activate(epoch, authority));
    drop(exclusive);
    let capability = controller
        .authorize_if(authority, || true)
        .await
        .expect("active exact authority");

    assert!(capability.revalidate(binding, authority));
    assert!(!capability.revalidate(foreign_binding, authority));
    assert!(!capability.revalidate(binding, different));
}

#[tokio::test]
async fn revoker_publishes_before_waiting_and_drains_a_started_post() {
    let (binding, controller) = controller_pair().expect("controller identity");
    let authority = AuthenticatedProtocolAuthority::for_test(ProtocolVersion::V2, 9);
    let epoch = controller.revocation_epoch();
    let activation = controller.acquire_exclusive().await;
    assert!(activation.activate(epoch, authority));
    drop(activation);
    let attempt = controller
        .authorize_if(authority, || true)
        .await
        .expect("active attempt");
    assert!(attempt.revalidate(binding, authority));

    let closer = controller.clone();
    let (published_tx, published_rx) = tokio::sync::oneshot::channel();
    let drain = tokio::spawn(async move {
        closer.request_revocation();
        let _ = published_tx.send(());
        closer.acquire_exclusive().await
    });
    published_rx.await.expect("revocation published");
    assert!(!attempt.revalidate(binding, authority));
    assert!(
        !drain.is_finished(),
        "read fence must retain the in-flight POST"
    );
    drop(attempt);
    let _exclusive = drain.await.expect("revocation drain joined");
}

#[tokio::test]
async fn cancelled_drain_waiter_cannot_restore_revoked_authority() {
    let (binding, controller) = controller_pair().expect("controller identity");
    let authority = AuthenticatedProtocolAuthority::for_test(ProtocolVersion::V2, 11);
    let epoch = controller.revocation_epoch();
    let activation = controller.acquire_exclusive().await;
    assert!(activation.activate(epoch, authority));
    drop(activation);
    let attempt = controller
        .authorize_if(authority, || true)
        .await
        .expect("active attempt");

    controller.request_revocation();
    let waiter_controller = controller.clone();
    let waiter = tokio::spawn(async move { waiter_controller.acquire_exclusive().await });
    tokio::task::yield_now().await;
    waiter.abort();
    assert!(waiter.await.expect_err("waiter canceled").is_cancelled());
    assert!(!attempt.revalidate(binding, authority));
    drop(attempt);
    assert!(
        controller.authorize_if(authority, || true).await.is_none(),
        "canceling a queued drain cannot reopen the revoked generation"
    );
}

#[tokio::test]
async fn revoke_wins_before_begin_and_transport_receives_zero_polls() {
    let (binding, controller) = controller_pair().expect("controller identity");
    let authority = AuthenticatedProtocolAuthority::for_test(ProtocolVersion::V2, 12);
    let epoch = controller.revocation_epoch();
    let activation = controller.acquire_exclusive().await;
    assert!(activation.activate(epoch, authority));
    drop(activation);
    let attempt = controller
        .authorize_if(authority, || true)
        .await
        .expect("active attempt");
    controller.request_revocation();

    let (first_poll_tx, first_poll_rx) = tokio::sync::oneshot::channel();
    let (_release_tx, release_rx) = tokio::sync::oneshot::channel();
    let result = poll_transport_after_begin(
        &attempt,
        binding,
        authority,
        TransportPollProbe {
            first_poll: Some(first_poll_tx),
            release: release_rx,
        },
    )
    .await;

    assert_eq!(result, Err(()));
    assert!(
        first_poll_rx.await.is_err(),
        "revoke-first transport must be dropped without one poll"
    );
}

#[tokio::test]
async fn begin_wins_at_first_transport_poll_and_revoker_drains_it() {
    let (binding, controller) = controller_pair().expect("controller identity");
    let authority = AuthenticatedProtocolAuthority::for_test(ProtocolVersion::V2, 13);
    let epoch = controller.revocation_epoch();
    let activation = controller.acquire_exclusive().await;
    assert!(activation.activate(epoch, authority));
    drop(activation);
    let attempt = controller
        .authorize_if(authority, || true)
        .await
        .expect("active attempt");
    let (first_poll_tx, first_poll_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let in_flight = tokio::spawn(async move {
        poll_transport_after_begin(
            &attempt,
            binding,
            authority,
            TransportPollProbe {
                first_poll: Some(first_poll_tx),
                release: release_rx,
            },
        )
        .await
    });

    first_poll_rx.await.expect("transport received first poll");
    controller.request_revocation();
    let closer = controller.clone();
    let drain = tokio::spawn(async move { closer.acquire_exclusive().await });
    tokio::task::yield_now().await;
    assert!(
        !drain.is_finished(),
        "begin-first attempt must retain the read fence until transport finishes"
    );

    release_tx.send(()).expect("release transport");
    assert_eq!(in_flight.await.expect("transport task joined"), Ok(()));
    let _exclusive = drain.await.expect("revocation drain joined");
}

#[tokio::test]
async fn first_transport_poll_panic_sticky_poisons_controller() {
    let (binding, controller) = controller_pair().expect("controller identity");
    let authority = AuthenticatedProtocolAuthority::for_test(ProtocolVersion::V2, 14);
    let epoch = controller.revocation_epoch();
    let activation = controller.acquire_exclusive().await;
    assert!(activation.activate(epoch, authority));
    drop(activation);
    let panicking_attempt = controller
        .authorize_if(authority, || true)
        .await
        .expect("panicking attempt");
    let later_attempt = controller
        .authorize_if(authority, || true)
        .await
        .expect("later attempt minted before poison");
    let polls = Arc::new(AtomicUsize::new(0));
    let panic_polls = Arc::clone(&polls);

    let panicked = tokio::spawn(async move {
        poll_transport_after_begin(
            &panicking_attempt,
            binding,
            authority,
            PanickingFirstPoll { polls: panic_polls },
        )
        .await
    })
    .await
    .expect_err("first transport poll must panic");
    assert!(panicked.is_panic());
    assert_eq!(polls.load(Ordering::Acquire), 1);
    assert!(controller.is_poisoned());
    assert!(controller.authorize_if(authority, || true).await.is_none());

    let (first_poll_tx, first_poll_rx) = tokio::sync::oneshot::channel();
    let (_release_tx, release_rx) = tokio::sync::oneshot::channel();
    assert_eq!(
        poll_transport_after_begin(
            &later_attempt,
            binding,
            authority,
            TransportPollProbe {
                first_poll: Some(first_poll_tx),
                release: release_rx,
            },
        )
        .await,
        Err(())
    );
    assert!(
        first_poll_rx.await.is_err(),
        "poisoned transport received a poll"
    );
}

#[tokio::test]
async fn authorization_predicate_panic_cannot_poison_mutex_open() {
    let (binding, controller) = controller_pair().expect("controller identity");
    let authority = AuthenticatedProtocolAuthority::for_test(ProtocolVersion::V2, 15);
    let epoch = controller.revocation_epoch();
    let activation = controller.acquire_exclusive().await;
    assert!(activation.activate(epoch, authority));
    drop(activation);
    let prior_attempt = controller
        .authorize_if(authority, || true)
        .await
        .expect("prior attempt");

    let predicate_controller = controller.clone();
    let panicked = tokio::spawn(async move {
        predicate_controller
            .authorize_if(authority, || panic!("hostile authorization predicate"))
            .await
    })
    .await
    .expect_err("predicate must panic");
    assert!(panicked.is_panic());
    assert!(controller.is_poisoned());
    assert!(controller.authorize_if(authority, || true).await.is_none());

    let (first_poll_tx, first_poll_rx) = tokio::sync::oneshot::channel();
    let (_release_tx, release_rx) = tokio::sync::oneshot::channel();
    assert_eq!(
        poll_transport_after_begin(
            &prior_attempt,
            binding,
            authority,
            TransportPollProbe {
                first_poll: Some(first_poll_tx),
                release: release_rx,
            },
        )
        .await,
        Err(())
    );
    assert!(
        first_poll_rx.await.is_err(),
        "predicate poison allowed a transport poll"
    );
}

#[tokio::test]
async fn recovered_mutex_poison_is_sticky_and_clears_active_authority() {
    let (binding, controller) = controller_pair().expect("controller identity");
    let authority = AuthenticatedProtocolAuthority::for_test(ProtocolVersion::V2, 16);
    let epoch = controller.revocation_epoch();
    let activation = controller.acquire_exclusive().await;
    assert!(activation.activate(epoch, authority));
    drop(activation);
    let attempt = controller
        .authorize_if(authority, || true)
        .await
        .expect("attempt before mutex poison");

    let state = Arc::clone(&controller.state);
    let poisoned = std::thread::spawn(move || {
        let _active = state.active.lock().expect("unpoisoned active mutex");
        panic!("hostile authority holder");
    })
    .join();
    assert!(poisoned.is_err());
    assert!(!attempt.revalidate(binding, authority));
    assert!(controller.is_poisoned());
    assert!(controller.authorize_if(authority, || true).await.is_none());

    let (first_poll_tx, first_poll_rx) = tokio::sync::oneshot::channel();
    let (_release_tx, release_rx) = tokio::sync::oneshot::channel();
    assert_eq!(
        poll_transport_after_begin(
            &attempt,
            binding,
            authority,
            TransportPollProbe {
                first_poll: Some(first_poll_tx),
                release: release_rx,
            },
        )
        .await,
        Err(())
    );
    assert!(
        first_poll_rx.await.is_err(),
        "recovered poison allowed a transport poll"
    );
}
