/*
    Appellation: clob_trade <module>
    Created At: 2026.05.30
    Contrib: @FL03
*/

use alloc::string::String;

/// A single public taker-trade print from the Polymarket CLOB market WebSocket channel.
///
/// Sourced from `last_trade_price` events on the unauthenticated market channel
/// (`wss://ws-subscriptions-clob.polymarket.com/ws/market`). Each `ClobTrade`
/// represents one matched order (taker fill) with aggressor-side preserved.
///
/// # Design notes
///
/// - `aggressor_side` is LOAD-BEARING — it is never `None`, never inferred.
///   When the wire event lacks a side, the value is `"unknown"` (not omitted).
/// - `slug` defaults to `token_id` at ingest. The worker task enriches it to
///   the human-readable market slug before writing to QuestDB.
/// - `ingest_ts`, `sequence`, and `trade_id` are `None` from the WS feed;
///   they are filled at write time or left null in QuestDB.
/// - Fidelity-first: no prints are filtered or dropped at this layer.
///
/// # Invariants
///
/// - `price` is in [0.0, 1.0] — Polymarket outcome token price space.
/// - `size` is in shares (fractional allowed).
/// - `exchange_ts` is milliseconds since Unix epoch as reported by the exchange.
#[derive(Clone, Debug, Default, PartialEq, PartialOrd)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
#[repr(C)]
pub struct ClobTrade {
    /// Market slug (e.g. `"will-btc-close-above-100k-dec-2024"`).
    /// Defaults to `token_id` string at ingest; enriched by the worker before
    /// writing to QuestDB.
    pub slug: String,
    /// Outcome token ID (decimal U256 string).
    pub token_id: String,
    /// Market condition ID (hex B256 string, `0x`-prefixed). `None` when the
    /// exchange omits the field (uncommon; treat as stale in downstream logic).
    pub condition_id: Option<String>,
    /// Aggressor (taker) side: `"buy"` or `"sell"`. LOAD-BEARING — never `None`,
    /// never inferred. Set to `"unknown"` when the wire event lacks a side.
    pub aggressor_side: String,
    /// Fill price per token in \[0, 1\].
    pub price: f64,
    /// Fill size in shares.
    pub size: f64,
    /// Exchange-side timestamp (milliseconds since Unix epoch).
    pub exchange_ts: Option<i64>,
    /// Local ingest timestamp (milliseconds since Unix epoch). Set by the worker
    /// at write time; `None` when read back before the worker enriches the record.
    pub ingest_ts: Option<i64>,
    /// Sequence number for ordering within a stream. Not available from the
    /// `last_trade_price` WS feed; `None` until a future feed version supplies it.
    pub sequence: Option<i64>,
    /// Exchange-assigned trade ID. Not available from the `last_trade_price` WS
    /// feed; `None` until a richer feed endpoint surfaces it.
    pub trade_id: Option<String>,
}

impl ClobTrade {
    /// Returns `true` when the aggressor is a buyer (taker bought).
    #[inline]
    pub fn is_buy(&self) -> bool {
        self.aggressor_side == "buy"
    }

    /// Returns `true` when the aggressor is a seller (taker sold).
    #[inline]
    pub fn is_sell(&self) -> bool {
        self.aggressor_side == "sell"
    }
}

#[cfg(feature = "watch")]
impl From<polymarket::clob::ws::types::response::LastTradePrice> for ClobTrade {
    /// Convert a raw `LastTradePrice` WS event into a [`ClobTrade`].
    ///
    /// # Mapping
    ///
    /// | `LastTradePrice` field | `ClobTrade` field | Notes |
    /// |---|---|---|
    /// | `asset_id` | `token_id` / `slug` | decimal U256 string; slug defaults to same |
    /// | `market` | `condition_id` | `0x`-prefixed hex via `{:#x}` |
    /// | `side` | `aggressor_side` | `"buy"` / `"sell"` / `"unknown"` |
    /// | `price` | `price` | `Decimal → f64` |
    /// | `size` | `size` | `Option<Decimal> → f64`; `0.0` when absent |
    /// | `timestamp` | `exchange_ts` | already milliseconds |
    fn from(ltp: polymarket::clob::ws::types::response::LastTradePrice) -> Self {
        use rust_decimal::prelude::ToPrimitive as _;

        let token_id = ltp.asset_id.to_string();

        let aggressor_side = ltp
            .side
            .map(|s| match s {
                polymarket::clob::types::Side::Buy => "buy",
                polymarket::clob::types::Side::Sell => "sell",
                _ => "unknown",
            })
            .unwrap_or("unknown")
            .to_string();

        Self {
            slug: token_id.clone(), // worker enriches this before QDB write
            token_id,
            condition_id: Some(format!("{:#x}", ltp.market)),
            aggressor_side,
            price: ltp.price.to_f64().unwrap_or(0.0),
            size: ltp.size.and_then(|s| s.to_f64()).unwrap_or(0.0),
            exchange_ts: Some(ltp.timestamp),
            ingest_ts: None,
            sequence: None,
            trade_id: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString as _;

    fn sample() -> ClobTrade {
        ClobTrade {
            slug: "btc-above-100k".to_string(),
            token_id: "12345".to_string(),
            condition_id: Some("0xdeadbeef".to_string()),
            aggressor_side: "buy".to_string(),
            price: 0.72,
            size: 50.0,
            exchange_ts: Some(1_700_000_000_000),
            ingest_ts: None,
            sequence: None,
            trade_id: None,
        }
    }

    #[test]
    fn is_buy_true_for_buy_side() {
        assert!(sample().is_buy());
        assert!(!sample().is_sell());
    }

    #[test]
    fn is_sell_true_for_sell_side() {
        let mut t = sample();
        t.aggressor_side = "sell".to_string();
        assert!(t.is_sell());
        assert!(!t.is_buy());
    }

    #[test]
    fn unknown_side_is_neither_buy_nor_sell() {
        let mut t = sample();
        t.aggressor_side = "unknown".to_string();
        assert!(!t.is_buy());
        assert!(!t.is_sell());
    }

    #[test]
    fn default_is_valid() {
        let t = ClobTrade::default();
        assert!(t.slug.is_empty());
        assert!(t.token_id.is_empty());
        assert!(t.condition_id.is_none());
        assert_eq!(t.price, 0.0);
        assert_eq!(t.size, 0.0);
    }
}
