/*
    Appellation: convert <module>
    Created At: 2026.08.08:07:07:46
    Contrib: @FL03
*/
use crate::types::PolymarketQdbRow;
/// Populate a [`PolymarketQdbRow`]'s extended Gamma / FPMM fields from a raw
/// Gamma API JSON value.
///
/// This covers the 20 columns that were permanently NULL before #1114:
/// `slug`, `title`, `icon`, `category`, `neg_risk`, `minimum_tick_size`,
/// `minimum_order_size`, `fpmm_address`, `maker_base_fee`, `taker_base_fee`,
/// `created_at`, `start_date`, `end_date_iso`, `accepting_orders`, `active`,
/// `closed`, `archived`, `funded`, `volume`, `liquidity`.
///
/// State-event fields (`ts`, `exchange`, `market_type`, `asset`, `market_id`,
/// `scope`, `window_start`, `window_end`, `state`, `question`, `outcome`,
/// `resolved_ts`) and CLOB-identifier fields (`condition_id`,
/// `clob_token_yes`, `clob_token_no`) are **not** overwritten — the caller
/// sets those from sprint-tracking / CLOB context.
///
/// All Gamma JSON keys are camelCase as returned by the Gamma API.  Missing
/// keys fall back to the field's `Default` value (empty string / 0 / false).
pub fn market_row_from_gamma_value(v: &serde_json::Value) -> PolymarketQdbRow {
    // Helper closures — borrow the JSON value by key.
    let str_field = |key: &str| -> String {
        v.get(key)
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string()
    };
    let bool_field = |key: &str| -> bool { v.get(key).and_then(|x| x.as_bool()).unwrap_or(false) };
    let f64_field = |key: &str| -> f64 {
        v.get(key)
            .and_then(|x| {
                x.as_f64()
                    .or_else(|| x.as_str().and_then(|s| s.parse().ok()))
            })
            .unwrap_or(0.0)
    };
    // Timestamps: Gamma returns ISO-8601 strings for dates; convert to Unix
    // nanoseconds where possible, otherwise store 0.
    let ts_from_iso = |key: &str| -> i64 {
        v.get(key)
            .and_then(|x| x.as_str())
            .and_then(|s| {
                // Nanosecond precision from seconds-epoch string, e.g. "1713974400".
                s.parse::<i64>()
                    .ok()
                    .map(|secs| secs.saturating_mul(1_000_000_000))
            })
            .unwrap_or(0)
    };

    PolymarketQdbRow {
        // State-event and CLOB-identifier fields left at Default — caller populates them.
        ts: 0,
        exchange: String::new(),
        market_type: String::new(),
        asset: String::new(),
        market_id: String::new(),
        scope: String::new(),
        window_start: 0,
        window_end: 0,
        state: String::new(),
        question: str_field("question"),
        outcome: String::new(),
        resolved_ts: 0,
        condition_id: str_field("conditionId"),
        clob_token_yes: String::new(),
        clob_token_no: String::new(),
        // ── Extended Gamma / FPMM fields ──────────────────────────────
        slug: str_field("slug"),
        title: str_field("title"),
        icon: str_field("icon"),
        category: str_field("category"),
        neg_risk: bool_field("negRisk"),
        minimum_tick_size: f64_field("orderPriceMinTickSize"),
        minimum_order_size: f64_field("orderMinSize"),
        fpmm_address: str_field("fpmmAddress"),
        // maker_base_fee / taker_base_fee: Gamma returns these as numeric
        // string or float; f64_field handles both.
        maker_base_fee: f64_field("makerBaseFee"),
        taker_base_fee: f64_field("takerBaseFee"),
        created_at: ts_from_iso("createdAt"),
        start_date: ts_from_iso("startDate"),
        end_date_iso: str_field("endDateIso"),
        accepting_orders: bool_field("acceptingOrders"),
        active: bool_field("active"),
        closed: bool_field("closed"),
        archived: bool_field("archived"),
        funded: bool_field("funded"),
        volume: f64_field("volume"),
        liquidity: f64_field("liquidity"),
    }
}
