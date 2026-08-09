//! Authenticated CLOB trade-history transport and response contract.
//!
//! Order construction and signing remain in the upstream SDK. This module owns
//! the recovery path because a recovery page must either decode completely or
//! fail with no response-derived detail beyond a validated structural path.
use crate::auth::{
    AuthenticatedHttpClient, cursor_is_terminal, cursor_is_valid, validate_page,
    venue_identifier_is_valid,
};
use crate::utils::{
    append_query_pair, decode_json, optional_quoted_decimal, quoted_decimal, quoted_hash_or_zero,
    quoted_i64,
};
use polymarket::{
    auth::{ApiKey, Normal, state::Authenticated},
    clob::Client,
    types::{Address, B256, Decimal, U256},
};
use serde::{Deserialize, Deserializer};

const TRADES_PATH: &str = "/data/trades";

/// One all-or-nothing page from the authenticated CLOB trade-history endpoint.
#[derive(Clone, PartialEq)]
pub struct AuthenticatedTradePage {
    pub data: Vec<AuthenticatedTrade>,
    pub next_cursor: String,
    pub limit: u64,
    pub count: u64,
}

impl core::fmt::Debug for AuthenticatedTradePage {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("AuthenticatedTradePage")
            .field("data_len", &self.data.len())
            .field("terminal", &cursor_is_terminal(&self.next_cursor))
            .field("limit", &self.limit)
            .field("count", &self.count)
            .finish()
    }
}

/// Authenticated venue action side.
///
/// This is the canonical CLOB order-direction type, not a parallel enum. It is
/// deliberately distinct from [`crate::types::Side`], whose variants represent
/// binary market outcomes (`Yes`/`No`).
pub use crate::types::ClobSide as AuthenticatedVenueSide;

/// Strict account role from an authenticated trade page.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
pub enum AuthenticatedTraderSide {
    #[serde(rename = "TAKER", alias = "taker")]
    Taker,
    #[serde(rename = "MAKER", alias = "maker")]
    Maker,
}

/// Strict documented trade lifecycle state.
///
/// Current CLOB responses use the `TRADE_STATUS_*` spellings. Explicit legacy
/// aliases remain accepted during the migration, but unknown text rejects the
/// complete page instead of becoming an open-ended SDK enum variant.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
pub enum AuthenticatedTradeStatus {
    #[serde(
        rename = "TRADE_STATUS_MATCHED_NOT_BROADCASTED",
        alias = "MATCHED_NOT_BROADCASTED",
        alias = "matched_not_broadcasted"
    )]
    MatchedNotBroadcasted,
    #[serde(rename = "TRADE_STATUS_MATCHED", alias = "MATCHED", alias = "matched")]
    Matched,
    #[serde(rename = "TRADE_STATUS_MINED", alias = "MINED", alias = "mined")]
    Mined,
    #[serde(
        rename = "TRADE_STATUS_CONFIRMED",
        alias = "CONFIRMED",
        alias = "confirmed"
    )]
    Confirmed,
    #[serde(
        rename = "TRADE_STATUS_RETRYING",
        alias = "RETRYING",
        alias = "retrying"
    )]
    Retrying,
    #[serde(rename = "TRADE_STATUS_FAILED", alias = "FAILED", alias = "failed")]
    Failed,
}

impl AuthenticatedTradePage {
    /// Return whether this page closes the venue's pagination traversal.
    ///
    /// Both an empty cursor and the documented `LTE=` sentinel are terminal.
    pub fn is_terminal(&self) -> bool {
        cursor_is_terminal(&self.next_cursor)
    }

    /// Decode a complete authenticated response without retaining parser text
    /// or response values in the public error.
    pub fn decode_json(
        body: &[u8],
    ) -> core::result::Result<Self, crate::auth::AuthenticatedEndpointError> {
        let decoded = (|| {
            let page: WireTradePage =
                decode_json(crate::auth::AuthenticatedEndpoint::Trades, body)?;
            validate_page(
                crate::auth::AuthenticatedEndpoint::Trades,
                page.data.len(),
                page.count,
                page.limit,
                &page.next_cursor,
            )?;
            let data = page
                .data
                .into_iter()
                .enumerate()
                .map(|(trade_index, trade)| AuthenticatedTrade::try_from_wire(trade_index, trade))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Self {
                data,
                next_cursor: page.next_cursor,
                limit: page.limit,
                count: page.count,
            })
        })();
        decoded.map_err(|error: crate::auth::AuthenticatedEndpointError| {
            error.with_response_digest(body)
        })
    }
}

/// Exact trade evidence retained by the Live recovery engine.
#[derive(Clone, PartialEq)]
pub struct AuthenticatedTrade {
    pub id: String,
    pub taker_order_id: String,
    pub market: B256,
    pub asset_id: U256,
    pub side: AuthenticatedVenueSide,
    pub size: Decimal,
    pub fee_rate_bps: Decimal,
    /// Exact fee amount charged for this leg when the venue includes V2
    /// match-time fee accounting in the authenticated response.
    pub fee_usdc: Option<Decimal>,
    pub price: Decimal,
    pub status: AuthenticatedTradeStatus,
    pub match_time: i64,
    /// Most recent venue-side update timestamp for this trade record.
    pub last_update: i64,
    /// Venue outcome label for the traded asset (for example `"YES"`/`"NO"`).
    pub outcome: String,
    /// Venue reward-eligibility bucket index for this trade.
    pub bucket_index: u32,
    pub owner: ApiKey,
    /// On-chain address that owned the aggregate maker side of this trade.
    pub maker_address: Address,
    pub maker_orders: Vec<AuthenticatedMakerOrder>,
    /// On-chain settlement transaction hash. Zero until the venue's async
    /// execution pipeline broadcasts the trade.
    pub transaction_hash: B256,
    pub trader_side: AuthenticatedTraderSide,
    /// Venue-supplied failure detail when this trade record represents a
    /// rejected order. Deliberately excluded from `Debug` — free-form venue
    /// text is not a validated structural path.
    pub error_msg: Option<String>,
}

impl core::fmt::Debug for AuthenticatedTrade {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("AuthenticatedTrade")
            .field("side", &self.side)
            .field("size", &self.size)
            .field("fee_rate_bps", &self.fee_rate_bps)
            .field("price", &self.price)
            .field("status", &self.status)
            .field("match_time", &self.match_time)
            .field("maker_count", &self.maker_orders.len())
            .field("trader_side", &self.trader_side)
            .finish_non_exhaustive()
    }
}

/// Exact maker-leg evidence retained by the Live recovery engine.
#[derive(Clone, PartialEq)]
pub struct AuthenticatedMakerOrder {
    pub order_id: String,
    pub owner: ApiKey,
    /// On-chain address that owned this maker leg.
    pub maker_address: Address,
    pub matched_amount: Decimal,
    pub price: Decimal,
    pub fee_rate_bps: Option<Decimal>,
    /// Exact V2 fee amount for this maker leg when reported by the venue.
    pub fee_usdc: Option<Decimal>,
    pub asset_id: U256,
    /// Venue outcome label for this maker leg's asset.
    pub outcome: String,
    pub side: AuthenticatedVenueSide,
}

impl core::fmt::Debug for AuthenticatedMakerOrder {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("AuthenticatedMakerOrder")
            .field("matched_amount", &self.matched_amount)
            .field("price", &self.price)
            .field("fee_rate_bps", &self.fee_rate_bps)
            .field("side", &self.side)
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireTradePage {
    data: Vec<WireTrade>,
    next_cursor: String,
    limit: u64,
    count: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireTrade {
    id: String,
    taker_order_id: String,
    market: B256,
    asset_id: U256,
    side: AuthenticatedVenueSide,
    #[serde(deserialize_with = "quoted_decimal")]
    size: Decimal,
    #[serde(deserialize_with = "quoted_decimal")]
    fee_rate_bps: Decimal,
    #[serde(
        default,
        alias = "feeUsdc",
        deserialize_with = "optional_quoted_decimal"
    )]
    fee_usdc: Option<Decimal>,
    #[serde(deserialize_with = "quoted_decimal")]
    price: Decimal,
    status: AuthenticatedTradeStatus,
    #[serde(deserialize_with = "quoted_i64")]
    match_time: i64,
    #[serde(deserialize_with = "quoted_i64")]
    last_update: i64,
    outcome: String,
    bucket_index: u32,
    owner: ApiKey,
    maker_address: Address,
    #[serde(default, deserialize_with = "wire_maker_orders_or_empty")]
    maker_orders: Vec<WireMakerOrder>,
    #[serde(default, deserialize_with = "quoted_hash_or_zero")]
    transaction_hash: B256,
    trader_side: AuthenticatedTraderSide,
    #[serde(default)]
    error_msg: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireMakerOrder {
    order_id: String,
    owner: ApiKey,
    maker_address: Address,
    #[serde(deserialize_with = "quoted_decimal")]
    matched_amount: Decimal,
    #[serde(deserialize_with = "quoted_decimal")]
    price: Decimal,
    #[serde(default, deserialize_with = "optional_quoted_decimal")]
    fee_rate_bps: Option<Decimal>,
    #[serde(
        default,
        alias = "feeUsdc",
        deserialize_with = "optional_quoted_decimal"
    )]
    fee_usdc: Option<Decimal>,
    asset_id: U256,
    outcome: String,
    side: AuthenticatedVenueSide,
}

impl AuthenticatedMakerOrder {
    fn try_from_wire(
        trade_index: usize,
        maker_index: usize,
        order: WireMakerOrder,
    ) -> Result<Self, crate::auth::AuthenticatedEndpointError> {
        let invalid_field = if !venue_identifier_is_valid(&order.order_id) {
            Some("order_id")
        } else if order.matched_amount < Decimal::ZERO {
            Some("matched_amount")
        } else if order.price < Decimal::ZERO || order.price > Decimal::ONE {
            Some("price")
        } else if order.fee_rate_bps.is_some_and(|fee| fee < Decimal::ZERO) {
            Some("fee_rate_bps")
        } else if order.fee_usdc.is_some_and(|fee| fee < Decimal::ZERO) {
            Some("fee_usdc")
        } else {
            None
        };
        if let Some(field) = invalid_field {
            return Err(indexed_maker_schema_error(trade_index, maker_index, field));
        }
        Ok(Self {
            order_id: order.order_id,
            owner: order.owner,
            maker_address: order.maker_address,
            matched_amount: order.matched_amount,
            price: order.price,
            fee_rate_bps: order.fee_rate_bps,
            fee_usdc: order.fee_usdc,
            asset_id: order.asset_id,
            outcome: order.outcome,
            side: order.side,
        })
    }
}

impl AuthenticatedTrade {
    fn try_from_wire(
        trade_index: usize,
        trade: WireTrade,
    ) -> Result<Self, crate::auth::AuthenticatedEndpointError> {
        let invalid_field = if !venue_identifier_is_valid(&trade.id) {
            Some("id")
        } else if !venue_identifier_is_valid(&trade.taker_order_id) {
            Some("taker_order_id")
        } else if trade.size < Decimal::ZERO {
            Some("size")
        } else if trade.price < Decimal::ZERO || trade.price > Decimal::ONE {
            Some("price")
        } else if trade.fee_rate_bps < Decimal::ZERO {
            Some("fee_rate_bps")
        } else if trade.fee_usdc.is_some_and(|fee| fee < Decimal::ZERO) {
            Some("fee_usdc")
        } else {
            None
        };
        if let Some(field) = invalid_field {
            return Err(indexed_trade_schema_error(trade_index, field));
        }
        let maker_orders = trade
            .maker_orders
            .into_iter()
            .enumerate()
            .map(|(maker_index, order)| {
                AuthenticatedMakerOrder::try_from_wire(trade_index, maker_index, order)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            id: trade.id,
            taker_order_id: trade.taker_order_id,
            market: trade.market,
            asset_id: trade.asset_id,
            side: trade.side,
            size: trade.size,
            fee_rate_bps: trade.fee_rate_bps,
            fee_usdc: trade.fee_usdc,
            price: trade.price,
            status: trade.status,
            match_time: trade.match_time,
            last_update: trade.last_update,
            outcome: trade.outcome,
            bucket_index: trade.bucket_index,
            owner: trade.owner,
            maker_address: trade.maker_address,
            maker_orders,
            transaction_hash: trade.transaction_hash,
            trader_side: trade.trader_side,
            error_msg: trade.error_msg,
        })
    }
}

fn indexed_trade_schema_error(
    trade_index: usize,
    field: &'static str,
) -> crate::auth::AuthenticatedEndpointError {
    crate::auth::AuthenticatedEndpointError::response_schema_decode(
        crate::auth::AuthenticatedEndpoint::Trades,
        format!("data[{trade_index}].{field}"),
    )
}

fn indexed_maker_schema_error(
    trade_index: usize,
    maker_index: usize,
    field: &'static str,
) -> crate::auth::AuthenticatedEndpointError {
    crate::auth::AuthenticatedEndpointError::response_schema_decode(
        crate::auth::AuthenticatedEndpoint::Trades,
        format!("data[{trade_index}].maker_orders[{maker_index}].{field}"),
    )
}

#[derive(Clone, Default, Eq, PartialEq)]
pub struct AuthenticatedTradesRequest {
    pub id: Option<String>,
    pub maker_address: Option<Address>,
    pub market: Option<B256>,
    pub asset_id: Option<U256>,
    pub before: Option<i64>,
    pub after: Option<i64>,
}

impl core::fmt::Debug for AuthenticatedTradesRequest {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("AuthenticatedTradesRequest")
            .field("has_id", &self.id.is_some())
            .field("has_maker_address", &self.maker_address.is_some())
            .field("has_market", &self.market.is_some())
            .field("has_asset_id", &self.asset_id.is_some())
            .field("before", &self.before)
            .field("after", &self.after)
            .finish()
    }
}

impl AuthenticatedTradesRequest {
    #[must_use]
    pub const fn with_after(mut self, after: i64) -> Self {
        self.after = Some(after);
        self
    }

    fn query(
        &self,
        next_cursor: Option<&str>,
    ) -> core::result::Result<String, crate::auth::AuthenticatedEndpointError> {
        if self.before.is_some_and(|value| value < 0)
            || self.after.is_some_and(|value| value < 0)
            || self
                .id
                .as_ref()
                .is_some_and(|value| !venue_identifier_is_valid(value))
            || next_cursor.is_some_and(|value| !cursor_is_valid(value))
        {
            return Err(crate::auth::AuthenticatedEndpointError::request_failed(
                crate::auth::AuthenticatedEndpoint::Trades,
            ));
        }

        let mut query = url::form_urlencoded::Serializer::new(String::new());
        append_query_pair(&mut query, "id", self.id.as_deref());
        let maker_address = self.maker_address.map(|value| value.to_string());
        append_query_pair(&mut query, "maker_address", maker_address.as_deref());
        let market = self.market.map(|value| value.to_string());
        append_query_pair(&mut query, "market", market.as_deref());
        let asset_id = self.asset_id.map(|value| value.to_string());
        append_query_pair(&mut query, "asset_id", asset_id.as_deref());
        let before = self.before.map(|value| value.to_string());
        append_query_pair(&mut query, "before", before.as_deref());
        let after = self.after.map(|value| value.to_string());
        append_query_pair(&mut query, "after", after.as_deref());
        append_query_pair(&mut query, "next_cursor", next_cursor);
        Ok(query.finish())
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct AuthenticatedTradesClient;

impl AuthenticatedTradesClient {
    pub(crate) async fn fetch(
        &self,
        http: &AuthenticatedHttpClient,
        client: &Client<Authenticated<Normal>>,
        request: &AuthenticatedTradesRequest,
        next_cursor: Option<&str>,
    ) -> core::result::Result<AuthenticatedTradePage, crate::auth::AuthenticatedEndpointError> {
        let endpoint = crate::auth::AuthenticatedEndpoint::Trades;
        let query = request.query(next_cursor)?;
        let body = http
            .get(client, endpoint, TRADES_PATH, Some(&query))
            .await?;
        AuthenticatedTradePage::decode_json(&body)
    }
}

fn wire_maker_orders_or_empty<'de, D>(
    deserializer: D,
) -> core::result::Result<Vec<WireMakerOrder>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<Vec<WireMakerOrder>>::deserialize(deserializer)?.unwrap_or_default())
}

#[cfg(test)]
mod tests;
