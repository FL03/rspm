//! Narrow read, cancellation, and private-stream operations.
use hashbrown::HashMap;

/// Owned cancellation evidence without an authenticated SDK client handle.
#[derive(Debug, Default, Eq, PartialEq)]
pub struct CancelOrdersOutcome {
    pub canceled: Vec<String>,
    pub not_canceled: HashMap<String, String>,
}
