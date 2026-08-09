/*
    Appellation: order <module>
    Created At: 2026.03.17:19:47:16
    Contrib: @FL03
*/
#[cfg(feature = "clob")]
use crate::Side;
use alloc::string::{String, ToString};
#[cfg(feature = "clob")]
use polymarket::clob::types::{AssetType, Side as PmSide};



/// A Polymarket order request — the payload sent to the CLOB to place a trade.
#[derive(Clone, Debug, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct OrderReq {
    /// CLOB token ID (large decimal integer string) for the outcome being traded.
    pub token_id: String,
    /// Limit price in USDC, in the range [0.01, 0.99].
    pub price: f64,
    /// Order size in shares (positive, non-zero).
    pub size: f64,
    /// Order direction: YES (buy) or NO (sell).
    pub side: crate::Side,
}

impl OrderReq {
    /// Construct a new order request.
    pub fn new(token_id: impl ToString, price: f64, size: f64, side: crate::Side) -> Self {
        Self {
            token_id: token_id.to_string(),
            price,
            size,
            side,
        }
    }

    /// Returns the SDK `AssetType` for this order's token.
    #[cfg(feature = "clob")]
    pub fn asset_type(&self) -> AssetType {
        AssetType::Unknown(self.token_id.clone())
    }
}

#[cfg(feature = "clob")]
impl From<Side> for PmSide {
    fn from(side: Side) -> Self {
        match side {
            Side::Yes => Self::Buy,
            Side::No => Self::Sell,
        }
    }
}

/// Lifecycle states for a placed order on any venue.
///
/// Used by [`TradingActor::status`] to report the current state of an order
/// without the caller needing to parse venue-specific response formats.
// NOTE: `strum::EnumString` is NOT derived — strum 0.28 generates an explicit
// `impl TryFrom<&str>` that conflicts with core's blanket impl (E0119).
// `FromStr` is implemented manually below.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, strum::AsRefStr, strum::Display, strum::EnumIs,
)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
#[strum(serialize_all = "snake_case")]
pub enum OrderStatus {
    /// The order has been accepted by the venue but not yet matched.
    #[default]
    Pending,
    /// The order was fully matched and settled.
    Filled,
    /// The order was partially matched; remainder is still open.
    PartialFill,
    /// The order was cancelled by the user or venue.
    Cancelled,
    /// The order window closed before it was matched.
    Expired,
    /// The venue rejected or failed to process the order.
    Failed,
}

impl core::str::FromStr for OrderStatus {
    type Err = strum::ParseError;

    fn from_str(s: &str) -> core::result::Result<Self, Self::Err> {
        match s {
            s if s.eq_ignore_ascii_case("pending") => Ok(Self::Pending),
            s if s.eq_ignore_ascii_case("filled") => Ok(Self::Filled),
            s if s.eq_ignore_ascii_case("partial_fill")
                || s.eq_ignore_ascii_case("partialfill") =>
            {
                Ok(Self::PartialFill)
            }
            s if s.eq_ignore_ascii_case("cancelled") => Ok(Self::Cancelled),
            s if s.eq_ignore_ascii_case("expired") => Ok(Self::Expired),
            s if s.eq_ignore_ascii_case("failed") => Ok(Self::Failed),
            _ => Err(strum::ParseError::VariantNotFound),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;


    // ── OrderStatus tests ───────────────────────────────────────────────────

    #[test]
    fn order_status_default_is_pending() {
        assert_eq!(OrderStatus::default(), OrderStatus::Pending);
    }

    #[test]
    fn order_status_display_is_snake_case() {
        assert_eq!(OrderStatus::Pending.to_string(), "pending");
        assert_eq!(OrderStatus::Filled.to_string(), "filled");
        assert_eq!(OrderStatus::PartialFill.to_string(), "partial_fill");
        assert_eq!(OrderStatus::Cancelled.to_string(), "cancelled");
        assert_eq!(OrderStatus::Expired.to_string(), "expired");
        assert_eq!(OrderStatus::Failed.to_string(), "failed");
    }

    /// `Display` and the hand-written `FromStr` must be inverses over every
    /// variant, and `FromStr` must reject anything it does not recognise.
    ///
    /// `FromStr` is hand-written (see the NOTE above the enum — `strum::EnumString`
    /// is deliberately not derived), so nothing generates it and nothing else in
    /// this crate covers it. Changing `#[strum(serialize_all = "snake_case")]`,
    /// mis-wiring an arm, or replacing the `_ => Err(…)` fallback with a silent
    /// default all turn this red.
    #[test]
    fn order_status_display_and_from_str_are_inverses() {
        use core::str::FromStr as _;

        const ALL: &[OrderStatus] = &[
            OrderStatus::Pending,
            OrderStatus::Filled,
            OrderStatus::PartialFill,
            OrderStatus::Cancelled,
            OrderStatus::Expired,
            OrderStatus::Failed,
        ];

        for status in ALL {
            // Exhaustiveness witness: a new variant that is not added to `ALL`
            // fails to compile here.
            let wire = match status {
                OrderStatus::Pending => "pending",
                OrderStatus::Filled => "filled",
                OrderStatus::PartialFill => "partial_fill",
                OrderStatus::Cancelled => "cancelled",
                OrderStatus::Expired => "expired",
                OrderStatus::Failed => "failed",
            };

            assert_eq!(
                status.to_string(),
                wire,
                "Display must emit the snake_case wire string for {status:?}"
            );
            assert_eq!(
                OrderStatus::from_str(wire).expect("every wire string must parse"),
                *status,
                "FromStr must invert Display for `{wire}`"
            );
            // Venues echo status casing inconsistently — parsing is case-insensitive.
            assert_eq!(
                OrderStatus::from_str(&wire.to_uppercase()).expect("uppercase must parse"),
                *status,
                "FromStr must be case-insensitive for `{wire}`"
            );
        }

        // Separator-free alias kept for venues that emit `partialFill`.
        assert_eq!(
            OrderStatus::from_str("partialFill").expect("separator-free alias must parse"),
            OrderStatus::PartialFill
        );

        // An unrecognised status must error, never silently become the default —
        // a silent `Pending` would report a filled order as still resting.
        assert!(
            OrderStatus::from_str("not_a_status").is_err(),
            "unknown status strings must be rejected, not defaulted"
        );
        assert!(
            OrderStatus::from_str("").is_err(),
            "the empty string must be rejected, not defaulted"
        );
    }

    #[test]
    fn order_status_copy_semantics() {
        let a = OrderStatus::Cancelled;
        let b = a; // copy
        assert_eq!(a, b);
    }

    #[test]
    fn order_status_is_helpers() {
        assert!(OrderStatus::Pending.is_pending());
        assert!(OrderStatus::Filled.is_filled());
        assert!(OrderStatus::PartialFill.is_partial_fill());
        assert!(OrderStatus::Cancelled.is_cancelled());
        assert!(OrderStatus::Expired.is_expired());
        assert!(OrderStatus::Failed.is_failed());

        assert!(!OrderStatus::Pending.is_filled());
        assert!(!OrderStatus::Filled.is_pending());
    }
}
