//! Exact raw-frame receipt, retention, and transport sequencing contract.

use std::sync::Arc;

use serde::Deserialize as _;

use super::{
    AuthenticatedUserEvent, AuthenticatedUserWsError, MAX_FRAME_BYTES, wire::WireUserEventBatch,
};
use crate::{
    AuthenticatedPrivateFrameIdentityEncodingV1, AuthenticatedPrivateFrameIdentityGapV1,
    AuthenticatedPrivateFrameIdentityV1,
};

/// Exact wire encoding retained for one authenticated private data frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthenticatedUserFrameEncoding {
    /// UTF-8 WebSocket text frame.
    Text,
    /// Binary WebSocket data frame.
    Binary,
    /// Raw WebSocket frame surfaced by the transport implementation.
    Raw,
}

/// Typed reason a retained authenticated data frame could not become events.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthenticatedUserFrameGap {
    /// Text bytes failed the exact authenticated event schema.
    InvalidTextSchema,
    /// The private protocol does not define binary account events.
    UnsupportedBinary,
    /// The transport surfaced a raw data frame outside the protocol contract.
    UnsupportedRawFrame,
}

/// Process and clock identity captured at the transport receive boundary.
///
/// Wall time makes the observation externally legible. The process-relative
/// monotonic time prevents a wall-clock adjustment from reordering receipts,
/// while the random process generation prevents identity reuse after restart.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthenticatedUserFrameReceipt {
    process_generation: uuid::Uuid,
    wall_time_ns: i64,
    monotonic_ns: u64,
}

impl AuthenticatedUserFrameReceipt {
    pub(super) fn capture() -> Option<Self> {
        struct ProcessClock {
            generation: uuid::Uuid,
            origin: std::time::Instant,
        }

        static PROCESS_CLOCK: std::sync::OnceLock<ProcessClock> = std::sync::OnceLock::new();
        let clock = PROCESS_CLOCK.get_or_init(|| ProcessClock {
            generation: uuid::Uuid::new_v4(),
            origin: std::time::Instant::now(),
        });
        let wall_time_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .and_then(|duration| i64::try_from(duration.as_nanos()).ok())
            .filter(|wall_time_ns| *wall_time_ns > 0)?;
        let monotonic_ns = u64::try_from(clock.origin.elapsed().as_nanos()).ok()?;
        Some(Self {
            process_generation: clock.generation,
            wall_time_ns,
            monotonic_ns,
        })
    }

    /// Random non-secret identity generated once for this process.
    #[must_use]
    pub const fn process_generation(self) -> uuid::Uuid {
        self.process_generation
    }

    /// Positive Unix time in nanoseconds captured at socket receipt.
    #[must_use]
    pub const fn wall_time_ns(self) -> i64 {
        self.wall_time_ns
    }

    /// Nanoseconds elapsed since this process receipt clock was initialized.
    #[must_use]
    pub const fn monotonic_ns(self) -> u64 {
        self.monotonic_ns
    }

    #[cfg(any(test, feature = "test-utils"))]
    fn for_test(process_generation: &str, wall_time_ns: i64, monotonic_ns: u64) -> Option<Self> {
        let process_generation = uuid::Uuid::parse_str(process_generation).ok()?;
        (wall_time_ns > 0 && monotonic_ns <= i64::MAX as u64).then_some(Self {
            process_generation,
            wall_time_ns,
            monotonic_ns,
        })
    }
}

/// Exact bounded bytes retained until their durable storage is acknowledged.
#[derive(Clone, PartialEq)]
pub struct AuthenticatedUserRawFrame {
    frame_sequence: u64,
    first_transport_sequence: u64,
    last_transport_sequence: u64,
    socket_generation: u64,
    socket_gap_version: u64,
    receipt: AuthenticatedUserFrameReceipt,
    encoding: AuthenticatedUserFrameEncoding,
    gap: Option<AuthenticatedUserFrameGap>,
    bytes: Arc<[u8]>,
}

impl AuthenticatedUserRawFrame {
    /// Monotonic identity within one authenticated socket owner.
    #[must_use]
    pub const fn frame_sequence(&self) -> u64 {
        self.frame_sequence
    }

    /// First transport event sequence covered by these exact bytes.
    #[must_use]
    pub const fn first_transport_sequence(&self) -> u64 {
        self.first_transport_sequence
    }

    /// Last transport event sequence covered by these exact bytes.
    #[must_use]
    pub const fn last_transport_sequence(&self) -> u64 {
        self.last_transport_sequence
    }

    /// Socket generation that received the frame.
    #[must_use]
    pub const fn socket_generation(&self) -> u64 {
        self.socket_generation
    }

    /// Gap frontier captured synchronously at receipt.
    #[must_use]
    pub const fn socket_gap_version(&self) -> u64 {
        self.socket_gap_version
    }

    /// Receipt identity captured before frame decoding or queueing.
    #[must_use]
    pub const fn receipt(&self) -> AuthenticatedUserFrameReceipt {
        self.receipt
    }

    /// Original WebSocket data-frame encoding.
    #[must_use]
    pub const fn encoding(&self) -> AuthenticatedUserFrameEncoding {
        self.encoding
    }

    /// Typed schema/protocol gap, or `None` for an exact decoded frame.
    #[must_use]
    pub const fn gap(&self) -> Option<AuthenticatedUserFrameGap> {
        self.gap
    }

    /// Original frame payload bytes, byte-for-byte.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Compute the canonical v1 identity from the transport-owned receipt and
    /// bytes plus the downstream process-local owner pair.
    #[doc(hidden)]
    pub fn private_frame_evidence_key_v1(
        &self,
        owner_id: u64,
        session_id: u64,
    ) -> Result<String, &'static str> {
        let process_generation = self.receipt.process_generation.to_string();
        AuthenticatedPrivateFrameIdentityV1 {
            owner_id,
            session_id,
            process_generation: &process_generation,
            receipt_wall_time_ns: self.receipt.wall_time_ns,
            receipt_monotonic_ns: self.receipt.monotonic_ns,
            frame_sequence: self.frame_sequence,
            first_transport_sequence: self.first_transport_sequence,
            last_transport_sequence: self.last_transport_sequence,
            socket_generation: self.socket_generation,
            socket_gap_version: self.socket_gap_version,
            encoding: match self.encoding {
                AuthenticatedUserFrameEncoding::Text => {
                    AuthenticatedPrivateFrameIdentityEncodingV1::Text
                }
                AuthenticatedUserFrameEncoding::Binary => {
                    AuthenticatedPrivateFrameIdentityEncodingV1::Binary
                }
                AuthenticatedUserFrameEncoding::Raw => {
                    AuthenticatedPrivateFrameIdentityEncodingV1::Raw
                }
            },
            gap: self.gap.map(|gap| match gap {
                AuthenticatedUserFrameGap::InvalidTextSchema => {
                    AuthenticatedPrivateFrameIdentityGapV1::InvalidTextSchema
                }
                AuthenticatedUserFrameGap::UnsupportedBinary => {
                    AuthenticatedPrivateFrameIdentityGapV1::UnsupportedBinary
                }
                AuthenticatedUserFrameGap::UnsupportedRawFrame => {
                    AuthenticatedPrivateFrameIdentityGapV1::UnsupportedRawFrame
                }
            }),
            payload: &self.bytes,
        }
        .evidence_key()
    }
}

impl core::fmt::Debug for AuthenticatedUserRawFrame {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("AuthenticatedUserRawFrame")
            .field("frame_sequence", &self.frame_sequence)
            .field("first_transport_sequence", &self.first_transport_sequence)
            .field("last_transport_sequence", &self.last_transport_sequence)
            .field("socket_generation", &self.socket_generation)
            .field("socket_gap_version", &self.socket_gap_version)
            .field("receipt", &self.receipt)
            .field("encoding", &self.encoding)
            .field("gap", &self.gap)
            .field("byte_len", &self.bytes.len())
            .finish_non_exhaustive()
    }
}

/// One exact raw frame plus its all-or-nothing decoded event projection.
#[derive(Clone, PartialEq)]
pub struct AuthenticatedUserEventBatch {
    frame_sequence: u64,
    first_transport_sequence: u64,
    socket_generation: u64,
    socket_gap_version: u64,
    receipt: Option<AuthenticatedUserFrameReceipt>,
    encoding: AuthenticatedUserFrameEncoding,
    frame_gap: Option<AuthenticatedUserFrameGap>,
    raw_frame: Arc<[u8]>,
    events: Vec<AuthenticatedUserEvent>,
}

impl Default for AuthenticatedUserEventBatch {
    fn default() -> Self {
        Self {
            frame_sequence: 0,
            first_transport_sequence: 0,
            socket_generation: 0,
            socket_gap_version: 0,
            receipt: None,
            encoding: AuthenticatedUserFrameEncoding::Text,
            frame_gap: None,
            raw_frame: Arc::from([]),
            events: Vec::new(),
        }
    }
}

impl core::fmt::Debug for AuthenticatedUserEventBatch {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("AuthenticatedUserEventBatch")
            .field("frame_sequence", &self.frame_sequence)
            .field("first_transport_sequence", &self.first_transport_sequence)
            .field("socket_generation", &self.socket_generation)
            .field("socket_gap_version", &self.socket_gap_version)
            .field("receipt", &self.receipt)
            .field("encoding", &self.encoding)
            .field("frame_gap", &self.frame_gap)
            .field("raw_byte_len", &self.raw_frame.len())
            .field("event_count", &self.events.len())
            .finish_non_exhaustive()
    }
}

impl AuthenticatedUserEventBatch {
    pub fn decode_frame(frame: &[u8]) -> Result<Self, AuthenticatedUserWsError> {
        if frame.is_empty() || frame.len() > MAX_FRAME_BYTES {
            return Err(AuthenticatedUserWsError::FrameSchema);
        }
        let mut deserializer = serde_json::Deserializer::from_slice(frame);
        let wire = WireUserEventBatch::deserialize(&mut deserializer)
            .map_err(|_| AuthenticatedUserWsError::FrameSchema)?;
        deserializer
            .end()
            .map_err(|_| AuthenticatedUserWsError::FrameSchema)?;
        let events = wire
            .0
            .into_iter()
            .map(AuthenticatedUserEvent::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            frame_sequence: 0,
            first_transport_sequence: 0,
            socket_generation: 0,
            socket_gap_version: 0,
            receipt: None,
            encoding: AuthenticatedUserFrameEncoding::Text,
            frame_gap: None,
            raw_frame: Arc::from(frame),
            events,
        })
    }

    pub(super) fn capture_text_frame(frame: &[u8]) -> Self {
        Self::decode_frame(frame).unwrap_or_else(|_| Self {
            encoding: AuthenticatedUserFrameEncoding::Text,
            frame_gap: Some(AuthenticatedUserFrameGap::InvalidTextSchema),
            raw_frame: Arc::from(frame),
            ..Self::default()
        })
    }

    pub(super) fn capture_binary_frame(frame: &[u8]) -> Self {
        Self {
            encoding: AuthenticatedUserFrameEncoding::Binary,
            frame_gap: Some(AuthenticatedUserFrameGap::UnsupportedBinary),
            raw_frame: Arc::from(frame),
            ..Self::default()
        }
    }

    pub(super) fn capture_raw_frame(frame: &[u8]) -> Self {
        Self {
            encoding: AuthenticatedUserFrameEncoding::Raw,
            frame_gap: Some(AuthenticatedUserFrameGap::UnsupportedRawFrame),
            raw_frame: Arc::from(frame),
            ..Self::default()
        }
    }

    #[must_use]
    pub fn as_slice(&self) -> &[AuthenticatedUserEvent] {
        &self.events
    }

    #[must_use]
    pub fn into_events(self) -> Vec<AuthenticatedUserEvent> {
        self.events
    }

    #[must_use]
    pub const fn frame_gap(&self) -> Option<AuthenticatedUserFrameGap> {
        self.frame_gap
    }

    pub(super) fn sequence_width(&self) -> u64 {
        u64::try_from(self.events.len().max(1))
            .expect("bounded authenticated frame event count fits u64")
    }

    /// Clone the exact raw evidence retained for this queued frame.
    #[must_use]
    pub fn raw_evidence(&self) -> AuthenticatedUserRawFrame {
        let receipt = self
            .receipt
            .expect("transport context always installs an authenticated frame receipt");
        let last_transport_sequence = self
            .first_transport_sequence
            .checked_add(self.sequence_width() - 1)
            .expect("reserved authenticated transport range is valid");
        AuthenticatedUserRawFrame {
            frame_sequence: self.frame_sequence,
            first_transport_sequence: self.first_transport_sequence,
            last_transport_sequence,
            socket_generation: self.socket_generation,
            socket_gap_version: self.socket_gap_version,
            receipt,
            encoding: self.encoding,
            gap: self.frame_gap,
            bytes: Arc::clone(&self.raw_frame),
        }
    }

    pub(super) fn with_transport_context(
        mut self,
        receipt: AuthenticatedUserFrameReceipt,
        frame_sequence: u64,
        sequence: u64,
        socket_generation: u64,
        socket_gap_version: u64,
    ) -> Self {
        self.frame_sequence = frame_sequence;
        self.first_transport_sequence = sequence;
        self.socket_generation = socket_generation;
        self.socket_gap_version = socket_gap_version;
        self.receipt = Some(receipt);
        self
    }

    /// Install deterministic transport metadata for cross-crate protocol
    /// tests. Production connection code owns the same transformation through
    /// its private method above.
    #[cfg(any(test, feature = "test-utils"))]
    #[doc(hidden)]
    #[must_use]
    pub fn with_transport_context_for_test(
        self,
        frame_sequence: u64,
        sequence: u64,
        socket_generation: u64,
        socket_gap_version: u64,
    ) -> Self {
        let wall_time_ns = i64::try_from(frame_sequence)
            .ok()
            .and_then(|value| value.checked_add(1_700_000_000_000_000_000))
            .expect("test authenticated receipt wall time fits i64");
        let receipt = AuthenticatedUserFrameReceipt::for_test(
            "00000000-0000-4000-8000-000000000001",
            wall_time_ns,
            frame_sequence,
        )
        .expect("fixed test authenticated receipt is valid");
        self.with_transport_context(
            receipt,
            frame_sequence,
            sequence,
            socket_generation,
            socket_gap_version,
        )
    }

    /// Override deterministic receipt identity for cross-process and timing
    /// protocol tests. Production receipt construction is transport-owned.
    #[cfg(any(test, feature = "test-utils"))]
    #[doc(hidden)]
    #[must_use]
    pub fn with_transport_receipt_for_test(
        mut self,
        process_generation: &str,
        wall_time_ns: i64,
        monotonic_ns: u64,
    ) -> Option<Self> {
        self.receipt = Some(AuthenticatedUserFrameReceipt::for_test(
            process_generation,
            wall_time_ns,
            monotonic_ns,
        )?);
        Some(self)
    }

    /// Consume the batch as transport-sequenced raw private events.
    ///
    /// The socket generation is frozen when the frame is enqueued. Consumers
    /// must never sample the connection's current generation at dequeue time:
    /// an old queued frame can outlive a reconnect.
    #[must_use]
    pub fn into_sequenced_events(self) -> Vec<(u64, u64, u64, AuthenticatedUserEvent)> {
        self.events
            .into_iter()
            .enumerate()
            .map(|(offset, event)| {
                let offset = u64::try_from(offset)
                    .expect("bounded authenticated frame event offset fits u64");
                let sequence = self
                    .first_transport_sequence
                    .checked_add(offset)
                    .expect("reserved authenticated transport sequence is valid");
                (
                    sequence,
                    self.socket_generation,
                    self.socket_gap_version,
                    event,
                )
            })
            .collect()
    }
}
