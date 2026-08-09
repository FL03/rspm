/*
    Appellation: address <module>
    Created At: 2026.08.08:06:25:20
    Contrib: @FL03
*/
use alloc::string::{String, ToString};

const WALLET_ADDRESS_HEX_LEN: usize = 40;
const WALLET_ADDRESS_LEN: usize = WALLET_ADDRESS_HEX_LEN + 2;

/// A validated, canonical Ethereum wallet address.
///
/// Construction accepts exactly `0x` followed by 40 ASCII hexadecimal digits.
/// Hexadecimal digits are normalized to lowercase so equality, ordering, and
/// hashing cannot disagree solely because of address casing.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize),
    serde(deny_unknown_fields, transparent)
)]
#[repr(transparent)]
pub struct WalletAddress(String);

impl WalletAddress {
    /// Parse and validate a wallet address.
    pub fn from_str<T>(value: T) -> Result<Self, crate::Error>
    where
        T: AsRef<str>,
    {
        core::str::FromStr::from_str(value.as_ref())
    }

    /// Return the canonical wallet address as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Return an immutable reference to the canonical owned string.
    pub const fn get(&self) -> &String {
        &self.0
    }

    /// Consume the address and return its canonical owned string.
    pub fn value(self) -> String {
        self.0
    }
}

impl core::fmt::Display for WalletAddress {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl core::str::FromStr for WalletAddress {
    type Err = crate::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let Some(hex) = value.strip_prefix("0x") else {
            return Err(crate::Error::InvalidAddress(value.to_string()));
        };
        if value.len() != WALLET_ADDRESS_LEN
            || hex.len() != WALLET_ADDRESS_HEX_LEN
            || !hex.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(crate::Error::InvalidAddress(value.to_string()));
        }

        let mut canonical = value.to_string();
        canonical.make_ascii_lowercase();
        Ok(Self(canonical))
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for WalletAddress {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = <String as serde::Deserialize>::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

impl AsRef<str> for WalletAddress {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl core::borrow::Borrow<str> for WalletAddress {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl core::ops::Deref for WalletAddress {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}
