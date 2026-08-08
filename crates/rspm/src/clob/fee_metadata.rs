//! Strict rspm-owned reader for Polymarket V2 market-fee metadata.
use super::ClobConfig;
use crate::{canonical_nonnegative_decimal_text, canonical_unsigned_integer_text};
use hashbrown::HashMap;
use polymarket::types::{B256, Decimal, U256};
use serde::{Deserialize, Deserializer, de::Error};
use serde_json::value::RawValue;
use std::{fmt, sync::Arc, time::Duration};
use tokio::sync::RwLock;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

/// Exact V2 fee schedule carried by a CLOB market's required `fd` record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlatformFeeSchedule {
    rate: Decimal,
    exponent: u32,
    taker_only: bool,
}

impl PlatformFeeSchedule {
    /// Construct a validated schedule. Negative rates are not meaningful and
    /// must never reach fee projection.
    pub fn new(rate: Decimal, exponent: u32, taker_only: bool) -> Result<Self, FeeMetadataError> {
        if rate < Decimal::ZERO {
            return Err(FeeMetadataError::InvalidSchedule);
        }
        Ok(Self {
            rate,
            exponent,
            taker_only,
        })
    }

    #[must_use]
    pub const fn rate(self) -> Decimal {
        self.rate
    }

    #[must_use]
    pub const fn exponent(self) -> u32 {
        self.exponent
    }

    #[must_use]
    pub const fn taker_only(self) -> bool {
        self.taker_only
    }
}

/// Closed public-fee-metadata failures. Request URLs, token identifiers, raw
/// responses, and parser details are deliberately unrepresentable.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum FeeMetadataError {
    #[error("CLOB fee metadata token is invalid")]
    InvalidToken,
    #[error("CLOB fee metadata request failed")]
    RequestFailed,
    #[error("CLOB fee metadata response schema is invalid")]
    ResponseSchema,
    #[error("CLOB fee metadata schedule is invalid")]
    InvalidSchedule,
}

impl FeeMetadataError {
    #[must_use]
    pub const fn error_class(self) -> &'static str {
        match self {
            Self::InvalidToken => "invalid_token",
            Self::RequestFailed => "request_failed",
            Self::ResponseSchema => "response_schema",
            Self::InvalidSchedule => "invalid_schedule",
        }
    }
}

/// Public, read-only Polymarket V2 market-fee metadata client.
///
/// rspm owns both wire schemas and both public HTTP reads. The SDK is not used
/// for metadata decoding, so its lossy `FeeInfo` projection cannot discard the
/// authoritative `fd.to` maker/taker policy.
#[derive(Clone)]
pub struct FeeMetadataClient {
    http: reqwest::Client,
    endpoint: url::Url,
    schedules: Arc<RwLock<HashMap<U256, PlatformFeeSchedule>>>,
}

impl fmt::Debug for FeeMetadataClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FeeMetadataClient")
            .field("endpoint_class", &"polymarket_clob_public_fee_metadata")
            .finish_non_exhaustive()
    }
}

impl FeeMetadataClient {
    /// Construct a public metadata reader from an explicit CLOB configuration.
    pub fn new(config: ClobConfig) -> crate::Result<Self> {
        config.validate()?;
        let endpoint =
            url::Url::parse(config.host()).map_err(|_| crate::Error::InvalidClobConfiguration)?;
        let mut builder = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none());
        if let Some(proxy) = config.proxy_url() {
            let proxy =
                reqwest::Proxy::all(proxy).map_err(|_| crate::Error::InvalidClobConfiguration)?;
            builder = builder.proxy(proxy);
        }
        let http = builder
            .build()
            .map_err(|_| crate::Error::InvalidClobConfiguration)?;
        Ok(Self {
            http,
            endpoint,
            schedules: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Resolve the complete exact V2 fee schedule for one outcome token.
    pub async fn platform_fee_schedule(
        &self,
        token_id: &str,
    ) -> Result<PlatformFeeSchedule, FeeMetadataError> {
        let token_id = parse_token_id(token_id)?;
        if let Some(schedule) = self.schedules.read().await.get(&token_id).copied() {
            return Ok(schedule);
        }

        let market: MarketByTokenWire = self
            .get_json(&format!("markets-by-token/{token_id}"))
            .await?;
        if token_id != market.primary_token_id && token_id != market.secondary_token_id {
            return Err(FeeMetadataError::ResponseSchema);
        }

        let info: ClobMarketWire = self
            .get_json(&format!("clob-markets/{}", market.condition_id))
            .await?;
        if info.condition_id != market.condition_id {
            return Err(FeeMetadataError::ResponseSchema);
        }
        let token_ids = info
            .tokens
            .into_iter()
            .flatten()
            .map(|token| token.token_id)
            .collect::<Vec<_>>();
        if token_ids.len() != 2
            || token_ids[0] == token_ids[1]
            || !token_ids.contains(&market.primary_token_id)
            || !token_ids.contains(&market.secondary_token_id)
        {
            return Err(FeeMetadataError::ResponseSchema);
        }

        let rate = parse_rate(&info.fee_details.rate)?;
        let schedule =
            PlatformFeeSchedule::new(rate, info.fee_details.exponent, info.fee_details.taker_only)?;
        let mut schedules = self.schedules.write().await;
        for token in token_ids {
            schedules.insert(token, schedule);
        }
        Ok(schedule)
    }

    /// Compatibility projection for existing taker-only paper callers.
    ///
    /// This is a view of the same rspm-owned exact schedule, not a competing
    /// fee authority. Live conversion must use [`Self::platform_fee_schedule`]
    /// so the `taker_only` policy is never discarded.
    pub async fn platform_fee_params(
        &self,
        token_id: &str,
    ) -> Result<(Decimal, u32), FeeMetadataError> {
        let schedule = self.platform_fee_schedule(token_id).await?;
        Ok((schedule.rate(), schedule.exponent()))
    }

    async fn get_json<T>(&self, path: &str) -> Result<T, FeeMetadataError>
    where
        T: serde::de::DeserializeOwned,
    {
        let mut url = self.endpoint.clone();
        url.set_path(path);
        let mut response = self
            .http
            .get(url)
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .map_err(|_| FeeMetadataError::RequestFailed)?;
        if !response.status().is_success()
            || response
                .content_length()
                .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
        {
            return Err(FeeMetadataError::RequestFailed);
        }

        let mut body = Vec::with_capacity(
            response
                .content_length()
                .and_then(|length| usize::try_from(length).ok())
                .unwrap_or_default(),
        );
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| FeeMetadataError::RequestFailed)?
        {
            if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                return Err(FeeMetadataError::RequestFailed);
            }
            body.extend_from_slice(&chunk);
        }

        let mut deserializer = serde_json::Deserializer::from_slice(&body);
        let value =
            T::deserialize(&mut deserializer).map_err(|_| FeeMetadataError::ResponseSchema)?;
        deserializer
            .end()
            .map_err(|_| FeeMetadataError::ResponseSchema)?;
        Ok(value)
    }

    #[cfg(test)]
    fn endpoint(&self) -> &url::Url {
        &self.endpoint
    }
}

#[derive(Deserialize)]
struct MarketByTokenWire {
    condition_id: B256,
    #[serde(deserialize_with = "quoted_token_id")]
    primary_token_id: U256,
    #[serde(deserialize_with = "quoted_token_id")]
    secondary_token_id: U256,
}

#[derive(Deserialize)]
struct ClobMarketWire {
    #[serde(rename = "c")]
    condition_id: B256,
    #[serde(rename = "t")]
    tokens: Vec<Option<ClobTokenWire>>,
    #[serde(rename = "fd")]
    fee_details: FeeDetailsWire,
}

#[derive(Deserialize)]
struct ClobTokenWire {
    #[serde(rename = "t", deserialize_with = "quoted_token_id")]
    token_id: U256,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FeeDetailsWire {
    #[serde(rename = "r")]
    rate: Box<RawValue>,
    #[serde(rename = "e")]
    exponent: u32,
    #[serde(rename = "to")]
    taker_only: bool,
}

fn quoted_token_id<'de, D>(deserializer: D) -> Result<U256, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    if !canonical_unsigned_integer_text(&value) || value == "0" {
        return Err(D::Error::custom(
            "expected canonical nonzero quoted token id",
        ));
    }
    U256::from_str_radix(&value, 10)
        .map_err(|_| D::Error::custom("quoted token id is out of range"))
}

fn parse_token_id(value: &str) -> Result<U256, FeeMetadataError> {
    if !canonical_unsigned_integer_text(value) || value == "0" {
        return Err(FeeMetadataError::InvalidToken);
    }
    U256::from_str_radix(value, 10).map_err(|_| FeeMetadataError::InvalidToken)
}

fn parse_rate(raw: &RawValue) -> Result<Decimal, FeeMetadataError> {
    let raw = raw.get();
    let owned;
    let value = if raw.starts_with('"') {
        owned =
            serde_json::from_str::<String>(raw).map_err(|_| FeeMetadataError::ResponseSchema)?;
        owned.as_str()
    } else {
        raw
    };
    if !canonical_nonnegative_decimal_text(value) {
        return Err(FeeMetadataError::ResponseSchema);
    }
    Decimal::from_str_exact(value).map_err(|_| FeeMetadataError::ResponseSchema)
}

#[cfg(test)]
mod tests {
    use crate::clob::CLOB_HOST;
    use std::sync::{Arc, Mutex};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    use super::*;

    const CONDITION: &str = "0x1111111111111111111111111111111111111111111111111111111111111111";

    fn http_response(body: &[u8]) -> Vec<u8> {
        let mut response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        response.extend_from_slice(body);
        response
    }

    async fn serve(
        responses: Vec<Vec<u8>>,
    ) -> (
        FeeMetadataClient,
        Arc<Mutex<Vec<String>>>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback server");
        let address = listener.local_addr().expect("loopback address");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&requests);
        let server = tokio::spawn(async move {
            for response in responses {
                let (mut socket, _) = listener.accept().await.expect("accept request");
                let mut request = vec![0_u8; 4096];
                let read = socket.read(&mut request).await.expect("read request");
                let head = String::from_utf8_lossy(&request[..read]);
                observed
                    .lock()
                    .expect("request lock")
                    .push(head.lines().next().unwrap_or_default().to_owned());
                socket.write_all(&response).await.expect("write response");
            }
        });
        let client = FeeMetadataClient::new(ClobConfig::new(format!("http://{address}"), None))
            .expect("loopback config");
        (client, requests, server)
    }

    fn market_by_token() -> Vec<u8> {
        format!(
            r#"{{"condition_id":"{CONDITION}","primary_token_id":"1","secondary_token_id":"2"}}"#
        )
        .into_bytes()
    }

    fn clob_market(fee_details: &str) -> Vec<u8> {
        format!(
            r#"{{"c":"{CONDITION}","t":[{{"t":"1","o":"Yes"}},{{"t":"2","o":"No"}}],"fd":{fee_details},"mts":"0.01"}}"#
        )
        .into_bytes()
    }

    #[tokio::test]
    async fn exact_schedule_retains_taker_only_and_caches_both_tokens() {
        let (client, requests, server) = serve(vec![
            http_response(&market_by_token()),
            http_response(&clob_market(r#"{"r":0.07,"e":1,"to":false}"#)),
        ])
        .await;

        let schedule = client
            .platform_fee_schedule("1")
            .await
            .expect("exact schedule");
        assert_eq!(schedule.rate(), Decimal::new(7, 2));
        assert_eq!(schedule.exponent(), 1);
        assert!(!schedule.taker_only());
        assert_eq!(
            client
                .platform_fee_schedule("2")
                .await
                .expect("second token uses same market schedule"),
            schedule
        );
        assert_eq!(
            client
                .platform_fee_params("2")
                .await
                .expect("compatibility tuple uses cached exact schedule"),
            (Decimal::new(7, 2), 1)
        );
        server.await.expect("metadata server");
        let requests = requests.lock().expect("request lock").clone();
        assert_eq!(
            requests,
            vec![
                "GET /markets-by-token/1 HTTP/1.1".to_owned(),
                format!("GET /clob-markets/{CONDITION} HTTP/1.1"),
            ]
        );
    }

    #[tokio::test]
    async fn missing_or_invalid_taker_only_fails_closed() {
        for fee_details in [
            r#"{"r":0.07,"e":1}"#,
            r#"{"r":0.07,"e":1,"to":null}"#,
            r#"{"r":0.07,"e":1,"to":"false"}"#,
            r#"{"r":0.07,"e":1,"to":true,"to":false}"#,
            r#"{"r":0.07,"e":1,"to":true,"unexpected":0}"#,
        ] {
            let (client, _, server) = serve(vec![
                http_response(&market_by_token()),
                http_response(&clob_market(fee_details)),
            ])
            .await;
            assert_eq!(
                client.platform_fee_schedule("1").await,
                Err(FeeMetadataError::ResponseSchema)
            );
            server.await.expect("metadata server");
        }
    }

    #[tokio::test]
    async fn invalid_token_and_hostile_response_details_are_redacted() {
        let invalid = FeeMetadataClient::new(ClobConfig::new(CLOB_HOST, None))
            .expect("canonical test metadata client")
            .platform_fee_schedule("private-token-id")
            .await
            .expect_err("invalid token must fail before network");
        assert_eq!(invalid, FeeMetadataError::InvalidToken);
        assert!(!format!("{invalid:?} {invalid}").contains("private-token-id"));

        let hostile = b"Bearer secret-token raw-order-id";
        let (client, _, server) = serve(vec![http_response(hostile)]).await;
        let error = client
            .platform_fee_schedule("1")
            .await
            .expect_err("hostile schema must fail closed");
        server.await.expect("metadata server");
        let rendered = format!("{error:?} {error}");
        for secret in ["Bearer", "secret-token", "raw-order-id"] {
            assert!(!rendered.contains(secret));
        }
    }

    #[test]
    fn debug_and_canonical_endpoint_never_render_configured_host() {
        let client = FeeMetadataClient::new(ClobConfig::new("https://clob.example", None))
            .expect("explicit test host");
        assert_eq!(client.endpoint().as_str(), "https://clob.example/");
        assert!(!format!("{client:?}").contains("clob.example"));
        let canonical = FeeMetadataClient::new(ClobConfig::new(CLOB_HOST, None))
            .expect("canonical test metadata client");
        assert_eq!(
            canonical.endpoint().as_str(),
            "https://clob.polymarket.com/"
        );
    }
}
