//! Exact public private-event contract.

use polymarket::{
    auth::ApiKey,
    types::{Address, B256, Decimal, U256},
};
use serde::Deserialize;

use super::{AuthenticatedTraderSide, AuthenticatedVenueSide};

/// Closed trade lifecycle contract for authenticated user frames.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
pub enum AuthenticatedUserTradeStatus {
    #[serde(
        rename = "MATCHED_NOT_BROADCASTED",
        alias = "matched_not_broadcasted",
        alias = "TRADE_STATUS_MATCHED_NOT_BROADCASTED"
    )]
    MatchedNotBroadcasted,
    #[serde(rename = "MATCHED", alias = "matched", alias = "TRADE_STATUS_MATCHED")]
    Matched,
    #[serde(rename = "MINED", alias = "mined", alias = "TRADE_STATUS_MINED")]
    Mined,
    #[serde(
        rename = "CONFIRMED",
        alias = "confirmed",
        alias = "TRADE_STATUS_CONFIRMED"
    )]
    Confirmed,
    #[serde(
        rename = "RETRYING",
        alias = "retrying",
        alias = "TRADE_STATUS_RETRYING"
    )]
    Retrying,
    #[serde(rename = "FAILED", alias = "failed", alias = "TRADE_STATUS_FAILED")]
    Failed,
}

/// Closed order event-kind contract for authenticated user frames.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
pub enum AuthenticatedUserOrderType {
    #[serde(rename = "PLACEMENT", alias = "placement")]
    Placement,
    #[serde(rename = "UPDATE", alias = "update")]
    Update,
    #[serde(rename = "CANCELLATION", alias = "cancellation")]
    Cancellation,
}

/// Closed venue order-state contract for authenticated user frames.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
pub enum AuthenticatedUserOrderStatus {
    #[serde(rename = "LIVE", alias = "live")]
    Live,
    #[serde(rename = "MATCHED", alias = "matched")]
    Matched,
    #[serde(rename = "DELAYED", alias = "delayed")]
    Delayed,
    #[serde(rename = "UNMATCHED", alias = "unmatched")]
    Unmatched,
    #[serde(rename = "CANCELED", alias = "canceled")]
    Canceled,
}

/// Closed time-in-force contract carried by official user order frames.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
pub enum AuthenticatedUserVenueOrderType {
    /// Good until cancelled.
    #[serde(rename = "GTC")]
    Gtc,
    /// Good until the explicit expiration timestamp.
    #[serde(rename = "GTD")]
    Gtd,
    /// Fill available quantity and kill the remainder.
    #[serde(rename = "FAK")]
    Fak,
    /// Fill the entire quantity or kill the order.
    #[serde(rename = "FOK")]
    Fok,
}

/// Exact maker leg from an authenticated user trade.
#[derive(Clone, PartialEq)]
pub struct AuthenticatedUserMakerOrder {
    /// Exact venue token identity.
    pub asset_id: U256,
    /// Exact matched quantity for this maker leg.
    pub matched_amount: Decimal,
    /// Exact venue order identity.
    pub order_id: String,
    /// Redacted API-key identity reported as owner.
    pub owner: ApiKey,
    /// Exact maker limit price.
    pub price: Decimal,
    /// Venue action side.
    pub side: AuthenticatedVenueSide,
    /// On-chain maker address when present in the official frame.
    pub maker_address: Option<Address>,
    /// Exact venue outcome label when present.
    pub outcome: Option<String>,
    /// Exact outcome index, including zero, when present.
    pub outcome_index: Option<u64>,
    /// `None` means absent or explicitly blank. `Some(0)` is distinct venue
    /// evidence and must not be collapsed into `None`.
    pub fee_rate_bps: Option<Decimal>,
    pub fee_usdc: Option<Decimal>,
}

impl core::fmt::Debug for AuthenticatedUserMakerOrder {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("AuthenticatedUserMakerOrder")
            .field("matched_amount", &self.matched_amount)
            .field("price", &self.price)
            .field("side", &self.side)
            .field("outcome", &self.outcome)
            .field("outcome_index", &self.outcome_index)
            .field("fee_rate_bps", &self.fee_rate_bps)
            .finish_non_exhaustive()
    }
}

/// Exact authenticated user trade.
#[derive(Clone, PartialEq)]
pub struct AuthenticatedUserTrade {
    /// Exact venue trade identity.
    pub id: String,
    /// Exact condition/market hash.
    pub market: B256,
    /// Exact venue token identity.
    pub asset_id: U256,
    /// Venue action side.
    pub side: AuthenticatedVenueSide,
    /// Exact traded quantity.
    pub size: Decimal,
    /// Exact trade price.
    pub price: Decimal,
    /// Closed venue trade lifecycle state.
    pub status: AuthenticatedUserTradeStatus,
    /// Venue last-update timestamp when supplied.
    pub last_update: Option<i64>,
    /// Venue match timestamp when supplied.
    pub match_time: Option<i64>,
    /// Legacy event timestamp when supplied.
    pub timestamp: Option<i64>,
    /// Redacted owner API-key identity when supplied.
    pub owner: Option<ApiKey>,
    /// Redacted trade-owner API-key identity when supplied.
    pub trade_owner: Option<ApiKey>,
    /// Exact taker order identity when supplied.
    pub taker_order_id: Option<String>,
    /// Exact maker legs carried by the frame.
    pub maker_orders: Vec<AuthenticatedUserMakerOrder>,
    /// Exact outcome label when supplied.
    pub outcome: Option<String>,
    /// On-chain maker address when supplied.
    pub maker_address: Option<Address>,
    /// Confirmed transaction hash. Empty venue values remain `None`.
    pub transaction_hash: Option<B256>,
    /// Exact venue bucket index, including zero, when supplied.
    pub bucket_index: Option<u64>,
    /// Exact reported fee rate in basis points.
    pub fee_rate_bps: Option<Decimal>,
    /// Exact reported charged fee in USDC.
    pub fee_usdc: Option<Decimal>,
    /// Authenticated maker/taker role when supplied.
    pub trader_side: Option<AuthenticatedTraderSide>,
}

impl core::fmt::Debug for AuthenticatedUserTrade {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("AuthenticatedUserTrade")
            .field("side", &self.side)
            .field("size", &self.size)
            .field("price", &self.price)
            .field("status", &self.status)
            .field("last_update", &self.last_update)
            .field("match_time", &self.match_time)
            .field("timestamp", &self.timestamp)
            .field("maker_orders", &self.maker_orders)
            .field("outcome", &self.outcome)
            .field("bucket_index", &self.bucket_index)
            .field("fee_rate_bps", &self.fee_rate_bps)
            .field("trader_side", &self.trader_side)
            .finish_non_exhaustive()
    }
}

/// Exact authenticated user order-lifecycle event.
#[derive(Clone, PartialEq)]
pub struct AuthenticatedUserOrder {
    /// Exact venue order identity.
    pub id: String,
    /// Exact condition/market hash.
    pub market: B256,
    /// Exact venue token identity.
    pub asset_id: U256,
    /// Venue action side.
    pub side: AuthenticatedVenueSide,
    /// Exact limit price.
    pub price: Decimal,
    /// Placement/update/cancellation event kind.
    pub event_type: Option<AuthenticatedUserOrderType>,
    /// Original order quantity when supplied.
    pub original_size: Option<Decimal>,
    /// Cumulative matched quantity when supplied.
    pub size_matched: Option<Decimal>,
    /// Legacy event timestamp when supplied.
    pub timestamp: Option<i64>,
    /// Exact associated venue trade identities when supplied.
    pub associated_trades: Option<Vec<String>>,
    /// Closed venue order lifecycle status when supplied.
    pub status: Option<AuthenticatedUserOrderStatus>,
    /// Redacted legacy owner API-key identity when supplied.
    pub owner: Option<ApiKey>,
    /// Redacted legacy order-owner API-key identity when supplied.
    pub order_owner: Option<ApiKey>,
    /// Exact venue outcome label when supplied.
    pub outcome: Option<String>,
    /// Venue creation timestamp when supplied.
    pub created_at: Option<i64>,
    /// Venue expiration timestamp when supplied.
    pub expiration: Option<i64>,
    /// Official time-in-force when supplied.
    pub order_type: Option<AuthenticatedUserVenueOrderType>,
    /// On-chain maker address when supplied.
    pub maker_address: Option<Address>,
}

impl core::fmt::Debug for AuthenticatedUserOrder {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("AuthenticatedUserOrder")
            .field("side", &self.side)
            .field("price", &self.price)
            .field("event_type", &self.event_type)
            .field("original_size", &self.original_size)
            .field("size_matched", &self.size_matched)
            .field("timestamp", &self.timestamp)
            .field("status", &self.status)
            .field("outcome", &self.outcome)
            .field("created_at", &self.created_at)
            .field("expiration", &self.expiration)
            .field("order_type", &self.order_type)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, PartialEq)]
pub enum AuthenticatedUserEvent {
    Trade(AuthenticatedUserTrade),
    Order(AuthenticatedUserOrder),
}

impl core::fmt::Debug for AuthenticatedUserEvent {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Trade(trade) => formatter.debug_tuple("Trade").field(trade).finish(),
            Self::Order(order) => formatter.debug_tuple("Order").field(order).finish(),
        }
    }
}
