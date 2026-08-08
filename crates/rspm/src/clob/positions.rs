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
            return Err(crate::auth::AuthenticatedEndpointError::response_schema_decode(
                endpoint, "asset",
            ));
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
            return Err(crate::auth::AuthenticatedEndpointError::request_failed(endpoint));
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
        return Err(crate::auth::AuthenticatedEndpointError::response_schema_decode(
            crate::auth::AuthenticatedEndpoint::Positions,
            path,
        ));
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
        crate::auth::AuthenticatedEndpointError::request_failed(crate::auth::AuthenticatedEndpoint::Positions)
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

