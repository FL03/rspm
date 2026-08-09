use std::backtrace::Backtrace;
use std::error::Error as StdError;
use std::fmt;

use alloy::primitives::ChainId;
use alloy::primitives::ruint::ParseError;
use hmac::digest::InvalidLength;
/// HTTP method type, re-exported for use with error inspection.
pub use reqwest::Method;
/// HTTP status code type, re-exported for use with error inspection.
pub use reqwest::StatusCode;
use reqwest::header;

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Error related to non-successful HTTP call
    Status,
    /// Error related to invalid state within polymarket-client-sdk
    Validation,
    /// Error related to synchronization of authenticated clients logging in and out
    Synchronization,
    /// Internal error from dependencies
    Internal,
    /// Error related to WebSocket connections
    WebSocket,
    /// Error related to geographic restrictions blocking access
    Geoblock,
}

#[derive(Debug)]
pub struct Error {
    kind: Kind,
    source: Option<Box<dyn StdError + Send + Sync + 'static>>,
    backtrace: Backtrace,
}

impl Error {
    pub fn with_source<S: StdError + Send + Sync + 'static>(kind: Kind, source: S) -> Self {
        Self {
            kind,
            source: Some(Box::new(source)),
            backtrace: Backtrace::capture(),
        }
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    pub fn backtrace(&self) -> &Backtrace {
        &self.backtrace
    }

    pub fn inner(&self) -> Option<&(dyn StdError + Send + Sync + 'static)> {
        self.source.as_deref()
    }

    pub fn downcast_ref<E: StdError + 'static>(&self) -> Option<&E> {
        let e = self.source.as_deref()?;
        e.downcast_ref::<E>()
    }

    pub fn validation<S: Into<String>>(message: S) -> Self {
        Validation {
            reason: message.into(),
        }
        .into()
    }

    pub fn status<S: Into<String>>(
        status_code: StatusCode,
        method: Method,
        path: String,
        message: S,
    ) -> Self {
        let path = redacted_request_path(&path);
        let message = redacted_status_message(status_code, &message.into());
        Status {
            status_code,
            method,
            path,
            message,
        }
        .into()
    }

    #[must_use]
    pub fn missing_contract_config(chain_id: ChainId, neg_risk: bool) -> Self {
        MissingContractConfig { chain_id, neg_risk }.into()
    }
}

pub(crate) fn redacted_request_path(raw: &str) -> String {
    let path = raw.split_once('?').map_or(raw, |(path, _)| path);
    for (prefix, template) in [
        ("/data/order/", "/data/order/{order_id}"),
        ("/clob-markets/", "/clob-markets/{condition_id}"),
        ("/markets/slug/", "/markets/slug/{slug}"),
        ("/markets/", "/markets/{market_id}"),
        ("/events/slug/", "/events/slug/{slug}"),
        ("/events/", "/events/{event_id}"),
        ("/series/", "/series/{series_id}"),
        ("/tags/slug/", "/tags/slug/{slug}"),
        ("/tags/", "/tags/{tag_id}"),
        ("/comments/", "/comments/{comment_id}"),
        ("/status/", "/status/{transaction_id}"),
        ("/rewards/markets/", "/rewards/markets/{market_id}"),
    ] {
        if path
            .strip_prefix(prefix)
            .is_some_and(|identity| !identity.is_empty())
        {
            return template.to_owned();
        }
    }

    const STATIC_PATHS: &[&str] = &[
        "/",
        "/activity",
        "/auth/api-key",
        "/auth/api-keys",
        "/auth/ban-status/closed-only",
        "/auth/builder-api-key",
        "/auth/derive-api-key",
        "/auth/readonly-api-key",
        "/balance-allowance",
        "/balance-allowance/update",
        "/book",
        "/books",
        "/builder/trades",
        "/cancel-all",
        "/cancel-market-orders",
        "/closed-positions",
        "/comments",
        "/data/orders",
        "/data/trades",
        "/deposit",
        "/events",
        "/fee-rate",
        "/holders",
        "/last-trade-price",
        "/last-trades-prices",
        "/live-volume",
        "/markets",
        "/midpoint",
        "/midpoints",
        "/neg-risk",
        "/notifications",
        "/oi",
        "/order",
        "/order-scoring",
        "/orders",
        "/orders-scoring",
        "/positions",
        "/price",
        "/prices",
        "/prices-history",
        "/public-search",
        "/quote",
        "/rewards/markets/current",
        "/rewards/user",
        "/rewards/user/markets",
        "/rewards/user/percentages",
        "/rewards/user/total",
        "/rfq/data/quotes",
        "/rfq/data/requests",
        "/rfq/quote",
        "/rfq/quote/approve",
        "/rfq/request",
        "/rfq/request/accept",
        "/sampling-markets",
        "/sampling-simplified-markets",
        "/series",
        "/simplified-markets",
        "/sports",
        "/sports/market-types",
        "/spread",
        "/spreads",
        "/status",
        "/supported-assets",
        "/tags",
        "/teams",
        "/tick-size",
        "/time",
        "/traded",
        "/trades",
        "/v1/builders/leaderboard",
        "/v1/builders/volume",
        "/v1/heartbeats",
        "/v1/leaderboard",
        "/value",
        "/version",
        "/withdraw",
    ];
    if STATIC_PATHS.contains(&path) {
        path.to_owned()
    } else {
        "/redacted".to_owned()
    }
}

fn redacted_status_message(status_code: StatusCode, raw: &str) -> String {
    let normalized = raw.to_ascii_lowercase();
    if normalized.contains("order_version_mismatch") {
        return "order_version_mismatch".to_owned();
    }
    if normalized.contains("not enough balance / allowance") && normalized.contains("balance: 0") {
        return "balance_allowance_authority".to_owned();
    }
    if normalized.contains("no orders")
        || (normalized.contains("fak") && normalized.contains("match"))
    {
        return "no_match".to_owned();
    }
    if status_code == StatusCode::TOO_MANY_REQUESTS {
        return bounded_retry_after_seconds(&normalized)
            .map(|seconds| format!("retry_after={seconds}"))
            .unwrap_or_else(|| "rate_limited".to_owned());
    }
    "redacted".to_owned()
}

fn bounded_retry_after_seconds(raw: &str) -> Option<u64> {
    const MAX_RETRY_AFTER_SECONDS: u64 = 86_400;
    let marker = raw
        .find("retry_after")
        .or_else(|| raw.find("retry-after"))?;
    let tail = raw.as_bytes().get(marker..)?;
    let mut value = 0_u64;
    let mut saw_digit = false;
    for byte in tail {
        if byte.is_ascii_digit() {
            saw_digit = true;
            value = value
                .saturating_mul(10)
                .saturating_add(u64::from(*byte - b'0'));
        } else if saw_digit {
            break;
        }
    }
    saw_digit.then(|| value.min(MAX_RETRY_AFTER_SECONDS))
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.source {
            Some(src) => write!(f, "{:?}: {}", self.kind, src),
            None => write!(f, "{:?}", self.kind),
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.source
            .as_deref()
            .map(|e| e as &(dyn StdError + 'static))
    }
}

#[non_exhaustive]
#[derive(Debug)]
pub struct Status {
    /// HTTP response class.
    pub status_code: StatusCode,
    /// HTTP request method.
    pub method: Method,
    /// Closed endpoint template retained for compatibility with status inspection.
    pub path: String,
    /// Closed response class. Raw response text is never retained here.
    pub message: String,
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "error({}) making {} call to {} with {}",
            self.status_code, self.method, self.path, self.message
        )
    }
}

impl StdError for Status {}

#[non_exhaustive]
#[derive(Debug)]
pub struct Validation {
    pub reason: String,
}

impl fmt::Display for Validation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid: {}", self.reason)
    }
}

impl StdError for Validation {}

#[non_exhaustive]
#[derive(Debug)]
pub struct Synchronization;

impl fmt::Display for Synchronization {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "synchronization error: multiple threads are attempting to log in or log out"
        )
    }
}

impl StdError for Synchronization {}

#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
pub struct MissingContractConfig {
    pub chain_id: ChainId,
    pub neg_risk: bool,
}

impl fmt::Display for MissingContractConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "missing contract config for chain id {} with neg_risk = {}",
            self.chain_id, self.neg_risk,
        )
    }
}

impl std::error::Error for MissingContractConfig {}

impl From<MissingContractConfig> for Error {
    fn from(err: MissingContractConfig) -> Self {
        Error::with_source(Kind::Internal, err)
    }
}

/// Error indicating that the user is blocked from accessing Polymarket due to geographic
/// restrictions.
///
/// This error contains information about the user's detected location.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct Geoblock {
    /// The detected IP address
    pub ip: String,
    /// ISO 3166-1 alpha-2 country code
    pub country: String,
    /// Region/state code
    pub region: String,
}

impl fmt::Display for Geoblock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "access blocked from country: {}, region: {}, ip: {}",
            self.country, self.region, self.ip
        )
    }
}

impl StdError for Geoblock {}

impl From<Geoblock> for Error {
    fn from(err: Geoblock) -> Self {
        Error::with_source(Kind::Geoblock, err)
    }
}

impl From<base64::DecodeError> for Error {
    fn from(e: base64::DecodeError) -> Self {
        Error::with_source(Kind::Internal, e)
    }
}

#[derive(Debug)]
struct HttpTransport;

impl fmt::Display for HttpTransport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("HTTP transport failed; request details redacted")
    }
}

impl StdError for HttpTransport {}

impl From<reqwest::Error> for Error {
    fn from(_error: reqwest::Error) -> Self {
        Error::with_source(Kind::Internal, HttpTransport)
    }
}

impl From<header::InvalidHeaderValue> for Error {
    fn from(e: header::InvalidHeaderValue) -> Self {
        Error::with_source(Kind::Internal, e)
    }
}

impl From<InvalidLength> for Error {
    fn from(e: InvalidLength) -> Self {
        Error::with_source(Kind::Internal, e)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::with_source(Kind::Internal, e)
    }
}

impl From<alloy::signers::Error> for Error {
    fn from(e: alloy::signers::Error) -> Self {
        Error::with_source(Kind::Internal, e)
    }
}

impl From<url::ParseError> for Error {
    fn from(e: url::ParseError) -> Self {
        Error::with_source(Kind::Internal, e)
    }
}

impl From<ParseError> for Error {
    fn from(e: ParseError) -> Self {
        Error::with_source(Kind::Internal, e)
    }
}

impl From<Validation> for Error {
    fn from(err: Validation) -> Self {
        Error::with_source(Kind::Validation, err)
    }
}

impl From<Status> for Error {
    fn from(err: Status) -> Self {
        Error::with_source(Kind::Status, err)
    }
}

impl From<Synchronization> for Error {
    fn from(err: Synchronization) -> Self {
        Error::with_source(Kind::Synchronization, err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geoblock_display_should_succeed() {
        let geoblock = Geoblock {
            ip: "192.168.1.1".to_owned(),
            country: "US".to_owned(),
            region: "NY".to_owned(),
        };

        assert_eq!(
            geoblock.to_string(),
            "access blocked from country: US, region: NY, ip: 192.168.1.1"
        );
    }

    #[test]
    fn geoblock_into_error_should_succeed() {
        let geoblock = Geoblock {
            ip: "10.0.0.1".to_owned(),
            country: "CU".to_owned(),
            region: "HAV".to_owned(),
        };

        let error: Error = geoblock.into();

        assert_eq!(error.kind(), Kind::Geoblock);
        assert!(error.to_string().contains("CU"));
    }

    #[test]
    fn hostile_status_body_is_reduced_to_a_closed_class() {
        let hostile = "Bearer private-key raw-body https://secret.invalid/order/account-token";
        let error = Error::status(
            StatusCode::BAD_REQUEST,
            Method::POST,
            "/order".to_owned(),
            hostile,
        );
        let status = error.downcast_ref::<Status>().expect("status error");
        assert_eq!(status.message, "redacted");
        let rendered = format!("{error:?} {error}");
        for secret in [
            "Bearer",
            "private-key",
            "raw-body",
            "secret.invalid",
            "account-token",
        ] {
            assert!(!rendered.contains(secret), "status error leaked {secret}");
        }
    }

    #[test]
    fn hostile_dynamic_path_is_reduced_to_an_endpoint_template() {
        let hostile_order = "private-order-id-0xdeadbeef";
        let hostile_wallet = "private-wallet-0xfeedface";
        let error = Error::status(
            StatusCode::BAD_REQUEST,
            Method::GET,
            format!("/data/order/{hostile_order}?owner={hostile_wallet}"),
            "redacted",
        );
        let status = error.downcast_ref::<Status>().expect("status error");
        assert_eq!(status.path, "/data/order/{order_id}");
        let rendered = format!("{error:?} {error}");
        for secret in [hostile_order, hostile_wallet, "owner="] {
            assert!(!rendered.contains(secret), "status path leaked {secret}");
        }
    }

    #[test]
    fn retry_and_no_match_status_classes_retain_no_raw_body() {
        let retry = Error::status(
            StatusCode::TOO_MANY_REQUESTS,
            Method::POST,
            "/order".to_owned(),
            "token=secret retry_after:999999999 trailing-private-data",
        );
        assert_eq!(
            retry
                .downcast_ref::<Status>()
                .expect("rate-limit status")
                .message,
            "retry_after=86400"
        );

        let no_match = Error::status(
            StatusCode::BAD_REQUEST,
            Method::POST,
            "/order".to_owned(),
            "secret no orders found to match with FAK order",
        );
        assert_eq!(
            no_match
                .downcast_ref::<Status>()
                .expect("no-match status")
                .message,
            "no_match"
        );
        let rendered = format!("{retry:?} {retry} {no_match:?} {no_match}");
        for secret in ["secret", "trailing-private-data", "999999999"] {
            assert!(!rendered.contains(secret), "status class leaked {secret}");
        }
    }

    #[test]
    fn stale_domain_balance_zero_has_a_closed_authority_class() {
        let raw = "not enough balance / allowance: balance: 0 private-order-id";
        let error = Error::status(
            StatusCode::BAD_REQUEST,
            Method::POST,
            "/order".to_owned(),
            raw,
        );
        let status = error.downcast_ref::<Status>().expect("status error");
        assert_eq!(status.message, "balance_allowance_authority");
        let rendered = format!("{error:?} {error}");
        for secret in ["private-order-id", "not enough", "balance: 0"] {
            assert!(
                !rendered.contains(secret),
                "authority class leaked {secret}"
            );
        }
    }

    #[tokio::test]
    async fn hostile_transport_url_is_not_retained_by_the_error() {
        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("reserve loopback endpoint");
        let address = listener.local_addr().expect("read loopback endpoint");
        drop(listener);
        let hostile_path = "private-order-id-0xdeadbeef";
        let hostile_query = "wallet=private-wallet-0xfeedface";
        let error = reqwest::Client::new()
            .get(format!("http://{address}/{hostile_path}?{hostile_query}"))
            .send()
            .await
            .expect_err("closed loopback endpoint must reject the request");
        let error = Error::from(error);
        let rendered = format!("{error:?} {error}");
        assert!(rendered.contains("request details redacted"));
        for secret in [hostile_path, hostile_query, "wallet="] {
            assert!(
                !rendered.contains(secret),
                "transport error leaked {secret}"
            );
        }
    }
}
