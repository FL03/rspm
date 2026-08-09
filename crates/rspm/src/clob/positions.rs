//! Exact public Polymarket Data API position inventory.

use std::{fmt, str::FromStr as _, time::Duration};

use hashbrown::HashSet;

use polymarket::types::{Address, B256, Decimal, U256};
use serde::Deserialize;
use serde_json::value::RawValue;

use crate::{canonical_unsigned_integer_text, decode_json};

const POSITIONS_URL: &str = "https://data-api.polymarket.com/positions";
const PAGE_LIMIT: usize = 500;
const MAX_OFFSET: usize = 10_000;
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// Exact current holding returned for the configured proxy wallet.
#[derive(Clone, Eq, PartialEq)]
pub struct PositionInventoryEntry {
    proxy_wallet: Address,
    asset: U256,
    condition_id: B256,
    size: Decimal,
    average_price: Decimal,
    slug: String,
    outcome: String,
    redeemable: bool,
    mergeable: bool,
}

impl PositionInventoryEntry {
    #[must_use]
    pub const fn proxy_wallet(&self) -> Address {
        self.proxy_wallet
    }

    #[must_use]
    pub const fn asset(&self) -> U256 {
        self.asset
    }

    #[must_use]
    pub const fn condition_id(&self) -> B256 {
        self.condition_id
    }

    #[must_use]
    pub const fn size(&self) -> Decimal {
        self.size
    }

    #[must_use]
    pub const fn average_price(&self) -> Decimal {
        self.average_price
    }

    #[must_use]
    pub fn slug(&self) -> &str {
        &self.slug
    }

    #[must_use]
    pub fn outcome(&self) -> &str {
        &self.outcome
    }

    #[must_use]
    pub const fn redeemable(&self) -> bool {
        self.redeemable
    }

    #[must_use]
    pub const fn mergeable(&self) -> bool {
        self.mergeable
    }
}

impl fmt::Debug for PositionInventoryEntry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PositionInventoryEntry")
            .field("size", &self.size)
            .field("average_price", &self.average_price)
            .field("redeemable", &self.redeemable)
            .field("mergeable", &self.mergeable)
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WirePosition {
    proxy_wallet: String,
    asset: String,
    condition_id: String,
    size: Box<RawValue>,
    avg_price: Box<RawValue>,
    slug: String,
    outcome: String,
    redeemable: bool,
    mergeable: bool,
}

impl TryFrom<WirePosition> for PositionInventoryEntry {
    type Error = crate::auth::AuthenticatedEndpointError;

    fn try_from(position: WirePosition) -> Result<Self, Self::Error> {
        let endpoint = crate::auth::AuthenticatedEndpoint::Positions;
        let proxy_wallet = Address::from_str(&position.proxy_wallet).map_err(|_| {
            crate::auth::AuthenticatedEndpointError::response_schema_decode(endpoint, "proxyWallet")
        })?;
        if !canonical_unsigned_integer_text(&position.asset) {
            return Err(
                crate::auth::AuthenticatedEndpointError::response_schema_decode(endpoint, "asset"),
            );
        }
        let asset = U256::from_str(&position.asset).map_err(|_| {
            crate::auth::AuthenticatedEndpointError::response_schema_decode(endpoint, "asset")
        })?;
        let condition_id = B256::from_str(&position.condition_id).map_err(|_| {
            crate::auth::AuthenticatedEndpointError::response_schema_decode(endpoint, "conditionId")
        })?;
        let size = exact_json_decimal(&position.size, "size")?;
        let average_price = exact_json_decimal(&position.avg_price, "avgPrice")?;
        if size <= Decimal::ZERO
            || average_price <= Decimal::ZERO
            || average_price > Decimal::ONE
            || position.slug.is_empty()
            || position.slug.len() > 512
            || position.outcome.is_empty()
            || position.outcome.len() > 256
        {
            return Err(crate::auth::AuthenticatedEndpointError::request_failed(
                endpoint,
            ));
        }
        Ok(Self {
            proxy_wallet,
            asset,
            condition_id,
            size,
            average_price,
            slug: position.slug,
            outcome: position.outcome,
            redeemable: position.redeemable,
            mergeable: position.mergeable,
        })
    }
}

fn exact_json_decimal(
    raw: &RawValue,
    path: &'static str,
) -> Result<Decimal, crate::auth::AuthenticatedEndpointError> {
    let text = raw.get();
    let canonical = crate::canonical_nonnegative_decimal_text(text);
    if !canonical {
        return Err(
            crate::auth::AuthenticatedEndpointError::response_schema_decode(
                crate::auth::AuthenticatedEndpoint::Positions,
                path,
            ),
        );
    }
    Decimal::from_str_exact(text).map_err(|_| {
        crate::auth::AuthenticatedEndpointError::response_schema_decode(
            crate::auth::AuthenticatedEndpoint::Positions,
            path,
        )
    })
}

fn decode_page(
    body: &[u8],
) -> Result<Vec<PositionInventoryEntry>, crate::auth::AuthenticatedEndpointError> {
    let wire: Vec<WirePosition> = decode_json(crate::auth::AuthenticatedEndpoint::Positions, body)?;
    if wire.len() > PAGE_LIMIT {
        return Err(crate::auth::AuthenticatedEndpointError::request_failed(
            crate::auth::AuthenticatedEndpoint::Positions,
        ));
    }
    wire.into_iter()
        .map(PositionInventoryEntry::try_from)
        .collect()
}

fn next_offset(
    offset: usize,
    page_len: usize,
) -> Result<Option<usize>, crate::auth::AuthenticatedEndpointError> {
    if page_len < PAGE_LIMIT {
        return Ok(None);
    }
    let next = offset.checked_add(PAGE_LIMIT).ok_or_else(|| {
        crate::auth::AuthenticatedEndpointError::request_failed(
            crate::auth::AuthenticatedEndpoint::Positions,
        )
    })?;
    if next > MAX_OFFSET {
        return Err(crate::auth::AuthenticatedEndpointError::request_failed(
            crate::auth::AuthenticatedEndpoint::Positions,
        ));
    }
    Ok(Some(next))
}

#[derive(Clone)]
pub(crate) struct PositionInventoryClient {
    http: reqwest::Client,
    endpoint: url::Url,
}

impl fmt::Debug for PositionInventoryClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PositionInventoryClient")
            .field("endpoint_class", &"polymarket_data_api")
            .finish_non_exhaustive()
    }
}

impl PositionInventoryClient {
    pub(crate) fn try_new() -> crate::Result<Self> {
        let endpoint =
            url::Url::parse(POSITIONS_URL).map_err(|_| crate::Error::InvalidClobConfiguration)?;
        Ok(Self {
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            endpoint,
        })
    }

    #[cfg(any(test, feature = "test-utils"))]
    pub(crate) fn for_test(endpoint: &str) -> Self {
        let endpoint = url::Url::parse(endpoint).expect("test position endpoint");
        assert!(
            endpoint.scheme() == "http"
                && endpoint
                    .host_str()
                    .is_some_and(|host| matches!(host, "localhost" | "127.0.0.1" | "::1")),
            "test position endpoint must be loopback HTTP"
        );
        Self {
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("test Data API HTTP policy must build"),
            endpoint,
        }
    }

    pub(crate) async fn fetch(
        &self,
        proxy_wallet: Address,
    ) -> Result<Vec<PositionInventoryEntry>, crate::auth::AuthenticatedEndpointError> {
        let mut offset = 0;
        let mut positions = Vec::new();
        let mut identities = HashSet::new();
        loop {
            let mut url = self.endpoint.clone();
            url.query_pairs_mut()
                .append_pair("user", &proxy_wallet.to_checksum(None))
                .append_pair("sizeThreshold", "0")
                .append_pair("limit", &PAGE_LIMIT.to_string())
                .append_pair("offset", &offset.to_string());
            let mut response = self
                .http
                .get(url)
                .timeout(REQUEST_TIMEOUT)
                .send()
                .await
                .map_err(|_| {
                    crate::auth::AuthenticatedEndpointError::request_failed(
                        crate::auth::AuthenticatedEndpoint::Positions,
                    )
                })?;
            if !response.status().is_success()
                || response
                    .content_length()
                    .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
            {
                return Err(crate::auth::AuthenticatedEndpointError::request_failed(
                    crate::auth::AuthenticatedEndpoint::Positions,
                ));
            }
            let mut body = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|_| {
                crate::auth::AuthenticatedEndpointError::request_failed(
                    crate::auth::AuthenticatedEndpoint::Positions,
                )
            })? {
                if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                    return Err(crate::auth::AuthenticatedEndpointError::request_failed(
                        crate::auth::AuthenticatedEndpoint::Positions,
                    ));
                }
                body.extend_from_slice(&chunk);
            }
            let page = decode_page(&body)?;
            let page_len = page.len();
            for position in page {
                if position.proxy_wallet != proxy_wallet
                    || !identities.insert((position.condition_id, position.asset))
                {
                    return Err(crate::auth::AuthenticatedEndpointError::request_failed(
                        crate::auth::AuthenticatedEndpoint::Positions,
                    ));
                }
                positions.push(position);
            }
            let Some(next) = next_offset(offset, page_len)? else {
                break;
            };
            offset = next;
        }
        positions.sort_by_key(|position| (position.condition_id, position.asset));
        Ok(positions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    use tokio::{
        io::{AsyncReadExt as _, AsyncWriteExt as _},
        net::TcpListener,
    };

    const WALLET: &str = "0x1111111111111111111111111111111111111111";

    fn wallet() -> Address {
        Address::from_str(WALLET).expect("wallet fixture")
    }

    fn position_row(proxy_wallet: &str, identity: usize) -> String {
        format!(
            r#"{{"proxyWallet":"{proxy_wallet}","asset":"{}","conditionId":"0x{identity:064x}","size":1.25,"avgPrice":0.45,"slug":"market-{identity}","outcome":"Yes","redeemable":false,"mergeable":false}}"#,
            identity + 1,
        )
    }

    fn position_page(proxy_wallet: &str, identities: impl IntoIterator<Item = usize>) -> Vec<u8> {
        let rows = identities
            .into_iter()
            .map(|identity| position_row(proxy_wallet, identity))
            .collect::<Vec<_>>()
            .join(",");
        format!("[{rows}]").into_bytes()
    }

    fn http_response(status: &str, body: &[u8]) -> Vec<u8> {
        let mut response = format!(
            "HTTP/1.1 {status}\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
            body.len()
        )
        .into_bytes();
        response.extend_from_slice(body);
        response
    }

    async fn serve(
        responses: Vec<Vec<u8>>,
    ) -> (
        PositionInventoryClient,
        Arc<Mutex<Vec<String>>>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let endpoint = format!(
            "http://{}/positions",
            listener.local_addr().expect("server address")
        );
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&requests);
        let server = tokio::spawn(async move {
            for response in responses {
                let (mut socket, _) = listener.accept().await.expect("accept");
                let mut request = vec![0_u8; 16_384];
                let length = socket.read(&mut request).await.expect("request");
                captured
                    .lock()
                    .expect("request capture")
                    .push(String::from_utf8_lossy(&request[..length]).into_owned());
                let _ = socket.write_all(&response).await;
            }
        });
        (
            PositionInventoryClient::for_test(&endpoint),
            requests,
            server,
        )
    }

    fn assert_redacted(error: &crate::auth::AuthenticatedEndpointError, forbidden: &[&str]) {
        let rendered = format!("{error:?} {error}");
        for value in forbidden {
            assert!(!rendered.contains(value), "error leaked {value}");
        }
        assert_eq!(error.error_class(), "request_failed");
        assert_eq!(error.endpoint_class(), "data.position_inventory");
    }

    fn position(size: &str) -> Vec<u8> {
        format!(
                r#"[{{"proxyWallet":"0x1111111111111111111111111111111111111111","asset":"7","conditionId":"0x{}","size":{size},"avgPrice":0.45,"slug":"market","outcome":"Yes","redeemable":false,"mergeable":false}}]"#,
                "2".repeat(64)
            )
            .into_bytes()
    }

    #[test]
    fn numeric_size_lexeme_remains_exact_beyond_f64_integer_precision() {
        let decoded = decode_page(&position("9007199254740993.123456"))
            .expect("exact high-precision JSON number");
        assert_eq!(decoded[0].size().to_string(), "9007199254740993.123456");
    }

    #[test]
    fn quoted_exponent_negative_and_nonfinite_sizes_fail_closed() {
        for size in ["\"1.25\"", "1e3", "-1", "0", "1.2.3", "null"] {
            assert!(decode_page(&position(size)).is_err(), "{size}");
        }
    }

    /// The class stays `request_failed`, not `response_schema_decode`: an
    /// unrecognized field name is exactly the unsafe-path case
    /// `authenticated_response_path_is_safe` exists to redact (see
    /// `authenticated_trade_diagnostic_accepts_only_owned_structural_paths` in
    /// `error.rs`, whose own fixture includes `data[0].unknown_field`) — the
    /// venue-controlled key text must never reach a rendered diagnostic.
    #[test]
    fn unknown_position_evidence_rejects_the_complete_page() {
        let mut value: serde_json::Value =
            serde_json::from_slice(&position("1.25")).expect("position fixture");
        value[0].as_object_mut().expect("position object").insert(
            "newSettlementEvidence".to_owned(),
            serde_json::json!("must-not-be-shed"),
        );
        let body = serde_json::to_vec(&value).expect("encode hostile page");
        let error = decode_page(&body).expect_err("unknown position evidence must reject page");
        assert_eq!(error.error_class(), "request_failed");
        assert!(!format!("{error:?}").contains("must-not-be-shed"));
    }

    #[test]
    fn page_bound_and_offset_ceiling_are_explicit() {
        assert_eq!(next_offset(0, 499).expect("terminal page"), None);
        assert_eq!(next_offset(0, 500).expect("next page"), Some(500));
        assert_eq!(
            next_offset(9_500, 500).expect("last allowed offset"),
            Some(10_000)
        );
        assert!(next_offset(10_000, 500).is_err());
    }

    #[test]
    fn debug_redacts_wallet_token_condition_slug_and_outcome() {
        let decoded = decode_page(&position("1.25")).expect("position");
        let rendered = format!("{:?}", decoded[0]);
        for private in [
            "1111111111111111111111111111111111111111",
            "2222222222222222222222222222222222222222",
            "market",
            "Yes",
        ] {
            assert!(!rendered.contains(private), "leaked {private}");
        }
    }

    #[tokio::test]
    async fn exact_query_and_multi_page_inventory_are_owned_and_bounded() {
        let first = position_page(WALLET, 0..PAGE_LIMIT);
        let second = position_page(WALLET, PAGE_LIMIT..=PAGE_LIMIT);
        let (client, requests, server) = serve(vec![
            http_response("200 OK", &first),
            http_response("200 OK", &second),
        ])
        .await;

        let inventory = client.fetch(wallet()).await.expect("two exact pages");
        server.await.expect("server");
        assert_eq!(inventory.len(), PAGE_LIMIT + 1);
        let requests = requests.lock().expect("requests");
        assert_eq!(requests.len(), 2);
        for (request, offset) in requests.iter().zip([0, PAGE_LIMIT]) {
            let request_line = request.lines().next().expect("request line");
            assert_eq!(
                request_line,
                format!(
                    "GET /positions?user={WALLET}&sizeThreshold=0&limit={PAGE_LIMIT}&offset={offset} HTTP/1.1"
                )
            );
        }
    }

    #[tokio::test]
    async fn wrong_wallet_and_cross_page_duplicate_fail_without_leaking_evidence() {
        let wrong = "0x2222222222222222222222222222222222222222";
        let body = position_page(wrong, 0..=0);
        let (client, _, server) = serve(vec![http_response("200 OK", &body)]).await;
        let error = client
            .fetch(wallet())
            .await
            .expect_err("wrong proxy wallet must fail");
        server.await.expect("wrong-wallet server");
        assert_redacted(&error, &[wrong, "market-0", "conditionId"]);

        let first = position_page(WALLET, 0..PAGE_LIMIT);
        let duplicate = position_page(WALLET, 0..=0);
        let (client, _, server) = serve(vec![
            http_response("200 OK", &first),
            http_response("200 OK", &duplicate),
        ])
        .await;
        let error = client
            .fetch(wallet())
            .await
            .expect_err("cross-page identity duplicate must fail");
        server.await.expect("duplicate server");
        assert_redacted(&error, &[WALLET, "market-0"]);
    }

    #[tokio::test]
    async fn offset_ceiling_rejects_an_additional_full_page() {
        let mut responses = Vec::new();
        for offset in (0..=MAX_OFFSET).step_by(PAGE_LIMIT) {
            let page = position_page(WALLET, offset..offset + PAGE_LIMIT);
            responses.push(http_response("200 OK", &page));
        }
        let (client, requests, server) = serve(responses).await;
        let error = client
            .fetch(wallet())
            .await
            .expect_err("offset beyond the explicit ceiling must fail");
        server.await.expect("ceiling server");
        assert_redacted(&error, &[WALLET, "market-10000"]);
        let requests = requests.lock().expect("requests");
        assert_eq!(requests.len(), MAX_OFFSET / PAGE_LIMIT + 1);
        assert!(requests.last().expect("last request").starts_with(&format!(
            "GET /positions?user={WALLET}&sizeThreshold=0&limit={PAGE_LIMIT}&offset={MAX_OFFSET} "
        )));
    }

    #[tokio::test]
    async fn redirect_non_success_and_declared_oversize_fail_closed() {
        let sink = TcpListener::bind("127.0.0.1:0").await.expect("sink bind");
        let location = format!(
            "http://{}/forwarded",
            sink.local_addr().expect("sink address")
        );
        let redirect = format!(
                "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .into_bytes();
        let (client, _, server) = serve(vec![redirect]).await;
        let error = client
            .fetch(wallet())
            .await
            .expect_err("redirect must fail");
        server.await.expect("redirect server");
        assert_redacted(&error, &[WALLET, &location]);
        assert!(
            tokio::time::timeout(Duration::from_millis(50), sink.accept())
                .await
                .is_err(),
            "redirect destination must receive no request"
        );

        let (client, _, server) = serve(vec![http_response(
            "503 Service Unavailable",
            b"hostile raw venue body token=secret-order-id",
        )])
        .await;
        let error = client
            .fetch(wallet())
            .await
            .expect_err("non-success must fail");
        server.await.expect("status server");
        assert_redacted(&error, &["secret-order-id", WALLET]);

        let declared = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            MAX_RESPONSE_BYTES + 1
        )
        .into_bytes();
        let (client, _, server) = serve(vec![declared]).await;
        let error = client
            .fetch(wallet())
            .await
            .expect_err("declared oversized body must fail");
        server.await.expect("declared-size server");
        assert_redacted(&error, &[WALLET]);
    }

    #[tokio::test]
    async fn streamed_oversize_without_content_length_fails_closed() {
        let mut response = b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n".to_vec();
        response.resize(response.len() + MAX_RESPONSE_BYTES + 1, b'x');
        let (client, _, server) = serve(vec![response]).await;
        let error = client
            .fetch(wallet())
            .await
            .expect_err("streamed oversized body must fail");
        server.await.expect("streamed-size server");
        assert_redacted(&error, &[WALLET, "xxxxxxxxxxxxxxxx"]);
    }
}
