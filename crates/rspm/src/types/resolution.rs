/*
    Appellation: resolve <module>
    Created At: 2026.03.17:19:46:59
    Contrib: @FL03
*/
use crate::Side;
use alloc::string::String;
use core::fmt;

/// Resolved market outcome recorded after the sprint closes.
#[derive(Clone, Debug, Default, PartialEq, PartialOrd)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
#[repr(C)]
pub struct Resolution {
    /// Prediction market identifier (e.g., Polymarket condition ID).
    pub market_id: String,
    /// The winning side as determined by the oracle resolution.
    pub winner: Side,
    /// Realised profit or loss for our position in USD.
    pub pnl: f64,
    /// Which side we held at resolution.
    pub our_side: Side,
    /// Our position size at resolution (tokens).
    pub our_size: f64,
    /// The fill price we paid when entering the position.
    pub entry_price: f64,
}

impl Resolution {
    /// Construct a new [`Resolution`].
    ///
    /// `pnl` is the realised net profit/loss after fees.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rspm::types::{Resolution, Side};
    ///
    /// let res = Resolution::new("mkt-001", Side::Yes, Side::Yes, 20.0, 0.62, 7.60);
    /// assert!(res.is_win());
    /// ```
    pub fn new(
        market_id: impl Into<String>,
        winner: Side,
        our_side: Side,
        our_size: f64,
        entry_price: f64,
        pnl: f64,
    ) -> Self {
        Self {
            market_id: market_id.into(),
            winner,
            our_side,
            our_size,
            entry_price,
            pnl,
        }
    }

    /// Returns `true` when the resolved winner matches our held side.
    pub fn is_win(&self) -> bool {
        self.winner == self.our_side
    }

    /// Returns `true` when the resolved winner is opposite to our held side.
    pub fn is_loss(&self) -> bool {
        !self.is_win()
    }

    /// Gross payout if we won: `(1.0 − entry_price) × our_size`.
    ///
    /// Returns `0.0` if the resolution was a loss.
    pub fn payout(&self) -> f64 {
        if self.is_win() {
            (1.0 - self.entry_price) * self.our_size
        } else {
            0.0
        }
    }

    /// Realised PnL for this resolution (may be negative on a loss).
    pub fn profit_loss(&self) -> f64 {
        self.pnl
    }

    /// Return on investment as a fraction: `pnl / (entry_price × our_size)`.
    ///
    /// Returns `0.0` when the position cost is zero.
    pub fn roi(&self) -> f64 {
        let cost = self.entry_price * self.our_size;
        if cost < f64::EPSILON {
            return 0.0;
        }
        self.pnl / cost
    }
}

impl fmt::Display for Resolution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let outcome = if self.is_win() { "WIN" } else { "LOSS" };
        write!(
            f,
            "Resolution({market} winner={winner} {outcome} pnl={pnl:.2})",
            market = self.market_id,
            winner = self.winner,
            outcome = outcome,
            pnl = self.pnl,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win() -> Resolution {
        // YES position, YES won, entry 0.62, size 20, pnl = (1-0.62)*20 = 7.60
        Resolution::new("m1", Side::Yes, Side::Yes, 20.0, 0.62, 7.60)
    }

    fn loss() -> Resolution {
        // YES position, NO won
        Resolution::new("m1", Side::No, Side::Yes, 20.0, 0.62, -12.40)
    }

    #[test]
    fn is_win_and_loss() {
        assert!(win().is_win());
        assert!(!win().is_loss());
        assert!(loss().is_loss());
        assert!(!loss().is_win());
    }

    #[test]
    fn payout_on_win() {
        // (1 - 0.62) * 20 = 7.6
        assert!((win().payout() - 7.60).abs() < 1e-9);
    }

    #[test]
    fn payout_on_loss_is_zero() {
        assert_eq!(loss().payout(), 0.0);
    }

    #[test]
    fn profit_loss_reflects_pnl_field() {
        assert!((win().profit_loss() - 7.60).abs() < 1e-9);
        assert!((loss().profit_loss() + 12.40).abs() < 1e-9);
    }

    #[test]
    fn roi_positive_on_win() {
        let roi = win().roi();
        // pnl=7.6, cost=0.62*20=12.4 → roi=7.6/12.4
        let expected = 7.60 / 12.40;
        assert!((roi - expected).abs() < 1e-9);
    }

    #[test]
    fn display_contains_outcome() {
        assert!(win().to_string().contains("WIN"));
        assert!(loss().to_string().contains("LOSS"));
    }
}
