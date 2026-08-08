/*
    Appellation: version <module>
    Created At: 2026.08.08:08:17:38
    Contrib: @FL03
*/
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Deserialize, serde::Serialize),
    serde(deny_unknown_fields)
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ProtocolVersionResponse {
    pub(crate) version: u32,
}

impl ProtocolVersionResponse {
    /// initialize a new instance of the [`ProtocolVersionResponse`].
    pub const fn new(version: u32) -> Self {
        Self { version }
    }
    /// consumes the current instance, returning another with the given version.
    pub fn with_version(self, version: u32) -> Self {
        Self { version }
    }
    /// Stable wire value returned by the CLOB `/version` endpoint.
    #[must_use]
    pub const fn version(&self) -> u32 {
        self.version
    }
}
