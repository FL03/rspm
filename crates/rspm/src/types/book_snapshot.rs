/*
    Appellation: book <module>
    Created At: 2026.05.04:16:09:15
    Contrib: @FL03
*/
#[cfg(feature = "alloc")]
use alloc::string::String;

#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
pub struct NamedBookSnapshot<T> {
    pub(crate) slug: String,
    #[cfg_attr(feature = "serde", serde(flatten))]
    pub(crate) snapshot: BookSnapshot<T>,
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
pub struct AskBid<T> {
    pub ask: T,
    pub bid: T,
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
pub struct BookSnapshot<T = f64> {
    /// Best bid for the YES token.
    pub yes_bid: T,
    /// Best ask for the YES token.
    pub yes_ask: T,
    /// Best bid for the NO token.
    pub no_bid: T,
    /// Best ask for the NO token.
    pub no_ask: T,
    /// Unix nanosecond timestamp of the most recent book change.
    #[cfg_attr(
        feature = "serde",
        serde(alias = "last_change_ts_ns", alias = "ts", alias = "timeStamp")
    )]
    pub timestamp: i64,
}

impl<T> BookSnapshot<T> {
    /// Construct a `BookSnapshot` with the given prices and last-change timestamp.
    pub const fn new(yes_bid: T, yes_ask: T, no_bid: T, no_ask: T, timestamp: i64) -> Self {
        Self {
            yes_bid,
            yes_ask,
            no_bid,
            no_ask,
            timestamp,
        }
    }
    #[cfg(feature = "std")]
    pub fn now(yes_bid: T, yes_ask: T, no_bid: T, no_ask: T) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as i64)
            .unwrap_or(0);
        Self::new(yes_bid, yes_ask, no_bid, no_ask, now)
    }
    /// Construct a snapshot whose `last_change_age_ms` will report ≈`age_ms`.
    ///
    /// Computes `timestamp` as `now − age_ms` so the staleness accessor
    /// returns the requested age. The `_timestamp` argument is kept for
    /// signature parity with test fixtures and is ignored — the deterministic
    /// age-derived stamp is authoritative. Std-only.
    #[cfg(feature = "std")]
    pub fn new_with_age(yes_bid: T, yes_ask: T, no_bid: T, no_ask: T, age_ms: u64) -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as i64)
            .unwrap_or(0);
        let ts = now - (age_ms as i64) * 1_000_000;
        Self::new(yes_bid, yes_ask, no_bid, no_ask, ts)
    }
    /// returns a copy of the snapshot timestamp
    pub const fn timestamp(&self) -> i64 {
        self.timestamp
    }
    /// consumes the current instance to create another with the given timestamp
    pub fn with_timestamp(self, timestamp: i64) -> Self {
        Self { timestamp, ..self }
    }

    /// Mid-price of the YES side: `(yes_bid + yes_ask) / 2`.
    pub fn yes_mid(&self) -> T
    where
        T: Copy
            + core::ops::Add<T, Output = T>
            + core::ops::Div<T, Output = T>
            + num_traits::FromPrimitive,
    {
        (self.yes_bid + self.yes_ask) / <T>::from_u8(2).unwrap()
    }

    /// Mid-price of the YES side: `(yes_bid + yes_ask) / 2`.
    pub fn no_mid(&self) -> T
    where
        T: Copy
            + core::ops::Add<T, Output = T>
            + core::ops::Div<T, Output = T>
            + num_traits::FromPrimitive,
    {
        (self.no_bid + self.no_ask) / <T>::from_u8(2).unwrap()
    }

    /// Spread of the YES side: `yes_ask − yes_bid`.
    pub fn spread(&self) -> T
    where
        T: Copy + core::ops::Sub<T, Output = T>,
    {
        self.yes_ask - self.yes_bid
    }

    /// Returns `true` when the spread is at or below `max`.
    pub fn is_tight(&self, max: T) -> bool
    where
        T: Copy + core::ops::Sub<T, Output = T> + PartialOrd,
    {
        self.spread() <= max
    }

    /// Milliseconds elapsed since the book last changed.
    ///
    /// Returns 0 when `timestamp ≤ 0` or when the wall clock reads earlier
    /// than the snapshot's timestamp (clock skew). Std-only.
    #[cfg(feature = "std")]
    pub fn last_change_age_ms(&self) -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as i64)
            .unwrap_or(0);
        self.last_change_age_ms_at(now)
    }

    /// Milliseconds elapsed since the book last changed at an explicit time.
    ///
    /// A non-positive snapshot timestamp or an evaluation time at/before the
    /// snapshot returns zero. Strategy code uses this deterministic API rather
    /// than consulting the host wall clock.
    #[inline]
    pub fn last_change_age_ms_at(&self, evaluated_at_ns: i64) -> u64 {
        if self.timestamp <= 0 || evaluated_at_ns <= self.timestamp {
            return 0;
        }
        ((evaluated_at_ns - self.timestamp) / 1_000_000) as u64
    }
}

impl<T> AskBid<T> {
    pub const fn new(ask: T, bid: T) -> Self {
        Self { ask, bid }
    }
    /// returns an immutable reference to the best ask
    pub const fn ask(&self) -> &T {
        &self.ask
    }
    /// returns a mutable reference to the best ask
    pub const fn ask_mut(&mut self) -> &mut T {
        &mut self.ask
    }
    /// returns an immutable reference to the best bid
    pub const fn bid(&self) -> &T {
        &self.bid
    }
    /// returns a mutable reference to the best bid
    pub const fn bid_mut(&mut self) -> &mut T {
        &mut self.bid
    }
    /// Mid-price of the YES side: `(yes_bid + yes_ask) / 2`.
    pub fn mid(&self) -> T
    where
        T: Copy
            + core::ops::Add<T, Output = T>
            + core::ops::Div<T, Output = T>
            + num_traits::FromPrimitive,
    {
        (self.bid + self.ask) / <T>::from_u8(2).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::BookSnapshot;

    #[test]
    fn explicit_book_age_is_deterministic_and_saturating() {
        let book = BookSnapshot::new(0.4, 0.41, 0.59, 0.6, 1_000_000_000);
        assert_eq!(book.last_change_age_ms_at(1_125_999_999), 125);
        assert_eq!(book.last_change_age_ms_at(1_000_000_000), 0);
        assert_eq!(book.last_change_age_ms_at(999_000_000), 0);

        let unstamped = book.with_timestamp(0);
        assert_eq!(unstamped.last_change_age_ms_at(i64::MAX), 0);
    }
}
