/*
    Appellation: tick_size <module>
    Created At: 2026.04.01
    Contrib: @FL03
*/
//! Tick size constraints for Polymarket CLOB orders.

#[cfg(not(feature = "std"))]
use num_traits::Float as _;

/// Valid tick sizes on the Polymarket CLOB.
///
/// Orders must be priced at multiples of the market's tick size.
/// Most markets use `Cent` (0.01). High-precision markets use `Millicent` (0.001).
// NOTE: `strum::EnumString` is NOT derived — strum 0.28 generates an explicit
// `impl TryFrom<&str>` that conflicts with core's blanket impl (E0119).
// `FromStr` is implemented manually below.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    strum::AsRefStr,
    strum::Display,
    strum::EnumCount,
    strum::EnumIs,
    strum::VariantNames,
)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Deserialize, serde::Serialize),
    serde(rename_all = "snake_case")
)]
#[strum(serialize_all = "snake_case")]
pub enum TickSize {
    /// Standard tick: $0.01 increments. Used by most markets.
    #[default]
    Cent,
    /// Fine tick: $0.001 increments. Used by high-volume markets.
    Millicent,
}

impl core::str::FromStr for TickSize {
    type Err = strum::ParseError;

    fn from_str(s: &str) -> core::result::Result<Self, Self::Err> {
        match s {
            s if s.eq_ignore_ascii_case("cent") => Ok(Self::Cent),
            s if s.eq_ignore_ascii_case("millicent") => Ok(Self::Millicent),
            _ => Err(strum::ParseError::VariantNotFound),
        }
    }
}

impl TickSize {
    /// Returns the tick size as a float.
    pub fn value(&self) -> f64 {
        match self {
            TickSize::Cent => 0.01,
            TickSize::Millicent => 0.001,
        }
    }

    /// Round a price to the nearest valid tick.
    pub fn round(&self, price: f64) -> f64 {
        let tick = self.value();
        (price / tick).round() * tick
    }

    /// Returns true if the price is a valid tick multiple (within floating-point tolerance).
    pub fn is_valid(&self, price: f64) -> bool {
        let tick = self.value();
        let rem = (price / tick).fract().abs();
        rem < 1e-9 || (1.0 - rem) < 1e-9
    }
}
