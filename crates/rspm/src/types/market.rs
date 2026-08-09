/*
    Appellation: market <module>
    Created At: 2026.05.04:15:48:35
    Contrib: @FL03
*/
use alloc::{string::String, vec::Vec};

/// Unified market row for the QuestDB `polymarkets` table.
///
/// Single struct written via `QuestDBWriter::write_market` per §19 markets-consolidation
/// (v0.3.2-dev.3). Combines state-event + CLOB-identifier columns + all extended Polymarket
/// Gamma / FPMM metadata fields (#1114). Empty/zero values are emitted only when the writer
/// is told to skip optional columns at the ILP level (STRING columns conditional on non-empty;
/// BOOL / numeric columns always emitted).
///
/// # Field groups
///
/// 1. **State-event** (`ts`..`resolved_ts`) — sprint window lifecycle.
/// 2. **CLOB identifiers** (`condition_id`, `clob_token_yes`, `clob_token_no`) — SYMBOL
///    columns, omitted when empty.
/// 3. **Extended Gamma / FPMM** (`slug`..`liquidity`) — the 20 fields that were previously
///    always NULL in the `polymarkets` table (closes #1114 + #1142 + #1265).
#[derive(Clone, Debug, Default, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PolymarketQdbRow {
    // ── State-event columns ───────────────────────────────────────────
    /// Write timestamp: Unix epoch nanoseconds.
    pub ts: i64,
    /// Exchange identifier, e.g. `"polymarket"`.
    pub exchange: String,
    /// Market type, e.g. `"sprint"`.
    pub market_type: String,
    /// Underlying asset symbol, e.g. `"BTC"`.
    pub asset: String,
    /// Market identifier / slug, e.g. `"btc-updown-5m-1712520000"`.
    pub market_id: String,
    /// Sprint window scope: `"5m"`, `"15m"`, or `"4h"`.
    pub scope: String,
    /// Window open timestamp: Unix epoch nanoseconds.
    pub window_start: i64,
    /// Window close timestamp: Unix epoch nanoseconds.
    pub window_end: i64,
    /// Lifecycle state: `"open"` | `"closed"` | `"resolved"`.
    pub state: String,
    /// Human-readable market question text.
    pub question: String,
    /// Resolution outcome: `"UP"` | `"DOWN"` | `""` (empty until resolved).
    pub outcome: String,
    /// Resolution timestamp: Unix epoch nanoseconds. `0` = not yet resolved.
    pub resolved_ts: i64,
    // ── CLOB-identifier columns ────────────────────────────────────────
    /// Polymarket condition ID (hex string). Empty when not yet known.
    pub condition_id: String,
    /// CLOB token ID for the YES outcome. Empty when not yet known.
    pub clob_token_yes: String,
    /// CLOB token ID for the NO outcome. Empty when not yet known.
    pub clob_token_no: String,
    // ── Extended Gamma / FPMM columns (closes #1114 + #1142 + #1265) ──
    /// Human-readable URL slug from Gamma (may differ from `market_id` for
    /// non-sprint markets). Gamma key: `slug`.
    #[cfg_attr(feature = "serde", serde(default))]
    pub slug: String,
    /// Full market title text from Gamma. Gamma key: `title`.
    #[cfg_attr(feature = "serde", serde(default))]
    pub title: String,
    /// URL of the market icon image. Gamma key: `icon`.
    #[cfg_attr(feature = "serde", serde(default))]
    pub icon: String,
    /// Market category tag, e.g. `"Crypto"`. Gamma key: `category`.
    #[cfg_attr(feature = "serde", serde(default))]
    pub category: String,
    /// True when the market uses the negative-risk / complementary-outcome
    /// settlement model. Gamma key: `negRisk`.
    #[cfg_attr(feature = "serde", serde(default, rename = "negRisk"))]
    pub neg_risk: bool,
    /// Minimum tradeable price increment (in USDC). Gamma key: `orderPriceMinTickSize`.
    #[cfg_attr(feature = "serde", serde(default, rename = "orderPriceMinTickSize"))]
    pub minimum_tick_size: f64,
    /// Minimum order size in shares. Gamma key: `orderMinSize`.
    #[cfg_attr(feature = "serde", serde(default, rename = "orderMinSize"))]
    pub minimum_order_size: f64,
    /// Automated market maker contract address for this market. Gamma key: `fpmmAddress`.
    #[cfg_attr(feature = "serde", serde(default, rename = "fpmmAddress"))]
    pub fpmm_address: String,
    /// Fee paid by takers as a fraction of order value (e.g. `0.02` = 2 %).
    /// Gamma key: `makerBaseFee`.
    #[cfg_attr(feature = "serde", serde(default, rename = "makerBaseFee"))]
    pub maker_base_fee: f64,
    /// Fee paid by takers as a fraction of order value (e.g. `0.02` = 2 %).
    /// Gamma key: `takerBaseFee`.
    #[cfg_attr(feature = "serde", serde(default, rename = "takerBaseFee"))]
    pub taker_base_fee: f64,
    /// Market creation timestamp: Unix epoch nanoseconds. `0` when not provided.
    /// Gamma key: `createdAt`.
    #[cfg_attr(feature = "serde", serde(default, rename = "createdAt"))]
    pub created_at: i64,
    /// Market start date: Unix epoch nanoseconds. `0` when not provided.
    /// Gamma key: `startDate`.
    #[cfg_attr(feature = "serde", serde(default, rename = "startDate"))]
    pub start_date: i64,
    /// ISO 8601 end-date string, e.g. `"2026-06-01T16:00:00Z"`. Gamma key: `endDateIso`.
    #[cfg_attr(feature = "serde", serde(default, rename = "endDateIso"))]
    pub end_date_iso: String,
    /// True when the market is currently accepting new orders. Gamma key: `acceptingOrders`.
    #[cfg_attr(feature = "serde", serde(default, rename = "acceptingOrders"))]
    pub accepting_orders: bool,
    /// True when the market is live and can be traded. Gamma key: `active`.
    #[cfg_attr(feature = "serde", serde(default))]
    pub active: bool,
    /// True when the market has been resolved and closed. Gamma key: `closed`.
    #[cfg_attr(feature = "serde", serde(default))]
    pub closed: bool,
    /// True when the market has been archived (no new orders). Gamma key: `archived`.
    #[cfg_attr(feature = "serde", serde(default))]
    pub archived: bool,
    /// True when the market's automated market maker pool is funded. Gamma key: `funded`.
    #[cfg_attr(feature = "serde", serde(default))]
    pub funded: bool,
    /// Total cumulative trading volume in USDC. Gamma key: `volume`.
    #[cfg_attr(feature = "serde", serde(default))]
    pub volume: f64,
    /// Current liquidity depth in USDC. Gamma key: `liquidity`.
    #[cfg_attr(feature = "serde", serde(default))]
    pub liquidity: f64,
}

/// A Polymarket prediction market.
#[derive(Clone, Debug, Default, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct Market {
    /// URL slug identifying the market.
    pub slug: String,
    /// Human-readable question the market resolves.
    pub question: String,
    /// CLOB token IDs: index 0 = YES, index 1 = NO.
    pub clob_token_ids: Vec<String>,
    /// Current outcome prices as strings (Polymarket API format).
    pub outcome_prices: Vec<String>,
    /// Total trading volume.
    pub volume: f64,
    /// Whether the market is currently accepting trades.
    pub active: bool,
    /// Whether the market has been resolved/closed.
    pub closed: bool,
    /// ISO 8601 end date, if set.
    pub end_date_iso: Option<String>,
}

/// A point-in-time snapshot of a single prediction market.
///
/// This is the canonical axiom projection of a Polymarket market.  It is
/// constructed either directly (e.g. from `MarketSnapshot::new`) or via
/// `From<Market>` which projects the Gamma REST wire type into this shape.
///
/// # Relationship to `Market`
///
/// [`Market`] is the raw Gamma REST deserialization shape; it MUST NOT be
/// renamed or removed (Gamma client returns `Vec<Market>`).  Callers that
/// need typed price fields, a clean token-id pair, or a numeric `end_date`
/// should convert with [`From<Market>`] and work with `MarketSnapshot`.
#[derive(Clone, Debug, Default, PartialEq, PartialOrd)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Deserialize, serde::Serialize),
    serde(rename_all = "snake_case")
)]
#[repr(C)]
pub struct MarketSnapshot {
    /// Unique market slug (e.g., `"will-btc-exceed-100k-by-dec-2025"`).
    pub slug: String,
    /// Human-readable market question.
    pub question: String,
    /// Current YES token price (0.0 – 1.0).
    pub yes_price: f64,
    /// Current NO token price (0.0 – 1.0).
    pub no_price: f64,
    /// 24-hour trading volume in USD.
    pub volume_24h: f64,
    /// Market resolution date, if known (unix epoch seconds).
    pub end_date: Option<i64>,
    /// \[YES token id, NO token id\]
    pub token_ids: Vec<String>,
    /// Whether the market has closed (no new orders accepted).
    pub closed: bool,
}

impl From<Market> for MarketSnapshot {
    /// Project the Gamma REST wire type into the canonical axiom snapshot.
    ///
    /// Field mapping:
    /// - `slug` / `question` / `closed` — copied directly.
    /// - `yes_price` / `no_price` — parsed from `outcome_prices[0]` /
    ///   `outcome_prices[1]`; defaults to `0.5` / `0.5` on parse failure.
    /// - `volume_24h` — taken from `volume`.
    /// - `token_ids` — taken from `clob_token_ids` (index 0 = YES, 1 = NO).
    /// - `end_date` — attempt to parse `end_date_iso` as a Unix epoch seconds
    ///   integer string (Gamma sometimes returns a numeric string); `None` on
    ///   any parse failure or when the field is absent.
    fn from(m: Market) -> Self {
        let yes_price = m
            .outcome_prices
            .first()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.5_f64);
        let no_price = m
            .outcome_prices
            .get(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.5_f64);
        let end_date = m
            .end_date_iso
            .as_deref()
            .and_then(|s| s.parse::<i64>().ok());
        Self {
            slug: m.slug,
            question: m.question,
            yes_price,
            no_price,
            volume_24h: m.volume,
            end_date,
            token_ids: m.clob_token_ids,
            closed: m.closed,
        }
    }
}

#[cfg(all(test, feature = "json"))]
mod tests {
    use super::*;

    /// #2027 — `PolymarketQdbRow`'s serde `rename` attributes must match the
    /// LIVE Gamma keys (`orderMinSize` / `orderPriceMinTickSize`), not the
    /// old `minimumOrderSize` / `minimumTickSize` names (which never existed
    /// in the live API and silently deserialized to 0.0 on every row).
    #[test]
    fn polymarket_qdb_row_deserializes_live_min_size_keys() {
        // Non-`#[serde(default)]` state-event / CLOB-identifier columns must
        // be present for deserialization to succeed at all; only the two
        // Gamma-keyed fields under test are the point of this fixture.
        let v = serde_json::json!({
            "ts": 0,
            "exchange": "polymarket",
            "market_type": "sprint",
            "asset": "BTC",
            "market_id": "btc-updown-5m-1712520000",
            "scope": "5m",
            "window_start": 0,
            "window_end": 0,
            "state": "open",
            "question": "",
            "outcome": "",
            "resolved_ts": 0,
            "condition_id": "",
            "clob_token_yes": "",
            "clob_token_no": "",
            "orderMinSize": 5,
            "orderPriceMinTickSize": 0.01,
        });
        let row: PolymarketQdbRow = serde_json::from_value(v).expect("deserialize row");
        assert!(
            (row.minimum_order_size - 5.0).abs() < f64::EPSILON,
            "minimum_order_size not parsed from live `orderMinSize` key: got {}",
            row.minimum_order_size
        );
        assert!(
            (row.minimum_tick_size - 0.01).abs() < 1e-12,
            "minimum_tick_size not parsed from live `orderPriceMinTickSize` key: got {}",
            row.minimum_tick_size
        );
    }
}
