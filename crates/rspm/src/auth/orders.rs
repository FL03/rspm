//! Strict authenticated order-inventory recovery.

use crate::auth::{
    AuthenticatedEndpoint, AuthenticatedEndpointError, AuthenticatedHttpClient,
    AuthenticatedVenueSide, cursor_is_terminal, cursor_is_valid, validate_page,
    venue_identifier_is_valid,
};
use crate::{append_query_pair, decode_json, quoted_decimal, quoted_i64};
use polymarket::{
    auth::{ApiKey, Normal, state::Authenticated},
    clob::Client,
    types::{Address, B256, Decimal, U256},
};
use serde::Deserialize;

const ORDERS_PATH: &str = "/data/orders";

/// Closed order-state contract for authenticated inventory recovery.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum AuthenticatedOrderStatus {
    #[serde(rename = "ORDER_STATUS_LIVE", alias = "LIVE", alias = "live")]
    Live,
    #[serde(rename = "ORDER_STATUS_INVALID", alias = "INVALID", alias = "invalid")]
    Invalid,
    #[serde(
        rename = "ORDER_STATUS_CANCELED_MARKET_RESOLVED",
        alias = "CANCELED_MARKET_RESOLVED",
        alias = "canceled_market_resolved"
    )]
    CanceledMarketResolved,
    #[serde(
        rename = "ORDER_STATUS_CANCELED",
        alias = "CANCELED",
        alias = "canceled"
    )]
    Canceled,
    #[serde(rename = "ORDER_STATUS_MATCHED", alias = "MATCHED", alias = "matched")]
    Matched,
    #[serde(rename = "ORDER_STATUS_DELAYED", alias = "DELAYED", alias = "delayed")]
    Delayed,
    #[serde(
        rename = "ORDER_STATUS_UNMATCHED",
        alias = "UNMATCHED",
        alias = "unmatched"
    )]
    Unmatched,
}

/// Exact owned representation of one authenticated account order.
#[derive(Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct AuthenticatedOrder {
    pub id: String,
    pub status: AuthenticatedOrderStatus,
    pub owner: ApiKey,
    pub maker_address: Address,
    pub market: B256,
    pub asset_id: U256,
    pub side: AuthenticatedVenueSide,
    pub original_size: Decimal,
    pub size_matched: Decimal,
    pub price: Decimal,
    pub created_at_seconds: i64,
    pub expiration_seconds: i64,
    /// Exact venue trade identifiers associated with this order.
    pub associate_trades: Vec<String>,
    /// Exact venue outcome label. It is retained rather than normalized.
    pub outcome: String,
    /// Exact venue time-in-force spelling. Unknown values remain evidence.
    pub order_type: String,
}

impl core::fmt::Debug for AuthenticatedOrder {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("AuthenticatedOrder")
            .field("status", &self.status)
            .field("side", &self.side)
            .field("original_size", &self.original_size)
            .field("size_matched", &self.size_matched)
            .field("price", &self.price)
            .field("created_at_seconds", &self.created_at_seconds)
            .field("expiration_seconds", &self.expiration_seconds)
            .finish_non_exhaustive()
    }
}

impl AuthenticatedOrder {
    pub fn decode_json(body: &[u8]) -> Result<Self, AuthenticatedEndpointError> {
        let wire: WireOrder = decode_json(AuthenticatedEndpoint::Order, body)?;
        Self::try_from(wire)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireOrder {
    id: String,
    status: AuthenticatedOrderStatus,
    owner: ApiKey,
    maker_address: Address,
    market: B256,
    asset_id: U256,
    side: AuthenticatedVenueSide,
    #[serde(deserialize_with = "quoted_decimal")]
    original_size: Decimal,
    #[serde(deserialize_with = "quoted_decimal")]
    size_matched: Decimal,
    #[serde(deserialize_with = "quoted_decimal")]
    price: Decimal,
    created_at: i64,
    #[serde(deserialize_with = "quoted_i64")]
    expiration: i64,
    #[serde(default)]
    associate_trades: Vec<String>,
    outcome: String,
    order_type: String,
}

impl TryFrom<WireOrder> for AuthenticatedOrder {
    type Error = AuthenticatedEndpointError;

    fn try_from(order: WireOrder) -> Result<Self, Self::Error> {
        let valid = venue_identifier_is_valid(&order.id)
            && order.created_at >= 0
            && order.expiration >= 0
            && order.original_size > Decimal::ZERO
            && order.size_matched >= Decimal::ZERO
            && order.size_matched <= order.original_size
            && order.price > Decimal::ZERO
            && order.price <= Decimal::ONE
            && order
                .associate_trades
                .iter()
                .all(|trade_id| venue_identifier_is_valid(trade_id))
            && !order.outcome.is_empty()
            && !order.order_type.is_empty();
        if !valid {
            return Err(AuthenticatedEndpointError::request_failed(
                AuthenticatedEndpoint::Order,
            ));
        }
        Ok(Self {
            id: order.id,
            status: order.status,
            owner: order.owner,
            maker_address: order.maker_address,
            market: order.market,
            asset_id: order.asset_id,
            side: order.side,
            original_size: order.original_size,
            size_matched: order.size_matched,
            price: order.price,
            created_at_seconds: order.created_at,
            expiration_seconds: order.expiration,
            associate_trades: order.associate_trades,
            outcome: order.outcome,
            order_type: order.order_type,
        })
    }
}

/// One validated account-order inventory page.
#[derive(Clone, PartialEq)]
pub struct AuthenticatedOrderPage {
    pub data: Vec<AuthenticatedOrder>,
    pub next_cursor: String,
    pub limit: u64,
    pub count: u64,
}

impl core::fmt::Debug for AuthenticatedOrderPage {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("AuthenticatedOrderPage")
            .field("data_len", &self.data.len())
            .field("terminal", &cursor_is_terminal(&self.next_cursor))
            .field("limit", &self.limit)
            .field("count", &self.count)
            .finish()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireOrderPage {
    data: Vec<WireOrder>,
    next_cursor: String,
    limit: u64,
    count: u64,
}

impl AuthenticatedOrderPage {
    /// Return whether this page closes the venue's pagination traversal.
    ///
    /// Polymarket has emitted both the documented `LTE=` sentinel and an
    /// empty string as terminal cursors. Callers must use this shared contract
    /// instead of comparing one wire spelling directly.
    pub fn is_terminal(&self) -> bool {
        cursor_is_terminal(&self.next_cursor)
    }

    pub fn decode_json(body: &[u8]) -> Result<Self, AuthenticatedEndpointError> {
        let wire: WireOrderPage = decode_json(AuthenticatedEndpoint::Order, body)?;
        validate_page(
            AuthenticatedEndpoint::Order,
            wire.data.len(),
            wire.count,
            wire.limit,
            &wire.next_cursor,
        )?;
        let data = wire
            .data
            .into_iter()
            .map(AuthenticatedOrder::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            data,
            next_cursor: wire.next_cursor,
            limit: wire.limit,
            count: wire.count,
        })
    }
}

/// Supported filters for authenticated account order inventory.
#[derive(Clone, Default, Eq, PartialEq)]
pub struct AuthenticatedOrdersRequest {
    pub order_id: Option<String>,
    pub market: Option<B256>,
    pub asset_id: Option<U256>,
}

impl core::fmt::Debug for AuthenticatedOrdersRequest {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("AuthenticatedOrdersRequest")
            .field("has_order_id", &self.order_id.is_some())
            .field("has_market", &self.market.is_some())
            .field("has_asset_id", &self.asset_id.is_some())
            .finish()
    }
}

impl AuthenticatedOrdersRequest {
    #[must_use]
    pub fn with_order_id(mut self, order_id: impl Into<String>) -> Self {
        self.order_id = Some(order_id.into());
        self
    }

    fn query(&self, next_cursor: Option<&str>) -> Result<String, AuthenticatedEndpointError> {
        if self
            .order_id
            .as_ref()
            .is_some_and(|value| !venue_identifier_is_valid(value))
            || next_cursor.is_some_and(|value| !cursor_is_valid(value))
        {
            return Err(AuthenticatedEndpointError::request_failed(
                AuthenticatedEndpoint::Order,
            ));
        }
        let mut query = url::form_urlencoded::Serializer::new(String::new());
        append_query_pair(&mut query, "id", self.order_id.as_deref());
        let market = self.market.map(|value| value.to_string());
        append_query_pair(&mut query, "market", market.as_deref());
        let asset_id = self.asset_id.map(|value| value.to_string());
        append_query_pair(&mut query, "asset_id", asset_id.as_deref());
        append_query_pair(&mut query, "next_cursor", next_cursor);
        Ok(query.finish())
    }
}

pub(crate) fn authenticated_order_path(
    order_id: &str,
) -> Result<String, AuthenticatedEndpointError> {
    if !venue_identifier_is_valid(order_id) {
        return Err(AuthenticatedEndpointError::request_failed(
            AuthenticatedEndpoint::Order,
        ));
    }
    // The current official single-order contract is `/data/order/{orderID}`.
    Ok(format!("/data/order/{order_id}"))
}

#[derive(Clone, Debug, Default)]
pub(crate) struct AuthenticatedOrdersClient;

impl AuthenticatedOrdersClient {
    pub(crate) async fn fetch(
        &self,
        http: &AuthenticatedHttpClient,
        client: &Client<Authenticated<Normal>>,
        request: &AuthenticatedOrdersRequest,
        next_cursor: Option<&str>,
    ) -> Result<AuthenticatedOrderPage, AuthenticatedEndpointError> {
        let query = request.query(next_cursor)?;
        let body = http
            .get(
                client,
                AuthenticatedEndpoint::Order,
                ORDERS_PATH,
                Some(&query),
            )
            .await?;
        AuthenticatedOrderPage::decode_json(&body)
    }

    pub(crate) async fn fetch_one(
        &self,
        http: &AuthenticatedHttpClient,
        client: &Client<Authenticated<Normal>>,
        order_id: &str,
    ) -> Result<AuthenticatedOrder, AuthenticatedEndpointError> {
        let path = authenticated_order_path(order_id)?;
        let body = http
            .get(client, AuthenticatedEndpoint::Order, &path, None)
            .await?;
        let order = AuthenticatedOrder::decode_json(&body)?;
        if order.id != order_id {
            return Err(AuthenticatedEndpointError::request_failed(
                AuthenticatedEndpoint::Order,
            ));
        }
        Ok(order)
    }
}

#[cfg(test)]
mod tests;
