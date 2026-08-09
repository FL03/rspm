use super::*;
use tokio::{
    net::TcpListener,
    sync::{Semaphore, oneshot},
    time::timeout,
};

const TEST_TIMEOUT: Duration = Duration::from_secs(2);

#[test]
fn retirement_capacity_is_reserved_before_spawn_and_refuses_saturation() {
    let slots = Arc::new(Semaphore::new(1));
    let permit = try_reserve_retirement_slot(Arc::clone(&slots)).expect("first retirement slot");
    for _ in 0..2 {
        assert_eq!(
            try_reserve_retirement_slot(Arc::clone(&slots)).expect_err("capacity must stay full"),
            AuthenticatedUserWsError::RetirementCapacityExhausted
        );
    }
    drop(permit);
    let recycled = try_reserve_retirement_slot(slots).expect("terminal release recycles capacity");
    drop(recycled);
}

#[tokio::test]
async fn failed_retirement_transfer_is_retained_aborted_and_joined_without_panic() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct ExitProbe(Arc<AtomicUsize>);

    impl Drop for ExitProbe {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::AcqRel);
        }
    }

    let state = StateCell::new();
    let (retirement_state_tx, mut retirement_state_rx) = watch::channel(RetirementState::Active);
    let exits = Arc::new(AtomicUsize::new(0));
    let probe = ExitProbe(Arc::clone(&exits));
    let task = tokio::spawn(async move {
        let _probe = probe;
        core::future::pending::<()>().await;
    });
    tokio::task::yield_now().await;
    let slots = Arc::new(Semaphore::new(1));
    let slot = try_reserve_retirement_slot(Arc::clone(&slots)).expect("test retirement slot");

    transfer_to_disconnected_custodian_for_test(task, state.clone(), retirement_state_tx, slot);

    assert_eq!(
        *retirement_state_rx.borrow_and_update(),
        RetirementState::RetainedByProcessCustodian
    );
    assert!(drain_failed_retirements_for_test().await);
    assert_eq!(
        *retirement_state_rx.borrow_and_update(),
        RetirementState::TerminalCancelled
    );
    assert_eq!(exits.load(Ordering::Acquire), 1);
    assert_eq!(slots.available_permits(), 1);
    assert!(state.snapshot().evidence_gap);
    assert!(!state.snapshot().is_ready());
}

async fn assert_cancelled_session_wait_transfers_to_process_custody(use_shutdown: bool) {
    let slots = Arc::new(Semaphore::new(1));
    let slot = try_reserve_retirement_slot(Arc::clone(&slots)).expect("test retirement slot");
    let (release_tx, release_rx) = oneshot::channel::<()>();
    let task = tokio::spawn(async move {
        let _ = release_rx.await;
    });
    let ws = AuthenticatedUserWs::with_retirement_task_for_test(task, slot);
    let mut retirement = ws.retirement_receiver();
    let transfers_before = process_custodian_transfer_count();

    let mut wait: core::pin::Pin<Box<dyn core::future::Future<Output = ()> + '_>> = if use_shutdown
    {
        Box::pin(async {
            let _ = ws.shutdown().await;
        })
    } else {
        Box::pin(ws.drain())
    };
    assert!(matches!(
        futures::poll!(&mut wait),
        core::task::Poll::Pending
    ));
    drop(wait);
    assert!(
        process_custodian_transfer_count() > transfers_before,
        "cancelled waiter must transfer its linear guard"
    );
    assert_eq!(
        *retirement.borrow_and_update(),
        RetirementState::RetainedByProcessCustodian
    );
    assert_eq!(slots.available_permits(), 0, "permit remains in custody");

    release_tx.send(()).expect("release retained session task");
    assert_eq!(
        wait_for_terminal_retirement(&mut retirement).await,
        RetirementState::TerminalCancelled
    );
    assert!(drain_authenticated_user_ws_retirements().await);
    assert_eq!(
        slots.available_permits(),
        1,
        "terminal task releases permit once"
    );
}

#[tokio::test]
async fn cancelled_shutdown_transfers_join_handle_and_permit_to_process_custody() {
    assert_cancelled_session_wait_transfers_to_process_custody(true).await;
}

#[tokio::test]
async fn cancelled_drain_transfers_join_handle_and_permit_to_process_custody() {
    assert_cancelled_session_wait_transfers_to_process_custody(false).await;
}

fn one_connection_config() -> AuthenticatedUserWsConfig {
    AuthenticatedUserWsConfig {
        connect_timeout: TEST_TIMEOUT,
        heartbeat_interval: Duration::from_millis(100),
        heartbeat_timeout: Duration::from_secs(1),
        initial_reconnect_delay: Duration::from_millis(10),
        max_reconnect_delay: Duration::from_millis(10),
        max_reconnect_attempts: Some(1),
    }
}

async fn wait_for_state(
    states: &mut watch::Receiver<AuthenticatedUserWsState>,
    predicate: impl Fn(AuthenticatedUserWsState) -> bool,
) -> AuthenticatedUserWsState {
    timeout(TEST_TIMEOUT, async {
        loop {
            let state = *states.borrow_and_update();
            if predicate(state) {
                return state;
            }
            states
                .changed()
                .await
                .expect("session state sender remains live");
        }
    })
    .await
    .expect("session state transition timed out")
}

#[tokio::test]
async fn stalled_peer_close_future_is_bounded() {
    let closed = close_socket_within(
        core::future::pending::<Result<(), ()>>(),
        Duration::from_millis(10),
    )
    .await;
    assert!(!closed, "a stalled close flush must time out fail-closed");
}

#[tokio::test]
async fn stalled_connection_future_is_cancelled_inside_its_finite_custodian() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct ExitProbe(Arc<AtomicUsize>);

    impl Drop for ExitProbe {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::AcqRel);
        }
    }

    let state = StateCell::new();
    let generation = state.mark_connected();
    state.mark_subscription_written(generation);
    let exits = Arc::new(AtomicUsize::new(0));
    let probe = ExitProbe(Arc::clone(&exits));
    let stalled_connection = async move {
        let _probe = probe;
        core::future::pending::<()>().await;
    };
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let (retirement_state_tx, mut retirement_state_rx) = watch::channel(RetirementState::Active);
    let task_state = state.clone();
    let custodian = tokio::spawn(retirement_custodian(
        stalled_connection,
        task_state,
        shutdown_rx,
        retirement_state_tx,
        Duration::from_millis(10),
    ));
    shutdown_tx.send_replace(true);
    timeout(TEST_TIMEOUT, custodian)
        .await
        .expect("bounded retirement timed out")
        .expect("custodian joined");

    assert_eq!(
        exits.load(Ordering::Acquire),
        1,
        "connection future was destroyed before acknowledgement"
    );
    assert_eq!(
        *retirement_state_rx.borrow_and_update(),
        RetirementState::TerminalCancelled
    );
    let closed = state.snapshot();
    assert!(closed.evidence_gap);
    assert_eq!(
        closed.connection,
        AuthenticatedUserConnectionState::Disconnected
    );
    assert!(!closed.is_ready());
}

#[tokio::test]
async fn simultaneous_received_message_wins_before_shutdown_and_transfers_once() {
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
    shutdown_tx.send_replace(true);
    let (event_tx, mut event_rx) = mpsc::channel(1);
    let permit = event_tx.reserve().await.expect("reserve event capacity");

    match super::connection::receive_or_shutdown(core::future::ready(17_u64), &mut shutdown_rx)
        .await
    {
        super::connection::SocketPoll::Received {
            output: value,
            receipt: Some(receipt),
        } => {
            assert!(receipt.wall_time_ns() > 0);
            permit.send(value);
        }
        super::connection::SocketPoll::Received { receipt: None, .. } => {
            panic!("socket receipt clock failed")
        }
        super::connection::SocketPoll::Shutdown => panic!("ready receipt lost to shutdown"),
    }
    drop(event_tx);

    assert_eq!(event_rx.recv().await, Some(17));
    assert_eq!(event_rx.recv().await, None, "received value duplicated");
}

async fn silent_server() -> (
    String,
    oneshot::Receiver<()>,
    oneshot::Sender<()>,
    JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind local authenticated user server");
    let endpoint = format!(
        "ws://{}",
        listener.local_addr().expect("read local server address")
    );
    let (subscribed_tx, subscribed_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel();
    let task = tokio::spawn(async move {
        let _socket = accept_subscribed_socket(&listener).await;
        let _ = subscribed_tx.send(());
        let _ = release_rx.await;
    });
    (endpoint, subscribed_rx, release_tx, task)
}

#[tokio::test]
async fn poisoned_event_receiver_custody_fails_closed_without_taking_receiver() {
    let (endpoint, subscribed, release, server) = silent_server().await;
    let ws = AuthenticatedUserWs::connect_with_config(
        &endpoint,
        Credentials::default(),
        one_connection_config(),
    )
    .expect("connect local authenticated socket");
    subscribed.await.expect("server observed subscription");

    ws.poison_events_for_test();
    assert!(matches!(
        ws.take_events(),
        Err(AuthenticatedUserWsError::AuthorityPoisoned)
    ));
    let poisoned = ws.state();
    assert!(poisoned.authority_poisoned);
    assert!(poisoned.consumer_closed);
    assert!(poisoned.evidence_gap);
    assert!(!poisoned.is_ready());

    let _released = release.send(());
    ws.drain().await;
    server.await.expect("local server joined");
}

#[tokio::test]
async fn poisoned_task_custody_recovers_and_joins_handle_while_staying_fail_closed() {
    let (endpoint, subscribed, release, server) = silent_server().await;
    let ws = AuthenticatedUserWs::connect_with_config(
        &endpoint,
        Credentials::default(),
        one_connection_config(),
    )
    .expect("connect local authenticated socket");
    subscribed.await.expect("server observed subscription");

    ws.poison_task_for_test();
    let _released = release.send(());
    ws.drain().await;
    server.await.expect("local server joined");
    let poisoned = ws.state();
    assert!(poisoned.authority_poisoned);
    assert!(poisoned.evidence_gap);
    assert!(!poisoned.is_ready());
    assert!(ws.retirement_receiver().borrow().is_terminal());
}

#[tokio::test]
async fn poisoned_retirement_slot_is_recovered_only_for_terminal_release() {
    let (endpoint, subscribed, release, server) = silent_server().await;
    let ws = AuthenticatedUserWs::connect_with_config(
        &endpoint,
        Credentials::default(),
        one_connection_config(),
    )
    .expect("connect local authenticated socket");
    subscribed.await.expect("server observed subscription");

    ws.poison_retirement_slot_for_test();
    let _released = release.send(());
    ws.drain().await;
    server.await.expect("local server joined");
    let poisoned = ws.state();
    assert!(poisoned.authority_poisoned);
    assert!(poisoned.evidence_gap);
    assert!(!poisoned.is_ready());
    assert!(ws.retirement_receiver().borrow().is_terminal());
}

async fn proven_server() -> (
    String,
    oneshot::Receiver<()>,
    oneshot::Sender<()>,
    JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind proven authenticated user server");
    let endpoint = format!(
        "ws://{}",
        listener.local_addr().expect("read local server address")
    );
    let (proven_tx, proven_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel();
    let task = tokio::spawn(async move {
        let mut socket = accept_subscribed_socket(&listener).await;
        prove_server_liveness(&mut socket).await;
        let _ = proven_tx.send(());
        let _ = release_rx.await;
    });
    (endpoint, proven_rx, release_tx, task)
}

async fn wait_for_terminal_retirement(
    retirement: &mut watch::Receiver<RetirementState>,
) -> RetirementState {
    timeout(TEST_TIMEOUT, async {
        loop {
            let state = *retirement.borrow_and_update();
            if state.is_terminal() {
                return state;
            }
            retirement
                .changed()
                .await
                .expect("retirement custodian remains observable until terminal");
        }
    })
    .await
    .expect("retirement custodian did not reach terminal state")
}

#[tokio::test]
async fn last_session_owner_drop_retires_connection_without_detaching_it() {
    let (endpoint, proven, release, server) = proven_server().await;
    let ws = AuthenticatedUserWs::connect_with_config(
        &endpoint,
        Credentials::default(),
        one_connection_config(),
    )
    .expect("construct last-owner retirement session");
    let mut retirement = ws.retirement_receiver();
    timeout(TEST_TIMEOUT, proven)
        .await
        .expect("last-owner proof timeout")
        .expect("last-owner proof notification");

    let transfers_before = process_custodian_transfer_count();
    drop(ws);
    let terminal = wait_for_terminal_retirement(&mut retirement).await;
    assert!(matches!(
        terminal,
        RetirementState::TerminalGraceful | RetirementState::TerminalCancelled
    ));
    assert!(
        process_custodian_transfer_count() > transfers_before,
        "last owner must transfer its retained JoinHandle"
    );
    assert!(
        drain_authenticated_user_ws_retirements().await,
        "process custodian drains every transferred handle"
    );

    let _ = release.send(());
    server.await.expect("last-owner server task");
}

#[tokio::test]
async fn process_final_drain_returns_only_after_terminal_connection_custody() {
    let (endpoint, proven, release, server) = proven_server().await;
    let ws = AuthenticatedUserWs::connect_with_config(
        &endpoint,
        Credentials::default(),
        one_connection_config(),
    )
    .expect("construct process-final drain session");
    let mut retirement = ws.retirement_receiver();
    timeout(TEST_TIMEOUT, proven)
        .await
        .expect("process-final proof timeout")
        .expect("process-final proof notification");

    ws.drain().await;
    assert!(retirement.borrow_and_update().is_terminal());

    let _ = release.send(());
    server.await.expect("process-final drain server task");
}

async fn accept_subscribed_socket(
    listener: &TcpListener,
) -> tokio_tungstenite::WebSocketStream<tokio::net::TcpStream> {
    let (socket, _) = listener.accept().await.expect("accept local client");
    let mut socket = tokio_tungstenite::accept_async(socket)
        .await
        .expect("complete local websocket handshake");
    let message = socket
        .next()
        .await
        .expect("subscription frame")
        .expect("valid subscription frame");
    assert!(matches!(message, Message::Text(_)));
    socket
}

async fn read_proof_ping(socket: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>) {
    let ping = socket
        .next()
        .await
        .expect("immediate proof ping")
        .expect("valid proof ping");
    assert!(matches!(ping, Message::Text(ref text) if text.as_str() == "PING"));
}

async fn prove_server_liveness(
    socket: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
) {
    read_proof_ping(socket).await;
    socket
        .send(Message::Text("PONG".into()))
        .await
        .expect("send documented proof pong");
}

fn trade_json(maker_fee: Option<serde_json::Value>) -> serde_json::Value {
    let mut maker = serde_json::json!({
        "asset_id": "2",
        "matched_amount": "4",
        "order_id": "maker-order",
        "owner": "ffffffff-ffff-ffff-ffff-ffffffffffff",
        "price": "0.4",
        "side": "SELL"
    });
    if let Some(fee) = maker_fee {
        maker
            .as_object_mut()
            .expect("maker fixture object")
            .insert("fee_rate_bps".to_owned(), fee);
    }
    serde_json::json!({
        "asset_id": "1",
        "event_type": "trade",
        "fee_rate_bps": "1",
        "id": "trade-id",
        "maker_orders": [maker],
        "market": format!("0x{}", "1".repeat(64)),
        "matchtime": "1700000000",
        "owner": "ffffffff-ffff-ffff-ffff-ffffffffffff",
        "price": "0.6",
        "side": "BUY",
        "size": "4",
        "status": "CONFIRMED",
        "taker_order_id": "taker-order",
        "trader_side": "MAKER",
        "type": "TRADE"
    })
}

#[test]
fn queued_subscription_while_disconnected_is_closed() {
    let state = StateCell::new();
    assert_eq!(state.snapshot().recovery_token(), None);
    assert!(!state.snapshot().is_ready());
}

/// [REGRESSION][EVAL] Monotonic private authority identities fail permanently
/// instead of saturating into aliases at `u64::MAX`.
#[test]
fn every_private_authority_counter_exhaustion_is_typed_and_fail_closed() {
    let generation = StateCell::new();
    generation.update(|state| state.generation = u64::MAX);
    assert_eq!(
        generation.try_mark_connected(),
        Err(AuthenticatedUserCounterExhaustion::Generation)
    );
    assert_eq!(
        generation.snapshot().counter_exhaustion,
        Some(AuthenticatedUserCounterExhaustion::Generation)
    );
    assert!(!generation.snapshot().is_ready());

    let gap = StateCell::new();
    gap.update(|state| state.gap_version = u64::MAX);
    gap.mark_schema_gap();
    assert_eq!(
        gap.snapshot().counter_exhaustion,
        Some(AuthenticatedUserCounterExhaustion::GapVersion)
    );

    let liveness = StateCell::new();
    let liveness_generation = liveness.mark_connected();
    liveness.mark_subscription_written(liveness_generation);
    liveness.update(|state| state.liveness_version = u64::MAX);
    liveness.mark_server_liveness_proven(liveness_generation);
    assert_eq!(
        liveness.snapshot().counter_exhaustion,
        Some(AuthenticatedUserCounterExhaustion::Liveness)
    );

    let transport = StateCell::new();
    transport.update(|state| state.transport_sequence = u64::MAX);
    assert_eq!(transport.reserve_transport_sequences(1), None);
    assert_eq!(
        transport.snapshot().counter_exhaustion,
        Some(AuthenticatedUserCounterExhaustion::TransportSequence)
    );

    let raw = StateCell::new();
    raw.update(|state| state.raw_frame_sequence = u64::MAX);
    assert_eq!(raw.reserve_raw_frame_sequence(), None);
    assert_eq!(
        raw.snapshot().counter_exhaustion,
        Some(AuthenticatedUserCounterExhaustion::RawFrameSequence)
    );

    assert_eq!(
        StateCell::next_durable_sequence(u64::MAX),
        Err(AuthenticatedUserCounterExhaustion::DurableSequence)
    );
}

#[test]
fn subscription_write_awaits_same_generation_server_proof() {
    let state = StateCell::new();
    let generation = state.mark_connected();
    state.mark_subscription_written(generation);
    let awaiting = state.snapshot();
    assert_eq!(
        awaiting.authentication,
        AuthenticatedUserAuthenticationState::CredentialsSubmitted
    );
    assert_eq!(
        awaiting.subscription,
        AuthenticatedUserSubscriptionState::AwaitingServerProof
    );
    // A subscription write alone never mints a recovery token: the socket
    // still "awaits" the same-generation server proof this test is named
    // for. `recovery_token()` requires `ServerResponsive`, not merely
    // `AwaitingServerProof` — `wait_for_state(.., |s|
    // s.recovery_token().is_some())` across this module's live-socket tests
    // depends on that exact gate to mean "responsive," not just "subscribed."
    assert!(awaiting.recovery_token().is_none());
    assert!(!awaiting.is_ready());
    assert!(!state.complete_catch_up(
        AuthenticatedUserRecoveryToken::for_test(
            generation,
            awaiting.gap_version,
            awaiting.credential_identity,
        ),
        0,
    ));
}

/// [REGRESSION] The REST-proof window closes `recovery_token` but must NOT
/// erase session identity.
///
/// `mark_authenticated_rest_proven` requires `ServerResponsive` on entry and
/// sets `AwaitingServerProof` on success, so `recovery_token()` — which
/// requires `ServerResponsive` — is `None` for the whole wait. A consumer
/// guarding "did my session move?" during that wait therefore cannot use it:
/// the guard fires unconditionally on its first read, before any server proof
/// can arrive.
///
/// That is exactly what wedged `complete_private_recovery_and_open`, the sole
/// caller of `finish_exposure_restore_if_version(ExposureTrack::Live, ..)`, so
/// Live admission could never open through authenticated recovery. This
/// asserts the invariant at the layer the defect actually lives in.
///
/// Pairs with `subscription_write_awaits_same_generation_server_proof` above,
/// which pins the other half: `recovery_token()` must STAY `None` here.
#[test]
fn rest_proof_closes_the_recovery_window_but_preserves_session_identity() {
    let state = StateCell::new();
    let generation = state.mark_connected();
    state.mark_subscription_written(generation);
    state.mark_server_liveness_proven(generation);

    let responsive = state.snapshot();
    let token = responsive
        .recovery_token()
        .expect("a responsive session is open for recovery");
    assert_eq!(
        responsive.recovery_identity(),
        Some(token),
        "identity and the open-window token agree while ServerResponsive"
    );

    let credential_authority =
        AuthenticatedCredentialAuthority::new(token.credential_identity(), 1)
            .expect("test credential generation");
    state
        .mark_authenticated_rest_proven(token, credential_authority)
        .expect("REST proof is accepted by a responsive session");

    let awaiting = state.snapshot();
    assert_eq!(
        awaiting.subscription,
        AuthenticatedUserSubscriptionState::AwaitingServerProof,
        "the REST proof deliberately reopens the wait for a same-generation PONG"
    );
    assert!(
        awaiting.recovery_token().is_none(),
        "the recovery window is closed until a later PONG proves liveness again"
    );
    assert_eq!(
        awaiting.recovery_identity(),
        Some(token),
        "session identity must survive the REST-proof window, or a guard reading \
         it during the wait can only ever fail"
    );
}

#[test]
fn stale_generation_server_proof_cannot_authorize_reconnect() {
    let state = StateCell::new();
    let first = state.mark_connected();
    state.mark_subscription_written(first);
    state.mark_disconnected();
    let second = state.mark_connected();
    state.mark_subscription_written(second);
    state.mark_server_liveness_proven(first);
    assert_eq!(
        state.snapshot().subscription,
        AuthenticatedUserSubscriptionState::AwaitingServerProof
    );
    // The stale-generation proof was a no-op, so subscription never reached
    // `ServerResponsive` and no recovery token exists yet.
    assert!(state.snapshot().recovery_token().is_none());
    state.mark_server_liveness_proven(second);
    assert!(state.snapshot().recovery_token().is_some());
}

#[test]
fn pong_and_catch_up_without_authenticated_rest_proof_never_become_ready() {
    let state = StateCell::new();
    let generation = state.mark_connected();
    state.mark_subscription_written(generation);
    state.mark_server_liveness_proven(generation);
    let snapshot = state.snapshot();
    let token = snapshot.recovery_token().expect("candidate recovery token");

    assert_eq!(
        snapshot.subscription,
        AuthenticatedUserSubscriptionState::ServerResponsive
    );
    let credential_authority =
        AuthenticatedCredentialAuthority::new(token.credential_identity(), 1)
            .expect("test credential generation");
    assert!(
        state
            .prepare_catch_up_finalization(token, credential_authority, 0, 0)
            .is_none()
    );
    assert!(!state.snapshot().is_ready());
}

#[test]
fn cross_credential_rest_proof_cannot_open_socket_authority() {
    let socket_identity = AuthenticatedCredentialIdentity::for_test("socket-a");
    let state = StateCell::new_with_identity(socket_identity);
    let generation = state.mark_connected();
    state.mark_subscription_written(generation);
    state.mark_server_liveness_proven(generation);
    let token = state.snapshot().recovery_token().expect("socket token");
    let foreign = AuthenticatedCredentialAuthority::new(
        AuthenticatedCredentialIdentity::for_test("rest-b"),
        1,
    )
    .expect("foreign authority");

    assert_eq!(state.mark_authenticated_rest_proven(token, foreign), None);
    assert!(!state.snapshot().is_ready());

    let matching =
        AuthenticatedCredentialAuthority::new(socket_identity, 1).expect("matching authority");
    let liveness_floor = state
        .mark_authenticated_rest_proven(token, matching)
        .expect("matching REST proof");
    state.mark_server_liveness_proven(generation);
    let finalization = state
        .prepare_catch_up_finalization(token, matching, 0, liveness_floor)
        .expect("matching finalization");
    assert!(state.commit_catch_up_finalization(finalization));
    assert!(state.authority_matches(token, matching, 0));
    assert!(!state.authority_matches(token, foreign, 0));
}

#[test]
fn prepared_catch_up_cannot_commit_after_socket_state_changes() {
    let socket_identity = AuthenticatedCredentialIdentity::for_test("socket-cas");
    let state = StateCell::new_with_identity(socket_identity);
    let generation = state.mark_connected();
    state.mark_subscription_written(generation);
    state.mark_server_liveness_proven(generation);
    let token = state.snapshot().recovery_token().expect("socket token");
    let authority =
        AuthenticatedCredentialAuthority::new(socket_identity, 1).expect("matching authority");
    let liveness_floor = state
        .mark_authenticated_rest_proven(token, authority)
        .expect("matching REST proof");
    state.mark_server_liveness_proven(generation);
    let finalization = state
        .prepare_catch_up_finalization(token, authority, 0, liveness_floor)
        .expect("prepared finalization");

    state.mark_evidence_gap();

    assert!(!state.commit_catch_up_finalization(finalization));
    assert!(!state.snapshot().is_ready());
}

#[test]
fn generation_and_gap_change_invalidate_final_catch_up_cas() {
    let state = StateCell::new();
    let first = state.mark_connected();
    state.mark_subscription_written(first);
    state.mark_server_liveness_proven(first);
    let before_gap = state.snapshot().recovery_token().expect("first token");

    state.mark_schema_gap();
    assert!(!state.complete_catch_up(before_gap, 0));
    // `mark_schema_gap` drops subscription back to `AwaitingServerProof`
    // (recorded REST proof is no longer trusted); a same-generation proof
    // must land again before a fresh recovery token exists to test against.
    state.mark_server_liveness_proven(first);
    let after_gap = state.snapshot().recovery_token().expect("gap token");

    state.mark_disconnected();
    let second = state.mark_connected();
    state.mark_subscription_written(second);
    state.mark_server_liveness_proven(second);
    assert_ne!(first, second);
    assert!(!state.complete_catch_up(after_gap, 0));

    let current = state.snapshot().recovery_token().expect("current token");
    assert!(state.complete_catch_up(current, 0));
    assert!(state.snapshot().is_ready());
}

#[test]
fn reconnect_alone_remains_closed_until_exact_catch_up() {
    let state = StateCell::new();
    let first = state.mark_connected();
    state.mark_subscription_written(first);
    state.mark_server_liveness_proven(first);
    let first_token = state.snapshot().recovery_token().expect("first token");
    assert!(!state.snapshot().is_ready());
    assert!(state.complete_catch_up(first_token, 0));
    assert!(state.snapshot().is_ready());

    state.mark_disconnected();
    let second = state.mark_connected();
    state.mark_subscription_written(second);
    state.mark_server_liveness_proven(second);
    let second_token = state.snapshot().recovery_token().expect("second token");
    assert_ne!(first, second);
    assert!(!state.snapshot().is_ready());
    assert!(!state.complete_catch_up(first_token, 0));
    assert!(state.complete_catch_up(second_token, 0));
    assert!(state.snapshot().is_ready());
}

#[test]
fn schema_gap_is_sticky_until_complete_catch_up() {
    let state = StateCell::new();
    let generation = state.mark_connected();
    state.mark_subscription_written(generation);
    state.mark_server_liveness_proven(generation);
    let initial = state.snapshot().recovery_token().expect("initial token");
    assert!(state.complete_catch_up(initial, 0));
    state.mark_schema_gap();
    assert!(!state.snapshot().is_ready());
    assert!(!state.complete_catch_up(initial, 0));
    // `mark_schema_gap` drops subscription back to `AwaitingServerProof`; a
    // same-generation proof must land again before a fresh recovery token
    // exists.
    state.mark_server_liveness_proven(generation);
    let recovery = state.snapshot().recovery_token().expect("recovery token");
    assert!(state.complete_catch_up(recovery, 0));
    assert!(state.snapshot().is_ready());
}

#[test]
fn catch_up_cannot_restore_readiness_after_consumer_loss() {
    let state = StateCell::new();
    let generation = state.mark_connected();
    state.mark_subscription_written(generation);
    state.mark_server_liveness_proven(generation);
    let token = state.snapshot().recovery_token().expect("recovery token");
    assert!(state.complete_catch_up(token, 0));
    state.mark_consumer_closed();
    assert!(!state.snapshot().is_ready());
    // `mark_consumer_closed` also drops subscription back to
    // `AwaitingServerProof`; re-prove liveness so a token exists to probe —
    // `prepare_catch_up_finalization` still independently refuses while
    // `consumer_closed` is sticky, which is the behavior under test below.
    state.mark_server_liveness_proven(generation);
    let after_loss = state.snapshot().recovery_token().expect("connected token");
    assert!(!state.complete_catch_up(after_loss, 0));
    assert!(state.snapshot().consumer_closed);
    assert!(!state.snapshot().is_ready());
}

#[test]
fn full_queue_drop_is_retired_only_by_exact_rest_recovery() {
    let state = StateCell::new();
    let generation = state.mark_connected();
    state.mark_subscription_written(generation);
    state.mark_server_liveness_proven(generation);

    let enqueued = state
        .reserve_transport_sequences(1)
        .expect("first sequence range");
    state.mark_enqueued(enqueued);
    state.acknowledge_durable(enqueued.first);

    let dropped = state
        .reserve_transport_sequences(2)
        .expect("dropped sequence range");
    assert!(state.mark_dropped(dropped));
    let snapshot = state.snapshot();
    let recovery = snapshot.recovery_token().expect("recovery token");
    assert_eq!(snapshot.transport_sequence, 3);
    assert_eq!(snapshot.enqueued_sequence, 1);
    assert_eq!(snapshot.durable_sequence, 1);
    assert!(snapshot.delivery_gap);
    assert!(!snapshot.is_ready());

    assert!(!state.complete_catch_up(recovery, 2));
    assert_eq!(state.snapshot().durable_sequence, 1);
    assert!(state.complete_catch_up(recovery, 3));
    let recovered = state.snapshot();
    assert_eq!(recovered.durable_sequence, 3);
    assert!(recovered.is_ready());
}

#[test]
fn successfully_enqueued_work_cannot_be_retired_by_rest_recovery() {
    let state = StateCell::new();
    let generation = state.mark_connected();
    state.mark_subscription_written(generation);
    state.mark_server_liveness_proven(generation);
    let enqueued = state
        .reserve_transport_sequences(1)
        .expect("queued sequence range");
    state.mark_enqueued(enqueued);
    let recovery = state.snapshot().recovery_token().expect("recovery token");

    assert!(!state.complete_catch_up(recovery, enqueued.last));
    assert_eq!(state.snapshot().durable_sequence, 0);
    assert!(!state.snapshot().is_ready());

    state.acknowledge_durable(enqueued.first);
    assert!(state.complete_catch_up(recovery, enqueued.last));
    assert!(state.snapshot().is_ready());
}

#[test]
fn alternating_drop_and_ack_pressure_is_permanently_bounded_and_closed() {
    let state = StateCell::new();
    let generation = state.mark_connected();
    state.mark_subscription_written(generation);
    state.mark_server_liveness_proven(generation);
    for index in 0..(MAX_DURABLE_OUT_OF_ORDER * 4) {
        let range = state
            .reserve_transport_sequences(1)
            .expect("bounded sequence range");
        if index % 2 == 0 {
            let _ = state.mark_dropped(range);
        } else {
            state.mark_enqueued(range);
            state.acknowledge_durable(range.first);
        }
    }

    assert!(state.dropped_ranges.lock().expect("dropped ranges").len() <= MAX_DROPPED_RANGES);
    assert!(
        state
            .durable_out_of_order
            .lock()
            .expect("durable set")
            .len()
            <= MAX_DURABLE_OUT_OF_ORDER
    );
    let snapshot = state.snapshot();
    assert!(snapshot.consumer_closed);
    assert!(snapshot.delivery_gap);
    assert!(!snapshot.is_ready());
    assert!(snapshot.recovery_token().is_some());
    assert!(!state.complete_catch_up(
        snapshot.recovery_token().expect("connected token"),
        snapshot.transport_sequence,
    ));
}

#[test]
fn mixed_array_rejects_atomically() {
    let payload = serde_json::json!([
        trade_json(Some(serde_json::json!("0"))),
        {
            "event_type": "order",
            "id": "broken-order",
            "market": format!("0x{}", "2".repeat(64)),
            "asset_id": "2",
            "side": "SELL",
            "price": 0.55
        }
    ]);
    let body = serde_json::to_vec(&payload).expect("fixture encodes");
    assert_eq!(
        AuthenticatedUserEventBatch::decode_frame(&body),
        Err(AuthenticatedUserWsError::FrameSchema)
    );
}

/// [REGRESSION][EVAL] Parsing through `serde_json::Value` silently kept
/// the last duplicate key. Direct map deserialization must reject every
/// duplicate identity, economic, and lifecycle field before publishing
/// any member of either a single-object or mixed-array frame.
#[test]
fn duplicate_known_fields_reject_single_and_mixed_frames_atomically() {
    let canonical = serde_json::to_string(&trade_json(Some(serde_json::json!("0"))))
        .expect("canonical fixture");
    for duplicate in [
        r#""id":"other-trade""#,
        r#""fee_rate_bps":"999""#,
        r#""status":"FAILED""#,
    ] {
        let duplicated = format!("{{{duplicate},{}", &canonical[1..]);
        assert_eq!(
            AuthenticatedUserEventBatch::decode_frame(duplicated.as_bytes()),
            Err(AuthenticatedUserWsError::FrameSchema),
            "single object accepted duplicate {duplicate}"
        );

        let mixed = format!("[{canonical},{duplicated}]");
        assert_eq!(
            AuthenticatedUserEventBatch::decode_frame(mixed.as_bytes()),
            Err(AuthenticatedUserWsError::FrameSchema),
            "mixed array partially accepted duplicate {duplicate}"
        );
    }
}

/// [REGRESSION][EVAL] Private frames are execution evidence. New fields at
/// the event or maker boundary reject the whole frame until retained by an
/// explicit contract.
#[test]
fn unknown_trade_order_or_maker_evidence_rejects_the_complete_frame() {
    let mut order = serde_json::json!({
        "event_type": "order",
        "id": "order-id",
        "market": format!("0x{}", "2".repeat(64)),
        "asset_id": "2",
        "side": "SELL",
        "price": "0.5",
        "type": "UPDATE",
        "original_size": "10",
        "size_matched": "1",
        "timestamp": "1700000000",
        "associate_trades": ["trade-id"],
        "status": "LIVE"
    });
    order.as_object_mut().expect("order object").insert(
        "new_order_evidence".to_owned(),
        serde_json::json!("must-not-be-shed"),
    );

    let mut trade = trade_json(Some(serde_json::json!("0")));
    trade.as_object_mut().expect("trade object").insert(
        "new_trade_evidence".to_owned(),
        serde_json::json!("must-not-be-shed"),
    );

    let mut maker = trade_json(Some(serde_json::json!("0")));
    maker["maker_orders"][0]
        .as_object_mut()
        .expect("maker object")
        .insert(
            "new_maker_evidence".to_owned(),
            serde_json::json!("must-not-be-shed"),
        );

    for hostile in [order, trade, maker] {
        let body = serde_json::to_vec(&hostile).expect("encode hostile frame");
        assert_eq!(
            AuthenticatedUserEventBatch::decode_frame(&body),
            Err(AuthenticatedUserWsError::FrameSchema)
        );
    }
}

/// [REGRESSION][EVAL] Every documented private-user field is explicit schema
/// evidence. The strict decoder must retain it instead of treating official
/// additions as unknown or silently discarding them after deserialization.
#[test]
fn full_official_trade_and_order_examples_retain_every_legacy_field() {
    let maker_address = "0x1111111111111111111111111111111111111111";
    let transaction_hash = format!("0x{}", "a".repeat(64));
    let trade = serde_json::json!({
        "asset_id": "1",
        "bucket_index": 0,
        "event_type": "trade",
        "fee_rate_bps": "1",
        "id": "trade-id",
        "maker_address": maker_address,
        "maker_orders": [{
            "asset_id": "2",
            "maker_address": maker_address,
            "matched_amount": "4",
            "order_id": "maker-order",
            "outcome": "Down",
            "outcome_index": "1",
            "owner": "ffffffff-ffff-ffff-ffff-ffffffffffff",
            "price": "0.4",
            "side": "SELL"
        }],
        "market": format!("0x{}", "1".repeat(64)),
        "matchtime": "1700000000",
        "outcome": "Up",
        "owner": "ffffffff-ffff-ffff-ffff-ffffffffffff",
        "price": "0.6",
        "side": "BUY",
        "size": "4",
        "status": "CONFIRMED",
        "taker_order_id": "taker-order",
        "trader_side": "MAKER",
        "transaction_hash": transaction_hash,
        "type": "TRADE"
    });
    let order = serde_json::json!({
        "asset_id": "2",
        "associate_trades": ["trade-id"],
        "created_at": 1700000000,
        "event_type": "order",
        "expiration": "1700003600",
        "id": "order-id",
        "maker_address": maker_address,
        "market": format!("0x{}", "2".repeat(64)),
        "order_owner": "eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee",
        "order_type": "GTC",
        "original_size": "10",
        "outcome": "Down",
        "owner": "ffffffff-ffff-ffff-ffff-ffffffffffff",
        "price": "0.5",
        "side": "SELL",
        "size_matched": "1",
        "status": "LIVE",
        "timestamp": "1700000001",
        "type": "UPDATE"
    });
    let body = serde_json::to_vec(&serde_json::json!([trade, order])).expect("official frame");
    let batch = AuthenticatedUserEventBatch::decode_frame(&body).expect("official fields decode");
    let AuthenticatedUserEvent::Trade(trade) = &batch.as_slice()[0] else {
        panic!("first official event must be trade")
    };
    assert_eq!(trade.outcome.as_deref(), Some("Up"));
    assert_eq!(trade.bucket_index, Some(0));
    assert!(trade.maker_address.is_some());
    assert!(trade.transaction_hash.is_some());
    assert_eq!(trade.maker_orders[0].outcome.as_deref(), Some("Down"));
    assert_eq!(trade.maker_orders[0].outcome_index, Some(1));
    assert!(trade.maker_orders[0].maker_address.is_some());
    let AuthenticatedUserEvent::Order(order) = &batch.as_slice()[1] else {
        panic!("second official event must be order")
    };
    assert!(order.owner.is_some());
    assert!(order.order_owner.is_some());
    assert_eq!(order.outcome.as_deref(), Some("Down"));
    assert_eq!(order.created_at, Some(1_700_000_000));
    assert_eq!(order.expiration, Some(1_700_003_600));
    assert_eq!(order.order_type, Some(AuthenticatedUserVenueOrderType::Gtc));
    assert!(order.maker_address.is_some());
}

#[test]
fn official_optional_fields_accept_absence_null_and_documented_empty_hash() {
    let mut trade = trade_json(Some(serde_json::json!("0")));
    for field in ["outcome", "maker_address", "bucket_index"] {
        trade
            .as_object_mut()
            .expect("trade object")
            .insert(field.to_owned(), serde_json::Value::Null);
    }
    trade["transaction_hash"] = serde_json::json!("");
    trade["maker_orders"][0]["maker_address"] = serde_json::Value::Null;
    trade["maker_orders"][0]["outcome"] = serde_json::Value::Null;
    trade["maker_orders"][0]["outcome_index"] = serde_json::Value::Null;
    let body = serde_json::to_vec(&trade).expect("nullable official frame");
    let batch = AuthenticatedUserEventBatch::decode_frame(&body).expect("nullable fields decode");
    let AuthenticatedUserEvent::Trade(trade) = &batch.as_slice()[0] else {
        panic!("nullable official event must be trade")
    };
    assert_eq!(trade.outcome, None);
    assert_eq!(trade.maker_address, None);
    assert_eq!(trade.transaction_hash, None);
    assert_eq!(trade.bucket_index, None);
    assert_eq!(trade.maker_orders[0].maker_address, None);
    assert_eq!(trade.maker_orders[0].outcome, None);
    assert_eq!(trade.maker_orders[0].outcome_index, None);
}

#[test]
fn maker_fee_absent_blank_and_zero_remain_distinct() {
    for (wire, expected) in [
        (None, None),
        (Some(serde_json::json!("")), None),
        (Some(serde_json::json!("0")), Some(Decimal::ZERO)),
    ] {
        let body = serde_json::to_vec(&trade_json(wire)).expect("fixture encodes");
        let batch = AuthenticatedUserEventBatch::decode_frame(&body).expect("strict frame");
        let AuthenticatedUserEvent::Trade(trade) = &batch.as_slice()[0] else {
            panic!("trade fixture decoded as order")
        };
        assert_eq!(trade.maker_orders[0].fee_rate_bps, expected);
    }
}

#[test]
fn malformed_maker_fee_rejects_complete_frame() {
    for fee in [serde_json::json!(0), serde_json::json!("NaN")] {
        let body = serde_json::to_vec(&trade_json(Some(fee))).expect("fixture encodes");
        assert_eq!(
            AuthenticatedUserEventBatch::decode_frame(&body),
            Err(AuthenticatedUserWsError::FrameSchema)
        );
    }
}

#[test]
fn every_official_prefixed_and_unprefixed_trade_status_is_accepted() {
    for (wire, expected) in [
        (
            "MATCHED_NOT_BROADCASTED",
            AuthenticatedUserTradeStatus::MatchedNotBroadcasted,
        ),
        (
            "TRADE_STATUS_MATCHED_NOT_BROADCASTED",
            AuthenticatedUserTradeStatus::MatchedNotBroadcasted,
        ),
        ("MATCHED", AuthenticatedUserTradeStatus::Matched),
        (
            "TRADE_STATUS_MATCHED",
            AuthenticatedUserTradeStatus::Matched,
        ),
        ("MINED", AuthenticatedUserTradeStatus::Mined),
        ("TRADE_STATUS_MINED", AuthenticatedUserTradeStatus::Mined),
        ("CONFIRMED", AuthenticatedUserTradeStatus::Confirmed),
        (
            "TRADE_STATUS_CONFIRMED",
            AuthenticatedUserTradeStatus::Confirmed,
        ),
        ("RETRYING", AuthenticatedUserTradeStatus::Retrying),
        (
            "TRADE_STATUS_RETRYING",
            AuthenticatedUserTradeStatus::Retrying,
        ),
        ("FAILED", AuthenticatedUserTradeStatus::Failed),
        ("TRADE_STATUS_FAILED", AuthenticatedUserTradeStatus::Failed),
    ] {
        assert_eq!(
            serde_json::from_str::<AuthenticatedUserTradeStatus>(&format!("\"{wire}\""))
                .expect("current official status"),
            expected,
        );
    }
}

#[test]
fn official_nullish_maker_orders_decodes_as_an_empty_atomic_list() {
    let mut fixture = trade_json(Some(serde_json::json!("0")));
    fixture["maker_orders"] = serde_json::Value::Null;
    let body = serde_json::to_vec(&fixture).expect("fixture encodes");
    let batch = AuthenticatedUserEventBatch::decode_frame(&body).expect("strict frame");
    let AuthenticatedUserEvent::Trade(trade) = &batch.as_slice()[0] else {
        panic!("trade fixture decoded as order")
    };
    assert!(trade.maker_orders.is_empty());
}

#[test]
fn all_market_user_subscription_has_only_official_minimal_keys() {
    let payload = subscription_payload(&Credentials::default()).expect("subscription JSON");
    let value: serde_json::Value = serde_json::from_str(&payload).expect("subscription value");
    let root = value.as_object().expect("subscription object");
    let mut root_keys = root.keys().map(String::as_str).collect::<Vec<_>>();
    root_keys.sort_unstable();
    assert_eq!(root_keys, ["auth", "type"]);
    assert_eq!(
        root.get("type").and_then(serde_json::Value::as_str),
        Some("user")
    );
    let auth = root
        .get("auth")
        .and_then(serde_json::Value::as_object)
        .expect("auth object");
    let mut auth_keys = auth.keys().map(String::as_str).collect::<Vec<_>>();
    auth_keys.sort_unstable();
    assert_eq!(auth_keys, ["apiKey", "passphrase", "secret"]);
    for removed in ["operation", "markets", "assets_ids", "initial_dump"] {
        assert!(!root.contains_key(removed));
    }

    let auth = UserAuthentication {
        api_key: "private-api-key".to_owned(),
        secret: "private-secret",
        passphrase: "private-passphrase",
    };
    let debug = format!("{auth:?}");
    for private in ["private-api-key", "private-secret", "private-passphrase"] {
        assert!(
            !debug.contains(private),
            "authentication Debug leaked {private}"
        );
    }
}

#[test]
fn negative_or_out_of_range_economics_reject_complete_frame() {
    for (path, value) in [
        ("/price", serde_json::json!("-0.1")),
        ("/price", serde_json::json!("1.000001")),
        ("/size", serde_json::json!("-0.1")),
        ("/maker_orders/0/matched_amount", serde_json::json!("-0.1")),
        ("/maker_orders/0/price", serde_json::json!("-0.1")),
        ("/maker_orders/0/fee_rate_bps", serde_json::json!("-1")),
    ] {
        let mut fixture = trade_json(Some(serde_json::json!("0")));
        *fixture
            .pointer_mut(path)
            .unwrap_or_else(|| panic!("fixture path {path}")) = value;
        let body = serde_json::to_vec(&fixture).expect("fixture encodes");
        assert_eq!(
            AuthenticatedUserEventBatch::decode_frame(&body),
            Err(AuthenticatedUserWsError::FrameSchema),
            "path {path}"
        );
    }

    for (original, matched, price) in [
        ("0", "0", "0.5"),
        ("10", "-1", "0.5"),
        ("10", "11", "0.5"),
        ("10", "0", "0"),
        ("10", "0", "1.1"),
    ] {
        let fixture = serde_json::json!({
            "event_type": "order",
            "id": "order-id",
            "market": format!("0x{}", "2".repeat(64)),
            "asset_id": "2",
            "side": "SELL",
            "price": price,
            "type": "UPDATE",
            "original_size": original,
            "size_matched": matched,
            "timestamp": "1700000000",
            "status": "LIVE"
        });
        let body = serde_json::to_vec(&fixture).expect("fixture encodes");
        assert_eq!(
            AuthenticatedUserEventBatch::decode_frame(&body),
            Err(AuthenticatedUserWsError::FrameSchema)
        );
    }
}

/// The private transport retains zero economics as raw evidence. Positive
/// execution validation belongs after durable quarantine in the node.
#[test]
fn zero_execution_economics_survive_authenticated_ws_decode() {
    let mut fixture = trade_json(Some(serde_json::json!("0")));
    fixture["size"] = serde_json::json!("0");
    fixture["price"] = serde_json::json!("0");
    fixture["maker_orders"][0]["matched_amount"] = serde_json::json!("0");
    fixture["maker_orders"][0]["price"] = serde_json::json!("0");
    let body = serde_json::to_vec(&fixture).expect("fixture encodes");
    let batch =
        AuthenticatedUserEventBatch::decode_frame(&body).expect("retain raw zero private evidence");
    let AuthenticatedUserEvent::Trade(trade) = &batch.as_slice()[0] else {
        panic!("trade fixture decoded as order")
    };
    assert_eq!(trade.size, Decimal::ZERO);
    assert_eq!(trade.price, Decimal::ZERO);
    assert_eq!(trade.maker_orders[0].matched_amount, Decimal::ZERO);
    assert_eq!(trade.maker_orders[0].price, Decimal::ZERO);
}

#[test]
fn private_frames_reject_delimiter_whitespace_and_control_identities() {
    for (path, invalid) in [
        ("/id", "trade:maker"),
        ("/taker_order_id", "order id"),
        ("/maker_orders/0/order_id", "maker\nforged-log"),
    ] {
        let mut fixture = trade_json(Some(serde_json::json!("0")));
        *fixture
            .pointer_mut(path)
            .unwrap_or_else(|| panic!("fixture path {path}")) = serde_json::json!(invalid);
        let body = serde_json::to_vec(&fixture).expect("fixture encoding");
        let error = AuthenticatedUserEventBatch::decode_frame(&body)
            .expect_err("invalid identity must reject complete private frame");
        assert_eq!(error, AuthenticatedUserWsError::FrameSchema);
        assert!(!format!("{error:?}").contains(invalid));
    }

    for (pointer, invalid) in [
        ("/id", "order:maker"),
        ("/associate_trades/0", "trade\tforged"),
    ] {
        let mut fixture = serde_json::json!({
            "event_type": "order",
            "id": "order-id",
            "market": format!("0x{}", "2".repeat(64)),
            "asset_id": "2",
            "side": "SELL",
            "price": "0.5",
            "type": "UPDATE",
            "original_size": "10",
            "size_matched": "1",
            "timestamp": "1700000000",
            "associate_trades": ["trade-id"],
            "status": "LIVE"
        });
        *fixture
            .pointer_mut(pointer)
            .unwrap_or_else(|| panic!("fixture path {pointer}")) = serde_json::json!(invalid);
        let body = serde_json::to_vec(&fixture).expect("fixture encoding");
        let error = AuthenticatedUserEventBatch::decode_frame(&body)
            .expect_err("invalid identity must reject complete private frame");
        assert_eq!(error, AuthenticatedUserWsError::FrameSchema);
        assert!(!format!("{error:?}").contains(invalid));
    }
}

#[test]
fn endpoint_is_normalized_to_exact_user_path() {
    assert_eq!(
        official_user_endpoint().expect("fixed production user endpoint"),
        "wss://ws-subscriptions-clob.polymarket.com/ws/user"
    );
    assert_eq!(
        user_endpoint("wss://example.test/ws/market").expect_err("wrong channel is rejected"),
        AuthenticatedUserWsError::InvalidEndpoint
    );
    assert_eq!(
        user_endpoint("wss://example.test/ws/user").expect("user endpoint"),
        "wss://example.test/ws/user"
    );
    assert_eq!(
        user_endpoint("ws://127.0.0.1:8080").expect("local endpoint"),
        "ws://127.0.0.1:8080/ws/user"
    );
    for rejected in [
        "ws://example.invalid",
        "ws://localhost:8080",
        "ws://ws-subscriptions-clob.polymarket.com/ws/user",
        "wss://user@example.invalid/ws/user",
        "wss://example.invalid/not-user",
        "wss://example.invalid/ws/user?forward=secret",
        "wss://example.invalid/ws/user#fragment",
    ] {
        assert_eq!(
            user_endpoint(rejected),
            Err(AuthenticatedUserWsError::InvalidEndpoint),
            "{rejected}"
        );
    }
    assert_eq!(
        loopback_user_endpoint("wss://attacker.invalid"),
        Err(AuthenticatedUserWsError::InvalidEndpoint)
    );
    assert_eq!(
        loopback_user_endpoint("ws://192.0.2.1:8080"),
        Err(AuthenticatedUserWsError::InvalidEndpoint)
    );
    assert_eq!(
        loopback_user_endpoint("ws://127.0.0.1:8080").expect("test-only loopback"),
        "ws://127.0.0.1:8080/ws/user"
    );
}

#[tokio::test]
async fn redirect_handshake_is_not_followed_to_a_credential_sink() {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    const REDIRECT_SECRET: &str = "redirect-secret-must-not-leave-origin";
    const REDIRECT_PASSPHRASE: &str = "redirect-passphrase-must-not-leave-origin";

    let redirect_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind local redirect endpoint");
    let redirect_address = redirect_listener
        .local_addr()
        .expect("read redirect endpoint address");
    let sink_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind local credential sink");
    let sink_address = sink_listener
        .local_addr()
        .expect("read credential sink address");
    let redirect = tokio::spawn(async move {
        let (mut socket, _) = redirect_listener
            .accept()
            .await
            .expect("accept redirect handshake");
        let request = timeout(TEST_TIMEOUT, async {
            let mut request = Vec::with_capacity(1_024);
            let mut chunk = [0_u8; 1_024];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let read = socket
                    .read(&mut chunk)
                    .await
                    .expect("read redirect handshake");
                assert!(read != 0, "redirect handshake ended before its headers");
                request.extend_from_slice(&chunk[..read]);
                assert!(
                    request.len() <= 4_096,
                    "redirect handshake exceeded test bound"
                );
            }
            request
        })
        .await
        .expect("redirect handshake timed out");
        let request = String::from_utf8_lossy(&request);
        assert!(request.starts_with("GET /ws/user HTTP/1.1"));
        assert!(!request.contains(REDIRECT_SECRET));
        assert!(!request.contains(REDIRECT_PASSPHRASE));
        let response = format!(
            "HTTP/1.1 302 Found\r\nLocation: ws://{sink_address}/ws/user\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        socket
            .write_all(response.as_bytes())
            .await
            .expect("write redirect response");
    });

    let credentials = Credentials::new(
        ApiKey::nil(),
        REDIRECT_SECRET.to_owned(),
        REDIRECT_PASSPHRASE.to_owned(),
    );
    let ws = AuthenticatedUserWs::connect_with_config(
        &format!("ws://{redirect_address}"),
        credentials,
        one_connection_config(),
    )
    .expect("construct redirect probe");
    redirect.await.expect("redirect server task");

    assert!(
        timeout(Duration::from_millis(250), sink_listener.accept())
            .await
            .is_err(),
        "the authenticated connector followed a redirect toward a credential sink"
    );
    assert_eq!(
        ws.state().authentication,
        AuthenticatedUserAuthenticationState::Inactive
    );
    ws.shutdown().await;
}

#[tokio::test]
async fn rejection_and_server_error_before_pong_never_expose_recovery() {
    for payload in [
        r#"{"error":"authentication failed"}"#,
        "ERROR authenticated user channel rejected",
    ] {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind rejection server");
        let endpoint = format!("ws://{}", listener.local_addr().expect("server address"));
        let payload = payload.to_owned();
        let server = tokio::spawn(async move {
            let mut socket = accept_subscribed_socket(&listener).await;
            read_proof_ping(&mut socket).await;
            socket
                .send(Message::Text(payload.into()))
                .await
                .expect("send rejection frame");
        });
        let ws = AuthenticatedUserWs::connect_with_config(
            &endpoint,
            Credentials::default(),
            one_connection_config(),
        )
        .expect("construct rejected session");
        let _events = ws.take_events().expect("own rejected event receiver");
        let mut states = ws.state_receiver();
        let terminal = wait_for_state(&mut states, |state| {
            state.generation == 1
                && state.schema_gap
                && matches!(
                    state.connection,
                    AuthenticatedUserConnectionState::Disconnected
                )
        })
        .await;
        assert_eq!(terminal.recovery_token(), None);
        assert!(!terminal.is_ready());
        server.await.expect("rejection server task");
        ws.shutdown().await;
    }
}

#[tokio::test]
async fn close_before_pong_never_exposes_recovery() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind close server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("server address"));
    let server = tokio::spawn(async move {
        let mut socket = accept_subscribed_socket(&listener).await;
        read_proof_ping(&mut socket).await;
        socket.close(None).await.expect("close unproven socket");
    });
    let ws = AuthenticatedUserWs::connect_with_config(
        &endpoint,
        Credentials::default(),
        one_connection_config(),
    )
    .expect("construct closed session");
    let _events = ws.take_events().expect("own closed event receiver");
    let mut states = ws.state_receiver();
    let terminal = wait_for_state(&mut states, |state| {
        state.generation == 1
            && matches!(
                state.connection,
                AuthenticatedUserConnectionState::Disconnected
            )
    })
    .await;
    assert_eq!(terminal.recovery_token(), None);
    assert!(!terminal.is_ready());
    server.await.expect("close server task");
    ws.shutdown().await;
}

#[tokio::test]
async fn pong_then_rejection_never_exposes_caught_up_readiness() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind post-proof rejection server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("server address"));
    let (proven_tx, proven_rx) = oneshot::channel();
    let (reject_tx, reject_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        let mut socket = accept_subscribed_socket(&listener).await;
        prove_server_liveness(&mut socket).await;
        let _ = proven_tx.send(());
        let _ = reject_rx.await;
        socket
            .send(Message::Text(r#"{"error":"authentication failed"}"#.into()))
            .await
            .expect("send post-proof rejection");
    });
    let ws = AuthenticatedUserWs::connect_with_config(
        &endpoint,
        Credentials::default(),
        one_connection_config(),
    )
    .expect("construct post-proof rejection session");
    let _events = ws.take_events().expect("own post-proof event receiver");
    let mut states = ws.state_receiver();
    timeout(TEST_TIMEOUT, proven_rx)
        .await
        .expect("proof notification timeout")
        .expect("proof notification");
    let proven = wait_for_state(&mut states, |state| state.recovery_token().is_some()).await;
    let token = proven.recovery_token().expect("proven token");
    // Nothing distinguishes this window from any other clean, proven,
    // responsive state yet — `reject_tx` is a test-orchestration signal the
    // real protocol never observes, not evidence available to the client.
    // Catch-up succeeds here exactly as it does in every other proven-state
    // test (see `heartbeat_loss_after_proof_revokes_caught_up_readiness`);
    // the rejection frame sent below is what revokes readiness afterward.
    assert!(ws.complete_catch_up(token, 0));
    assert!(ws.is_ready());

    reject_tx.send(()).expect("release rejection");
    let rejected = wait_for_state(&mut states, |state| {
        state.generation == token.generation()
            && state.schema_gap
            && matches!(
                state.connection,
                AuthenticatedUserConnectionState::Disconnected
            )
    })
    .await;
    assert_eq!(rejected.recovery_token(), None);
    assert!(!rejected.is_ready());
    assert!(!ws.complete_catch_up(token, 0));
    server.await.expect("post-proof rejection server task");
    ws.shutdown().await;
}

#[tokio::test]
async fn heartbeat_loss_after_proof_revokes_caught_up_readiness() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind heartbeat-loss server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("server address"));
    let (proven_tx, proven_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        let mut socket = accept_subscribed_socket(&listener).await;
        prove_server_liveness(&mut socket).await;
        let _ = proven_tx.send(());
        while socket.next().await.is_some() {}
    });
    let config = AuthenticatedUserWsConfig {
        heartbeat_interval: Duration::from_millis(20),
        heartbeat_timeout: Duration::from_millis(60),
        ..one_connection_config()
    };
    let ws = AuthenticatedUserWs::connect_with_config(&endpoint, Credentials::default(), config)
        .expect("construct heartbeat-loss session");
    let _events = ws.take_events().expect("own heartbeat event receiver");
    let mut states = ws.state_receiver();
    timeout(TEST_TIMEOUT, proven_rx)
        .await
        .expect("heartbeat proof timeout")
        .expect("heartbeat proof notification");
    let proven = wait_for_state(&mut states, |state| state.recovery_token().is_some()).await;
    let token = proven.recovery_token().expect("heartbeat proof token");
    assert!(ws.complete_catch_up(token, 0));
    assert!(ws.is_ready());

    let disconnected = wait_for_state(&mut states, |state| {
        matches!(
            state.connection,
            AuthenticatedUserConnectionState::Disconnected
        )
    })
    .await;
    assert_eq!(disconnected.recovery_token(), None);
    assert!(!disconnected.is_ready());
    server.await.expect("heartbeat-loss server task");
    ws.shutdown().await;
}

#[tokio::test]
async fn valid_same_generation_pong_plus_exact_catch_up_becomes_ready() {
    let (endpoint, proven, release, server) = proven_server().await;
    let ws = AuthenticatedUserWs::connect_with_config(
        &endpoint,
        Credentials::default(),
        one_connection_config(),
    )
    .expect("construct proven session");
    let _events = ws.take_events().expect("own proven event receiver");
    let mut states = ws.state_receiver();
    timeout(TEST_TIMEOUT, proven)
        .await
        .expect("valid proof timeout")
        .expect("valid proof notification");
    let responsive = wait_for_state(&mut states, |state| {
        matches!(
            state.subscription,
            AuthenticatedUserSubscriptionState::ServerResponsive
        )
    })
    .await;
    let token = responsive.recovery_token().expect("responsive token");
    assert!(!responsive.is_ready());
    assert!(ws.complete_catch_up(token, responsive.transport_sequence));
    assert!(ws.is_ready());

    release.send(()).expect("release proven server");
    server.await.expect("proven server task");
    ws.shutdown().await;
}

#[tokio::test]
async fn schema_gap_reconnects_but_stays_closed_until_exact_catch_up() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind reconnect server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("server address"));
    let (reconnected_tx, reconnected_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel();
    let mut unknown_nested_evidence = trade_json(Some(serde_json::json!("0")));
    unknown_nested_evidence["maker_orders"][0]
        .as_object_mut()
        .expect("maker object")
        .insert(
            "new_private_execution_evidence".to_owned(),
            serde_json::json!("must-not-be-shed"),
        );
    let unknown_nested_evidence = unknown_nested_evidence.to_string();
    let server = tokio::spawn(async move {
        let mut first = accept_subscribed_socket(&listener).await;
        prove_server_liveness(&mut first).await;
        first
            .send(Message::Text(unknown_nested_evidence.into()))
            .await
            .expect("send nested unknown-field frame");
        drop(first);

        let mut second = accept_subscribed_socket(&listener).await;
        prove_server_liveness(&mut second).await;
        let _ = reconnected_tx.send(());
        let _ = release_rx.await;
    });
    let config = AuthenticatedUserWsConfig {
        max_reconnect_attempts: Some(3),
        ..one_connection_config()
    };
    let ws = AuthenticatedUserWs::connect_with_config(&endpoint, Credentials::default(), config)
        .expect("construct reconnecting user channel");
    let mut states = ws.state_receiver();
    timeout(TEST_TIMEOUT, reconnected_rx)
        .await
        .expect("schema-gap reconnect timed out")
        .expect("reconnect server remained live");
    let reconnected = wait_for_state(&mut states, |state| {
        state.generation >= 2
            && matches!(
                state.subscription,
                AuthenticatedUserSubscriptionState::ServerResponsive
            )
    })
    .await;

    assert!(reconnected.schema_gap);
    assert!(reconnected.evidence_gap);
    assert!(!reconnected.is_ready());
    // The malformed nested-evidence frame reserved transport sequence 1 and
    // was retained as raw evidence before the socket closed on it. It never
    // decoded into a usable record, so it is unrecoverable rather than
    // durable: the consumer must explicitly register it as a permanent gap
    // and acknowledge its raw retention before an exact catch-up can close.
    // `mark_dropped` advances `gap_version`, so the recovery token must be
    // captured from a fresh snapshot taken *after* these calls.
    assert!(ws.mark_recoverable_gap(reconnected.transport_sequence));
    assert!(ws.acknowledge_raw_frame_durable(reconnected.transport_sequence));
    let recovery = ws.state().recovery_token().expect("recovery token");
    assert!(ws.complete_catch_up(recovery, reconnected.transport_sequence));
    assert!(ws.is_ready());

    let _ = release_tx.send(());
    ws.shutdown().await;
    server.await.expect("schema-gap reconnect server");
}

#[tokio::test]
async fn full_queue_stops_socket_consumption_until_owned_capacity_returns() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind backpressure server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("server address"));
    let (proven_tx, proven_rx) = oneshot::channel();
    let (burst_tx, burst_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel();
    let frame = trade_json(Some(serde_json::json!("0"))).to_string();
    let server = tokio::spawn(async move {
        let mut socket = accept_subscribed_socket(&listener).await;
        prove_server_liveness(&mut socket).await;
        let _ = proven_tx.send(());
        let _ = burst_rx.await;
        for _ in 0..=EVENT_CHANNEL_CAPACITY {
            socket
                .send(Message::Text(frame.clone().into()))
                .await
                .expect("send bounded private frame burst");
        }
        let _ = release_rx.await;
    });
    let config = AuthenticatedUserWsConfig {
        heartbeat_interval: Duration::from_secs(10),
        heartbeat_timeout: Duration::from_secs(20),
        ..one_connection_config()
    };
    let ws = AuthenticatedUserWs::connect_with_config(&endpoint, Credentials::default(), config)
        .expect("construct backpressured user channel");
    let mut events = ws.take_events().expect("take bounded event stream");
    timeout(TEST_TIMEOUT, proven_rx)
        .await
        .expect("backpressure proof timed out")
        .expect("backpressure server remained live");
    let responsive = timeout(TEST_TIMEOUT, async {
        loop {
            let state = ws.state();
            if state.recovery_token().is_some() {
                break state;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("responsive state did not become observable");
    let recovery = responsive.recovery_token().expect("responsive token");
    assert!(ws.complete_catch_up(recovery, 0));
    assert!(ws.is_ready());
    burst_tx.send(()).expect("release private frame burst");

    let full = timeout(TEST_TIMEOUT, async {
        loop {
            let state = ws.state();
            if state.transport_sequence == EVENT_CHANNEL_CAPACITY as u64 {
                break state;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("bounded queue never reached capacity");
    assert_eq!(full.enqueued_sequence, EVENT_CHANNEL_CAPACITY as u64);
    assert_eq!(full.durable_sequence, 0);
    assert!(!full.delivery_gap);
    assert!(!full.is_ready(), "durability lag must close readiness");

    // The extra frame was not consumed from the socket while no permit was
    // available. Releasing one slot admits that exact next frame.
    let first = timeout(TEST_TIMEOUT, events.recv())
        .await
        .expect("first queued private frame timed out")
        .expect("queued private event stream ended");
    let first = first.into_sequenced_events();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].0, 1);
    ws.acknowledge_durable(1);
    // Every server message in this fixture carries exactly one event, so the
    // raw-frame sequence and the transport sequence advance in lockstep;
    // readiness requires both durability ledgers to be acknowledged, not
    // just the transport one.
    ws.acknowledge_raw_frame_durable(1);
    timeout(TEST_TIMEOUT, async {
        while ws.state().transport_sequence != (EVENT_CHANNEL_CAPACITY + 1) as u64 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("extra socket frame was not admitted after capacity returned");

    for expected in 2..=(EVENT_CHANNEL_CAPACITY + 1) as u64 {
        let batch = timeout(TEST_TIMEOUT, events.recv())
            .await
            .expect("queued private frame timed out")
            .expect("queued private event stream ended");
        let events = batch.into_sequenced_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, expected);
        ws.acknowledge_durable(expected);
        ws.acknowledge_raw_frame_durable(expected);
    }
    let drained = ws.state();
    assert_eq!(
        drained.transport_sequence,
        (EVENT_CHANNEL_CAPACITY + 1) as u64
    );
    assert_eq!(drained.durable_sequence, drained.transport_sequence);
    assert!(ws.is_ready());

    let _ = release_tx.send(());
    ws.shutdown().await;
    server.await.expect("backpressure server");
}

#[test]
fn socket_receive_is_structurally_guarded_by_a_capacity_permit() {
    let source = include_str!("connection.rs");
    let reserve = source
        .find("event_tx.reserve()")
        .expect("bounded capacity reservation");
    let receive = source
        .find("socket.next()")
        .expect("private socket receive");
    assert!(
        reserve < receive,
        "capacity must be reserved before socket receipt"
    );
    assert!(
        !source.contains("try_send("),
        "post-receive loss path returned"
    );
}

#[tokio::test]
async fn silent_server_never_exposes_recovery_or_zero_event_readiness() {
    let (endpoint, subscribed, release, server) = silent_server().await;
    let ws = AuthenticatedUserWs::connect_with_config(
        &endpoint,
        Credentials::default(),
        one_connection_config(),
    )
    .expect("construct silent local session");
    let _events = ws.take_events().expect("own event receiver");
    let mut states = ws.state_receiver();
    timeout(TEST_TIMEOUT, subscribed)
        .await
        .expect("subscription timeout")
        .expect("subscription notification");
    let awaiting = wait_for_state(&mut states, |state| {
        matches!(
            state.subscription,
            AuthenticatedUserSubscriptionState::AwaitingServerProof
        )
    })
    .await;
    assert_eq!(awaiting.recovery_token(), None);
    assert!(!ws.complete_catch_up(
        AuthenticatedUserRecoveryToken::for_test(
            awaiting.generation,
            awaiting.gap_version,
            awaiting.credential_identity,
        ),
        0,
    ));
    assert!(!ws.is_ready());

    release.send(()).expect("release silent server");
    server.await.expect("silent server task");
    wait_for_state(&mut states, |state| {
        matches!(
            state.connection,
            AuthenticatedUserConnectionState::Disconnected
        )
    })
    .await;
    assert!(!ws.is_ready());
    ws.shutdown().await;
}

#[tokio::test]
async fn websocket_control_pong_cannot_prove_user_channel_liveness() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind control-pong server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("server address"));
    let (pong_tx, pong_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        let mut socket = accept_subscribed_socket(&listener).await;
        read_proof_ping(&mut socket).await;
        socket
            .send(Message::Pong(Vec::new().into()))
            .await
            .expect("send websocket control pong");
        let _ = pong_tx.send(());
        let _ = release_rx.await;
    });
    let ws = AuthenticatedUserWs::connect_with_config(
        &endpoint,
        Credentials::default(),
        one_connection_config(),
    )
    .expect("construct control-pong session");
    let _events = ws.take_events().expect("own event receiver");
    let mut states = ws.state_receiver();
    timeout(TEST_TIMEOUT, pong_rx)
        .await
        .expect("control pong timeout")
        .expect("control pong notification");
    assert!(
        timeout(Duration::from_millis(50), async {
            loop {
                let state = *states.borrow_and_update();
                if matches!(
                    state.subscription,
                    AuthenticatedUserSubscriptionState::ServerResponsive
                ) {
                    break;
                }
                states
                    .changed()
                    .await
                    .expect("session state sender remains live");
            }
        })
        .await
        .is_err()
    );
    let state = ws.state();
    assert_eq!(
        state.subscription,
        AuthenticatedUserSubscriptionState::AwaitingServerProof
    );
    assert_eq!(state.recovery_token(), None);
    assert!(!state.is_ready());

    release_tx.send(()).expect("release control-pong server");
    server.await.expect("control-pong server task");
    ws.shutdown().await;
}

#[tokio::test]
async fn socket_drop_after_valid_proof_revokes_caught_up_readiness() {
    let (endpoint, proven, release, server) = proven_server().await;
    let ws = AuthenticatedUserWs::connect_with_config(
        &endpoint,
        Credentials::default(),
        one_connection_config(),
    )
    .expect("construct local session");
    let _events = ws.take_events().expect("own event receiver");
    let mut states = ws.state_receiver();
    timeout(TEST_TIMEOUT, proven)
        .await
        .expect("server proof timeout")
        .expect("server proof notification");
    let active = wait_for_state(&mut states, |state| state.recovery_token().is_some()).await;
    assert!(ws.complete_catch_up(active.recovery_token().expect("active recovery token"), 0,));
    assert!(ws.is_ready());

    release.send(()).expect("release local server");
    server.await.expect("local server task");
    wait_for_state(&mut states, |state| {
        matches!(
            state.connection,
            AuthenticatedUserConnectionState::Disconnected
        )
    })
    .await;
    assert!(!ws.is_ready());
    ws.shutdown().await;
}

#[tokio::test]
async fn dropping_event_receiver_is_terminal_for_session() {
    let (endpoint, proven, release, server) = proven_server().await;
    let ws = AuthenticatedUserWs::connect_with_config(
        &endpoint,
        Credentials::default(),
        one_connection_config(),
    )
    .expect("construct local session");
    let events = ws.take_events().expect("own event receiver");
    let mut states = ws.state_receiver();
    timeout(TEST_TIMEOUT, proven)
        .await
        .expect("server proof timeout")
        .expect("server proof notification");
    let active = wait_for_state(&mut states, |state| state.recovery_token().is_some()).await;
    let token = active.recovery_token().expect("active recovery token");
    assert!(ws.complete_catch_up(token, 0));
    drop(events);

    let terminal = wait_for_state(&mut states, |state| state.consumer_closed).await;
    assert!(terminal.consumer_closed);
    assert!(!ws.complete_catch_up(token, 0));
    assert!(!ws.is_ready());
    let _ = release.send(());
    server.await.expect("local server task");
    ws.shutdown().await;
}

fn ready_state_cell() -> (StateCell, AuthenticatedUserRecoveryToken) {
    let state = StateCell::new();
    let generation = state.mark_connected();
    state.mark_subscription_written(generation);
    state.mark_server_liveness_proven(generation);
    let token = state.snapshot().recovery_token().expect("recovery token");
    assert!(state.complete_catch_up(token, 0));
    assert!(state.snapshot().is_ready());
    (state, token)
}

fn assert_authority_poison_is_terminal(state: &StateCell) {
    let snapshot = state.snapshot();
    assert!(snapshot.authority_poisoned);
    assert!(snapshot.consumer_closed);
    assert!(snapshot.schema_gap);
    assert!(snapshot.delivery_gap);
    assert!(snapshot.evidence_gap);
    assert!(snapshot.rest_credential_authority.is_none());
    assert!(!snapshot.is_ready());
    assert!(snapshot.recovery_token().is_none());
}

/// [REGRESSION][EVAL] A panic while holding the primary readiness state can
/// never be recovered into a ready connection or a successful catch-up.
#[test]
fn poisoned_primary_state_is_sticky_and_terminal() {
    let (state, token) = ready_state_cell();
    state.poison_state_for_test();

    assert_authority_poison_is_terminal(&state);
    assert!(!state.complete_catch_up(token, 0));
    assert_authority_poison_is_terminal(&state);
}

/// [REGRESSION][EVAL] Poisoning raw-frame custody preserves the retained bytes
/// while permanently rejecting their acknowledgement and any readiness repair.
#[tokio::test]
async fn poisoned_raw_frame_store_preserves_custody_and_rejects_ack() {
    let (state, token) = ready_state_cell();
    let capacity = state
        .reserve_raw_frame_capacity()
        .await
        .expect("raw frame capacity");
    let frame_sequence = state
        .reserve_raw_frame_sequence()
        .expect("raw frame sequence");
    let raw = AuthenticatedUserEventBatch::decode_frame(b"[]")
        .expect("empty exact frame")
        .with_transport_context_for_test(frame_sequence, 1, token.generation(), token.gap_version())
        .raw_evidence();
    assert!(state.retain_raw_frame(raw, capacity));
    assert_eq!(state.pending_raw_frames().len(), 1);

    state.poison_raw_frames_for_test();

    assert_eq!(state.pending_raw_frames().len(), 1);
    assert!(!state.acknowledge_raw_frame_durable(frame_sequence));
    assert_eq!(state.pending_raw_frames().len(), 1);
    assert!(!state.complete_catch_up(token, 0));
    assert_authority_poison_is_terminal(&state);
}

/// [REGRESSION][EVAL] A poisoned out-of-order durability set rejects every
/// acknowledgement and cannot participate in a catch-up finalization.
#[test]
fn poisoned_durable_store_rejects_ack_and_finalization() {
    let (state, _) = ready_state_cell();
    let range = state
        .reserve_transport_sequences(1)
        .expect("transport sequence");
    state.mark_enqueued(range);
    let token = state.snapshot().recovery_token().expect("recovery token");

    state.poison_durable_for_test();

    assert!(!state.acknowledge_durable(range.first));
    assert!(!state.complete_catch_up(token, range.last));
    assert_authority_poison_is_terminal(&state);
}

/// [REGRESSION][EVAL] A poisoned dropped-range ledger cannot be recovered into
/// a contiguous durable frontier or reopen authenticated readiness.
#[test]
fn poisoned_dropped_store_rejects_finalization() {
    let (state, _) = ready_state_cell();
    let range = state
        .reserve_transport_sequences(1)
        .expect("transport sequence");
    assert!(state.mark_dropped(range));
    let token = state.snapshot().recovery_token().expect("recovery token");

    state.poison_dropped_for_test();

    assert!(!state.complete_catch_up(token, range.last));
    assert_authority_poison_is_terminal(&state);
}
