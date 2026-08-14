//! Opaque engine-owned submission mint bound to one authenticated CLOB client.
use crate::auth::AuthenticatedProtocolAuthority;
use alloc::sync::{Arc, Weak};
use core::{
    future::Future,
    pin::Pin,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    task::Context,
    task::Poll,
};
use polymarket::clob::types::response::PostOrderResponse;
use std::sync::{Mutex, MutexGuard};
use tokio::sync::{OwnedRwLockReadGuard, OwnedRwLockWriteGuard, RwLock};

static NEXT_CONTROLLER_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Eq, PartialEq)]
pub(super) enum PostOrderOutcome {
    Accepted(String),
    NotAccepted,
}

pub(super) fn classify_post_order_response(
    response: PostOrderResponse,
) -> Result<PostOrderOutcome, ()> {
    if response.success {
        let order_id = response.order_id;
        if crate::auth::venue_identifier_is_valid(&order_id) {
            Ok(PostOrderOutcome::Accepted(order_id))
        } else {
            Err(())
        }
    } else if response.order_id.is_empty() {
        Ok(PostOrderOutcome::NotAccepted)
    } else {
        Err(())
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct ActiveSubmissionAuthority {
    revocation_epoch: u64,
    protocol_authority: AuthenticatedProtocolAuthority,
}

struct SubmissionControllerState {
    revocation_epoch: AtomicU64,
    poisoned: AtomicBool,
    active: Mutex<Option<ActiveSubmissionAuthority>>,
    fence: Arc<RwLock<()>>,
}

impl SubmissionControllerState {
    fn lock_active(&self) -> Result<MutexGuard<'_, Option<ActiveSubmissionAuthority>>, ()> {
        match self.active.lock() {
            Ok(mut active) if self.poisoned.load(Ordering::Acquire) => {
                *active = None;
                Err(())
            }
            Ok(active) => Ok(active),
            Err(poison) => {
                let mut active = poison.into_inner();
                *active = None;
                self.poisoned.store(true, Ordering::Release);
                Err(())
            }
        }
    }

    fn poison_and_clear_active(&self) {
        self.poisoned.store(true, Ordering::Release);
        let mut active = match self.active.lock() {
            Ok(active) => active,
            Err(poison) => poison.into_inner(),
        };
        *active = None;
    }
}

/// Client-private identity used to reject capabilities minted for another client.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct SubmissionControllerBinding(u64);

/// Engine-owned controller that is paired once with an exact [`super::ClobClient`].
///
/// The CLOB client stores only the non-minting binding. Engine admission owns
/// this controller and can mint an attempt capability only while its private
/// recovery, budget, credential, and protocol checks remain exact.
#[derive(Clone)]
pub struct SubmissionController {
    binding: SubmissionControllerBinding,
    state: Arc<SubmissionControllerState>,
}

/// Exclusive open/close lease. While held, no submission capability exists.
#[derive(Debug)]
pub struct SubmissionActivationLease {
    controller: SubmissionController,
    _fence: OwnedRwLockWriteGuard<()>,
}

/// One opaque build-sign-POST capability.
///
/// Fields and minting are private. The capability retains the shared read
/// fence and is bound to one controller ID, revocation epoch, credential
/// identity, and protocol generation.
#[derive(Debug)]
pub struct SubmissionAttemptAuthority {
    binding: SubmissionControllerBinding,
    revocation_epoch: u64,
    protocol_authority: AuthenticatedProtocolAuthority,
    state: Weak<SubmissionControllerState>,
    _fence: OwnedRwLockReadGuard<()>,
}

fn reserve_controller_identity(counter: &AtomicU64) -> crate::Result<u64> {
    counter
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |identity| {
            (identity != 0).then(|| identity.checked_add(1)).flatten()
        })
        .map_err(|_| crate::Error::SubmissionControllerIdentityExhausted)
}

pub(super) fn controller_pair() -> crate::Result<(SubmissionControllerBinding, SubmissionController)>
{
    let identity = reserve_controller_identity(&NEXT_CONTROLLER_ID)?;
    let binding = SubmissionControllerBinding(identity);
    let controller = SubmissionController {
        binding,
        state: Arc::new(SubmissionControllerState {
            revocation_epoch: AtomicU64::new(0),
            poisoned: AtomicBool::new(false),
            active: Mutex::new(None),
            fence: Arc::new(RwLock::new(())),
        }),
    };
    Ok((binding, controller))
}

impl core::fmt::Debug for SubmissionController {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("SubmissionController")
            .field("revocation_epoch", &self.revocation_epoch())
            .field("poisoned", &self.is_poisoned())
            .finish_non_exhaustive()
    }
}

impl SubmissionController {
    /// Publish revocation synchronously before any closer waits for in-flight work.
    pub fn request_revocation(&self) {
        let mut active = match self.state.active.lock() {
            Ok(active) => active,
            Err(poison) => {
                self.state.poisoned.store(true, Ordering::Release);
                poison.into_inner()
            }
        };
        if self
            .state
            .revocation_epoch
            .try_update(Ordering::AcqRel, Ordering::Acquire, |epoch| {
                epoch.checked_add(1)
            })
            .is_err()
        {
            self.state.poisoned.store(true, Ordering::Release);
        }
        *active = None;
    }

    #[must_use]
    pub fn revocation_epoch(&self) -> u64 {
        self.state.revocation_epoch.load(Ordering::Acquire)
    }

    #[must_use]
    fn is_poisoned(&self) -> bool {
        self.state.poisoned.load(Ordering::Acquire)
    }

    /// Drain every capability minted before this exclusive boundary.
    pub async fn acquire_exclusive(&self) -> SubmissionActivationLease {
        let fence = Arc::clone(&self.state.fence).write_owned().await;
        SubmissionActivationLease {
            controller: self.clone(),
            _fence: fence,
        }
    }

    /// Mint one capability after acquiring the shared fence and rechecking
    /// engine-owned private recovery and budget authority.
    pub async fn authorize_if<F>(
        &self,
        expected_protocol_authority: AuthenticatedProtocolAuthority,
        authorize: F,
    ) -> Option<SubmissionAttemptAuthority>
    where
        F: FnOnce() -> bool,
    {
        let fence = Arc::clone(&self.state.fence).read_owned().await;
        let authorized = std::panic::catch_unwind(std::panic::AssertUnwindSafe(authorize));
        let authorized = match authorized {
            Ok(authorized) => authorized,
            Err(payload) => {
                self.state.poison_and_clear_active();
                std::panic::resume_unwind(payload);
            }
        };
        let Ok(active) = self.state.lock_active() else {
            return None;
        };
        let revocation_epoch = self.revocation_epoch();
        let expected = ActiveSubmissionAuthority {
            revocation_epoch,
            protocol_authority: expected_protocol_authority,
        };
        if !authorized || self.is_poisoned() || *active != Some(expected) {
            drop(active);
            self.request_revocation();
            return None;
        }
        drop(active);
        Some(SubmissionAttemptAuthority {
            binding: self.binding,
            revocation_epoch,
            protocol_authority: expected_protocol_authority,
            state: Arc::downgrade(&self.state),
            _fence: fence,
        })
    }
}

impl SubmissionActivationLease {
    /// Activate the exact authority only if no revocation occurred since the
    /// recovery barrier captured `expected_revocation_epoch`.
    pub fn activate(
        &self,
        expected_revocation_epoch: u64,
        protocol_authority: AuthenticatedProtocolAuthority,
    ) -> bool {
        if self.controller.is_poisoned()
            || self.controller.revocation_epoch() != expected_revocation_epoch
        {
            return false;
        }
        let Ok(mut active) = self.controller.state.lock_active() else {
            return false;
        };
        if self.controller.is_poisoned()
            || active.is_some()
            || self.controller.revocation_epoch() != expected_revocation_epoch
        {
            return false;
        }
        *active = Some(ActiveSubmissionAuthority {
            revocation_epoch: expected_revocation_epoch,
            protocol_authority,
        });
        true
    }
}

impl SubmissionAttemptAuthority {
    pub(super) fn revalidate(
        &self,
        expected_binding: SubmissionControllerBinding,
        expected_protocol_authority: AuthenticatedProtocolAuthority,
    ) -> bool {
        if self.binding != expected_binding
            || self.protocol_authority != expected_protocol_authority
        {
            return false;
        }
        let Some(state) = self.state.upgrade() else {
            return false;
        };
        let Ok(active) = state.lock_active() else {
            return false;
        };
        !state.poisoned.load(Ordering::Acquire)
            && state.revocation_epoch.load(Ordering::Acquire) == self.revocation_epoch
            && *active
                == Some(ActiveSubmissionAuthority {
                    revocation_epoch: self.revocation_epoch,
                    protocol_authority: self.protocol_authority,
                })
    }

    /// Validate authority and perform the first credential-bearing transport
    /// poll while holding the same mutex used by synchronous revocation.
    fn poll_first_transport<F>(
        &self,
        expected_binding: SubmissionControllerBinding,
        expected_protocol_authority: AuthenticatedProtocolAuthority,
        transport: Pin<&mut F>,
        context: &mut Context<'_>,
    ) -> Result<Poll<F::Output>, ()>
    where
        F: Future,
    {
        if self.binding != expected_binding
            || self.protocol_authority != expected_protocol_authority
        {
            return Err(());
        }
        let Some(state) = self.state.upgrade() else {
            return Err(());
        };
        let Ok(mut active) = state.lock_active() else {
            return Err(());
        };
        let expected = ActiveSubmissionAuthority {
            revocation_epoch: self.revocation_epoch,
            protocol_authority: self.protocol_authority,
        };
        if state.poisoned.load(Ordering::Acquire)
            || state.revocation_epoch.load(Ordering::Acquire) != self.revocation_epoch
            || *active != Some(expected)
        {
            return Err(());
        }
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| transport.poll(context)));
        let result = match result {
            Ok(result) => result,
            Err(payload) => {
                *active = None;
                state.poisoned.store(true, Ordering::Release);
                drop(active);
                std::panic::resume_unwind(payload);
            }
        };
        drop(active);
        Ok(result)
    }
}

/// Poll a credential-bearing transport only after atomically linearizing its
/// first poll against submission revocation.
///
/// Constructing an `async fn` future does not poll it. Holding the authority
/// mutex through the first transport poll gives the boundary one exact order:
///
/// - revocation publishes first: validation fails and the transport receives
///   zero polls;
/// - first poll wins: revocation cannot publish until that poll returns, and
///   the attempt's read fence then retains all later in-flight polls.
pub(super) async fn poll_transport_after_begin<F>(
    attempt: &SubmissionAttemptAuthority,
    expected_binding: SubmissionControllerBinding,
    expected_protocol_authority: AuthenticatedProtocolAuthority,
    transport: F,
) -> Result<F::Output, ()>
where
    F: Future,
{
    let mut transport = core::pin::pin!(transport);
    let mut began = false;
    core::future::poll_fn(|context| {
        if !began {
            match attempt.poll_first_transport(
                expected_binding,
                expected_protocol_authority,
                transport.as_mut(),
                context,
            ) {
                Ok(Poll::Ready(output)) => return Poll::Ready(Ok(output)),
                Ok(Poll::Pending) => began = true,
                Err(()) => return Poll::Ready(Err(())),
            }
            return Poll::Pending;
        }
        transport.as_mut().poll(context).map(Ok)
    })
    .await
}

#[cfg(test)]
mod tests {
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
}
