/*
    Appellation: clob_side <module>
    Created At: 2026.06.30
    Contrib: @FL03
*/
//! Order-direction vocabulary for CLOB (central limit order book) trades.
//!
//! [`ClobSide`] is deliberately **distinct** from two other, easily-confused
//! vocabularies in this crate:
//!
//! | Type | Variants | Encodes |
//! |------|----------|---------|
//! | [`Side`](crate::types::Side) | `Yes` / `No` | The market OUTCOME a token pays out on. |
//! | [`ClobSide`] (this type) | `Buy` / `Sell` | The DIRECTION of an order placed on the CLOB. |
//! | `polymarket::clob::types::Side` (external SDK) | `Buy` / `Sell` | The wire-level order-direction type used by `polymarket_client_sdk_v2`; bridged privately in `types::side` via `From<ClobSide /* SDK */> for Side` for outcome conversions only — do not confuse with this crate's own [`ClobSide`]. |
//!
//! A market outcome (`Side::Yes`/`Side::No`) is not the same axis as an order
//! direction (`ClobSide::Buy`/`ClobSide::Sell`): a caller can BUY the NO token
//! just as easily as SELL the YES token. Collapsing the two into a single
//! `&str` (as pre-typed call sites did) silently accepted either vocabulary
//! ("yes"/"no" or "BUY"/"SELL") at the same call site with no compile-time
//! signal that they mean different things. `ClobSide` closes that hole for
//! order-direction call sites (GH #2006).
//!
//! ## Parsing
//!
//! `ClobSide` implements [`core::str::FromStr`] manually with ASCII-case-
//! insensitive matching:
//!
//! ```rust
//! use std::str::FromStr;
//! use rspm::types::ClobSide;
//!
//! assert_eq!(ClobSide::from_str("buy").unwrap(), ClobSide::Buy);
//! assert_eq!(ClobSide::from_str("BUY").unwrap(), ClobSide::Buy);
//! assert_eq!(ClobSide::from_str("Buy").unwrap(), ClobSide::Buy);
//! assert_eq!(ClobSide::from_str("sell").unwrap(), ClobSide::Sell);
//! assert_eq!(ClobSide::from_str("SELL").unwrap(), ClobSide::Sell);
//! assert_eq!(ClobSide::from_str("Sell").unwrap(), ClobSide::Sell);
//! ```
// NOTE: `strum::EnumString` is intentionally NOT derived here, matching the
// sibling `Side` enum in `types::side` — strum 0.28 generates an explicit
// `impl TryFrom<&str>` which conflicts with core's blanket
// `impl<T, U> TryFrom<U> for T where U: Into<T>` if this type ever gains a
// `From<&str>` impl (E0119). `FromStr` is implemented manually below instead.
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
    strum::IntoStaticStr,
    strum::VariantNames,
)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "UPPERCASE")
)]
#[cfg_attr(feature = "sqlx", derive(sqlx::Type), sqlx(rename_all = "UPPERCASE"))]
#[strum(ascii_case_insensitive, serialize_all = "UPPERCASE")]
pub enum ClobSide {
    /// Buy order — opens or adds to a position at the limit price or better.
    #[default]
    Buy,
    /// Sell order — closes or reduces a position at the limit price or better.
    Sell,
}

impl ClobSide {
    /// Returns the opposite order direction (`Buy` → `Sell`, `Sell` → `Buy`).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rspm::types::ClobSide;
    ///
    /// assert_eq!(ClobSide::Buy.invert(), ClobSide::Sell);
    /// assert_eq!(ClobSide::Sell.invert(), ClobSide::Buy);
    /// ```
    #[inline]
    pub fn invert(self) -> Self {
        match self {
            Self::Buy => Self::Sell,
            Self::Sell => Self::Buy,
        }
    }
    /// Alias for [`invert`](Self::invert) — returns the opposite order direction.
    pub fn opposite(self) -> Self {
        self.invert()
    }
    /// The canonical uppercase wire string for this variant (`"BUY"` / `"SELL"`).
    ///
    /// Matches the CLOB API's own order-direction wire format.
    ///
    /// **NOT** the same vocabulary as `ClobTickRow.side`
    /// (`axiom::store::qdb::ClobTickRow`, defined in `crates/store`): that
    /// column stores the market-OUTCOME label (`"yes"` / `"no"`, via
    /// [`Side::as_db_str`](crate::types::Side::as_db_str)) written by
    /// `bin/node`'s CLOB tick-stream ingestion (`build_buffered_tick` /
    /// `flush_tick_buffer` in `bin/node/src/services/pm/clob/stream.rs`), not
    /// this type's order-DIRECTION vocabulary. The two were previously
    /// conflated in this doc comment (GH #2039).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Buy => "BUY",
            Self::Sell => "SELL",
        }
    }
}

// NOTE: `Display` is provided by the `strum::Display` derive above (combined
// with `#[strum(serialize_all = "UPPERCASE")]`), which already produces
// exactly `as_str()`'s output — no separate manual `impl Display` needed
// (and none is provided, to avoid an E0119 conflict with the derive).

// NOTE: `strum::EnumString` is NOT derived — see the comment above the derive
// list. This manual impl mirrors `types::side::Side::from_str`.
impl core::str::FromStr for ClobSide {
    type Err = strum::ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.eq_ignore_ascii_case("buy") {
            Ok(Self::Buy)
        } else if s.eq_ignore_ascii_case("sell") {
            Ok(Self::Sell)
        } else {
            Err(strum::ParseError::VariantNotFound)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invert_and_opposite_are_symmetric() {
        assert_eq!(ClobSide::Buy.invert(), ClobSide::Sell);
        assert_eq!(ClobSide::Sell.invert(), ClobSide::Buy);
        assert_eq!(ClobSide::Buy.opposite(), ClobSide::Sell);
        assert_eq!(ClobSide::Sell.opposite(), ClobSide::Buy);
    }

    #[test]
    fn strum_enumis_is_buy_is_sell() {
        // strum::EnumIs generates these automatically from the variant names.
        assert!(ClobSide::Buy.is_buy());
        assert!(!ClobSide::Buy.is_sell());
        assert!(ClobSide::Sell.is_sell());
        assert!(!ClobSide::Sell.is_buy());
    }

    #[test]
    fn as_str_is_uppercase() {
        assert_eq!(ClobSide::Buy.as_str(), "BUY");
        assert_eq!(ClobSide::Sell.as_str(), "SELL");
    }

    #[test]
    fn display_matches_as_str() {
        assert_eq!(ClobSide::Buy.to_string(), "BUY");
        assert_eq!(ClobSide::Sell.to_string(), "SELL");
    }

    #[test]
    fn from_str_case_insensitive() {
        use core::str::FromStr;
        assert_eq!(ClobSide::from_str("buy").unwrap(), ClobSide::Buy);
        assert_eq!(ClobSide::from_str("BUY").unwrap(), ClobSide::Buy);
        assert_eq!(ClobSide::from_str("Buy").unwrap(), ClobSide::Buy);
        assert_eq!(ClobSide::from_str("sell").unwrap(), ClobSide::Sell);
        assert_eq!(ClobSide::from_str("SELL").unwrap(), ClobSide::Sell);
        assert_eq!(ClobSide::from_str("Sell").unwrap(), ClobSide::Sell);
        assert!(ClobSide::from_str("unknown").is_err());
        // Deliberately NOT accepted: market-outcome vocabulary. `ClobSide` is
        // order-direction only; `Side::from_str` already accepts "buy"/"sell"
        // as YES/NO aliases, but the reverse must not hold, or the two
        // vocabularies collapse back into the ambiguity #2006 removes.
        assert!(ClobSide::from_str("yes").is_err());
        assert!(ClobSide::from_str("no").is_err());
    }

    #[test]
    fn default_is_buy() {
        assert_eq!(ClobSide::default(), ClobSide::Buy);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_round_trip_is_tagged_uppercase_string() {
        // Unlike `Side` (which serialises `untagged` as JSON `null` for a
        // historical wire-format reason — see `types::side`), `ClobSide` uses
        // a plain tagged string representation: `"BUY"` / `"SELL"`. This is
        // the more useful representation at order-direction API boundaries.
        // NOTE: `ClobTickRow.side` (`axiom::store::qdb::ClobTickRow`) does
        // NOT use this vocabulary — it stores the market-OUTCOME label
        // (`"yes"`/`"no"`) instead (GH #2039); see `as_str`'s doc above.
        let buy_json = serde_json::to_string(&ClobSide::Buy).unwrap();
        let sell_json = serde_json::to_string(&ClobSide::Sell).unwrap();
        assert_eq!(buy_json, "\"BUY\"");
        assert_eq!(sell_json, "\"SELL\"");

        let from_buy: ClobSide = serde_json::from_str("\"BUY\"").unwrap();
        let from_sell: ClobSide = serde_json::from_str("\"SELL\"").unwrap();
        assert_eq!(from_buy, ClobSide::Buy);
        assert_eq!(from_sell, ClobSide::Sell);
    }
}
