/*
    Appellation: fill <module>
    Created At: 2026.03.17:19:46:41
    Contrib: @FL03
*/
use crate::Side;
use alloc::string::String;
use core::fmt;

/// The Polymarket CLOB taker fee rate applied to aggressive (market) fills.
///
/// Polymarket currently charges 2 bps (0.02%) on each matched trade.
const TAKER_FEE_RATE: f64 = 0.0002;

/// Result of an order submission.
#[derive(Clone, Debug, Default, PartialEq, PartialOrd)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
#[repr(C)]
pub struct Fill {
    /// Exchange-assigned order identifier.
    pub order_id: String,
    /// Order status string returned by the exchange (e.g., `"filled"`, `"cancelled"`).
    pub status: String,
    /// Asset or market slug this fill is for.
    pub asset: String,
    /// Which side of the market was traded.
    pub side: Side,
    /// Fill price per token.
    pub price: f64,
    /// Number of tokens filled.
    pub size: f64,
    /// Total cost of the fill in USD (`price × size`).
    pub cost: f64,
    /// Unix timestamp (seconds) when the fill was reported.
    pub timestamp: i64,
}

impl Fill {
    /// Construct a new [`Fill`] from its constituent fields.
    ///
    /// `cost` is computed automatically as `price × size`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use axiom_core::types::{Fill, Side};
    ///
    /// let fill = Fill::new("ord-001", "filled", "btc-sprint", Side::Yes, 0.60, 20.0, 1_700_000_000);
    /// assert_eq!(fill.cost, 12.0);
    /// ```
    pub fn new(
        order_id: impl Into<String>,
        status: impl Into<String>,
        asset: impl Into<String>,
        side: Side,
        price: f64,
        size: f64,
        timestamp: i64,
    ) -> Self {
        Self {
            order_id: order_id.into(),
            status: status.into(),
            asset: asset.into(),
            side,
            price,
            size,
            cost: price * size,
            timestamp,
        }
    }

    /// Returns `true` when the fill status is `"filled"` (fully matched).
    pub fn is_filled(&self) -> bool {
        self.status.eq_ignore_ascii_case("filled")
    }

    /// Returns `true` when this was a YES-side fill.
    pub fn is_buy(&self) -> bool {
        self.side == Side::Yes
    }

    /// Returns `true` when this was a NO-side fill.
    pub fn is_sell(&self) -> bool {
        self.side == Side::No
    }

    /// Gross profit or loss on the fill, assuming binary payout at 1.0.
    ///
    /// For a YES fill that wins: `(1.0 − price) × size`.
    /// For a YES fill that loses: `-price × size`.
    ///
    /// This method computes the *potential* PnL assuming resolution at 1.0.
    /// Pass the actual resolved payout if it differs.
    ///
    /// ```rust
    /// use axiom_core::types::{Fill, Side};
    ///
    /// let fill = Fill::new("o1", "filled", "mkt", Side::Yes, 0.60, 10.0, 0);
    /// // If YES wins: payout = 1.0 * 10 = 10, cost = 6, PnL = 4
    /// assert!((fill.profit_loss_if_win() - 4.0).abs() < 1e-9);
    /// ```
    pub fn profit_loss_if_win(&self) -> f64 {
        (1.0 - self.price) * self.size
    }

    /// Loss if the fill resolves against the position.
    ///
    /// For a YES fill that loses, the entire `cost` is forfeited.
    pub fn profit_loss_if_lose(&self) -> f64 {
        -self.cost
    }

    /// Approximate taker fee for this fill.
    ///
    /// Uses `TAKER_FEE_RATE` applied to the fill cost.
    pub fn fee(&self) -> f64 {
        self.cost * TAKER_FEE_RATE
    }

    /// Net return if the trade wins, after deducting the taker fee.
    ///
    /// `profit_loss_if_win() − fee()`
    pub fn net_return_if_win(&self) -> f64 {
        self.profit_loss_if_win() - self.fee()
    }

    /// Expected value of this fill given an estimated win probability `p_win`.
    ///
    /// `EV = p_win * profit_loss_if_win() + (1 - p_win) * profit_loss_if_lose() - fee()`
    pub fn expected_value(&self, p_win: f64) -> f64 {
        p_win * self.profit_loss_if_win() + (1.0 - p_win) * self.profit_loss_if_lose() - self.fee()
    }
}

impl fmt::Display for Fill {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Fill(id={id} {asset} {side} p={price:.4} sz={size:.2} cost={cost:.2} [{status}])",
            id = self.order_id,
            asset = self.asset,
            side = self.side,
            price = self.price,
            size = self.size,
            cost = self.cost,
            status = self.status,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_fill() -> Fill {
        Fill::new(
            "ord-001",
            "filled",
            "btc-sprint",
            Side::Yes,
            0.60,
            20.0,
            1_700_000_000,
        )
    }

    #[test]
    fn cost_is_price_times_size() {
        let fill = sample_fill();
        assert!((fill.cost - 12.0).abs() < 1e-9);
    }

    #[test]
    fn is_filled_check() {
        let fill = sample_fill();
        assert!(fill.is_filled());

        let partial = Fill::new("ord-002", "partial", "btc-sprint", Side::Yes, 0.55, 5.0, 0);
        assert!(!partial.is_filled());
    }

    #[test]
    fn buy_sell_flags() {
        let yes_fill = sample_fill();
        assert!(yes_fill.is_buy());
        assert!(!yes_fill.is_sell());

        let no_fill = Fill::new("ord-003", "filled", "btc-sprint", Side::No, 0.40, 10.0, 0);
        assert!(no_fill.is_sell());
        assert!(!no_fill.is_buy());
    }

    #[test]
    fn profit_loss_if_win() {
        // YES at 0.60, size 20: win PnL = (1 - 0.60) * 20 = 8.0
        let fill = sample_fill();
        assert!((fill.profit_loss_if_win() - 8.0).abs() < 1e-9);
    }

    #[test]
    fn profit_loss_if_lose_equals_negative_cost() {
        let fill = sample_fill();
        assert!((fill.profit_loss_if_lose() + fill.cost).abs() < 1e-9);
    }

    #[test]
    fn fee_is_nonnegative() {
        let fill = sample_fill();
        assert!(fill.fee() >= 0.0);
    }

    #[test]
    fn net_return_if_win_less_than_gross() {
        let fill = sample_fill();
        assert!(fill.net_return_if_win() <= fill.profit_loss_if_win());
    }

    #[test]
    fn expected_value_at_certainty() {
        let fill = sample_fill();
        // p_win = 1.0 → EV = gross PnL − fee
        let ev = fill.expected_value(1.0);
        assert!((ev - fill.net_return_if_win()).abs() < 1e-9);
    }

    #[test]
    fn display_format() {
        let s = sample_fill().to_string();
        assert!(s.contains("Fill("));
        assert!(s.contains("ord-001"));
        assert!(s.contains("YES"));
    }
}
