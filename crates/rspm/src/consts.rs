/*
    Appellation: consts <module>
    Created At: 2026.04.01
    Contrib: @FL03
*/

/// Default chain ID: Polygon PoS mainnet.
pub const DEFAULT_CHAIN_ID: u64 = 137;
/// The base url for the Gamma API markets endpoint
pub const GAMMA_MARKETS_URL: &str = "https://gamma-api.polymarket.com/markets";

/// Polymarket settlement fee: 2% of *gross profit* on a resolved binary
/// position.
///
/// Applied once, at settlement, to `payout − bet` (the gross profit), never
/// to the full payout or the stake.
pub const SETTLEMENT_FEE_RATE: f64 = 0.02;

/// FABLE-negotiated taker fee: 1.25% per side, charged at fill time when
/// crossing the book as taker.
///
/// Distinct from [`SETTLEMENT_FEE_RATE`] — this is levied on each side of a
/// fill, not on gross profit at resolution. Distinct from [`MAKER_FEE_RATE`]
/// — taker and maker are independently tunable even though they share a
/// value today. Canonical consumer: `axiom_attribution::resolver`
/// (`TAKER_FEE_RATE`).
pub const TAKER_FEE_RATE: f64 = 0.0125;

/// FABLE-negotiated maker fee: 1.25% per side, charged at fill time when
/// resting as maker.
///
/// Equal to [`TAKER_FEE_RATE`] today, but kept as its own canonical constant
/// because maker and taker fee schedules are independently tunable — FABLE
/// owns any future divergence.
pub const MAKER_FEE_RATE: f64 = 0.0125;
