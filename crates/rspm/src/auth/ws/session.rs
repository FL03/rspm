//! Owned authenticated user-channel session and task custody.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use polymarket::auth::Credentials;
use tokio::{
    sync::{OwnedSemaphorePermit, mpsc, watch},
    task::JoinHandle,
};

#[cfg(any(test, feature = "test-utils"))]
use super::endpoint::loopback_user_endpoint;
use super::{
    AuthenticatedCredentialAuthority, AuthenticatedCredentialIdentity,
    AuthenticatedUserCatchUpFinalization, AuthenticatedUserEventBatch, AuthenticatedUserRawFrame,
    AuthenticatedUserRecoveryToken, AuthenticatedUserWsConfig, AuthenticatedUserWsError,
    AuthenticatedUserWsState, EVENT_CHANNEL_CAPACITY, RETIREMENT_RESPONSE_MARGIN,
    SESSION_SHUTDOWN_TIMEOUT,
    connection::connection_loop,
    endpoint::{official_user_endpoint, subscription_payload},
    retirement::{
        SessionRetirementGuard, reserve_process_retirement_slot, retirement_custodian,
        transfer_to_process_custodian, wait_for_retirement_terminal,
    },
    state_cell::{StateCell, TransportSequenceRange},
};

pub(super) use super::retirement::RetirementState;
struct AuthenticatedUserWsInner {
    state: StateCell,
    custody_poisoned: AtomicBool,
    events: Mutex<Option<mpsc::Receiver<AuthenticatedUserEventBatch>>>,
    shutdown_tx: watch::Sender<bool>,
    task: Mutex<Option<JoinHandle<()>>>,
    retirement_slot: Mutex<Option<OwnedSemaphorePermit>>,
    retirement_state_tx: watch::Sender<RetirementState>,
}

impl AuthenticatedUserWsInner {
    fn fail_closed(&self) {
        self.custody_poisoned.store(true, Ordering::Release);
        self.state.mark_authority_poisoned();
        self.shutdown_tx.send_replace(true);
    }

    fn recover_mutex<'a, T>(&self, mutex: &'a Mutex<T>) -> (std::sync::MutexGuard<'a, T>, bool) {
        match mutex.lock() {
            Ok(guard) => (guard, false),
            Err(poison) => {
                self.fail_closed();
                (poison.into_inner(), true)
            }
        }
    }
}

/// Single-consumer bounded event stream. Batches preserve the all-or-nothing
/// frame boundary.
pub struct AuthenticatedUserEvents {
    receiver: mpsc::Receiver<AuthenticatedUserEventBatch>,
}

impl AuthenticatedUserEvents {
    pub async fn recv(&mut self) -> Option<AuthenticatedUserEventBatch> {
        self.receiver.recv().await
    }
}

/// Owned authenticated user-channel session.
#[derive(Clone)]
pub struct AuthenticatedUserWs(Arc<AuthenticatedUserWsInner>);

impl AuthenticatedUserWs {
    pub fn connect(credentials: Credentials) -> Result<Self, AuthenticatedUserWsError> {
        Self::connect_official_with_config(credentials, AuthenticatedUserWsConfig::default())
    }

    #[cfg(any(test, feature = "test-utils"))]
    pub fn connect_to(
        endpoint: &str,
        credentials: Credentials,
    ) -> Result<Self, AuthenticatedUserWsError> {
        Self::connect_with_config(endpoint, credentials, AuthenticatedUserWsConfig::default())
    }

    #[cfg(any(test, feature = "test-utils"))]
    pub fn connect_with_config(
        endpoint: &str,
        credentials: Credentials,
        config: AuthenticatedUserWsConfig,
    ) -> Result<Self, AuthenticatedUserWsError> {
        Self::connect_inner(loopback_user_endpoint(endpoint)?, credentials, config)
    }

    fn connect_official_with_config(
        credentials: Credentials,
        config: AuthenticatedUserWsConfig,
    ) -> Result<Self, AuthenticatedUserWsError> {
        Self::connect_inner(official_user_endpoint()?, credentials, config)
    }

    fn connect_inner(
        endpoint: String,
        credentials: Credentials,
        config: AuthenticatedUserWsConfig,
    ) -> Result<Self, AuthenticatedUserWsError> {
        if !config.is_valid() {
            return Err(AuthenticatedUserWsError::InvalidConfiguration);
        }
        let subscription = subscription_payload(&credentials)?;
        let runtime = tokio::runtime::Handle::try_current()
            .map_err(|_| AuthenticatedUserWsError::RuntimeUnavailable)?;
        let retirement_slot = reserve_process_retirement_slot()?;
        let credential_identity = AuthenticatedCredentialIdentity::from_credentials(&credentials);
        let state = StateCell::new_with_identity(credential_identity);
        let (event_tx, event_rx) = mpsc::channel(EVENT_CHANNEL_CAPACITY);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let connection_state = state.clone();
        let custodian_state = state.clone();
        let custodian_shutdown_rx = shutdown_rx.clone();
        let (retirement_state_tx, _) = watch::channel(RetirementState::Active);
        let custodian_state_tx = retirement_state_tx.clone();
        let task = runtime.spawn(retirement_custodian(
            async move {
                connection_loop(
                    endpoint,
                    subscription,
                    config,
                    connection_state,
                    event_tx,
                    shutdown_rx,
                )
                .await;
            },
            custodian_state,
            custodian_shutdown_rx,
            custodian_state_tx,
            SESSION_SHUTDOWN_TIMEOUT,
        ));
        Ok(Self(Arc::new(AuthenticatedUserWsInner {
            state,
            custody_poisoned: AtomicBool::new(false),
            events: Mutex::new(Some(event_rx)),
            shutdown_tx,
            task: Mutex::new(Some(task)),
            retirement_slot: Mutex::new(Some(retirement_slot)),
            retirement_state_tx,
        })))
    }

    pub fn take_events(&self) -> Result<AuthenticatedUserEvents, AuthenticatedUserWsError> {
        if self.0.custody_poisoned.load(Ordering::Acquire) {
            return Err(AuthenticatedUserWsError::AuthorityPoisoned);
        }
        let (mut events, poisoned) = self.0.recover_mutex(&self.0.events);
        if poisoned {
            return Err(AuthenticatedUserWsError::AuthorityPoisoned);
        }
        let receiver = events
            .take()
            .ok_or(AuthenticatedUserWsError::EventReceiverTaken)?;
        Ok(AuthenticatedUserEvents { receiver })
    }

    #[must_use]
    pub fn state(&self) -> AuthenticatedUserWsState {
        self.0.state.snapshot()
    }

    #[must_use]
    pub fn state_receiver(&self) -> watch::Receiver<AuthenticatedUserWsState> {
        self.0.state.state_tx.subscribe()
    }

    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.state().is_ready()
    }

    #[must_use]
    pub fn recovery_token(&self) -> Option<AuthenticatedUserRecoveryToken> {
        self.state().recovery_token()
    }

    /// Non-secret identity of the fixed credential tuple used by this socket.
    #[must_use]
    pub fn credential_identity(&self) -> AuthenticatedCredentialIdentity {
        self.state().credential_identity
    }

    /// Bind authenticated REST recovery to the exact connected generation.
    pub fn mark_authenticated_rest_proven(
        &self,
        token: AuthenticatedUserRecoveryToken,
        credential_authority: AuthenticatedCredentialAuthority,
    ) -> Option<u64> {
        self.0
            .state
            .mark_authenticated_rest_proven(token, credential_authority)
    }

    /// Clear gaps only after complete recovery for the connected generation.
    ///
    /// Mirrors `StateCell::complete_catch_up`: `mark_authenticated_rest_proven`
    /// deliberately drops `subscription` back to `AwaitingServerProof` (a
    /// strictly later documented text PONG must prove the socket survived the
    /// REST interval), so this synthesizes that later proof before finalizing
    /// — otherwise `prepare_catch_up_finalization`'s `ServerResponsive`
    /// requirement could never be satisfied inside one synchronous call.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn complete_catch_up(
        &self,
        token: AuthenticatedUserRecoveryToken,
        transport_watermark: u64,
    ) -> bool {
        let credential_authority =
            AuthenticatedCredentialAuthority::new(token.credential_identity(), 1)
                .expect("test credential generation");
        let Some(minimum_liveness_version) =
            self.mark_authenticated_rest_proven(token, credential_authority)
        else {
            return false;
        };
        self.0.state.mark_server_liveness_proven(token.generation());
        self.prepare_catch_up_finalization(
            token,
            credential_authority,
            transport_watermark,
            minimum_liveness_version,
        )
        .is_some_and(|finalization| self.commit_catch_up_finalization(finalization))
    }

    pub fn prepare_catch_up_finalization(
        &self,
        token: AuthenticatedUserRecoveryToken,
        credential_authority: AuthenticatedCredentialAuthority,
        transport_watermark: u64,
        minimum_liveness_version: u64,
    ) -> Option<AuthenticatedUserCatchUpFinalization> {
        self.0.state.prepare_catch_up_finalization(
            token,
            credential_authority,
            transport_watermark,
            minimum_liveness_version,
        )
    }

    pub fn commit_catch_up_finalization(
        &self,
        finalization: AuthenticatedUserCatchUpFinalization,
    ) -> bool {
        self.0.state.commit_catch_up_finalization(finalization)
    }

    #[must_use]
    pub fn authority_matches(
        &self,
        token: AuthenticatedUserRecoveryToken,
        credential_authority: AuthenticatedCredentialAuthority,
        transport_watermark: u64,
    ) -> bool {
        self.0
            .state
            .authority_matches(token, credential_authority, transport_watermark)
    }

    pub fn mark_delivery_gap(&self) {
        self.0.state.mark_delivery_gap();
    }

    pub fn mark_evidence_gap(&self) {
        self.0.state.mark_evidence_gap();
    }

    pub fn mark_recoverable_gap(&self, sequence: u64) -> bool {
        self.0.state.mark_dropped(TransportSequenceRange {
            first: sequence,
            last: sequence,
        })
    }

    pub fn acknowledge_durable(&self, sequence: u64) -> bool {
        self.0.state.acknowledge_durable(sequence)
    }

    #[must_use]
    pub fn pending_raw_frames(&self) -> Vec<AuthenticatedUserRawFrame> {
        self.0.state.pending_raw_frames()
    }

    pub fn acknowledge_raw_frame_durable(&self, frame_sequence: u64) -> bool {
        self.0.state.acknowledge_raw_frame_durable(frame_sequence)
    }

    pub fn request_shutdown(&self) {
        self.0.state.mark_disconnected();
        self.0.shutdown_tx.send_replace(true);
    }

    pub async fn shutdown(&self) -> bool {
        self.request_shutdown();
        let (guard, custody_poisoned) = self.take_retirement_guard();
        let response_timeout = SESSION_SHUTDOWN_TIMEOUT + RETIREMENT_RESPONSE_MARGIN;
        let Some(mut guard) = guard else {
            let terminal = wait_for_retirement_terminal(
                self.0.retirement_state_tx.subscribe(),
                response_timeout,
            )
            .await;
            return terminal && !custody_poisoned;
        };
        match tokio::time::timeout(response_timeout, guard.task()).await {
            Ok(Ok(())) => {
                let terminal = self.0.retirement_state_tx.borrow().is_terminal();
                guard.complete();
                terminal && !custody_poisoned
            }
            Ok(Err(_)) => {
                self.0.state.mark_evidence_gap();
                self.0
                    .retirement_state_tx
                    .send_replace(RetirementState::TerminalCancelled);
                guard.complete();
                false
            }
            Err(_) => false,
        }
    }

    pub async fn drain(&self) {
        self.request_shutdown();
        let (guard, _) = self.take_retirement_guard();
        if let Some(mut guard) = guard {
            if guard.task().await.is_err() {
                self.0.state.mark_evidence_gap();
                self.0
                    .retirement_state_tx
                    .send_replace(RetirementState::TerminalCancelled);
            }
            guard.complete();
        } else if !self.0.retirement_state_tx.borrow().is_terminal() {
            let mut retirement = self.0.retirement_state_tx.subscribe();
            while !retirement.borrow_and_update().is_terminal()
                && retirement.changed().await.is_ok()
            {}
        }
    }

    #[cfg(test)]
    pub(super) fn retirement_receiver(&self) -> watch::Receiver<RetirementState> {
        self.0.retirement_state_tx.subscribe()
    }

    #[cfg(test)]
    pub(super) fn with_retirement_task_for_test(
        task: JoinHandle<()>,
        retirement_slot: OwnedSemaphorePermit,
    ) -> Self {
        let state = StateCell::new();
        let (_, event_rx) = mpsc::channel(EVENT_CHANNEL_CAPACITY);
        let (shutdown_tx, _) = watch::channel(false);
        let (retirement_state_tx, _) = watch::channel(RetirementState::Active);
        Self(Arc::new(AuthenticatedUserWsInner {
            state,
            custody_poisoned: AtomicBool::new(false),
            events: Mutex::new(Some(event_rx)),
            shutdown_tx,
            task: Mutex::new(Some(task)),
            retirement_slot: Mutex::new(Some(retirement_slot)),
            retirement_state_tx,
        }))
    }

    #[cfg(test)]
    pub(super) fn poison_events_for_test(&self) {
        let ws = self.clone();
        let poisoned = std::thread::spawn(move || {
            let _events = ws.0.events.lock().expect("healthy event custody mutex");
            panic!("hostile event custody holder");
        })
        .join();
        assert!(poisoned.is_err());
    }

    #[cfg(test)]
    pub(super) fn poison_task_for_test(&self) {
        let ws = self.clone();
        let poisoned = std::thread::spawn(move || {
            let _task = ws.0.task.lock().expect("healthy task custody mutex");
            panic!("hostile task custody holder");
        })
        .join();
        assert!(poisoned.is_err());
    }

    #[cfg(test)]
    pub(super) fn poison_retirement_slot_for_test(&self) {
        let ws = self.clone();
        let poisoned = std::thread::spawn(move || {
            let _slot =
                ws.0.retirement_slot
                    .lock()
                    .expect("healthy retirement-slot mutex");
            panic!("hostile retirement-slot custody holder");
        })
        .join();
        assert!(poisoned.is_err());
    }

    fn take_retirement_guard(&self) -> (Option<SessionRetirementGuard>, bool) {
        let (mut task_slot, task_poisoned) = self.0.recover_mutex(&self.0.task);
        let guard = task_slot.take().map(|task| {
            SessionRetirementGuard::new(
                task,
                self.0.state.clone(),
                self.0.retirement_state_tx.clone(),
            )
        });
        drop(task_slot);
        let Some(mut guard) = guard else {
            return (None, task_poisoned);
        };
        let (mut slot, slot_poisoned) = self.0.recover_mutex(&self.0.retirement_slot);
        guard.install_slot(slot.take());
        drop(slot);
        (Some(guard), task_poisoned || slot_poisoned)
    }
}

impl core::fmt::Debug for AuthenticatedUserWs {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("AuthenticatedUserWs")
            .field("state", &self.state())
            .finish_non_exhaustive()
    }
}

impl Drop for AuthenticatedUserWsInner {
    fn drop(&mut self) {
        self.state.mark_disconnected();
        self.shutdown_tx.send_replace(true);
        let (mut task_slot, task_poisoned) = match self.task.lock() {
            Ok(task) => (task, false),
            Err(poison) => (poison.into_inner(), true),
        };
        if task_poisoned {
            self.fail_closed();
        }
        let task = task_slot.take();
        drop(task_slot);
        if let Some(task) = task {
            let (mut slot_guard, slot_poisoned) = match self.retirement_slot.lock() {
                Ok(slot) => (slot, false),
                Err(poison) => (poison.into_inner(), true),
            };
            if slot_poisoned {
                self.fail_closed();
            }
            let slot = slot_guard.take();
            drop(slot_guard);
            transfer_to_process_custodian(
                task,
                self.state.clone(),
                self.retirement_state_tx.clone(),
                slot,
            );
        }
    }
}
