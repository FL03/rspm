/*
    Appellation: order_type <module>
    Created At: 2026.08.09:06:58:15
    Contrib: @FL03
*/
#[cfg(feature = "alloc")]
use alloc::string::String;

/// Typed order-type discriminant for Axiom CLOB orders.
///
/// Each variant encodes the wire-level TIF string sent to Polymarket:
///
/// | Variant     | Wire value | Semantics                                          |
/// |-------------|------------|----------------------------------------------------|
/// | `GtcMaker`  | `"GTC"`    | Good-Til-Cancelled; rests on the book as a maker.  |
/// | `FakTaker`  | `"FAK"`    | Fill-and-Kill; takes available liquidity, cancels rest. |
/// | `Fok`       | `"FOK"`    | Fill-or-Kill; fills in full or cancels entirely.   |
///
/// # Serde
///
/// Each variant round-trips to its wire string via `serde`:
/// `GtcMaker ↔ "GTC"`, `FakTaker ↔ "FAK"`, `Fok ↔ "FOK"`.
///
/// # Parsing and wire compatibility
///
/// [`core::str::FromStr`] rejects unknown values. The retained `From<&str>` and
/// `From<String>` compatibility conversions default unknown values to
/// `GtcMaker`, matching the pre-typed behavior; new code should use the
/// fallible parser.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
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
pub enum OrderType {
    /// Good-Til-Cancelled; rests on the book as a maker order.
    #[default]
    #[strum(serialize = "GTC")]
    #[cfg_attr(feature = "serde", serde(rename = "GTC"))]
    #[cfg_attr(feature = "sqlx", sqlx(rename = "GTC"))]
    GtcMaker,
    /// Fill-and-Kill; takes available liquidity, cancels remainder.
    #[strum(serialize = "FAK")]
    #[cfg_attr(feature = "serde", serde(rename = "FAK"))]
    #[cfg_attr(feature = "sqlx", sqlx(rename = "FAK"))]
    FakTaker,
    /// Fill-or-Kill; fills entirely or cancels.
    #[strum(serialize = "FOK")]
    #[cfg_attr(feature = "serde", serde(rename = "FOK"))]
    #[cfg_attr(feature = "sqlx", sqlx(rename = "FOK"))]
    Fok,
}

impl OrderType {
    /// Parse a wire value without silently selecting an order policy.
    pub fn from_str<T>(value: T) -> Result<Self, strum::ParseError>
    where
        T: AsRef<str>,
    {
        core::str::FromStr::from_str(value.as_ref())
    }

    /// The canonical wire string for this variant (always uppercase ASCII).
    pub const fn as_wire_str(self) -> &'static str {
        match self {
            Self::GtcMaker => "GTC",
            Self::FakTaker => "FAK",
            Self::Fok => "FOK",
        }
    }
}

impl core::str::FromStr for OrderType {
    type Err = strum::ParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.eq_ignore_ascii_case("GTC") {
            Ok(Self::GtcMaker)
        } else if value.eq_ignore_ascii_case("FAK") {
            Ok(Self::FakTaker)
        } else if value.eq_ignore_ascii_case("FOK") {
            Ok(Self::Fok)
        } else {
            Err(strum::ParseError::VariantNotFound)
        }
    }
}

impl From<&str> for OrderType {
    fn from(value: &str) -> Self {
        Self::from_str(value).unwrap_or_default()
    }
}

#[cfg(feature = "alloc")]
impl From<String> for OrderType {
    fn from(value: String) -> Self {
        Self::from(value.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::OrderType;
    use alloc::string::ToString as _;

    // ── OrderType typed variant tests ───────────────────────────────────────

    #[test]
    fn order_type_wire_strings() {
        assert_eq!(OrderType::GtcMaker.as_wire_str(), "GTC");
        assert_eq!(OrderType::FakTaker.as_wire_str(), "FAK");
        assert_eq!(OrderType::Fok.as_wire_str(), "FOK");
    }

    #[test]
    fn order_type_display_is_wire_string() {
        assert_eq!(OrderType::GtcMaker.to_string(), "GTC");
        assert_eq!(OrderType::FakTaker.to_string(), "FAK");
        assert_eq!(OrderType::Fok.to_string(), "FOK");
    }

    #[test]
    fn order_type_fallible_parser_rejects_unknown_policy() {
        assert_eq!(OrderType::from_str("GTC"), Ok(OrderType::GtcMaker));
        assert_eq!(OrderType::from_str("fak"), Ok(OrderType::FakTaker));
        assert_eq!(OrderType::from_str("FOK"), Ok(OrderType::Fok));
        assert!(OrderType::from_str("UNKNOWN").is_err());
    }

    #[test]
    fn order_type_compatibility_conversions_retain_the_legacy_default() {
        assert_eq!(OrderType::from("GTC"), OrderType::GtcMaker);
        assert_eq!(OrderType::from("FAK".to_string()), OrderType::FakTaker);
        assert_eq!(OrderType::from("FOK".to_string()), OrderType::Fok);
        assert_eq!(OrderType::from("UNKNOWN"), OrderType::GtcMaker);
    }

    #[test]
    fn order_type_default_is_gtc_maker() {
        assert_eq!(OrderType::default(), OrderType::GtcMaker);
    }

    #[cfg(all(feature = "sqlx", feature = "postgres"))]
    #[test]
    fn order_type_postgres_encoding_uses_canonical_wire_tags() {
        use sqlx::{Postgres, postgres::PgArgumentBuffer};

        for (variant, wire) in [
            (OrderType::GtcMaker, b"GTC".as_slice()),
            (OrderType::FakTaker, b"FAK".as_slice()),
            (OrderType::Fok, b"FOK".as_slice()),
        ] {
            let mut buffer = PgArgumentBuffer::default();
            let null = <OrderType as sqlx::Encode<Postgres>>::encode_by_ref(&variant, &mut buffer)
                .expect("encode OrderType");
            assert!(!null.is_null());
            assert_eq!(&buffer[..], wire, "Postgres wire tag for {variant:?}");
        }
    }

    #[cfg(feature = "json")]
    #[test]
    fn order_type_serde_round_trip() {
        // Each variant ↔ wire string
        let cases: &[(&str, OrderType)] = &[
            (r#""GTC""#, OrderType::GtcMaker),
            (r#""FAK""#, OrderType::FakTaker),
            (r#""FOK""#, OrderType::Fok),
        ];
        for (wire, variant) in cases {
            let serialized = serde_json::to_string(variant).expect("serialize OrderType");
            assert_eq!(serialized, *wire, "serialize {variant:?}");
            let deserialized: OrderType =
                serde_json::from_str(wire).expect("deserialize OrderType");
            assert_eq!(deserialized, *variant, "deserialize {wire}");
        }
    }
}
