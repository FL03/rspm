/*
    Appellation: error <module>
    Created At: 2026.08.08:06:37:28
    Contrib: @FL03
*/
#![cfg(feature = "alloc")]
use crate::auth::AuthenticatedEndpoint;

/// Bounded, non-secret classification of an authenticated request failure.
///
/// Every variant maps to a fixed `&'static str` drawn from a closed set. No
/// response body, header, or parser value can reach this type; the widest thing
/// it carries is an HTTP status *class*, which cannot encode a secret.
///
/// This exists because a fully detail-free `request_failed` made a
/// live-blocking auth failure undiagnosable in production: a rejected
/// credential (401), a permission or region refusal (403), throttling (429),
/// and a response over the local byte cap were all indistinguishable in the
/// logs, so `Live admission remains closed` named no mechanism a reader could
/// act on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum RequestFailureClass {
    /// No response was produced (connect, TLS, timeout, or stream abort).
    Transport,
    /// HTTP 401 — credentials absent, malformed, or rejected.
    Unauthorized,
    /// HTTP 403 — credentials understood but not permitted.
    Forbidden,
    /// HTTP 429 — throttled by the venue.
    RateLimited,
    /// Any other 4xx.
    ClientError,
    /// Any 5xx.
    ServerError,
    /// Response exceeded the local byte cap and never reached the parser.
    OversizedResponse,
    /// Raised below the HTTP layer, or by a call site predating classification.
    Unclassified,
}

impl RequestFailureClass {
    /// Stable, non-secret label. Safe for logs, metrics, and dashboards.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Transport => "transport",
            Self::Unauthorized => "unauthorized",
            Self::Forbidden => "forbidden",
            Self::RateLimited => "rate_limited",
            Self::ClientError => "client_error",
            Self::ServerError => "server_error",
            Self::OversizedResponse => "oversized_response",
            Self::Unclassified => "unclassified",
        }
    }

    /// Classify a received HTTP status. Only the status integer is consulted.
    #[must_use]
    pub const fn from_status(status: u16) -> Self {
        match status {
            401 => Self::Unauthorized,
            403 => Self::Forbidden,
            429 => Self::RateLimited,
            400..=499 => Self::ClientError,
            500..=599 => Self::ServerError,
            _ => Self::Unclassified,
        }
    }
}

impl core::fmt::Display for RequestFailureClass {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

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
    #[error(
        "authenticated endpoint `{endpoint_class}` request failed; response details redacted (class: {failure_class})"
    )]
    RequestFailed {
        endpoint_class: &'static str,
        failure_class: RequestFailureClass,
    },
}

/// Redacted failure from an authenticated venue endpoint.
///
/// Response bodies and parser values are deliberately unrepresentable. The only
/// response-derived details retained are a validated structural field path, a
/// one-way digest, and a bounded HTTP status class — none of which can carry a
/// secret, and each of which is drawn from a closed or validated set.
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

    /// Build a failure carrying no response-derived detail beyond the endpoint.
    ///
    /// Retained for call sites below the HTTP layer that genuinely have no
    /// status to report. Prefer [`Self::request_failed_as`] or
    /// [`Self::request_failed_for_status`] wherever a cause is known — an
    /// unclassified failure is what made this class undiagnosable in
    /// production.
    #[must_use]
    pub const fn request_failed(endpoint: AuthenticatedEndpoint) -> Self {
        Self::request_failed_as(endpoint, RequestFailureClass::Unclassified)
    }

    /// Build a request failure with an explicit, non-secret cause class.
    #[must_use]
    pub const fn request_failed_as(
        endpoint: AuthenticatedEndpoint,
        failure_class: RequestFailureClass,
    ) -> Self {
        Self(AuthenticatedEndpointErrorKind::RequestFailed {
            endpoint_class: endpoint.as_str(),
            failure_class,
        })
    }

    /// Build a request failure classified from a received HTTP status.
    ///
    /// Only the status integer is consulted; the response is never read.
    #[must_use]
    pub const fn request_failed_for_status(
        endpoint: AuthenticatedEndpoint,
        status: u16,
    ) -> Self {
        Self::request_failed_as(endpoint, RequestFailureClass::from_status(status))
    }

    /// Return the stable, non-secret endpoint class.
    #[must_use]
    pub const fn endpoint_class(&self) -> &'static str {
        match &self.0 {
            AuthenticatedEndpointErrorKind::ResponseSchemaDecode { endpoint_class, .. }
            | AuthenticatedEndpointErrorKind::RequestFailed { endpoint_class, .. } => endpoint_class,
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

    /// Return the non-secret cause class of a request failure.
    ///
    /// `None` for schema-decode failures, which carry `response_path` instead.
    #[must_use]
    pub const fn failure_class(&self) -> Option<RequestFailureClass> {
        match &self.0 {
            AuthenticatedEndpointErrorKind::RequestFailed { failure_class, .. } => {
                Some(*failure_class)
            }
            AuthenticatedEndpointErrorKind::ResponseSchemaDecode { .. } => None,
        }
    }

    /// Return the stable, non-secret failure class.
    ///
    /// Deliberately unchanged by cause classification: dashboards and the
    /// canonical-reject contract key on these exact strings. Read
    /// [`Self::failure_class`] for the cause.
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

    /// The exact substring `bin/node/tests/dev0_private_readiness_bridge.rs`
    /// asserts on. Cause classification appends AFTER it, never splices into
    /// it, so adding a cause cannot silently break that consumer.
    #[test]
    fn classified_request_failure_preserves_the_asserted_redaction_substring() {
        for class in [
            RequestFailureClass::Unauthorized,
            RequestFailureClass::OversizedResponse,
            RequestFailureClass::Unclassified,
        ] {
            let rendered = AuthenticatedEndpointError::request_failed_as(
                AuthenticatedEndpoint::ProtocolVersion,
                class,
            )
            .to_string();
            assert!(
                rendered.contains("request failed; response details redacted"),
                "downstream assertion substring must survive: {rendered}"
            );
            assert!(
                rendered.contains(class.as_str()),
                "cause must be legible in the message: {rendered}"
            );
        }
    }

    /// The defect this classification exists to remove: 401, 403, 429 and an
    /// over-cap response previously rendered IDENTICALLY, so a live-blocking
    /// auth failure named no mechanism. Assert they are now distinguishable
    /// from one another — a test that merely checked "a class is present"
    /// would pass even if every status mapped to the same variant.
    #[test]
    fn distinct_causes_render_distinguishably() {
        let endpoint = AuthenticatedEndpoint::ProtocolVersion;
        let rendered: alloc::vec::Vec<String> = [
            AuthenticatedEndpointError::request_failed_for_status(endpoint, 401),
            AuthenticatedEndpointError::request_failed_for_status(endpoint, 403),
            AuthenticatedEndpointError::request_failed_for_status(endpoint, 429),
            AuthenticatedEndpointError::request_failed_for_status(endpoint, 503),
            AuthenticatedEndpointError::request_failed_as(
                endpoint,
                RequestFailureClass::OversizedResponse,
            ),
            AuthenticatedEndpointError::request_failed_as(
                endpoint,
                RequestFailureClass::Transport,
            ),
        ]
        .iter()
        .map(alloc::string::ToString::to_string)
        .collect();

        let mut unique = rendered.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(
            unique.len(),
            rendered.len(),
            "each cause must render distinctly; got {rendered:?}"
        );
    }

    #[test]
    fn status_classification_covers_the_auth_relevant_codes() {
        assert_eq!(
            RequestFailureClass::from_status(401),
            RequestFailureClass::Unauthorized
        );
        assert_eq!(
            RequestFailureClass::from_status(403),
            RequestFailureClass::Forbidden
        );
        assert_eq!(
            RequestFailureClass::from_status(429),
            RequestFailureClass::RateLimited
        );
        assert_eq!(
            RequestFailureClass::from_status(422),
            RequestFailureClass::ClientError
        );
        assert_eq!(
            RequestFailureClass::from_status(500),
            RequestFailureClass::ServerError
        );
        assert_eq!(
            RequestFailureClass::from_status(503),
            RequestFailureClass::ServerError
        );
        // 2xx/3xx never reach this constructor; classify rather than panic.
        assert_eq!(
            RequestFailureClass::from_status(200),
            RequestFailureClass::Unclassified
        );
    }

    /// `request_failed` keeps its old meaning so untouched call sites below the
    /// HTTP layer stay correct rather than silently claiming a cause.
    #[test]
    fn bare_request_failed_is_unclassified() {
        let error =
            AuthenticatedEndpointError::request_failed(AuthenticatedEndpoint::ProtocolVersion);
        assert_eq!(
            error.failure_class(),
            Some(RequestFailureClass::Unclassified)
        );
    }

    /// `error_class` keys dashboards and the canonical-reject contract; cause
    /// classification must NOT perturb it.
    #[test]
    fn error_class_is_unchanged_by_cause() {
        let endpoint = AuthenticatedEndpoint::ProtocolVersion;
        assert_eq!(
            AuthenticatedEndpointError::request_failed(endpoint).error_class(),
            "request_failed"
        );
        assert_eq!(
            AuthenticatedEndpointError::request_failed_for_status(endpoint, 401).error_class(),
            "request_failed"
        );
    }

    #[test]
    fn schema_decode_carries_a_path_and_no_failure_class() {
        let error = AuthenticatedEndpointError::response_schema_decode(
            AuthenticatedEndpoint::ProtocolVersion,
            "?",
        );
        assert_eq!(error.failure_class(), None);
        assert_eq!(error.response_path(), Some("?"));
    }

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
