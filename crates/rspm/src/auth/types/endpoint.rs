/*
    Appellation: endpoint <module>
    Created At: 2026.08.08:06:31:01
    Contrib: @FL03
*/



/// Authenticated endpoint classes whose names are safe to emit in diagnostics.
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
#[non_exhaustive]
pub enum AuthenticatedEndpoint {
    #[default]
    /// Public protocol-version read required to resolve active spender contracts.
    ProtocolVersion,
    /// Authenticated trade-history pages.
    Trades,
    /// One authenticated order snapshot.
    Order,
    /// Authenticated collateral balance and allowance snapshot.
    BalanceAllowance,
    /// Public Data API position inventory for the configured proxy wallet.
    Positions,
}

impl AuthenticatedEndpoint {
    #[cfg(feature = "json")]
    pub fn decode<T>(self, body: &[u8]) -> Result<T, crate::auth::AuthenticatedEndpointError>
    where
        T: serde::de::DeserializeOwned,
    {
        let mut deserializer = serde_json::Deserializer::from_slice(body);
        let value = serde_path_to_error::deserialize(&mut deserializer).map_err(|error| {
            crate::auth::AuthenticatedEndpointError::response_schema_decode(
                self,
                error.path().to_string(),
            )
        })?;
        deserializer.end().map_err(|_| {
            crate::auth::AuthenticatedEndpointError::response_schema_decode(self, "?")
        })?;
        Ok(value)
    }
    /// Return the stable diagnostic class for this endpoint.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ProtocolVersion => "clob.protocol_version",
            Self::Trades => "clob.authenticated_trades",
            Self::Order => "clob.authenticated_order",
            Self::BalanceAllowance => "clob.authenticated_balance_allowance",
            Self::Positions => "data.position_inventory",
        }
    }
}
