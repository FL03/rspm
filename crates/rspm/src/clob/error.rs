/*
    Appellation: error <module>
    Created At: 2026.08.08:08:24:50
    Contrib: @FL03
*/

pub type Clobresult<T> = core::result::Result<T, ClobOperationError>;

/// Closed, redacted failure classes for authenticated order submission and
/// cancellation. SDK response bodies, paths, order identifiers, and request
/// values are deliberately unrepresentable.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ClobOperationError {
    /// Caller-owned Live submission authority was absent or revoked before a
    /// network attempt began.
    #[error("CLOB submission authority is closed")]
    SubmissionRevoked,
    /// The SDK protocol contract no longer matches the exact balance and
    /// allowance authority recovered while Live admission was closed.
    #[error("CLOB protocol authority is revoked")]
    ProtocolAuthorityRevoked,
    /// Request bytes may have reached the venue, but no trusted acceptance or
    /// rejection response was received. Callers must classify the opportunity
    /// as indeterminate and complete exact account recovery before retrying.
    #[error("CLOB submission outcome is indeterminate")]
    PostSendIndeterminate,
    /// The request failed local validation before reaching the venue.
    #[error("CLOB request validation failed")]
    InvalidRequest,
    /// The venue rejected the configured authentication authority.
    #[error("CLOB authentication failed")]
    Authentication,
    /// A fill-or-kill style order found no executable counterpart.
    #[error("CLOB order found no executable match")]
    NoMatch,
    /// The venue rejected the operation without an executable no-match class.
    #[error("CLOB operation was rejected")]
    Rejected,
    /// The venue rate-limited the operation after bounded retries.
    #[error("CLOB operation was rate limited")]
    RateLimited {
        /// Parsed retry hint when the response exposed one safely.
        retry_after: Option<core::time::Duration>,
    },
    /// The network or remote service failed without a trusted response class.
    #[error("CLOB transport failed")]
    Transport,
}

impl ClobOperationError {
    #[must_use]
    pub const fn error_class(self) -> &'static str {
        match self {
            Self::SubmissionRevoked => "submission_revoked",
            Self::ProtocolAuthorityRevoked => "protocol_authority_revoked",
            Self::PostSendIndeterminate => "post_send_indeterminate",
            Self::InvalidRequest => "invalid_request",
            Self::Authentication => "authentication",
            Self::NoMatch => "no_match",
            Self::Rejected => "rejected",
            Self::RateLimited { .. } => "rate_limited",
            Self::Transport => "transport",
        }
    }
}
