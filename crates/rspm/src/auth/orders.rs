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
mod tests {
    use super::*;
    use crate::auth::TERMINAL_CURSOR;

    fn order(status: &str) -> serde_json::Value {
        serde_json::json!({
            "id": "synthetic-order",
            "status": status,
            "owner": "ffffffff-ffff-ffff-ffff-ffffffffffff",
            "maker_address": "0x2222222222222222222222222222222222222222",
            "market": format!("0x{}", "0".repeat(64)),
            "asset_id": "7",
            "side": "BUY",
            "original_size": "10.0",
            "size_matched": "2.5",
            "price": "0.45",
            "associate_trades": [],
            "outcome": "YES",
            "created_at": 1_705_322_096,
            "expiration": "1705708800",
            "order_type": "GTC"
        })
    }

    fn page(data: Vec<serde_json::Value>, count: u64, limit: u64, cursor: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "data": data,
            "limit": limit,
            "count": count,
            "next_cursor": cursor
        }))
        .expect("fixture")
    }

    #[test]
    fn current_statuses_decode_and_unknown_rejects_page() {
        for status in [
            "ORDER_STATUS_LIVE",
            "ORDER_STATUS_INVALID",
            "ORDER_STATUS_CANCELED_MARKET_RESOLVED",
            "ORDER_STATUS_CANCELED",
            "ORDER_STATUS_MATCHED",
            "ORDER_STATUS_DELAYED",
            "ORDER_STATUS_UNMATCHED",
        ] {
            AuthenticatedOrderPage::decode_json(&page(
                vec![order(status)],
                1,
                100,
                TERMINAL_CURSOR,
            ))
            .expect("documented status");
        }
        let error = AuthenticatedOrderPage::decode_json(&page(
            vec![order("ORDER_STATUS_NEW")],
            1,
            100,
            TERMINAL_CURSOR,
        ))
        .expect_err("unknown status must reject page");
        assert_eq!(error.response_path(), Some("data[0].status"));
    }

    #[test]
    fn delayed_and_unmatched_aliases_decode_atomically_as_complete_pages() {
        for status in [
            "ORDER_STATUS_DELAYED",
            "DELAYED",
            "delayed",
            "ORDER_STATUS_UNMATCHED",
            "UNMATCHED",
            "unmatched",
        ] {
            let decoded = AuthenticatedOrderPage::decode_json(&page(
                vec![order(status), order("ORDER_STATUS_LIVE")],
                2,
                100,
                TERMINAL_CURSOR,
            ))
            .expect("official nonterminal status must preserve the complete page");
            assert_eq!(decoded.data.len(), 2);
            assert!(matches!(
                decoded.data[0].status,
                AuthenticatedOrderStatus::Delayed | AuthenticatedOrderStatus::Unmatched
            ));
        }
    }

    #[test]
    fn size_price_and_expiration_require_quoted_exact_values() {
        for field in ["original_size", "size_matched", "price", "expiration"] {
            let mut value = order("ORDER_STATUS_LIVE");
            value[field] = serde_json::json!(1);
            assert!(
                AuthenticatedOrderPage::decode_json(&page(vec![value], 1, 100, TERMINAL_CURSOR))
                    .is_err(),
                "{field}"
            );
        }
    }

    #[test]
    fn order_economics_reject_zero_negative_and_overfilled_values() {
        for (field, invalid) in [
            ("original_size", "0"),
            ("size_matched", "-1"),
            ("size_matched", "11"),
            ("price", "0"),
            ("price", "1.000001"),
        ] {
            let mut value = order("ORDER_STATUS_LIVE");
            value[field] = serde_json::json!(invalid);
            assert!(
                AuthenticatedOrderPage::decode_json(&page(vec![value], 1, 100, TERMINAL_CURSOR))
                    .is_err(),
                "{field}={invalid}"
            );
        }
    }

    #[test]
    fn page_invariants_fail_closed() {
        AuthenticatedOrderPage::decode_json(&page(vec![order("ORDER_STATUS_LIVE")], 1, 100, ""))
            .expect("nonempty final page with empty cursor is terminal");
        for body in [
            page(vec![order("ORDER_STATUS_LIVE")], 0, 100, TERMINAL_CURSOR),
            page(vec![order("ORDER_STATUS_LIVE")], 2, 100, TERMINAL_CURSOR),
            page(vec![order("ORDER_STATUS_LIVE")], 1, 0, TERMINAL_CURSOR),
            page(Vec::new(), 0, 100, "MTAw"),
            page(Vec::new(), 0, 100, "not a cursor"),
        ] {
            assert!(AuthenticatedOrderPage::decode_json(&body).is_err());
        }
    }

    #[test]
    fn request_serializes_only_documented_filters() {
        let request = AuthenticatedOrdersRequest {
            order_id: Some("order-id".to_owned()),
            market: Some(B256::ZERO),
            asset_id: Some(U256::from(7)),
        };
        assert_eq!(
            request.query(Some("MTAw")).expect("bounded request"),
            concat!(
                "id=order-id&",
                "market=0x0000000000000000000000000000000000000000000000000000000000000000&",
                "asset_id=7&next_cursor=MTAw"
            )
        );
    }

    #[test]
    fn authenticated_order_debug_redacts_identity_and_cursor_values() {
        let decoded = AuthenticatedOrderPage::decode_json(&page(
            vec![order("ORDER_STATUS_LIVE")],
            1,
            100,
            "cHJpdmF0ZQ==",
        ))
        .expect("valid nonterminal page");
        let request = AuthenticatedOrdersRequest {
            order_id: Some("private-order-filter".to_owned()),
            market: Some(B256::repeat_byte(0x22)),
            asset_id: Some(U256::from(123_456_u64)),
        };
        let rendered = format!("{decoded:?} {:?} {request:?}", decoded.data[0]);
        for private in [
            "synthetic-order",
            "cHJpdmF0ZQ==",
            "private-order-filter",
            "2222222222222222222222222222222222222222",
            "123456",
        ] {
            assert!(!rendered.contains(private), "leaked {private}");
        }
    }

    #[test]
    fn terminal_order_path_is_validated_and_exact() {
        assert_eq!(
            authenticated_order_path("0xabc_DEF-123").expect("safe order id"),
            "/data/order/0xabc_DEF-123"
        );
        for rejected in [
            "",
            "../orders",
            "order/id",
            "order id",
            "order?id=1",
            "order:maker",
            "order\nforged",
            "ördër",
        ] {
            assert!(authenticated_order_path(rejected).is_err(), "{rejected}");
        }
    }

    #[test]
    fn order_response_rejects_collision_and_control_id_without_echoing_it() {
        for invalid in ["order:maker", "order\tforged", "order\nforged", "ördër"] {
            let mut value = order("ORDER_STATUS_LIVE");
            value["id"] = serde_json::json!(invalid);
            let error =
                AuthenticatedOrderPage::decode_json(&page(vec![value], 1, 100, TERMINAL_CURSOR))
                    .expect_err("invalid order identity must reject complete page");
            assert!(!format!("{error:?}").contains(invalid));
        }
    }

    #[test]
    fn dedicated_order_decode_accepts_terminal_canceled_and_matched() {
        for status in [
            "ORDER_STATUS_CANCELED",
            "ORDER_STATUS_MATCHED",
            "ORDER_STATUS_DELAYED",
            "ORDER_STATUS_UNMATCHED",
        ] {
            AuthenticatedOrder::decode_json(&serde_json::to_vec(&order(status)).expect("fixture"))
                .expect("documented terminal order");
        }
    }

    /// [REGRESSION][EVAL] Account-order inventory is reconciliation evidence.
    /// New page or order fields reject atomically until their retention
    /// contract is explicit. The class stays `request_failed`, not
    /// `response_schema_decode`: an unrecognized field name is exactly the
    /// unsafe-path case `authenticated_response_path_is_safe` exists to redact
    /// (see `authenticated_trade_diagnostic_accepts_only_owned_structural_paths`
    /// in `error.rs`, whose own fixture includes `data[0].unknown_field`) — the
    /// venue-controlled key text must never reach a rendered diagnostic.
    #[test]
    fn unknown_page_or_order_evidence_rejects_atomically() {
        let base_order = order("ORDER_STATUS_LIVE");
        let mut unknown_order = base_order.clone();
        unknown_order.as_object_mut().expect("order object").insert(
            "new_execution_evidence".to_owned(),
            serde_json::json!("must-not-be-shed"),
        );
        let order_error = AuthenticatedOrderPage::decode_json(&page(
            vec![base_order, unknown_order],
            2,
            100,
            TERMINAL_CURSOR,
        ))
        .expect_err("unknown nested order evidence must reject the complete page");
        assert_eq!(order_error.error_class(), "request_failed");
        assert!(!format!("{order_error:?}").contains("must-not-be-shed"));

        let hostile_page = serde_json::json!({
            "data": [order("ORDER_STATUS_LIVE")],
            "limit": 100,
            "count": 1,
            "next_cursor": TERMINAL_CURSOR,
            "new_page_evidence": "must-not-be-shed"
        });
        let page_error = AuthenticatedOrderPage::decode_json(
            &serde_json::to_vec(&hostile_page).expect("encode hostile page"),
        )
        .expect_err("unknown page evidence must reject the complete page");
        assert_eq!(page_error.error_class(), "request_failed");
        assert!(!format!("{page_error:?}").contains("must-not-be-shed"));
    }
}
