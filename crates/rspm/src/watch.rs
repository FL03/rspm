/*
    Appellation: watch <module>
    Contrib: @FL03
*/
//! [`BookWatcher`] — unified WebSocket book watcher for Polymarket CLOB markets.
//!
//! Provides ergonomic, feature-gated subscription methods over the raw
//! `polymarket_client_sdk_v2` WS client. PERSISTENCE-FREE: callers own all
//! loops and persistence sinks.
//!
//! # Feature gate
//!
//! Gated by `#[cfg(feature = "watch")]` which activates `clob` + `stream`.
//!
//! # Design
//!
//! [`BookWatcher`] is constructed with [`BookWatcher::new`] and provides:
//! - [`subscribe_orderbook`](BookWatcher::subscribe_orderbook) — L1 book-update stream (for FAK-trigger watcher)
//! - [`subscribe_trades`](BookWatcher::subscribe_trades) — last-trade-price stream (for trade monitoring)
//! - [`unsubscribe_orderbook`](BookWatcher::unsubscribe_orderbook) — explicit unsub cleanup
//!
//! Each method converts human-readable decimal token-ID strings to the U256
//! form the SDK expects (via [`crate::utils::parse_token_id`]) and maps SDK
//! errors to [`anyhow::Error`].
//!
//! # Internals
//!
//! The inner [`ws::Client`] is `Arc<ClientInner>` under the hood. All
//! subscriptions and unsubscriptions share the same connection via the Arc.
//! The broadcast receivers returned by the SDK are independently owned, so
//! the streams do not borrow from the `BookWatcher` — callers may drive them
//! without holding a `BookWatcher` reference.
use futures::{Stream, StreamExt as _};

use polymarket::clob::ws;

use crate::utils::parse_token_id;

/// A feature-gated WebSocket book watcher for Polymarket CLOB markets.
///
/// Wraps [`ws::Client`] to provide ergonomic, typed subscription methods
/// without exposing the SDK's internal U256 token-ID conversion or raw error
/// types. PERSISTENCE-FREE: callers own all event loops and sinks.
///
/// # Lifecycle
///
/// Construct once with [`BookWatcher::new`]. Re-use across multiple
/// subscribe/unsubscribe cycles; the inner WS client is stateful (it manages
/// the broadcast ring internally). On node shutdown, the caller simply drops
/// the instance.
///
/// The inner [`ws::Client`] is `Arc`-based: `clone()` gives a second handle
/// to the **same** connection, not a new one.
///
/// # Feature gate
///
/// Requires the `watch` feature (which activates `clob` and `stream`).
#[derive(Clone, Default)]
pub struct BookWatcher {
    ws: ws::Client,
}

impl BookWatcher {
    /// Construct a new [`BookWatcher`] with a default WS client.
    pub fn new() -> Self {
        Self::default()
    }

    /// Subscribe to L1 orderbook-update events for the given token IDs.
    ///
    /// Returns a stream of [`ws::BookUpdate`] values — one snapshot per CLOB
    /// book change for any subscribed token. Best-ask extraction and price
    /// comparison are the caller's responsibility.
    ///
    /// Decimal-string token IDs that fail to parse are silently dropped; the
    /// subscription proceeds with the IDs that did parse. If all IDs fail to
    /// parse the resulting `Vec` is empty and the SDK may return an error.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying WS subscription fails (e.g. network
    /// is unreachable or the SDK's broadcast ring is exhausted).
    pub fn subscribe_orderbook(
        &self,
        token_ids: &[String],
    ) -> anyhow::Result<impl Stream<Item = anyhow::Result<ws::BookUpdate>>> {
        let ids: Vec<polymarket::types::U256> = token_ids
            .iter()
            .filter_map(|id| parse_token_id(id).ok())
            .collect();
        let stream = self.ws.subscribe_orderbook(ids)?;
        Ok(stream.map(|r| r.map_err(anyhow::Error::from)))
    }

    /// Unsubscribe from orderbook events for the given token IDs.
    ///
    /// Should be called before re-subscribing with a new token set to avoid
    /// stale broadcast slots. Errors from the underlying SDK call (e.g. the
    /// channel was never created) are propagated to the caller.
    ///
    /// # Errors
    ///
    /// Returns an error if the SDK unsubscription call fails.
    pub fn unsubscribe_orderbook(&self, token_ids: &[String]) -> anyhow::Result<()> {
        let ids: Vec<polymarket::types::U256> = token_ids
            .iter()
            .filter_map(|id| parse_token_id(id).ok())
            .collect();
        self.ws
            .unsubscribe_orderbook(&ids)
            .map_err(anyhow::Error::from)
    }

    /// Subscribe to last-trade-price events for the given token IDs.
    ///
    /// Returns a stream of [`crate::types::ClobTrade`] values — one event per
    /// matched trade on any subscribed token. Use this for trade-monitoring
    /// and signal-enrichment workflows; for FAK-trigger watcher logic prefer
    /// [`subscribe_orderbook`](Self::subscribe_orderbook).
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying WS subscription fails.
    pub fn subscribe_trades(
        &self,
        token_ids: &[String],
    ) -> anyhow::Result<impl Stream<Item = anyhow::Result<crate::types::ClobTrade>>> {
        let ids: Vec<polymarket::types::U256> = token_ids
            .iter()
            .filter_map(|id| parse_token_id(id).ok())
            .collect();
        let stream = self.ws.subscribe_last_trade_price(ids)?;
        Ok(stream.map(|r| {
            r.map(crate::types::ClobTrade::from)
                .map_err(anyhow::Error::from)
        }))
    }
}

impl core::fmt::Debug for BookWatcher {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // ws::Client does not implement Debug; surface struct name only.
        f.debug_struct("BookWatcher").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ws::Client` has no `Debug` impl, so [`BookWatcher`]'s manual one must
    /// surface the struct name with an elided body and never render inner
    /// connection state. A `#[derive(Debug)]` (once the SDK grows one) or a
    /// hand-written impl that prints the socket/credential-bearing client
    /// would leak connection internals into every `tracing` line and panic
    /// message that formats a watcher.
    ///
    /// `BookWatcher::new()` is `Self::default()`, so this also witnesses that
    /// both constructors still build (no separate `default_constructs` test is
    /// needed — it would exercise the identical code path).
    #[test]
    fn book_watcher_new_constructs() {
        assert_eq!(format!("{:?}", BookWatcher::new()), "BookWatcher { .. }");
    }
}
