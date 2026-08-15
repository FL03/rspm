/*
    Appellation: version <module>
    Created At: 2026.08.08:07:45:57
    Contrib: @FL03
*/

#[cfg(feature = "alloc")]
use alloc::string::ToString;

/// Protocol contract generation reported by the CLOB.
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
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[strum(ascii_case_insensitive, serialize_all = "UPPERCASE")]
#[non_exhaustive]
pub enum ProtocolVersion {
    /// Legacy Polygon exchange contracts.
    V1,
    #[default]
    /// Current Polygon exchange contracts.
    V2,
}

impl ProtocolVersion {
    /// Stable wire value returned by the CLOB `/version` endpoint.
    #[must_use]
    pub const fn as_u32(self) -> u32 {
        match self {
            Self::V1 => 1,
            Self::V2 => 2,
        }
    }
}

impl TryFrom<u32> for ProtocolVersion {
    type Error = crate::Error;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::V1),
            2 => Ok(Self::V2),
            _ => Err(crate::error::Error::InvalidProtocolVersion(
                value.to_string(),
            )),
        }
    }
}
