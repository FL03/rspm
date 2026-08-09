/*
    Appellation: error <module>
    Contrib: @FL03
*/
#[cfg(feature = "alloc")]
use alloc::{
    boxed::Box,
    string::{String, ToString},
};
/// A type alias for a [`Result`](core::result::Result) with an error type of [`Error`]
pub type Result<T> = core::result::Result<T, Error>;

/// Errors produced by the Polymarket client.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("Unable to parse the given private key")]
    InvalidPrivateKey,
    #[error("invalid or unsafe CLOB configuration")]
    InvalidClobConfiguration,
    #[error("submission controller identity space exhausted")]
    SubmissionControllerIdentityExhausted,
    #[error("not found")]
    NotFound,
    /// The CLOB responded 429 (Too Many Requests) - the structured submit
    /// error for #2056/#2057 pre-flip execution hardening. `retry_after`, when
    /// known, is the wait hint recovered from the response; `None` when the
    /// vendored `polymarket_client_sdk_v2` error surface did not expose one
    /// (it does not forward response headers - see
    /// [`crate::retry::classify_clob_error`]'s doc for the full limitation and
    /// the body-embedded-hint fallback).
    #[error("rate limited by CLOB (retry_after={retry_after:?})")]
    RateLimited {
        /// Parsed wait hint, or `None` when no hint could be recovered.
        retry_after: Option<core::time::Duration>,
    },
    // conditional
    #[cfg(feature = "alloc")]
    #[error("HTTP error {status}: {message}")]
    Http { status: u16, message: String },
    #[cfg(feature = "alloc")]
    #[error("invalid address: {0}")]
    InvalidAddress(String),
    #[cfg(feature = "alloc")]
    #[error("invalid parameter: {0}")]
    InvalidParam(String),
    #[cfg(feature = "alloc")]
    #[error("Unable to parse the given token id ({0})")]
    TokenId(String),
    #[cfg(feature = "alloc")]
    #[error("An invalid version of the protocol was attempted {0}")]
    InvalidProtocolVersion(String),
    #[cfg(feature = "alloc")]
    #[error("Unable to parse the given U256 value of {0}")]
    U256ParseError(String),
    // external errors
    #[error(transparent)]
    AddrParseError(#[from] core::net::AddrParseError),
    #[cfg(feature = "clob")]
    #[error(transparent)]
    AuthEndpointError(#[from] crate::auth::AuthenticatedEndpointError),
    #[error(transparent)]
    Infallible(#[from] core::convert::Infallible),
    #[cfg(feature = "json")]
    #[error(transparent)]
    JsonError(#[from] serde_json::Error),
    #[cfg(feature = "reqwest")]
    #[error(transparent)]
    ReqwestError(#[from] reqwest::Error),
    #[cfg(feature = "std")]
    #[error(transparent)]
    IoError(#[from] std::io::Error),
    #[cfg(feature = "sdk")]
    #[error(transparent)]
    PolymarketError(#[from] polymarket::error::Error),
    #[cfg(feature = "std")]
    #[error(transparent)]
    VarError(#[from] std::env::VarError),
    // catch-all
    #[cfg(feature = "alloc")]
    #[error(transparent)]
    AnyError(#[from] anyhow::Error),
    #[cfg(feature = "alloc")]
    #[error(transparent)]
    BoxError(Box<dyn core::error::Error + Send + Sync + 'static>),
    #[cfg(feature = "alloc")]
    #[error("{0}")]
    Unknown(String),
}

#[cfg(feature = "alloc")]
impl Error {
    pub fn http(status: u16, message: impl ToString) -> Self {
        Self::Http {
            status,
            message: message.to_string(),
        }
    }

    pub fn invalid<E>(msg: E) -> Self
    where
        E: ToString,
    {
        Self::InvalidParam(msg.to_string())
    }
}

impl Error {
    /// Construct the [`Error::RateLimited`] variant.
    ///
    /// Unconditional (unlike [`Error::http`]/[`Error::invalid`] above) -
    /// `Option<Duration>` needs neither `alloc` nor `std`, so this
    /// constructor is available in every build configuration.
    pub fn rate_limited(retry_after: Option<core::time::Duration>) -> Self {
        Self::RateLimited { retry_after }
    }
}
