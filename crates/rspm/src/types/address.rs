/*
    Appellation: address <module>
    Created At: 2026.08.08:06:25:20
    Contrib: @FL03
*/
use alloc::string::{String, ToString};

/// A type defining compatible wallet or network addresses for on-chain entities.
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Deserialize, serde::Serialize),
    serde(deny_unknown_fields, transparent)
)]
#[repr(transparent)]
pub struct WalletAddress(pub String);

impl WalletAddress {
    pub fn from_str<T>(value: T) -> Result<Self, crate::Error>
    where
        T: AsRef<str>,
    {
        core::str::FromStr::from_str(value.as_ref())
    }
    /// returns a reference to the wallet address as a `str`
    pub fn as_str(&self) -> &str {
        &self.0
    }
    /// returns an immutable reference to the string value of the address
    pub const fn get(&self) -> &String {
        &self.0
    }

    pub const fn get_mut(&mut self) -> &mut String {
        &mut self.0
    }
    /// consumes the current instance, returning the inner value
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

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != 42 || !s.starts_with("0x") {
            return Err(crate::Error::InvalidAddress(s.to_string()));
        }
        Ok(Self(s.to_string()))
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
