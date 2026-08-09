/*
    Appellation: side <module>
    Created At: 2026.03.17:19:38:22
    Contrib: @FL03
*/
use polymarket::clob::types::Side as ClobSide;
/// Side of a binary outcome prediction market (YES or NO).
///
/// ## Parsing
///
/// `Side` implements [`core::str::FromStr`] manually with ASCII-case-insensitive
/// matching, so all of the following parse correctly:
///
/// ```rust
/// use std::str::FromStr;
/// use rspm::types::Side;
///
/// assert_eq!(Side::from_str("yes").unwrap(), Side::Yes);
/// assert_eq!(Side::from_str("YES").unwrap(), Side::Yes);
/// assert_eq!(Side::from_str("Yes").unwrap(), Side::Yes);
/// assert_eq!(Side::from_str("no").unwrap(),  Side::No);
/// assert_eq!(Side::from_str("NO").unwrap(),  Side::No);
/// assert_eq!(Side::from_str("No").unwrap(),  Side::No);
/// ```
// NOTE: `strum::EnumString` is intentionally NOT derived here.
// strum 0.28 generates an explicit `impl TryFrom<&str>` which conflicts with
// core's blanket `impl<T, U> TryFrom<U> for T where U: Into<T>` — the coherence
// checker sees a potential future overlap (E0119). Instead, `FromStr` is
// implemented manually below; it does not generate a `TryFrom<&str>` impl.
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
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[strum(ascii_case_insensitive, serialize_all = "UPPERCASE")]
pub enum Side {
    /// The YES outcome — BTC closes above the strike price.
    #[default]
    #[cfg_attr(feature = "serde", serde(alias = "yes", alias = "YES"))]
    Yes = 1,
    /// The NO outcome — BTC closes at or below the strike price.
    #[cfg_attr(feature = "serde", serde(alias = "no", alias = "NO"))]
    No = 0,
}

impl core::str::FromStr for Side {
    type Err = strum::ParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.eq_ignore_ascii_case("yes") || value.eq_ignore_ascii_case("buy") {
            Ok(Self::Yes)
        } else if value.eq_ignore_ascii_case("no") || value.eq_ignore_ascii_case("sell") {
            Ok(Self::No)
        } else {
            Err(strum::ParseError::VariantNotFound)
        }
    }
}

impl Side {
    /// Returns the NO side.
    pub const fn no() -> Self {
        Self::No
    }
    /// Returns the YES side.
    pub const fn yes() -> Self {
        Self::Yes
    }

    pub fn from_str<T>(value: T) -> Result<Self, <Self as core::str::FromStr>::Err>
    where
        T: AsRef<str>,
    {
        core::str::FromStr::from_str(value.as_ref())
    }
    /// Returns the opposite side (`Yes` → `No`, `No` → `Yes`).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rspm::types::Side;
    ///
    /// assert_eq!(Side::Yes.invert(), Side::No);
    /// assert_eq!(Side::No.invert(), Side::Yes);
    /// ```
    #[inline]
    pub fn invert(self) -> Self {
        match self {
            Self::Yes => Self::No,
            Self::No => Self::Yes,
        }
    }
    /// Alias for [`invert`](Self::invert) — returns the opposite side.
    ///
    /// Provided for callers using the conventional buy/sell vocabulary.
    pub fn opposite(self) -> Self {
        self.invert()
    }
    /// Returns `true` when this is the YES (buy) side.
    ///
    /// Note: `strum::EnumIs` already generates `is_yes()` and `is_no()` methods.
    /// This method is an explicit alias with a conventional trading name.
    pub fn is_buy(self) -> bool {
        matches!(self, Self::Yes)
    }
    /// Returns `true` when this is the NO (sell) side.
    pub fn is_sell(self) -> bool {
        matches!(self, Self::No)
    }
    /// Lowercase database string for the side — used where a CHECK constraint
    /// requires lowercase (e.g. `positions_side_check` on `axiom.positions`).
    ///
    /// CLOB API still requires uppercase (`Display` → "YES"/"NO"). This method
    /// is additive and does NOT change `Display` behaviour.
    pub const fn as_db_str(self) -> &'static str {
        match self {
            Self::Yes => "yes",
            Self::No => "no",
        }
    }
}

impl From<bool> for Side {
    fn from(value: bool) -> Self {
        match value {
            true => Self::Yes,
            _ => Self::No,
        }
    }
}

impl From<Side> for bool {
    fn from(value: Side) -> Self {
        matches!(value, Side::Yes)
    }
}

#[cfg(feature = "clob")]
impl From<ClobSide> for Side {
    fn from(value: ClobSide) -> Self {
        match value {
            ClobSide::Buy => Self::Yes,
            ClobSide::Sell => Self::No,
            _ => Self::No,
        }
    }
}

macro_rules! impl_from_side {
    ($($t:ty),* $(,)?) => {
        $(
            impl From<Side> for $t {
                fn from(value: Side) -> Self {
                    match value {
                        Side::Yes => 1 as $t,
                        Side::No => 0 as $t,
                    }
                }
            }

            impl From<$t> for Side {
                fn from(value: $t) -> Self {
                    match value % 2 as $t {
                        0 => Side::No,
                        _ => Side::Yes,
                    }
                }
            }
        )*
    };
}

impl_from_side! {
    u8, u16, u32, u64, u128, usize,
    i8, i16, i32, i64, i128, isize
}

#[cfg(feature = "alloc")]
impl TryFrom<String> for Side {
    type Error = <Self as core::str::FromStr>::Err;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::from_str(&s)
    }
}

#[cfg(feature = "alloc")]
impl From<Side> for String {
    fn from(p: Side) -> String {
        p.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invert_and_opposite_are_symmetric() {
        assert_eq!(Side::Yes.invert(), Side::No);
        assert_eq!(Side::No.invert(), Side::Yes);
        assert_eq!(Side::Yes.opposite(), Side::No);
        assert_eq!(Side::No.opposite(), Side::Yes);
    }

    #[test]
    fn is_buy_and_is_sell() {
        assert!(Side::Yes.is_buy());
        assert!(!Side::Yes.is_sell());
        assert!(Side::No.is_sell());
        assert!(!Side::No.is_buy());
    }

    #[test]
    fn strum_enumis_is_yes_is_no() {
        // strum::EnumIs generates these automatically
        assert!(Side::Yes.is_yes());
        assert!(!Side::Yes.is_no());
        assert!(Side::No.is_no());
        assert!(!Side::No.is_yes());
    }

    #[test]
    fn numeric_roundtrip() {
        assert_eq!(Side::from(1u8), Side::Yes);
        assert_eq!(Side::from(0u8), Side::No);
        assert_eq!(u8::from(Side::Yes), 1u8);
        assert_eq!(u8::from(Side::No), 0u8);
    }

    #[test]
    fn display_is_uppercase() {
        assert_eq!(Side::Yes.to_string(), "YES");
        assert_eq!(Side::No.to_string(), "NO");
    }

    /// FAIL-ON-REVERT: Display must remain uppercase; as_db_str must be lowercase.
    /// If this test is deleted or the tolerance is loosened, lag_tick.rs will
    /// write uppercase "YES"/"NO" to positions_side_check constraint → constraint
    /// violation → all position writes silently rejected.
    #[test]
    fn display_uppercase_db_str_lowercase() {
        assert_eq!(
            format!("{}", Side::Yes),
            "YES",
            "Display must be uppercase YES"
        );
        assert_eq!(
            format!("{}", Side::No),
            "NO",
            "Display must be uppercase NO"
        );
        assert_eq!(
            Side::Yes.as_db_str(),
            "yes",
            "as_db_str must be lowercase yes"
        );
        assert_eq!(Side::No.as_db_str(), "no", "as_db_str must be lowercase no");
    }

    #[test]
    fn from_str_case_insensitive() {
        assert_eq!(Side::from_str("yes").unwrap(), Side::Yes);
        assert_eq!(Side::from_str("YES").unwrap(), Side::Yes);
        assert_eq!(Side::from_str("Yes").unwrap(), Side::Yes);
        assert_eq!(Side::from_str("buy").unwrap(), Side::Yes);
        assert_eq!(Side::from_str("BUY").unwrap(), Side::Yes);
        assert_eq!(Side::from_str("no").unwrap(), Side::No);
        assert_eq!(Side::from_str("NO").unwrap(), Side::No);
        assert_eq!(Side::from_str("No").unwrap(), Side::No);
        assert_eq!(Side::from_str("sell").unwrap(), Side::No);
        assert_eq!(Side::from_str("SELL").unwrap(), Side::No);
        assert!(Side::from_str("unknown").is_err());
    }

    /// #2418 regression pin — INVERTED from the test that previously pinned the
    /// `serde(untagged)` null-collapse as intentional. Under `untagged`, BOTH
    /// unit variants serialized to JSON `null` and `null` deserialized to
    /// `Yes` (the first/#[default] variant): a silent NO→YES direction
    /// inversion at every JSON boundary — on the type that is the `side`
    /// field of the canonical `Order` submitted to the CLOB. Tagged strings
    /// must round-trip, the lowercase aliases must be honored, and `null`
    /// must FAIL LOUD, never coerce to a direction.
    #[cfg(feature = "serde")]
    #[test]
    fn serde_side_round_trips_tagged_strings_and_rejects_null() {
        // Canonical serialization: UPPERCASE tagged strings.
        assert_eq!(serde_json::to_string(&Side::Yes).unwrap(), "\"YES\"");
        assert_eq!(serde_json::to_string(&Side::No).unwrap(), "\"NO\"");

        // Round trip both variants, canonical and aliased spellings.
        for (json, side) in [
            ("\"YES\"", Side::Yes),
            ("\"yes\"", Side::Yes),
            ("\"NO\"", Side::No),
            ("\"no\"", Side::No),
        ] {
            assert_eq!(serde_json::from_str::<Side>(json).unwrap(), side);
        }

        // null must never deserialize into a trade direction (#2418).
        assert!(
            serde_json::from_str::<Side>("null").is_err(),
            "null must never coerce to a Side"
        );
        // BUY/SELL spellings remain FromStr-only, not serde.
        assert!(serde_json::from_str::<Side>("\"buy\"").is_err());
        assert!(serde_json::from_str::<Side>("\"sell\"").is_err());
    }
}
