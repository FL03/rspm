/*
    Appellation: position <module>
    Contrib: @FL03
*/
//! Canonical `axiom.positions` row type and exchange position snapshot.
//! `PositionRow` mirrors the `axiom.positions` Supabase table and is the single
//! source of truth for position inserts across `bin/node` and any other
//! consumer that writes live or paper positions.
//! # Write path
//! Call [`PositionRow::insert`] (gated behind the `postgres` feature) after a
//! confirmed CLOB fill or paper-mode evaluation tick.
//! # Close path
//! Call [`PositionRow::close`] with the resolved `won` flag and realized `pnl`
//! when the resolver sweep marks a position as closed.

use crate::Side;
use alloc::string::String;

/// Open position on a venue — returned by `ExecutionAdapter::positions()`.
#[derive(Clone, Debug, PartialEq, PartialOrd)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
pub struct Position {
    /// Exchange or venue identifier, e.g. `"polymarket"`.
    pub venue: String,
    /// Outcome side held: [`Side::Yes`] or [`Side::No`].
    pub side: Side,
    /// Position size in USD notional.
    pub size: f64,
    /// Volume-weighted average fill price.
    pub avg_price: f64,
    /// Market slug this position belongs to.
    pub market_id: String,
    /// CLOB token ID for the held outcome token.
    pub token_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_constructs() {
        let p = Position {
            venue: "polymarket".into(),
            market_id: "m".into(),
            token_id: "tok".into(),
            side: Side::Yes,
            size: 10.0,
            avg_price: 0.62,
        };
        assert_eq!(p.side, Side::Yes);
        assert!((p.avg_price - 0.62).abs() < f64::EPSILON);
    }
}
