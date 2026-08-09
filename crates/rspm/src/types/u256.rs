/*
    Appellation: u256 <module>
    Created At: 2026.08.08:06:24:51
    Contrib: @FL03
*/
use crate::canonical_unsigned_integer_text;
use core::str::FromStr;
use polymarket::types::U256;

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize),
    serde(deny_unknown_fields, transparent)
)]
#[repr(transparent)]
pub struct CanonicalU256(pub U256);

impl CanonicalU256 {
    pub fn from_str<T>(value: T) -> Result<Self, crate::Error>
    where
        T: AsRef<str>,
    {
        core::str::FromStr::from_str(value.as_ref())
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for CanonicalU256 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::de::Deserializer<'de>,
    {
        struct Visitor;

        impl serde::de::Visitor<'_> for Visitor {
            type Value = U256;

            fn expecting(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                formatter.write_str("a canonical unsigned base-unit integer")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                if !canonical_unsigned_integer_text(value) {
                    return Err(E::custom("expected canonical unsigned base-unit integer"));
                }
                U256::from_str(value)
                    .map_err(|_| E::custom("unsigned base-unit integer is out of range"))
            }
        }

        deserializer.deserialize_str(Visitor).map(Self)
    }
}

impl core::str::FromStr for CanonicalU256 {
    type Err = crate::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if !canonical_unsigned_integer_text(s) {
            return Err(crate::Error::U256ParseError(s.to_string()));
        }
        U256::from_str(s)
            .map_err(|_| crate::Error::U256ParseError(s.to_string()))
            .map(Self)
    }
}
