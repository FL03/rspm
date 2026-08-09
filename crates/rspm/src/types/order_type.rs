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
/// # Wire-compat shim
///
/// `From<&str>` / `From<String>` accept any case-sensitive wire string and
/// default to `GtcMaker` for unknown values (matching the pre-typed behaviour).
/// The shim will be removed once all callers use the typed variant directly.
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
    strum::EnumString,
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
#[non_exhaustive]
pub enum OrderType {
    /// Good-Til-Cancelled; rests on the book as a maker order.
    #[default]
    #[cfg_attr(feature = "serde", serde(rename = "GTC"))]
    GtcMaker,
    /// Fill-and-Kill; takes available liquidity, cancels remainder.
    #[cfg_attr(feature = "serde", serde(rename = "FAK"))]
    FakTaker,
    /// Fill-or-Kill; fills entirely or cancels.
    #[cfg_attr(feature = "serde", serde(rename = "FOK"))]
    Fok,
}

impl OrderType {
    pub fn from_str<T>(value: T) -> Result<Self, <Self as core::str::FromStr>::Err>
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

#[cfg(feature = "alloc")]
impl From<String> for OrderType {
    fn from(s: String) -> Self {
        Self::from_str(s).expect("failed to parse")
    }
}

#[cfg(test)]
mod tests {
    use super::OrderType;
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
    fn order_type_from_str_shim() {
        assert_eq!(OrderType::from_str("GTC"), Ok(OrderType::GtcMaker));
        assert_eq!(OrderType::from_str("FAK"), Ok(OrderType::FakTaker));
        assert_eq!(OrderType::from_str("FOK"), Ok(OrderType::Fok));
        // Unknown values fall through to the default (GtcMaker)
        assert_eq!(OrderType::from_str("UNKNOWN"), Ok(OrderType::GtcMaker));
    }

    #[test]
    fn order_type_from_string_shim() {
        assert_eq!(OrderType::from("GTC".to_string()), OrderType::GtcMaker);
        assert_eq!(OrderType::from("FAK".to_string()), OrderType::FakTaker);
        assert_eq!(OrderType::from("FOK".to_string()), OrderType::Fok);
    }

    #[test]
    fn order_type_default_is_gtc_maker() {
        assert_eq!(OrderType::default(), OrderType::GtcMaker);
    }

    #[cfg(feature = "serde")]
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
