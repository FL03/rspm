//! Public authenticated user-channel state contract.

use super::{AuthenticatedCredentialAuthority, AuthenticatedCredentialIdentity};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AuthenticatedUserConnectionState {
    #[default]
    Disconnected,
    Connecting,
    Connected,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AuthenticatedUserAuthenticationState {
    #[default]
    Inactive,
    /// Credentials were submitted on this socket generation. This is not a
    /// server authentication acknowledgement; the protocol documents none.
    CredentialsSubmitted,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AuthenticatedUserSubscriptionState {
    #[default]
    Inactive,
    /// The subscription was written, but this generation has not answered the
    /// client's documented text `PING` with a corresponding text `PONG`.
    AwaitingServerProof,
    /// This generation has supplied documented server-liveness evidence.
    /// Exact authenticated REST catch-up remains independently required.
    ServerResponsive,
}

/// Permanent fail-closed reason for an exhausted monotonic authority counter.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum AuthenticatedUserCounterExhaustion {
    /// Socket-generation identity cannot advance without collision.
    #[error("authenticated socket generation exhausted")]
    Generation,
    /// Documented heartbeat proof identity cannot advance without collision.
    #[error("authenticated liveness version exhausted")]
    Liveness,
    /// Gap compare-and-set identity cannot advance without collision.
    #[error("authenticated gap version exhausted")]
    GapVersion,
    /// Transport event identity cannot advance without collision.
    #[error("authenticated transport sequence exhausted")]
    TransportSequence,
    /// Raw-frame identity cannot advance without collision.
    #[error("authenticated raw frame sequence exhausted")]
    RawFrameSequence,
    /// Durable transport frontier cannot advance without collision.
    #[error("authenticated durable sequence exhausted")]
    DurableSequence,
}

/// Exact state used by Live admission.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AuthenticatedUserWsState {
    /// Closed non-secret identity of the credential tuple submitted by this
    /// socket tree. The raw key, secret, and passphrase are never retained.
    pub credential_identity: AuthenticatedCredentialIdentity,
    /// Current socket connection lifecycle.
    pub connection: AuthenticatedUserConnectionState,
    /// Credential-submission lifecycle for the current socket.
    pub authentication: AuthenticatedUserAuthenticationState,
    /// Server-liveness proof lifecycle for the current socket.
    pub subscription: AuthenticatedUserSubscriptionState,
    /// Monotonic socket replacement generation.
    pub generation: u64,
    /// Monotonic count of documented text heartbeat responses accepted from
    /// the current pinned server connection tree. Engine recovery captures
    /// this value before authenticated REST and requires a strictly newer
    /// response before publishing Live authority.
    pub liveness_version: u64,
    /// Monotonic version of every schema, delivery, consumer, or evidence gap.
    /// A recovery may clear gaps only when this value still matches the token
    /// captured before the authenticated catch-up began.
    pub gap_version: u64,
    /// Last strictly decoded raw private event assigned a sequence while its
    /// bounded-queue capacity permit was already owned.
    pub transport_sequence: u64,
    /// Last raw private event successfully accepted into the bounded
    /// transport queue. Normal socket receipt makes this frontier advance in
    /// the same synchronous step as `transport_sequence`.
    pub enqueued_sequence: u64,
    /// Highest contiguous transport sequence acknowledged after its required
    /// durable/no-op lifecycle completed.
    pub durable_sequence: u64,
    /// Monotonic version of every retained raw private data frame.
    pub raw_frame_sequence: u64,
    /// Exact raw frames not yet acknowledged after durable storage.
    pub pending_raw_frame_count: usize,
    /// Socket generation whose complete REST catch-up is installed.
    pub catch_up_generation: Option<u64>,
    /// Generation whose authenticated REST recovery completed through its
    /// terminal cursor. This is recorded before the required post-REST PONG.
    pub rest_proof_generation: Option<u64>,
    /// Exact REST credential identity and recovery generation whose account
    /// traversal was completed for this socket generation.
    pub rest_credential_authority: Option<AuthenticatedCredentialAuthority>,
    /// Liveness frontier captured before REST began. The composite channel
    /// proof requires a strictly newer documented text PONG.
    pub rest_proof_liveness_floor: Option<u64>,
    /// Sticky proof that a retained frame failed strict schema decoding.
    pub schema_gap: bool,
    /// Sticky proof that derived event delivery lost authority.
    pub delivery_gap: bool,
    /// Sticky proof that the sole event consumer terminated.
    pub consumer_closed: bool,
    /// Sticky proof that exact account evidence is incomplete.
    pub evidence_gap: bool,
    /// Sticky proof that an authority-bearing synchronization primitive
    /// panicked while locked. No recovery may reopen this connection.
    pub authority_poisoned: bool,
    /// Permanent typed counter exhaustion. Recovery can never clear it.
    pub counter_exhaustion: Option<AuthenticatedUserCounterExhaustion>,
}

/// Compare-and-set token for one authenticated user-channel recovery.
///
/// The generation proves the socket is unchanged. `gap_version` proves no new
/// loss or malformed evidence landed while REST catch-up was in flight.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthenticatedUserRecoveryToken {
    generation: u64,
    gap_version: u64,
    credential_identity: AuthenticatedCredentialIdentity,
}

/// One-shot compare-and-set authority for publishing an authenticated REST
/// catch-up. Preparing this value never changes readiness. Committing it is
/// valid only while the socket state and durability frontiers remain exactly
/// those observed during preparation.
///
/// The fields are intentionally opaque. Callers may hold this token while
/// acquiring their own admission locks without keeping any user-channel mutex
/// locked, then consume it at the final socket-state linearization point.
#[derive(Debug, Eq, PartialEq)]
pub struct AuthenticatedUserCatchUpFinalization {
    pub(super) expected_state: AuthenticatedUserWsState,
    pub(super) next_state: AuthenticatedUserWsState,
    pub(super) expected_durable_out_of_order: Vec<u64>,
    pub(super) next_durable_out_of_order: Vec<u64>,
    pub(super) expected_dropped_ranges: Vec<(u64, u64)>,
    pub(super) next_dropped_ranges: Vec<(u64, u64)>,
}

impl AuthenticatedUserRecoveryToken {
    /// Construct a token from explicit field values for tests that must
    /// probe catch-up rejection with a token `recovery_token()` itself would
    /// never produce (for example, a stale or not-yet-ready generation).
    #[cfg(test)]
    pub(super) const fn for_test(
        generation: u64,
        gap_version: u64,
        credential_identity: AuthenticatedCredentialIdentity,
    ) -> Self {
        Self {
            generation,
            gap_version,
            credential_identity,
        }
    }

    #[must_use]
    pub const fn generation(self) -> u64 {
        self.generation
    }

    #[must_use]
    pub const fn gap_version(self) -> u64 {
        self.gap_version
    }

    /// Non-secret credential identity fixed for this socket tree.
    #[must_use]
    pub const fn credential_identity(self) -> AuthenticatedCredentialIdentity {
        self.credential_identity
    }
}

impl AuthenticatedUserWsState {
    #[must_use]
    pub fn channel_authority_intact(self) -> bool {
        matches!(self.connection, AuthenticatedUserConnectionState::Connected)
            && matches!(
                self.authentication,
                AuthenticatedUserAuthenticationState::CredentialsSubmitted
            )
            && matches!(
                self.subscription,
                AuthenticatedUserSubscriptionState::ServerResponsive
            )
            && self.generation != 0
            && self.catch_up_generation == Some(self.generation)
            && self.rest_credential_authority.is_some()
            && !self.schema_gap
            && !self.delivery_gap
            && !self.consumer_closed
            && !self.evidence_gap
            && !self.authority_poisoned
            && self.counter_exhaustion.is_none()
    }

    #[must_use]
    pub fn is_ready(self) -> bool {
        self.channel_authority_intact()
            && self.durable_sequence == self.transport_sequence
            && self.pending_raw_frame_count == 0
    }

    /// Identity of the authenticated session this socket is bound to, valid
    /// ACROSS the REST-proof window.
    ///
    /// [`Self::recovery_token`] answers a different question — "is this
    /// session open for recovery RIGHT NOW" — and is deliberately `None`
    /// while `subscription` sits at
    /// [`AuthenticatedUserSubscriptionState::AwaitingServerProof`]. That makes
    /// it unusable as a session-change guard DURING the REST-proof wait:
    /// `mark_authenticated_rest_proven` sets `AwaitingServerProof` on its
    /// success path, so a guard reading `recovery_token` inside that wait can
    /// only ever fail. This accessor answers the identity question alone.
    ///
    /// Identity is stable across the legitimate handshake because
    /// `mark_server_liveness_proven` — the only writer of `ServerResponsive` —
    /// touches neither `generation` nor `gap_version`. Every abort path moves
    /// one of them, and a disconnect additionally clears `connection` and
    /// resets `subscription`, so this still goes `None` on real session loss.
    #[must_use]
    pub fn recovery_identity(self) -> Option<AuthenticatedUserRecoveryToken> {
        if matches!(self.connection, AuthenticatedUserConnectionState::Connected)
            && matches!(
                self.authentication,
                AuthenticatedUserAuthenticationState::CredentialsSubmitted
            )
            && matches!(
                self.subscription,
                AuthenticatedUserSubscriptionState::AwaitingServerProof
                    | AuthenticatedUserSubscriptionState::ServerResponsive
            )
            && self.generation != 0
        {
            Some(AuthenticatedUserRecoveryToken {
                generation: self.generation,
                gap_version: self.gap_version,
                credential_identity: self.credential_identity,
            })
        } else {
            None
        }
    }

    /// Session identity PLUS proof that the session is open for recovery now.
    ///
    /// Defined in terms of [`Self::recovery_identity`] so the two can never
    /// drift apart. Semantics are unchanged and remain load-bearing: still
    /// `None` at `AwaitingServerProof`, which this module's tests assert
    /// directly and which every caller outside the REST-proof window relies on
    /// to mean "responsive", not merely "subscribed".
    #[must_use]
    pub fn recovery_token(self) -> Option<AuthenticatedUserRecoveryToken> {
        self.recovery_identity().filter(|_| {
            matches!(
                self.subscription,
                AuthenticatedUserSubscriptionState::ServerResponsive
            )
        })
    }
}
