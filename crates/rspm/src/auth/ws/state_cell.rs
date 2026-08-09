//! Stateful recovery, gap, durability, and readiness owner.

mod ledger;
mod recovery;

use core::sync::atomic::{AtomicBool, Ordering};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex, MutexGuard},
};

use tokio::sync::{OwnedSemaphorePermit, Semaphore, watch};

use super::{
    AuthenticatedCredentialAuthority, AuthenticatedCredentialIdentity,
    AuthenticatedUserAuthenticationState, AuthenticatedUserCatchUpFinalization,
    AuthenticatedUserConnectionState, AuthenticatedUserCounterExhaustion,
    AuthenticatedUserRawFrame, AuthenticatedUserRecoveryToken, AuthenticatedUserSubscriptionState,
    AuthenticatedUserWsState, MAX_DROPPED_RANGES, MAX_DURABLE_OUT_OF_ORDER, MAX_PENDING_RAW_FRAMES,
};

#[derive(Clone)]
pub(super) struct StateCell {
    state: Arc<Mutex<AuthenticatedUserWsState>>,
    authority_poisoned: Arc<AtomicBool>,
    pub(in crate::auth::ws) durable_out_of_order: Arc<Mutex<BTreeSet<u64>>>,
    pub(in crate::auth::ws) dropped_ranges: Arc<Mutex<Vec<(u64, u64)>>>,
    raw_frames: Arc<Mutex<BTreeMap<u64, PendingRawFrame>>>,
    raw_frame_capacity: Arc<Semaphore>,
    pub(super) state_tx: watch::Sender<AuthenticatedUserWsState>,
}

struct PendingRawFrame {
    evidence: AuthenticatedUserRawFrame,
    _capacity: OwnedSemaphorePermit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct TransportSequenceRange {
    pub(super) first: u64,
    pub(super) last: u64,
}

impl StateCell {
    fn fail_counter(
        state: &mut AuthenticatedUserWsState,
        exhaustion: AuthenticatedUserCounterExhaustion,
    ) {
        if state.counter_exhaustion.is_none() {
            state.counter_exhaustion = Some(exhaustion);
        }
        state.connection = AuthenticatedUserConnectionState::Disconnected;
        state.authentication = AuthenticatedUserAuthenticationState::Inactive;
        state.subscription = AuthenticatedUserSubscriptionState::Inactive;
        state.schema_gap = true;
        state.delivery_gap = true;
        state.evidence_gap = true;
        state.catch_up_generation = None;
        state.rest_proof_generation = None;
        state.rest_credential_authority = None;
        state.rest_proof_liveness_floor = None;
    }

    fn advance_gap_version(state: &mut AuthenticatedUserWsState) -> bool {
        let Some(next) = state.gap_version.checked_add(1) else {
            Self::fail_counter(state, AuthenticatedUserCounterExhaustion::GapVersion);
            return false;
        };
        state.gap_version = next;
        true
    }

    pub(super) fn next_durable_sequence(
        current: u64,
    ) -> Result<u64, AuthenticatedUserCounterExhaustion> {
        current
            .checked_add(1)
            .ok_or(AuthenticatedUserCounterExhaustion::DurableSequence)
    }

    #[cfg(test)]
    pub(super) fn new() -> Self {
        Self::new_with_identity(AuthenticatedCredentialIdentity::default())
    }

    pub(super) fn new_with_identity(credential_identity: AuthenticatedCredentialIdentity) -> Self {
        let initial = AuthenticatedUserWsState {
            credential_identity,
            ..AuthenticatedUserWsState::default()
        };
        let (state_tx, _) = watch::channel(initial);
        Self {
            state: Arc::new(Mutex::new(initial)),
            authority_poisoned: Arc::new(AtomicBool::new(false)),
            durable_out_of_order: Arc::new(Mutex::new(BTreeSet::new())),
            dropped_ranges: Arc::new(Mutex::new(Vec::new())),
            raw_frames: Arc::new(Mutex::new(BTreeMap::new())),
            raw_frame_capacity: Arc::new(Semaphore::new(MAX_PENDING_RAW_FRAMES)),
            state_tx,
        }
    }

    fn fail_closed_after_poison(&self, state: &mut AuthenticatedUserWsState) {
        let first_poison = !self.authority_poisoned.swap(true, Ordering::AcqRel);
        state.connection = AuthenticatedUserConnectionState::Disconnected;
        state.authentication = AuthenticatedUserAuthenticationState::Inactive;
        state.subscription = AuthenticatedUserSubscriptionState::Inactive;
        state.schema_gap = true;
        state.delivery_gap = true;
        state.consumer_closed = true;
        state.evidence_gap = true;
        state.authority_poisoned = true;
        state.catch_up_generation = None;
        state.rest_proof_generation = None;
        state.rest_credential_authority = None;
        state.rest_proof_liveness_floor = None;
        if first_poison {
            Self::advance_gap_version(state);
        }
    }

    fn publish_poisoned(&self) {
        let snapshot = match self.state.lock() {
            Ok(mut state) => {
                self.fail_closed_after_poison(&mut state);
                *state
            }
            Err(poison) => {
                let mut state = poison.into_inner();
                self.fail_closed_after_poison(&mut state);
                *state
            }
        };
        self.state_tx.send_replace(snapshot);
    }

    pub(super) fn mark_authority_poisoned(&self) {
        self.publish_poisoned();
    }

    fn lock(&self) -> Result<MutexGuard<'_, AuthenticatedUserWsState>, ()> {
        if self.authority_poisoned.load(Ordering::Acquire) {
            self.publish_poisoned();
            return Err(());
        }
        match self.state.lock() {
            Ok(state) => Ok(state),
            Err(poison) => {
                let mut state = poison.into_inner();
                self.fail_closed_after_poison(&mut state);
                let snapshot = *state;
                drop(state);
                self.state_tx.send_replace(snapshot);
                Err(())
            }
        }
    }

    pub(super) fn update(&self, update: impl FnOnce(&mut AuthenticatedUserWsState)) -> bool {
        let snapshot = {
            let Ok(mut state) = self.lock() else {
                return false;
            };
            update(&mut state);
            if self.authority_poisoned.load(Ordering::Acquire) {
                self.fail_closed_after_poison(&mut state);
            }
            *state
        };
        self.state_tx.send_replace(snapshot);
        !self.authority_poisoned.load(Ordering::Acquire)
    }

    pub(super) fn snapshot(&self) -> AuthenticatedUserWsState {
        match self.state.lock() {
            Ok(mut state) => {
                if self.authority_poisoned.load(Ordering::Acquire) {
                    self.fail_closed_after_poison(&mut state);
                    self.state_tx.send_replace(*state);
                }
                *state
            }
            Err(poison) => {
                let mut state = poison.into_inner();
                self.fail_closed_after_poison(&mut state);
                let snapshot = *state;
                drop(state);
                self.state_tx.send_replace(snapshot);
                snapshot
            }
        }
    }

    pub(super) fn mark_connecting(&self) {
        self.update(|state| {
            if state.counter_exhaustion.is_some() {
                return;
            }
            state.connection = AuthenticatedUserConnectionState::Connecting;
            state.authentication = AuthenticatedUserAuthenticationState::Inactive;
            state.subscription = AuthenticatedUserSubscriptionState::Inactive;
            state.rest_proof_generation = None;
            state.rest_credential_authority = None;
            state.rest_proof_liveness_floor = None;
        });
    }

    pub(super) fn try_mark_connected(&self) -> Result<u64, AuthenticatedUserCounterExhaustion> {
        let mut generation = None;
        let updated = self.update(|state| {
            if let Some(exhaustion) = state.counter_exhaustion {
                generation = Some(Err(exhaustion));
                return;
            }
            let Some(next) = state.generation.checked_add(1) else {
                Self::fail_counter(state, AuthenticatedUserCounterExhaustion::Generation);
                generation = Some(Err(AuthenticatedUserCounterExhaustion::Generation));
                return;
            };
            state.generation = next;
            state.connection = AuthenticatedUserConnectionState::Connected;
            state.authentication = AuthenticatedUserAuthenticationState::Inactive;
            state.subscription = AuthenticatedUserSubscriptionState::Inactive;
            state.catch_up_generation = None;
            state.rest_proof_generation = None;
            state.rest_credential_authority = None;
            state.rest_proof_liveness_floor = None;
            // Every new socket starts untrusted until a complete REST catch-up
            // attests this exact generation.
            state.evidence_gap = true;
            if !Self::advance_gap_version(state) {
                generation = Some(Err(AuthenticatedUserCounterExhaustion::GapVersion));
                return;
            }
            generation = Some(Ok(next));
        });
        if !updated {
            return Err(AuthenticatedUserCounterExhaustion::Generation);
        }
        generation.unwrap_or(Err(AuthenticatedUserCounterExhaustion::Generation))
    }

    #[cfg(test)]
    pub(super) fn mark_connected(&self) -> u64 {
        self.try_mark_connected()
            .expect("test socket authority counters are not exhausted")
    }

    pub(super) fn mark_subscription_written(&self, generation: u64) {
        self.update(|state| {
            if state.generation == generation
                && matches!(
                    state.connection,
                    AuthenticatedUserConnectionState::Connected
                )
            {
                state.authentication = AuthenticatedUserAuthenticationState::CredentialsSubmitted;
                state.subscription = AuthenticatedUserSubscriptionState::AwaitingServerProof;
            }
        });
    }

    pub(super) fn mark_server_liveness_proven(&self, generation: u64) {
        self.update(|state| {
            if state.generation == generation
                && matches!(
                    state.connection,
                    AuthenticatedUserConnectionState::Connected
                )
                && matches!(
                    state.authentication,
                    AuthenticatedUserAuthenticationState::CredentialsSubmitted
                )
                && matches!(
                    state.subscription,
                    AuthenticatedUserSubscriptionState::AwaitingServerProof
                        | AuthenticatedUserSubscriptionState::ServerResponsive
                )
            {
                let Some(next) = state.liveness_version.checked_add(1) else {
                    Self::fail_counter(state, AuthenticatedUserCounterExhaustion::Liveness);
                    return;
                };
                state.liveness_version = next;
                state.subscription = AuthenticatedUserSubscriptionState::ServerResponsive;
            }
        });
    }

    /// Record completion of authenticated REST recovery for this exact
    /// connected generation. This never promotes subscription authority by
    /// itself: a strictly later documented text PONG must prove that the same
    /// socket survived the REST interval.
    pub(super) fn mark_authenticated_rest_proven(
        &self,
        token: AuthenticatedUserRecoveryToken,
        credential_authority: AuthenticatedCredentialAuthority,
    ) -> Option<u64> {
        let mut liveness_floor = None;
        let updated = self.update(|state| {
            if state.recovery_token() == Some(token)
                && token.credential_identity() == credential_authority.identity()
                && matches!(
                    state.subscription,
                    AuthenticatedUserSubscriptionState::ServerResponsive
                )
            {
                state.rest_proof_generation = Some(token.generation());
                state.rest_credential_authority = Some(credential_authority);
                state.rest_proof_liveness_floor = Some(state.liveness_version);
                state.subscription = AuthenticatedUserSubscriptionState::AwaitingServerProof;
                liveness_floor = Some(state.liveness_version);
            }
        });
        updated.then_some(liveness_floor).flatten()
    }

    pub(super) fn mark_disconnected(&self) {
        self.update(|state| {
            state.connection = AuthenticatedUserConnectionState::Disconnected;
            state.authentication = AuthenticatedUserAuthenticationState::Inactive;
            state.subscription = AuthenticatedUserSubscriptionState::Inactive;
            state.catch_up_generation = None;
            state.rest_proof_generation = None;
            state.rest_credential_authority = None;
            state.rest_proof_liveness_floor = None;
            state.evidence_gap = true;
            Self::advance_gap_version(state);
        });
    }

    pub(super) fn mark_schema_gap(&self) {
        self.update(|state| {
            state.schema_gap = true;
            state.subscription = AuthenticatedUserSubscriptionState::AwaitingServerProof;
            state.rest_proof_generation = None;
            state.rest_credential_authority = None;
            state.rest_proof_liveness_floor = None;
            Self::advance_gap_version(state);
        });
    }

    pub(super) fn mark_delivery_gap(&self) {
        self.update(|state| {
            state.delivery_gap = true;
            state.subscription = AuthenticatedUserSubscriptionState::AwaitingServerProof;
            state.rest_proof_generation = None;
            state.rest_credential_authority = None;
            state.rest_proof_liveness_floor = None;
            Self::advance_gap_version(state);
        });
    }

    pub(super) fn mark_consumer_closed(&self) {
        self.update(|state| {
            state.consumer_closed = true;
            state.delivery_gap = true;
            state.subscription = AuthenticatedUserSubscriptionState::AwaitingServerProof;
            state.rest_proof_generation = None;
            state.rest_credential_authority = None;
            state.rest_proof_liveness_floor = None;
            Self::advance_gap_version(state);
        });
    }

    pub(super) fn mark_evidence_gap(&self) {
        self.update(|state| {
            state.evidence_gap = true;
            state.subscription = AuthenticatedUserSubscriptionState::AwaitingServerProof;
            state.rest_proof_generation = None;
            state.rest_credential_authority = None;
            state.rest_proof_liveness_floor = None;
            Self::advance_gap_version(state);
        });
    }

    pub(super) fn reserve_transport_sequences(
        &self,
        event_count: usize,
    ) -> Option<TransportSequenceRange> {
        let event_count = u64::try_from(event_count).ok()?;
        if event_count == 0 {
            return None;
        }
        let mut range = None;
        let updated = self.update(|state| {
            let Some(next) = state.transport_sequence.checked_add(1) else {
                Self::fail_counter(state, AuthenticatedUserCounterExhaustion::TransportSequence);
                return;
            };
            let Some(offset) = event_count.checked_sub(1) else {
                state.delivery_gap = true;
                Self::advance_gap_version(state);
                return;
            };
            let Some(last) = next.checked_add(offset) else {
                Self::fail_counter(state, AuthenticatedUserCounterExhaustion::TransportSequence);
                return;
            };
            range = Some(TransportSequenceRange { first: next, last });
            state.transport_sequence = last;
        });
        updated.then_some(range).flatten()
    }

    #[cfg(test)]
    fn poison_mutex_for_test<T: Send + 'static>(mutex: Arc<Mutex<T>>) {
        let poisoned = std::thread::spawn(move || {
            let _guard = mutex.lock().expect("test authority mutex starts healthy");
            panic!("hostile authority mutex holder");
        })
        .join();
        assert!(poisoned.is_err());
    }

    #[cfg(test)]
    pub(super) fn poison_state_for_test(&self) {
        Self::poison_mutex_for_test(Arc::clone(&self.state));
    }

    #[cfg(test)]
    pub(super) fn poison_raw_frames_for_test(&self) {
        Self::poison_mutex_for_test(Arc::clone(&self.raw_frames));
    }

    #[cfg(test)]
    pub(super) fn poison_durable_for_test(&self) {
        Self::poison_mutex_for_test(Arc::clone(&self.durable_out_of_order));
    }

    #[cfg(test)]
    pub(super) fn poison_dropped_for_test(&self) {
        Self::poison_mutex_for_test(Arc::clone(&self.dropped_ranges));
    }
}
