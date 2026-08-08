/*
    Appellation: impl_market_snapshot <module>
    Created At: 2026.04.18:07:04:09
    Contrib: @FL03
*/
use crate::MarketSnapshot;
use alloc::string::ToString;
use alloc::vec::Vec;

impl MarketSnapshot {
    /// Maximum acceptable spread (in token price units) for a market to be
    /// considered liquid enough to trade.
    pub(crate) const LIQUIDITY_SPREAD_THRESHOLD: f64 = 0.10;
    /// Construct a new [`MarketSnapshot`] with the required fields.
    ///
    /// `token_ids` should be provided as `[yes_token_id, no_token_id]`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use axiom_core::types::MarketSnapshot;
    ///
    /// let snap = MarketSnapshot::new(
    ///     "btc-100k",
    ///     "Will BTC exceed 100k?",
    ///     0.62,
    ///     0.38,
    ///     50_000.0,
    /// );
    /// assert!((snap.spread() - 0.0).abs() < 1e-9); // no_price == 1 - yes_price assumed
    /// ```
    pub fn new(
        slug: impl ToString,
        question: impl ToString,
        yes_price: f64,
        no_price: f64,
        volume_24h: f64,
    ) -> Self {
        Self {
            slug: slug.to_string(),
            question: question.to_string(),
            yes_price,
            no_price,
            volume_24h,
            end_date: None,
            token_ids: Vec::new(),
            closed: false,
        }
    }

    /// The best available bid price — the YES token price.
    ///
    /// In a binary prediction market the YES price is the probability-implied
    /// bid for the YES outcome.
    pub fn best_bid(&self) -> f64 {
        self.yes_price
    }

    /// The best available ask price — the complement of the NO token price.
    ///
    /// On Polymarket `yes_price + no_price ≈ 1.0` (the spread is the gap).
    /// The effective ask for a YES position is `1.0 − no_price`.
    pub fn best_ask(&self) -> f64 {
        1.0 - self.no_price
    }

    /// Bid-ask spread: `best_ask − best_bid`.
    ///
    /// A tight spread (< 1 cent) indicates high liquidity.
    pub fn spread(&self) -> f64 {
        self.best_ask() - self.best_bid()
    }

    /// Mid-price between the best bid and ask.
    pub fn midprice(&self) -> f64 {
        (self.best_bid() + self.best_ask()) / 2.0
    }

    /// Returns `true` when the market spread is below the liquidity threshold.
    ///
    /// Markets with spreads above [`LIQUIDITY_SPREAD_THRESHOLD`](Self::LIQUIDITY_SPREAD_THRESHOLD) are considered
    /// illiquid and should not be traded.
    pub fn is_liquid(&self) -> bool {
        self.spread() < Self::LIQUIDITY_SPREAD_THRESHOLD
    }

    /// Returns `true` when the market is still open for trading.
    pub fn is_open(&self) -> bool {
        !self.closed
    }

    /// Returns `true` when a resolution date is known and the market is open.
    pub fn has_end_date(&self) -> bool {
        self.end_date.is_some()
    }

    /// Returns the YES token ID, if present.
    pub fn yes_token_id(&self) -> Option<&str> {
        self.token_ids.first().map(|s| s.as_str())
    }

    /// Returns the NO token ID, if present.
    pub fn no_token_id(&self) -> Option<&str> {
        self.token_ids.get(1).map(|s| s.as_str())
    }
}

impl core::fmt::Display for MarketSnapshot {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "MarketSnapshot({slug} YES={yes:.3} NO={no:.3} spread={spread:.4} vol24h={vol:.0})",
            slug = self.slug,
            yes = self.yes_price,
            no = self.no_price,
            spread = self.spread(),
            vol = self.volume_24h,
        )
    }
}
