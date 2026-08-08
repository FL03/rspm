/*
    Appellation: serialize <module>
    Created At: 2026.08.08:06:55:37
    Contrib: @FL03
*/
#![cfg(feature = "serde")]
use crate::{canonical_nonnegative_decimal_text, canonical_unsigned_integer_text};
use crate::types::CanonicalU256;
use core::str::FromStr;
use hashbrown::HashMap;
use polymarket::types::{Address, U256};
use serde::de::{Deserialize, Deserializer, Error as DeError};

pub fn deserialize_u256<'de, D>(deserializer: D) -> Result<U256, D::Error>
where
    D: Deserializer<'de>,
{
    Deserialize::<'de>::deserialize::<D>(deserializer)
}

pub fn deserialize_allowances<'de, D>(deserializer: D) -> Result<HashMap<Address, U256>, D::Error>
where
    D: Deserializer<'de>,
{
    struct Visitor;

    impl<'de> serde::de::Visitor<'de> for Visitor {
        type Value = HashMap<Address, U256>;

        fn expecting(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            formatter.write_str("an address-to-base-unit allowance object")
        }

        fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
        where
            M: serde::de::MapAccess<'de>,
        {
            let mut allowances = HashMap::with_capacity(map.size_hint().unwrap_or_default());
            while let Some((raw_address, CanonicalU256(value))) =
                map.next_entry::<String, CanonicalU256>()?
            {
                let address = Address::from_str(&raw_address)
                    .map_err(|_| serde::de::Error::custom("invalid allowance address"))?;
                if allowances.insert(address, value).is_some() {
                    return Err(serde::de::Error::custom("duplicate allowance address"));
                }
            }
            Ok(allowances)
        }
    }

    deserializer.deserialize_map(Visitor)
}

#[cfg(feature = "json")]
pub fn decode_json<T>(
    endpoint: crate::auth::AuthenticatedEndpoint,
    body: &[u8],
) -> Result<T, crate::auth::AuthenticatedEndpointError>
where
    T: serde::de::DeserializeOwned,
{
    crate::auth::AuthenticatedEndpoint::decode(endpoint, body)
}

pub fn quoted_i64<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    if !canonical_unsigned_integer_text(&value) {
        return Err(D::Error::custom("expected canonical quoted integer"));
    }
    value
        .parse()
        .map_err(|_| D::Error::custom("quoted integer is out of range"))
}

pub fn quoted_decimal<'de, D>(deserializer: D) -> Result<polymarket::types::Decimal, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    if !canonical_nonnegative_decimal_text(&value) {
        return Err(D::Error::custom(
            "expected canonical nonnegative decimal text",
        ));
    }
    polymarket::types::Decimal::from_str_exact(&value)
        .map_err(|_| D::Error::custom("expected exact quoted decimal"))
}

pub fn optional_quoted_decimal<'de, D>(
    deserializer: D,
) -> Result<Option<polymarket::types::Decimal>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    if value.is_empty() {
        return Ok(None);
    }
    if !canonical_nonnegative_decimal_text(&value) {
        return Err(D::Error::custom(
            "expected canonical nonnegative decimal text or empty string",
        ));
    }
    polymarket::types::Decimal::from_str_exact(&value)
        .map(Some)
        .map_err(|_| D::Error::custom("expected quoted decimal or empty string"))
}

/// Decode an on-chain transaction hash, treating an empty string as the venue's
/// documented "not yet broadcast" sentinel rather than a malformed value.
///
/// Mirrors the upstream SDK's `empty_string_as_zero_hash` semantics: servers
/// running the async execution pipeline create trades before broadcasting, so
/// `transaction_hash` is legitimately blank until settlement.
pub fn quoted_hash_or_zero<'de, D>(deserializer: D) -> Result<polymarket::types::B256, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    if value.trim().is_empty() {
        return Ok(polymarket::types::B256::ZERO);
    }
    value
        .parse()
        .map_err(|_| D::Error::custom("expected quoted transaction hash or empty string"))
}
