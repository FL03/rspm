/*
    appellation: error <module>
    authors: @FL03
*/
//! this module defines the [`Error`] type for the `gvf` framework, enumerating various
//! errors types that commonly occur within the framework.
#[cfg(feature = "alloc")]
use alloc::{boxed::Box, string::String};

/// a type alias for [`Result`](core::result::Result) that uses the custom [`Error`] type
pub type Result<T = ()> = core::result::Result<T, Error>;

/// The [`Error`] type for the `gvf` framework, enumerating various error types that commonly
/// occur within the framework.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[cfg(feature = "alloc")]
    #[error("Unknown error: {0}")]
    UnknownError(String),
    #[cfg(feature = "alloc")]
    #[error(transparent)]
    BoxError(#[from] Box<dyn core::error::Error + Send + Sync>),
    #[error(transparent)]
    FmtError(#[from] core::fmt::Error),
    #[cfg(feature = "json")]
    #[error(transparent)]
    JsonError(#[from] serde_json::Error),
    #[cfg(feature = "std")]
    #[error(transparent)]
    IOError(#[from] std::io::Error),
}

#[cfg(feature = "alloc")]
impl From<String> for Error {
    fn from(value: String) -> Self {
        Error::UnknownError(value)
    }
}

#[cfg(feature = "alloc")]
impl From<&str> for Error {
    fn from(value: &str) -> Self {
        Error::UnknownError(String::from(value))
    }
}
