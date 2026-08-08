/*
    Appellation: transport <module>
    Created At: 2026.08.08:08:19:18
    Contrib: @FL03
*/
//! Strict balance/version wire decoding and authenticated transport.
use crate::auth::{
    AuthenticatedBalanceSnapshot, AuthenticatedEndpoint, AuthenticatedEndpointError,
    AuthenticatedProtocolAuthority, COLLATERAL_SCALE,
};
use crate::types::{ProtocolVersion, RawBalanceAllowanceResponse};
use hashbrown::HashMap;
use polymarket::{
    POLYGON,
    types::{Address, Decimal, U256},
};

pub fn required_spenders(
    version: ProtocolVersion,
) -> Result<[Address; 3], AuthenticatedEndpointError> {
    let endpoint = AuthenticatedEndpoint::BalanceAllowance;
    let normal = polymarket::contract_config(POLYGON, false)
        .ok_or_else(|| AuthenticatedEndpointError::request_failed(endpoint))?;
    let neg_risk = polymarket::contract_config(POLYGON, true)
        .ok_or_else(|| AuthenticatedEndpointError::request_failed(endpoint))?;
    let neg_risk_adapter = neg_risk
        .neg_risk_adapter
        .ok_or_else(|| AuthenticatedEndpointError::request_failed(endpoint))?;
    let (normal_exchange, neg_risk_exchange) = match version {
        ProtocolVersion::V1 => (normal.exchange, neg_risk.exchange),
        ProtocolVersion::V2 => (
            normal
                .exchange_v2
                .ok_or_else(|| AuthenticatedEndpointError::request_failed(endpoint))?,
            neg_risk
                .exchange_v2
                .ok_or_else(|| AuthenticatedEndpointError::request_failed(endpoint))?,
        ),
    };
    Ok([normal_exchange, neg_risk_exchange, neg_risk_adapter])
}

pub fn select_required_snapshot(
    raw: RawBalanceAllowanceResponse,
    protocol_authority: AuthenticatedProtocolAuthority,
) -> Result<AuthenticatedBalanceSnapshot, AuthenticatedEndpointError> {
    let mut required = HashMap::with_capacity(3);
    for spender in required_spenders(protocol_authority.version())? {
        let allowance = raw.allowances.get(&spender).copied().ok_or_else(|| {
            AuthenticatedEndpointError::response_schema_decode(
                AuthenticatedEndpoint::BalanceAllowance,
                "allowances",
            )
        })?;
        required.insert(spender, allowance);
    }
    AuthenticatedBalanceSnapshot::try_from_required_raw(protocol_authority, raw.balance, required)
}

pub fn scale_collateral(raw: U256) -> Result<Decimal, AuthenticatedEndpointError> {
    let scale = U256::from(COLLATERAL_SCALE);
    let whole = raw / scale;
    let fraction = (raw % scale).to_string();
    let mut text = whole.to_string();
    text.push('.');
    text.extend(std::iter::repeat_n(
        '0',
        6_usize.saturating_sub(fraction.len()),
    ));
    text.push_str(&fraction);
    Decimal::from_str_exact(&text).map_err(|_| {
        AuthenticatedEndpointError::response_schema_decode(
            crate::auth::AuthenticatedEndpoint::BalanceAllowance,
            "balance",
        )
    })
}
