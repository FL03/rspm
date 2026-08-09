//! Owned authenticated Polymarket user-channel transport.
//!
//! The upstream SDK remains the order constructor/signer/submitter. Live
//! authority cannot depend on its lazy subscription or best-effort parser,
//! however, because either behavior can silently skip private account events.
//! This module therefore owns the user socket, authenticated subscription,
//! strict frame decoding, bounded delivery, and reconnect/catch-up generation.

use core::time::Duration;

pub const USER_WS_HOST: &str = "wss://ws-subscriptions-clob.polymarket.com";

pub(super) const EVENT_CHANNEL_CAPACITY: usize = 256;
pub(super) const MAX_DURABLE_OUT_OF_ORDER: usize = EVENT_CHANNEL_CAPACITY * 4;
pub(super) const MAX_DROPPED_RANGES: usize = 1;
pub(super) const MAX_PENDING_RAW_FRAMES: usize = EVENT_CHANNEL_CAPACITY * 4;
pub(super) const MAX_FRAME_BYTES: usize = 1024 * 1024;
pub(super) const SOCKET_CLOSE_TIMEOUT: Duration = Duration::from_millis(500);
pub(super) const SESSION_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(1);
pub(super) const RETIREMENT_RESPONSE_MARGIN: Duration = Duration::from_millis(500);

/// Production connection policy for the authenticated user channel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthenticatedUserWsConfig {
    pub connect_timeout: Duration,
    pub heartbeat_interval: Duration,
    pub heartbeat_timeout: Duration,
    pub initial_reconnect_delay: Duration,
    pub max_reconnect_delay: Duration,
    pub max_reconnect_attempts: Option<u32>,
}

impl Default for AuthenticatedUserWsConfig {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(15),
            heartbeat_interval: Duration::from_secs(10),
            heartbeat_timeout: Duration::from_secs(15),
            initial_reconnect_delay: Duration::from_secs(1),
            max_reconnect_delay: Duration::from_secs(60),
            max_reconnect_attempts: None,
        }
    }
}

impl AuthenticatedUserWsConfig {
    pub(super) fn is_valid(self) -> bool {
        !self.connect_timeout.is_zero()
            && !self.heartbeat_interval.is_zero()
            && self.heartbeat_timeout >= self.heartbeat_interval
            && !self.initial_reconnect_delay.is_zero()
            && self.max_reconnect_delay >= self.initial_reconnect_delay
            && self.max_reconnect_attempts != Some(0)
    }
}

/// Redacted user-channel failure class. Raw frames, credentials, headers, and
/// response-derived identifiers are deliberately absent.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum AuthenticatedUserWsError {
    #[error("invalid authenticated user-channel endpoint")]
    InvalidEndpoint,
    #[error("invalid authenticated user-channel configuration")]
    InvalidConfiguration,
    #[error("authenticated user-channel requires an active Tokio runtime")]
    RuntimeUnavailable,
    #[error("authenticated user-channel subscription could not be encoded")]
    SubscriptionEncoding,
    #[error("authenticated user-channel event receiver was already taken")]
    EventReceiverTaken,
    #[error("authenticated user-channel retirement capacity is exhausted")]
    RetirementCapacityExhausted,
    #[error("authenticated user-channel process custodian is unavailable")]
    RetirementCustodianUnavailable,
    #[error("authenticated user-channel authority custody is poisoned")]
    AuthorityPoisoned,
    #[error("authenticated user-channel frame failed strict schema validation")]
    FrameSchema,
}
