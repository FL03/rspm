/*
    Appellation: address <module>
    Created At: 2026.08.08:06:25:20
    Contrib: @FL03
*/
use alloc::string::{String, ToString};

#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize),
    serde(deny_unknown_fields, transparent)
)]
#[repr(transparent)]
pub struct WalletAddress(String);

impl WalletAddress {
    pub fn from_str<T>(value: T) -> Result<Self, crate::Error>
    where
        T: AsRef<str>,
    {
        core::str::FromStr::from_str(value.as_ref())
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
