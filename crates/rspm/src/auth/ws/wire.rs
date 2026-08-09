//! Strict all-or-nothing private-frame decoder.

use crate::auth::ws::{
    AuthenticatedUserEvent, AuthenticatedUserMakerOrder, AuthenticatedUserOrder,
    AuthenticatedUserOrderStatus, AuthenticatedUserOrderType, AuthenticatedUserTrade,
    AuthenticatedUserTradeStatus, AuthenticatedUserVenueOrderType, AuthenticatedUserWsError,
};
use crate::auth::{AuthenticatedTraderSide, AuthenticatedVenueSide, venue_identifier_is_valid};
use crate::utils::{canonical_unsigned_integer_text, optional_quoted_decimal, quoted_decimal};
use core::str::FromStr as _;
use polymarket::{
    auth::ApiKey,
    types::{Address, B256, Decimal, U256},
};
use serde::{Deserialize, Deserializer, de::Error as _};

pub(super) struct WireUserEventBatch(pub(super) Vec<WireUserEvent>);

impl<'de> Deserialize<'de> for WireUserEventBatch {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct OneOrMany;

        impl<'de> serde::de::Visitor<'de> for OneOrMany {
            type Value = WireUserEventBatch;

            fn expecting(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                formatter.write_str("one authenticated user event or an array of events")
            }

            fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                let event =
                    WireUserEvent::deserialize(serde::de::value::MapAccessDeserializer::new(map))?;
                Ok(WireUserEventBatch(vec![event]))
            }

            fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let mut events = Vec::with_capacity(sequence.size_hint().unwrap_or(0));
                while let Some(event) = sequence.next_element::<WireUserEvent>()? {
                    events.push(event);
                }
                Ok(WireUserEventBatch(events))
            }
        }

        deserializer.deserialize_any(OneOrMany)
    }
}

#[derive(Deserialize)]
#[serde(tag = "event_type")]
pub(super) enum WireUserEvent {
    #[serde(rename = "trade")]
    Trade(WireTrade),
    #[serde(rename = "order")]
    Order(WireOrder),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireMakerOrder {
    asset_id: U256,
    #[serde(deserialize_with = "quoted_decimal")]
    matched_amount: Decimal,
    order_id: String,
    owner: ApiKey,
    #[serde(deserialize_with = "quoted_decimal")]
    price: Decimal,
    side: AuthenticatedVenueSide,
    #[serde(default)]
    maker_address: Option<Address>,
    #[serde(default)]
    outcome: Option<String>,
    #[serde(default, deserialize_with = "optional_u64")]
    outcome_index: Option<u64>,
    #[serde(default, deserialize_with = "optional_quoted_decimal")]
    fee_rate_bps: Option<Decimal>,
    #[serde(
        default,
        alias = "feeUsdc",
        deserialize_with = "optional_quoted_decimal"
    )]
    fee_usdc: Option<Decimal>,
}

#[derive(Clone, Copy, Deserialize)]
enum WireTradeType {
    #[serde(rename = "TRADE", alias = "trade")]
    Trade,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WireTrade {
    id: String,
    market: B256,
    asset_id: U256,
    side: AuthenticatedVenueSide,
    #[serde(deserialize_with = "quoted_decimal")]
    size: Decimal,
    #[serde(deserialize_with = "quoted_decimal")]
    price: Decimal,
    status: AuthenticatedUserTradeStatus,
    #[serde(rename = "type", default)]
    message_type: Option<WireTradeType>,
    #[serde(default, deserialize_with = "optional_quoted_i64")]
    last_update: Option<i64>,
    #[serde(
        default,
        alias = "match_time",
        deserialize_with = "optional_quoted_i64"
    )]
    matchtime: Option<i64>,
    #[serde(default, deserialize_with = "optional_quoted_i64")]
    timestamp: Option<i64>,
    #[serde(default)]
    owner: Option<ApiKey>,
    #[serde(default)]
    trade_owner: Option<ApiKey>,
    #[serde(default)]
    taker_order_id: Option<String>,
    #[serde(default, deserialize_with = "wire_maker_orders_or_empty")]
    maker_orders: Vec<WireMakerOrder>,
    #[serde(default, deserialize_with = "optional_quoted_decimal")]
    fee_rate_bps: Option<Decimal>,
    #[serde(
        default,
        alias = "feeUsdc",
        deserialize_with = "optional_quoted_decimal"
    )]
    fee_usdc: Option<Decimal>,
    #[serde(default)]
    trader_side: Option<AuthenticatedTraderSide>,
    #[serde(default)]
    outcome: Option<String>,
    #[serde(default)]
    maker_address: Option<Address>,
    #[serde(default, deserialize_with = "optional_empty_b256")]
    transaction_hash: Option<B256>,
    #[serde(default, deserialize_with = "optional_u64")]
    bucket_index: Option<u64>,
}

fn wire_maker_orders_or_empty<'de, D>(deserializer: D) -> Result<Vec<WireMakerOrder>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<Vec<WireMakerOrder>>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WireOrder {
    id: String,
    market: B256,
    asset_id: U256,
    side: AuthenticatedVenueSide,
    #[serde(deserialize_with = "quoted_decimal")]
    price: Decimal,
    #[serde(rename = "type", default)]
    event_type: Option<AuthenticatedUserOrderType>,
    #[serde(default, deserialize_with = "optional_quoted_decimal")]
    original_size: Option<Decimal>,
    #[serde(default, deserialize_with = "optional_quoted_decimal")]
    size_matched: Option<Decimal>,
    #[serde(default, deserialize_with = "optional_quoted_i64")]
    timestamp: Option<i64>,
    #[serde(default, rename = "associate_trades")]
    associated_trades: Option<Vec<String>>,
    #[serde(default)]
    status: Option<AuthenticatedUserOrderStatus>,
    #[serde(default)]
    owner: Option<ApiKey>,
    #[serde(default)]
    order_owner: Option<ApiKey>,
    #[serde(default)]
    outcome: Option<String>,
    #[serde(default, deserialize_with = "optional_i64")]
    created_at: Option<i64>,
    #[serde(default, deserialize_with = "optional_i64")]
    expiration: Option<i64>,
    #[serde(default)]
    order_type: Option<AuthenticatedUserVenueOrderType>,
    #[serde(default)]
    maker_address: Option<Address>,
}

fn optional_outcome_is_valid(outcome: Option<&str>) -> bool {
    outcome.is_none_or(|value| {
        !value.trim().is_empty()
            && value.len() <= 256
            && value
                .bytes()
                .all(|byte| byte.is_ascii() && !byte.is_ascii_control())
    })
}

impl TryFrom<WireMakerOrder> for AuthenticatedUserMakerOrder {
    type Error = AuthenticatedUserWsError;

    fn try_from(order: WireMakerOrder) -> Result<Self, Self::Error> {
        if !venue_identifier_is_valid(&order.order_id)
            || order.matched_amount < Decimal::ZERO
            || order.price < Decimal::ZERO
            || order.price > Decimal::ONE
            || order.fee_rate_bps.is_some_and(|fee| fee < Decimal::ZERO)
            || order.fee_usdc.is_some_and(|fee| fee < Decimal::ZERO)
            || !optional_outcome_is_valid(order.outcome.as_deref())
        {
            return Err(AuthenticatedUserWsError::FrameSchema);
        }
        Ok(Self {
            asset_id: order.asset_id,
            matched_amount: order.matched_amount,
            order_id: order.order_id,
            owner: order.owner,
            price: order.price,
            side: order.side,
            maker_address: order.maker_address,
            outcome: order.outcome,
            outcome_index: order.outcome_index,
            fee_rate_bps: order.fee_rate_bps,
            fee_usdc: order.fee_usdc,
        })
    }
}

impl TryFrom<WireTrade> for AuthenticatedUserTrade {
    type Error = AuthenticatedUserWsError;

    fn try_from(trade: WireTrade) -> Result<Self, Self::Error> {
        if !venue_identifier_is_valid(&trade.id)
            || trade
                .taker_order_id
                .as_deref()
                .is_some_and(|value| !venue_identifier_is_valid(value))
            || trade.size < Decimal::ZERO
            || trade.price < Decimal::ZERO
            || trade.price > Decimal::ONE
            || trade.fee_rate_bps.is_some_and(|fee| fee < Decimal::ZERO)
            || trade.fee_usdc.is_some_and(|fee| fee < Decimal::ZERO)
            || trade.message_type.is_none()
            || !optional_outcome_is_valid(trade.outcome.as_deref())
        {
            return Err(AuthenticatedUserWsError::FrameSchema);
        }
        let maker_orders = trade
            .maker_orders
            .into_iter()
            .map(AuthenticatedUserMakerOrder::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            id: trade.id,
            market: trade.market,
            asset_id: trade.asset_id,
            side: trade.side,
            size: trade.size,
            price: trade.price,
            status: trade.status,
            last_update: trade.last_update,
            match_time: trade.matchtime,
            timestamp: trade.timestamp,
            owner: trade.owner,
            trade_owner: trade.trade_owner,
            taker_order_id: trade.taker_order_id,
            maker_orders,
            outcome: trade.outcome,
            maker_address: trade.maker_address,
            transaction_hash: trade.transaction_hash,
            bucket_index: trade.bucket_index,
            fee_rate_bps: trade.fee_rate_bps,
            fee_usdc: trade.fee_usdc,
            trader_side: trade.trader_side,
        })
    }
}

impl TryFrom<WireOrder> for AuthenticatedUserOrder {
    type Error = AuthenticatedUserWsError;

    fn try_from(order: WireOrder) -> Result<Self, Self::Error> {
        let size_valid = match (order.original_size, order.size_matched) {
            (Some(original), Some(matched)) => {
                original > Decimal::ZERO && matched >= Decimal::ZERO && matched <= original
            }
            (None, None) => true,
            _ => false,
        };
        let associated_valid = order.associated_trades.as_ref().is_none_or(|identifiers| {
            identifiers
                .iter()
                .all(|value| venue_identifier_is_valid(value))
        });
        if !venue_identifier_is_valid(&order.id)
            || order.price <= Decimal::ZERO
            || order.price > Decimal::ONE
            || !size_valid
            || !associated_valid
            || !optional_outcome_is_valid(order.outcome.as_deref())
        {
            return Err(AuthenticatedUserWsError::FrameSchema);
        }
        Ok(Self {
            id: order.id,
            market: order.market,
            asset_id: order.asset_id,
            side: order.side,
            price: order.price,
            event_type: order.event_type,
            original_size: order.original_size,
            size_matched: order.size_matched,
            timestamp: order.timestamp,
            associated_trades: order.associated_trades,
            status: order.status,
            owner: order.owner,
            order_owner: order.order_owner,
            outcome: order.outcome,
            created_at: order.created_at,
            expiration: order.expiration,
            order_type: order.order_type,
            maker_address: order.maker_address,
        })
    }
}

impl TryFrom<WireUserEvent> for AuthenticatedUserEvent {
    type Error = AuthenticatedUserWsError;

    fn try_from(event: WireUserEvent) -> Result<Self, Self::Error> {
        match event {
            WireUserEvent::Trade(trade) => AuthenticatedUserTrade::try_from(trade).map(Self::Trade),
            WireUserEvent::Order(order) => AuthenticatedUserOrder::try_from(order).map(Self::Order),
        }
    }
}

fn optional_quoted_i64<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    if !canonical_unsigned_integer_text(&value) {
        return Err(D::Error::custom("expected canonical quoted timestamp"));
    }
    value
        .parse()
        .map(Some)
        .map_err(|_| D::Error::custom("quoted timestamp is out of range"))
}

#[derive(Deserialize)]
#[serde(untagged)]
enum IntegerWire {
    Number(i64),
    Text(String),
}

fn optional_i64<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    let Some(value) = Option::<IntegerWire>::deserialize(deserializer)? else {
        return Ok(None);
    };
    match value {
        IntegerWire::Number(value) if value >= 0 => Ok(Some(value)),
        IntegerWire::Text(value) if value.is_empty() => Ok(None),
        IntegerWire::Text(value) if canonical_unsigned_integer_text(&value) => value
            .parse()
            .map(Some)
            .map_err(|_| D::Error::custom("integer text is out of range")),
        IntegerWire::Number(_) | IntegerWire::Text(_) => {
            Err(D::Error::custom("expected a canonical nonnegative integer"))
        }
    }
}

fn optional_u64<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    optional_i64(deserializer)?
        .map(|value| u64::try_from(value).map_err(|_| D::Error::custom("integer is out of range")))
        .transpose()
}

fn optional_empty_b256<'de, D>(deserializer: D) -> Result<Option<B256>, D::Error>
where
    D: Deserializer<'de>,
{
    let Some(value) = Option::<String>::deserialize(deserializer)? else {
        return Ok(None);
    };
    if value.is_empty() {
        return Ok(None);
    }
    B256::from_str(&value)
        .map(Some)
        .map_err(|_| D::Error::custom("expected an exact transaction hash or empty string"))
}
