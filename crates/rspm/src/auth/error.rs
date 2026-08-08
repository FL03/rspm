/*
    Appellation: error <module>
    Created At: 2026.08.08:06:37:28
    Contrib: @FL03
*/
#![cfg(feature = "alloc")]
use crate::auth::AuthenticatedEndpoint;

#[derive(Debug, thiserror::Error)]
enum AuthenticatedEndpointErrorKind {
    #[error(
        "authenticated endpoint `{endpoint_class}` response schema decode failed at `{response_path}`"
    )]
    ResponseSchemaDecode {
        endpoint_class: &'static str,
        response_path: String,
        response_digest: Option<String>,
    },
    #[error("authenticated endpoint `{endpoint_class}` request failed; response details redacted")]
    RequestFailed { endpoint_class: &'static str },
}

/// Redacted failure from an authenticated venue endpoint.
///
/// Response bodies and parser values are deliberately unrepresentable. The
/// only response detail retained is a validated structural field path.
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct AuthenticatedEndpointError(AuthenticatedEndpointErrorKind);

fn authenticated_response_path_is_safe(endpoint: AuthenticatedEndpoint, path: &str) -> bool {
    const TRADE_FIELDS: &[&str] = &[
        "asset_id",
        "bucket_index",
        "count",
        "data",
        "error_msg",
        "fee_rate_bps",
        "id",
        "last_update",
        "limit",
        "maker_address",
        "maker_orders",
        "market",
        "match_time",
        "matched_amount",
        "next_cursor",
        "order_id",
        "outcome",
        "owner",
        "price",
        "side",
        "size",
        "status",
        "taker_order_id",
        "trader_side",
        "transaction_hash",
    ];

    if path == "?" {
        return true;
    }

    const ORDER_FIELDS: &[&str] = &[
        "asset_id",
        "count",
        "created_at",
        "data",
        "expiration",
        "id",
        "limit",
        "market",
        "next_cursor",
        "original_size",
        "price",
        "side",
        "size_matched",
        "status",
    ];
    const BALANCE_FIELDS: &[&str] = &["allowances", "balance"];
    const VERSION_FIELDS: &[&str] = &["version"];
    const POSITION_FIELDS: &[&str] = &[
        "asset",
        "avgPrice",
        "conditionId",
        "mergeable",
        "outcome",
        "proxyWallet",
        "redeemable",
        "size",
        "slug",
    ];
    let fields = match endpoint {
        AuthenticatedEndpoint::ProtocolVersion => VERSION_FIELDS,
        AuthenticatedEndpoint::Trades => TRADE_FIELDS,
        AuthenticatedEndpoint::Order => ORDER_FIELDS,
        AuthenticatedEndpoint::BalanceAllowance => BALANCE_FIELDS,
        AuthenticatedEndpoint::Positions => POSITION_FIELDS,
    };

    path.split('.').all(|segment| {
        let field_end = segment.find('[').unwrap_or(segment.len());
        let (field, mut indexes) = segment.split_at(field_end);
        if !fields.contains(&field) {
            return false;
        }
        while !indexes.is_empty() {
            let Some(index) = indexes.strip_prefix('[') else {
                return false;
            };
            let Some(end) = index.find(']') else {
                return false;
            };
            let (digits, remaining) = index.split_at(end);
            if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
                return false;
            }
            indexes = &remaining[1..];
        }
        true
    })
}

#[cfg(feature = "alloc")]
impl AuthenticatedEndpointError {
    /// Build a response-schema failure, degrading unsafe paths to a fully
    /// redacted request failure.
    #[must_use]
    pub fn response_schema_decode(
        endpoint: AuthenticatedEndpoint,
        response_path: impl Into<String>,
    ) -> Self {
        let response_path = response_path.into();
        if !authenticated_response_path_is_safe(endpoint, &response_path) {
            return Self::request_failed(endpoint);
        }

        Self(AuthenticatedEndpointErrorKind::ResponseSchemaDecode {
            endpoint_class: endpoint.as_str(),
            response_path,
            response_digest: None,
        })
    }

    /// Build a failure that carries no response-derived detail.
    #[must_use]
    pub const fn request_failed(endpoint: AuthenticatedEndpoint) -> Self {
        Self(AuthenticatedEndpointErrorKind::RequestFailed {
            endpoint_class: endpoint.as_str(),
        })
    }

    /// Return the stable, non-secret endpoint class.
    #[must_use]
    pub const fn endpoint_class(&self) -> &'static str {
        match &self.0 {
            AuthenticatedEndpointErrorKind::ResponseSchemaDecode { endpoint_class, .. }
            | AuthenticatedEndpointErrorKind::RequestFailed { endpoint_class } => endpoint_class,
        }
    }

    /// Return the validated structural response path, when available.
    #[must_use]
    pub fn response_path(&self) -> Option<&str> {
        match &self.0 {
            AuthenticatedEndpointErrorKind::ResponseSchemaDecode { response_path, .. } => {
                Some(response_path)
            }
            AuthenticatedEndpointErrorKind::RequestFailed { .. } => None,
        }
    }

    /// Attach only a SHA-256 digest of the rejected response. The response
    /// bytes themselves remain unrepresentable at every public boundary.
    #[cfg(feature = "sha2")]
    #[must_use]
    pub fn with_response_digest(mut self, body: &[u8]) -> Self {
        use sha2::{Digest as _, Sha256};

        if let AuthenticatedEndpointErrorKind::ResponseSchemaDecode {
            response_digest, ..
        } = &mut self.0
        {
            *response_digest = Some(crate::utils::to_hex(Sha256::digest(body).as_slice()));
        }
        self
    }

    /// Return the one-way digest of a rejected response, when decoding began.
    #[must_use]
    pub fn response_digest(&self) -> Option<&str> {
        match &self.0 {
            AuthenticatedEndpointErrorKind::ResponseSchemaDecode {
                response_digest, ..
            } => response_digest.as_deref(),
            AuthenticatedEndpointErrorKind::RequestFailed { .. } => None,
        }
    }

    /// Return the stable, non-secret failure class.
    #[must_use]
    pub const fn error_class(&self) -> &'static str {
        match &self.0 {
            AuthenticatedEndpointErrorKind::ResponseSchemaDecode { .. } => "response_schema_decode",
            AuthenticatedEndpointErrorKind::RequestFailed { .. } => "request_failed",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authenticated_trade_diagnostic_accepts_only_owned_structural_paths() {
        let safe = AuthenticatedEndpointError::response_schema_decode(
            AuthenticatedEndpoint::Trades,
            "data[0].maker_orders[17].fee_rate_bps",
        );
        assert_eq!(
            safe.response_path(),
            Some("data[0].maker_orders[17].fee_rate_bps")
        );

        for unsafe_path in [
            "Bearer",
            "data[secret]",
            "data[0].unknown_field",
            "data[0].owner.api_key_value",
            "data[0]..fee_rate_bps",
        ] {
            let redacted = AuthenticatedEndpointError::response_schema_decode(
                AuthenticatedEndpoint::Trades,
                unsafe_path,
            );
            assert_eq!(redacted.error_class(), "request_failed");
            assert_eq!(redacted.response_path(), None);
            assert!(!redacted.to_string().contains(unsafe_path));
        }
    }

    #[test]
    fn sdk_owned_endpoints_never_expose_response_paths() {
        for endpoint in [
            AuthenticatedEndpoint::Order,
            AuthenticatedEndpoint::BalanceAllowance,
        ] {
            let redacted = AuthenticatedEndpointError::response_schema_decode(
                endpoint,
                "data[0].fee_rate_bps",
            );
            assert_eq!(redacted.error_class(), "request_failed");
            assert_eq!(redacted.response_path(), None);
        }
    }
}
