/*
    Appellation: fees <module>
    Contrib: @FL03
*/
//! Polymarket V2 platform-fee calculation.
//!
//! V2 market metadata supplies the fee coefficient as `fd.r` and its power
//! exponent as `fd.e`. [`FeeCalculator::from_market_metadata`] is the
//! authoritative constructor for those values. Arithmetic is owned here so
//! this client remains independently buildable and has no Axiom dependency.

use rust_decimal::{Decimal, RoundingStrategy};

/// Decimal places used by the current Polymarket platform-fee schedule.
pub const POLYMARKET_FEE_PRECISION: u32 = 5;

/// Failure returned by exact power-curve fee arithmetic.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PowerFeeError {
    NonPositiveShares,
    PriceOutOfBounds,
    NegativeRate,
    ArithmeticOverflow,
    FloatConversion,
}

impl core::fmt::Display for PowerFeeError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(match self {
            Self::NonPositiveShares => "shares must be greater than zero",
            Self::PriceOutOfBounds => "price must be within the inclusive range [0, 1]",
            Self::NegativeRate => "fee rate must be nonnegative",
            Self::ArithmeticOverflow => "fee arithmetic overflowed Decimal",
            Self::FloatConversion => "f64 compatibility conversion failed",
        })
    }
}

impl core::error::Error for PowerFeeError {}

const CURRENT_CRYPTO_RATE: Decimal = Decimal::from_parts(7, 0, 0, false, 2);
const CURRENT_CRYPTO_EXPONENT: u32 = 1;

/// Sprint window label used by market-discovery callers.
///
/// Fee parameters are not selected from this label. They come from each
/// market's `fd.r` and `fd.e` metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum WindowKind {
    /// 5-minute sprint window.
    W5m,
    /// 15-minute sprint window.
    W15m,
    /// 4-hour sprint window.
    W4h,
}

/// Exact Polymarket V2 platform-fee parameters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Deserialize, serde::Serialize),
    serde(rename_all = "snake_case")
)]
pub struct FeeCalculator {
    fee_rate: Decimal,
    fee_exponent: u32,
}

impl FeeCalculator {
    /// Constructs the authoritative schedule from V2 `fd.r` and `fd.e`.
    ///
    /// # Errors
    ///
    /// Returns [`PowerFeeError::NegativeRate`] when `rate` is negative.
    pub fn from_market_metadata(rate: Decimal, exponent: u32) -> Result<Self, PowerFeeError> {
        if rate < Decimal::ZERO {
            return Err(PowerFeeError::NegativeRate);
        }
        Ok(Self {
            fee_rate: rate,
            fee_exponent: exponent,
        })
    }

    /// Constructs the current Crypto helper schedule: rate `0.07`, exponent `1`.
    ///
    /// Metadata-driven construction remains authoritative for live markets.
    pub const fn crypto() -> Self {
        Self {
            fee_rate: CURRENT_CRYPTO_RATE,
            fee_exponent: CURRENT_CRYPTO_EXPONENT,
        }
    }

    /// Returns the V2 `fd.r` fee-rate coefficient.
    pub const fn fee_rate(&self) -> Decimal {
        self.fee_rate
    }

    /// Returns the V2 `fd.e` power exponent.
    pub const fn fee_exponent(&self) -> u32 {
        self.fee_exponent
    }

    /// Computes a taker fee at Polymarket's current five-decimal precision.
    pub fn taker_fee(&self, shares: Decimal, price: Decimal) -> Result<Decimal, PowerFeeError> {
        if shares <= Decimal::ZERO {
            return Err(PowerFeeError::NonPositiveShares);
        }
        if !(Decimal::ZERO..=Decimal::ONE).contains(&price) {
            return Err(PowerFeeError::PriceOutOfBounds);
        }
        if self.fee_rate.is_zero()
            || self.fee_exponent > 0 && (price.is_zero() || price == Decimal::ONE)
        {
            return Ok(Decimal::ZERO);
        }

        let complement = Decimal::ONE
            .checked_sub(price)
            .ok_or(PowerFeeError::ArithmeticOverflow)?;
        let base = price
            .checked_mul(complement)
            .ok_or(PowerFeeError::ArithmeticOverflow)?;
        let curve =
            checked_pow(base, self.fee_exponent).ok_or(PowerFeeError::ArithmeticOverflow)?;
        let fee = shares
            .checked_mul(self.fee_rate)
            .and_then(|amount| amount.checked_mul(curve))
            .ok_or(PowerFeeError::ArithmeticOverflow)?;

        Ok(fee.round_dp_with_strategy(
            POLYMARKET_FEE_PRECISION,
            RoundingStrategy::MidpointAwayFromZero,
        ))
    }

    /// Returns the V2 maker platform fee, which is exactly zero.
    pub const fn maker_fee(&self) -> Decimal {
        Decimal::ZERO
    }

    /// Compatibility projection for callers that still store fills as `f64`.
    ///
    /// Conversion happens before the canonical exact formula is evaluated.
    pub fn taker_fee_f64(&self, shares: f64, price: f64) -> Result<f64, PowerFeeError> {
        let shares = Decimal::try_from(shares).map_err(|_| PowerFeeError::FloatConversion)?;
        let price = Decimal::try_from(price).map_err(|_| PowerFeeError::FloatConversion)?;
        let fee = self.taker_fee(shares, price)?;
        f64::try_from(fee).map_err(|_| PowerFeeError::FloatConversion)
    }
}

fn checked_pow(mut base: Decimal, mut exponent: u32) -> Option<Decimal> {
    let mut product = Decimal::ONE;
    while exponent > 0 {
        if exponent & 1 == 1 {
            product = product.checked_mul(base)?;
        }
        exponent >>= 1;
        if exponent > 0 {
            base = base.checked_mul(base)?;
        }
    }
    Some(product)
}

#[cfg(test)]
mod tests {
    use rust_decimal::Decimal;

    use super::*;

    #[test]
    fn metadata_rate_and_exponent_drive_taker_fee() {
        let calculator = FeeCalculator::from_market_metadata(Decimal::new(7, 2), 2)
            .expect("fd.r=0.07 and fd.e=2 are valid");

        let fee = calculator
            .taker_fee(Decimal::ONE_HUNDRED, Decimal::new(5, 1))
            .expect("official midpoint inputs are valid");

        assert_eq!(calculator.fee_rate(), Decimal::new(7, 2));
        assert_eq!(calculator.fee_exponent(), 2);
        assert_eq!(fee, Decimal::new(4375, 4));
    }

    #[test]
    fn current_crypto_helper_matches_official_anchor() {
        let calculator = FeeCalculator::crypto();

        let fee = calculator
            .taker_fee(Decimal::ONE_HUNDRED, Decimal::new(5, 1))
            .expect("official midpoint inputs are valid");

        assert_eq!(calculator.fee_rate(), Decimal::new(7, 2));
        assert_eq!(calculator.fee_exponent(), 1);
        assert_eq!(fee, Decimal::new(175, 2));
    }

    #[test]
    fn maker_fee_is_exactly_zero() {
        let calculator = FeeCalculator::from_market_metadata(Decimal::new(7, 2), 2)
            .expect("fd.r=0.07 and fd.e=2 are valid");

        assert_eq!(calculator.maker_fee(), Decimal::ZERO);
    }

    #[test]
    fn endpoints_return_exact_zero() {
        let calculator = FeeCalculator::crypto();

        assert_eq!(
            calculator
                .taker_fee(Decimal::ONE_HUNDRED, Decimal::ZERO)
                .expect("zero is a valid endpoint"),
            Decimal::ZERO
        );
        assert_eq!(
            calculator
                .taker_fee(Decimal::ONE_HUNDRED, Decimal::ONE)
                .expect("one is a valid endpoint"),
            Decimal::ZERO
        );
    }

    #[test]
    fn f64_projection_preserves_the_official_anchor() {
        let calculator = FeeCalculator::from_market_metadata(Decimal::new(7, 2), 1)
            .expect("fd.r=0.07 and fd.e=1 are valid");

        assert_eq!(
            calculator
                .taker_fee_f64(100.0, 0.5)
                .expect("official f64 projection inputs are valid"),
            1.75
        );
    }

    #[test]
    fn invalid_metadata_rate_is_rejected() {
        assert_eq!(
            FeeCalculator::from_market_metadata(Decimal::NEGATIVE_ONE, 1),
            Err(PowerFeeError::NegativeRate)
        );
    }

    /// [EVAL] The metadata formula must remain total over valid endpoints and
    /// fail closed over invalid numbers, overflow, and non-finite projections.
    #[test]
    fn fee_contract_generalizes_beyond_the_official_anchor() {
        let linear = FeeCalculator::from_market_metadata(Decimal::new(7, 2), 1).unwrap();
        for shares in [Decimal::ZERO, Decimal::NEGATIVE_ONE] {
            assert_eq!(
                linear.taker_fee(shares, Decimal::new(5, 1)),
                Err(PowerFeeError::NonPositiveShares)
            );
        }
        for price in [Decimal::new(-1, 1), Decimal::new(11, 1)] {
            assert_eq!(
                linear.taker_fee(Decimal::ONE, price),
                Err(PowerFeeError::PriceOutOfBounds)
            );
        }
        assert_eq!(
            linear.taker_fee_f64(f64::NAN, 0.5),
            Err(PowerFeeError::FloatConversion)
        );

        let exponent_zero = FeeCalculator::from_market_metadata(Decimal::new(7, 2), 0).unwrap();
        assert_eq!(
            exponent_zero.taker_fee(Decimal::ONE_HUNDRED, Decimal::ZERO),
            Ok(Decimal::new(7, 0))
        );
        assert_eq!(
            exponent_zero.taker_fee(Decimal::ONE_HUNDRED, Decimal::ONE),
            Ok(Decimal::new(7, 0))
        );

        let overflowing = FeeCalculator::from_market_metadata(Decimal::MAX, 1).unwrap();
        assert_eq!(
            overflowing.taker_fee(Decimal::MAX, Decimal::new(5, 1)),
            Err(PowerFeeError::ArithmeticOverflow)
        );
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_round_trip_preserves_metadata_schedule() {
        let calculator = FeeCalculator::from_market_metadata(Decimal::new(7, 2), 2)
            .expect("fd.r=0.07 and fd.e=2 are valid");
        let encoded = serde_json::to_string(&calculator).expect("fee calculator serializes");
        let decoded: FeeCalculator =
            serde_json::from_str(&encoded).expect("fee calculator deserializes");

        assert_eq!(decoded, calculator);
        assert_eq!(
            decoded
                .taker_fee(Decimal::ONE_HUNDRED, Decimal::new(5, 1))
                .expect("round-tripped schedule remains valid"),
            Decimal::new(4375, 4)
        );
    }
}
