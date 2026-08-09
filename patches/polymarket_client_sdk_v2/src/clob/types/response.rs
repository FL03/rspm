#![allow(
    clippy::module_name_repetitions,
    reason = "Response suffix is intentional for clarity"
)]

use std::collections::HashMap;

use bon::Builder;
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use serde_with::{
    DefaultOnError, DefaultOnNull, DisplayFromStr, NoneAsEmptyString, TimestampMilliSeconds,
    TimestampSeconds, TryFromInto, serde_as,
};
use sha2::{Digest as _, Sha256};
use uuid::Uuid;

use crate::Result;
use crate::auth::ApiKey;
use crate::clob::types::{OrderStatusType, OrderType, Side, TickSize, TradeStatusType, TraderSide};
use crate::serde_helpers::StringFromAny;
use crate::types::{Address, B256, Decimal, U256};

#[non_exhaustive]
#[derive(Clone, Debug, Deserialize, Builder, PartialEq)]
pub struct MidpointResponse {
    pub mid: Decimal,
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, Deserialize, Builder, PartialEq)]
#[serde(transparent)]
pub struct MidpointsResponse {
    pub midpoints: HashMap<U256, Decimal>,
}

#[non_exhaustive]
#[derive(Clone, Debug, Deserialize, Builder, PartialEq)]
pub struct PriceResponse {
    pub price: Decimal,
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, Deserialize, Builder, PartialEq)]
#[serde(transparent)]
pub struct PricesResponse {
    pub prices: Option<HashMap<U256, HashMap<Side, Decimal>>>,
}

#[non_exhaustive]
#[derive(Clone, Debug, Deserialize, Builder, PartialEq)]
pub struct SpreadResponse {
    pub spread: Decimal,
}

#[non_exhaustive]
#[derive(Clone, Debug, Deserialize, Builder, PartialEq)]
pub struct SpreadsResponse {
    pub spreads: Option<HashMap<U256, Decimal>>,
}

#[non_exhaustive]
#[derive(Clone, Debug, Deserialize, Builder, PartialEq)]
pub struct PriceHistoryResponse {
    pub history: Vec<PricePoint>,
}

#[non_exhaustive]
#[derive(Clone, Debug, Deserialize, Builder, PartialEq)]
pub struct PricePoint {
    pub t: i64,
    pub p: Decimal,
}

#[non_exhaustive]
#[derive(Clone, Debug, Deserialize, Builder, PartialEq)]
#[builder(on(TickSize, into))]
pub struct TickSizeResponse {
    pub minimum_tick_size: TickSize,
}

#[non_exhaustive]
#[derive(Clone, Debug, Deserialize, Builder, PartialEq)]
pub struct NegRiskResponse {
    pub neg_risk: bool,
}

#[non_exhaustive]
#[derive(Clone, Debug, Deserialize, Builder, PartialEq)]
pub struct FeeRateResponse {
    pub base_fee: u32,
}

/// Response from the Polymarket geoblock endpoint.
///
/// This indicates whether the requesting IP address is blocked from placing orders
/// due to geographic restrictions.
#[non_exhaustive]
#[derive(Clone, Debug, Deserialize, Builder, PartialEq)]
pub struct GeoblockResponse {
    /// Whether the user is blocked from placing orders
    pub blocked: bool,
    /// The detected IP address
    pub ip: String,
    /// ISO 3166-1 alpha-2 country code
    pub country: String,
    /// Region/state code
    pub region: String,
}

#[non_exhaustive]
#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize, Builder, PartialEq)]
#[builder(on(String, into))]
pub struct OrderBookSummaryResponse {
    /// The market condition ID.
    pub market: B256,
    pub asset_id: U256,
    #[serde_as(as = "TimestampMilliSeconds<String>")]
    pub timestamp: DateTime<Utc>,
    #[serde(default)]
    pub hash: Option<String>,
    #[builder(default)]
    #[serde(default)]
    #[serde_as(deserialize_as = "DefaultOnNull")]
    pub bids: Vec<OrderSummary>,
    #[builder(default)]
    #[serde(default)]
    #[serde_as(deserialize_as = "DefaultOnNull")]
    pub asks: Vec<OrderSummary>,
    pub min_order_size: Decimal,
    pub neg_risk: bool,
    #[serde_as(as = "TryFromInto<Decimal>")]
    pub tick_size: TickSize,
    #[serde(default)]
    #[serde_as(deserialize_as = "DefaultOnError")]
    pub last_trade_price: Option<Decimal>,
}

impl OrderBookSummaryResponse {
    pub fn hash(&self) -> Result<String> {
        let json = serde_json::to_string(&self)?;

        let mut hasher = Sha256::new();
        hasher.update(json.as_bytes());
        let result = hasher.finalize();

        Ok(format!("{result:x}"))
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, Serialize, Deserialize, Hash, Builder, PartialEq)]
pub struct OrderSummary {
    pub price: Decimal,
    pub size: Decimal,
}

#[non_exhaustive]
#[derive(Debug, Deserialize, Builder, PartialEq)]
pub struct LastTradePriceResponse {
    pub price: Decimal,
    pub side: Side,
}

#[non_exhaustive]
#[derive(Debug, Deserialize, Builder, PartialEq)]
#[builder(on(String, into))]
pub struct LastTradesPricesResponse {
    pub token_id: U256,
    pub price: Decimal,
    pub side: Side,
}

/// Response from `GET /markets-by-token/{token_id}`. This endpoint returns a minimal
/// market descriptor — just the condition ID and the two outcome token IDs — not a full
/// [`MarketResponse`]. Used to resolve `token_id -> condition_id` before fetching the
/// full clob-market info.
#[non_exhaustive]
#[serde_as]
#[derive(Debug, Clone, Deserialize, Builder, PartialEq)]
pub struct MarketByTokenResponse {
    pub condition_id: B256,
    #[serde_as(as = "DisplayFromStr")]
    pub primary_token_id: U256,
    #[serde_as(as = "DisplayFromStr")]
    pub secondary_token_id: U256,
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "The current API has these fields, so we have to capture this"
)]
#[non_exhaustive]
#[serde_as]
#[derive(Debug, Serialize, Deserialize, Clone, Builder, PartialEq)]
#[builder(on(String, into))]
pub struct MarketResponse {
    pub enable_order_book: bool,
    pub active: bool,
    pub closed: bool,
    pub archived: bool,
    pub accepting_orders: bool,
    pub accepting_order_timestamp: Option<DateTime<Utc>>,
    pub minimum_order_size: Decimal,
    pub minimum_tick_size: Decimal,
    /// The market condition ID (unique market identifier).
    #[serde_as(as = "NoneAsEmptyString")]
    #[serde(default)]
    pub condition_id: Option<B256>,
    /// The CTF question ID.
    #[serde_as(as = "NoneAsEmptyString")]
    #[serde(default)]
    pub question_id: Option<B256>,
    pub question: String,
    pub description: String,
    pub market_slug: String,
    pub end_date_iso: Option<DateTime<Utc>>,
    pub game_start_time: Option<DateTime<Utc>>,
    pub seconds_delay: u64,
    /// The FPMM (Fixed Product Market Maker) contract address.
    #[serde_as(as = "NoneAsEmptyString")]
    #[serde(default)]
    pub fpmm: Option<Address>,
    pub maker_base_fee: Decimal,
    pub taker_base_fee: Decimal,
    pub notifications_enabled: bool,
    pub neg_risk: bool,
    /// The negative risk market ID (empty string if not a neg risk market).
    #[serde_as(as = "DefaultOnError<NoneAsEmptyString>")]
    #[serde(default)]
    pub neg_risk_market_id: Option<B256>,
    /// The negative risk request ID (empty string if not a neg risk market).
    #[serde_as(as = "NoneAsEmptyString")]
    #[serde(default)]
    pub neg_risk_request_id: Option<B256>,
    pub icon: String,
    pub image: String,
    pub rewards: Rewards,
    pub is_50_50_outcome: bool,
    #[serde(default)]
    #[serde_as(deserialize_as = "DefaultOnNull")]
    pub tokens: Vec<Token>,
    #[serde(default)]
    #[serde_as(deserialize_as = "DefaultOnNull")]
    pub tags: Vec<String>,
}

#[non_exhaustive]
#[derive(Debug, Serialize, Deserialize, Clone, Builder, PartialEq)]
#[builder(on(String, into))]
pub struct Token {
    pub token_id: U256,
    pub outcome: String,
    pub price: Decimal,
    #[serde(default)]
    pub winner: bool,
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "The current API has these fields"
)]
#[non_exhaustive]
#[serde_as]
#[derive(Debug, Default, Serialize, Deserialize, Clone, Builder, PartialEq)]
#[builder(on(String, into))]
pub struct SimplifiedMarketResponse {
    /// The market condition ID (unique market identifier).
    #[serde_as(as = "NoneAsEmptyString")]
    #[serde(default)]
    pub condition_id: Option<B256>,
    #[serde(default)]
    #[serde_as(deserialize_as = "DefaultOnNull")]
    pub tokens: Vec<Token>,
    pub rewards: Rewards,
    pub active: bool,
    pub closed: bool,
    pub archived: bool,
    pub accepting_orders: bool,
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, Deserialize, Builder, PartialEq)]
pub struct ApiKeysResponse {
    #[serde(rename = "apiKeys")]
    keys: Option<Vec<ApiKey>>,
}

#[non_exhaustive]
#[derive(Debug, Clone, Deserialize, Builder, PartialEq)]
pub struct BanStatusResponse {
    pub closed_only: bool,
}

#[non_exhaustive]
#[serde_as]
#[derive(Debug, Clone, Deserialize, Builder, PartialEq)]
#[serde(rename_all = "camelCase")]
#[builder(on(String, into))]
pub struct PostOrderResponse {
    pub error_msg: Option<String>,
    #[serde(deserialize_with = "empty_string_as_zero")]
    pub making_amount: Decimal,
    #[serde(deserialize_with = "empty_string_as_zero")]
    pub taking_amount: Decimal,
    #[serde(rename = "orderID")]
    pub order_id: String,
    pub status: OrderStatusType,
    pub success: bool,
    /// Settlement transaction hashes for the order's trades, returned on a
    /// best-effort basis when the order matched.
    #[builder(default)]
    #[serde(default)]
    #[serde_as(deserialize_as = "DefaultOnNull")]
    #[serde(alias = "transactionsHashes")]
    pub transaction_hashes: Vec<B256>,
    /// IDs of the trades created when the order matched.
    #[builder(default)]
    #[serde(default)]
    #[serde_as(deserialize_as = "DefaultOnNull")]
    #[serde(alias = "tradeIDs")]
    pub trade_ids: Vec<String>,
}

pub fn empty_string_as_zero<'de, D>(deserializer: D) -> std::result::Result<Decimal, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;

    if s.trim().is_empty() {
        Ok(Decimal::ZERO)
    } else {
        s.parse::<Decimal>().map_err(serde::de::Error::custom)
    }
}

/// Deserialize a possibly-blank decimal string as `Option<Decimal>`, treating a
/// blank/absent value as "no evidence reported" rather than "reported zero".
///
/// The CLOB API returns `fee_rate_bps: ""` for trade/maker-order records that
/// carry no per-record V2 fee accounting (for example legacy trades matched
/// before per-leg fee tracking existed). That is NOT the same fact as a
/// reported `"0"`: a present `"0"` is the venue asserting the fee was exactly
/// zero, while blank/missing is the venue asserting nothing at all. Folding
/// both into `Decimal::ZERO` would forge fee evidence our own reconciliation
/// treats as meaningfully distinct (see `reported_fee_rate_bps: Option<Decimal>`
/// and its "exact zero authorized by documented match-time `fee_rate_bps = 0`"
/// contract in `crates/store`). A non-empty value that still fails to parse
/// as a `Decimal` is real protocol drift and must keep rejecting the page —
/// this helper only widens what counts as "absent," never what counts as
/// "valid."
pub fn empty_string_as_none<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<Decimal>, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;

    if s.trim().is_empty() {
        Ok(None)
    } else {
        s.parse::<Decimal>()
            .map(Some)
            .map_err(serde::de::Error::custom)
    }
}

pub fn empty_string_as_zero_hash<'de, D>(deserializer: D) -> std::result::Result<B256, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;

    if s.trim().is_empty() {
        Ok(B256::ZERO)
    } else {
        s.parse::<B256>().map_err(serde::de::Error::custom)
    }
}

#[non_exhaustive]
#[serde_as]
#[derive(Debug, Clone, Deserialize, Builder, PartialEq)]
#[builder(on(String, into))]
pub struct OpenOrderResponse {
    pub id: String,
    pub status: OrderStatusType,
    pub owner: ApiKey,
    pub maker_address: Address,
    /// The market condition ID.
    pub market: B256,
    pub asset_id: U256,
    pub side: Side,
    pub original_size: Decimal,
    pub size_matched: Decimal,
    pub price: Decimal,
    #[serde(default)]
    #[serde_as(deserialize_as = "DefaultOnNull")]
    pub associate_trades: Vec<String>,
    pub outcome: String,
    #[serde(with = "chrono::serde::ts_seconds")]
    pub created_at: DateTime<Utc>,
    #[serde_as(as = "TimestampSeconds<String>")]
    pub expiration: DateTime<Utc>,
    pub order_type: OrderType,
}

#[non_exhaustive]
#[serde_as]
#[derive(Debug, Default, Deserialize, Builder, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CancelOrdersResponse {
    #[builder(default)]
    #[serde(default)]
    #[serde_as(deserialize_as = "DefaultOnNull")]
    pub canceled: Vec<String>,
    #[builder(default)]
    #[serde(default)]
    #[serde_as(deserialize_as = "DefaultOnNull")]
    #[serde(alias = "not_canceled")]
    pub not_canceled: HashMap<String, String>,
}

#[non_exhaustive]
#[serde_as]
#[derive(Debug, Clone, Deserialize, Builder, PartialEq)]
#[builder(on(String, into))]
pub struct TradeResponse {
    pub id: String,
    pub taker_order_id: String,
    /// The market condition ID.
    pub market: B256,
    pub asset_id: U256,
    pub side: Side,
    pub size: Decimal,
    /// Basis-point fee rate the venue reported for this trade.
    ///
    /// `None` when the venue's response omits or blanks this field (no V2
    /// per-record fee evidence exists yet); `Some(Decimal::ZERO)` when the
    /// venue explicitly reported a zero fee. See [`empty_string_as_none`].
    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub fee_rate_bps: Option<Decimal>,
    pub price: Decimal,
    pub status: TradeStatusType,
    #[serde_as(as = "TimestampSeconds<String>")]
    pub match_time: DateTime<Utc>,
    #[serde_as(as = "TimestampSeconds<String>")]
    pub last_update: DateTime<Utc>,
    pub outcome: String,
    pub bucket_index: u32,
    pub owner: ApiKey,
    pub maker_address: Address,
    #[serde(default)]
    #[serde_as(deserialize_as = "DefaultOnNull")]
    pub maker_orders: Vec<MakerOrder>,
    /// On-chain transaction hash. Zero until the trade's transaction has been
    /// submitted (servers running the async execution pipeline create trades
    /// before broadcasting).
    #[serde(default, deserialize_with = "empty_string_as_zero_hash")]
    pub transaction_hash: B256,
    pub trader_side: TraderSide,
    #[serde(default)]
    pub error_msg: Option<String>,
}

#[non_exhaustive]
#[derive(Debug, Clone, Deserialize, Builder, PartialEq)]
pub struct NotificationResponse {
    pub r#type: u32,
    pub owner: ApiKey,
    pub payload: NotificationPayload,
}

#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, Builder, PartialEq)]
#[builder(on(String, into))]
pub struct NotificationPayload {
    pub asset_id: U256,
    /// The market condition ID (unique market identifier).
    pub condition_id: B256,
    #[serde(rename = "eventSlug")]
    pub event_slug: String,
    pub icon: String,
    pub image: String,
    /// The market condition ID (same as `condition_id`).
    pub market: B256,
    pub market_slug: String,
    pub matched_size: Decimal,
    pub order_id: String,
    pub original_size: Decimal,
    pub outcome: String,
    pub outcome_index: u64,
    pub owner: ApiKey,
    pub price: Decimal,
    pub question: String,
    pub remaining_size: Decimal,
    #[serde(rename = "seriesSlug")]
    pub series_slug: String,
    pub side: Side,
    pub trade_id: String,
    /// On-chain transaction hash.
    pub transaction_hash: B256,
    #[serde(alias = "type")]
    pub order_type: OrderType,
}

#[non_exhaustive]
#[allow(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "Bon will generate code that has an allow attribute for some reason on the `allowances` field"
)]
#[derive(Debug, Default, Clone, Deserialize, Builder, PartialEq)]
pub struct BalanceAllowanceResponse {
    pub balance: Decimal,
    #[serde(default)]
    #[builder(default)]
    pub allowances: HashMap<Address, String>,
}

#[non_exhaustive]
#[derive(Debug, Clone, Deserialize, Builder, PartialEq)]
pub struct OrderScoringResponse {
    pub scoring: bool,
}

pub type OrdersScoringResponse = HashMap<String, bool>;

#[non_exhaustive]
#[derive(Clone, Debug, Deserialize, Builder, PartialEq)]
pub struct PriceSideResponse {
    pub side: Side,
    pub price: Decimal,
}

#[non_exhaustive]
#[derive(Debug, Serialize, Deserialize, Clone, Builder, PartialEq)]
pub struct RewardRate {
    pub asset_address: Address,
    pub rewards_daily_rate: Decimal,
}

#[non_exhaustive]
#[serde_as]
#[derive(Debug, Default, Clone, Serialize, Deserialize, Builder, PartialEq)]
pub struct Rewards {
    #[builder(default)]
    #[serde(default)]
    #[serde_as(deserialize_as = "DefaultOnNull")]
    pub rates: Vec<RewardRate>,
    pub min_size: Decimal,
    pub max_spread: Decimal,
}

#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, Builder, PartialEq)]
#[builder(on(String, into))]
pub struct UserInfo {
    pub address: Address,
    pub username: String,
    pub profile_picture: String,
    pub optimized_profile_picture: String,
    pub pseudonym: String,
}

#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, Builder, PartialEq)]
#[builder(on(String, into))]
pub struct MakerOrder {
    pub order_id: String,
    pub owner: ApiKey,
    pub maker_address: Address,
    pub matched_amount: Decimal,
    pub price: Decimal,
    /// Basis-point fee rate the venue reported for this maker leg.
    ///
    /// The venue returns `""` for maker orders with no per-leg V2 fee
    /// evidence — observed in production (2026-08-03) crashing the whole
    /// `Page<TradeResponse>` decode via the bare-`Decimal` field this
    /// replaces. `None` here means "no evidence reported," distinct from
    /// `Some(Decimal::ZERO)` meaning "the venue reported exactly zero." See
    /// [`empty_string_as_none`].
    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub fee_rate_bps: Option<Decimal>,
    pub asset_id: U256,
    pub outcome: String,
    pub side: Side,
}

#[non_exhaustive]
#[derive(Debug, Clone, Deserialize, Builder, PartialEq)]
#[builder(on(String, into))]
pub struct UserEarningResponse {
    pub date: NaiveDate,
    /// The market condition ID (unique market identifier).
    pub condition_id: B256,
    pub asset_address: Address,
    pub maker_address: Address,
    pub earnings: Decimal,
    pub asset_rate: Decimal,
}

#[non_exhaustive]
#[derive(Debug, Clone, Deserialize, Builder, PartialEq)]
#[builder(on(String, into))]
pub struct TotalUserEarningResponse {
    pub date: NaiveDate,
    pub asset_address: Address,
    pub maker_address: Address,
    pub earnings: Decimal,
    pub asset_rate: Decimal,
}

#[non_exhaustive]
#[serde_as]
#[derive(Debug, Clone, Deserialize, Builder, PartialEq)]
#[builder(on(String, into))]
pub struct UserRewardsEarningResponse {
    /// The market condition ID (unique market identifier).
    pub condition_id: B256,
    pub question: String,
    pub market_slug: String,
    pub event_slug: String,
    pub image: String,
    pub rewards_max_spread: Decimal,
    pub rewards_min_size: Decimal,
    pub market_competitiveness: Decimal,
    #[serde(default)]
    #[serde_as(deserialize_as = "DefaultOnNull")]
    pub tokens: Vec<Token>,
    #[serde(default)]
    #[serde_as(deserialize_as = "DefaultOnNull")]
    pub rewards_config: Vec<RewardsConfig>,
    pub maker_address: Address,
    pub earning_percentage: Decimal,
    #[serde(default)]
    #[serde_as(deserialize_as = "DefaultOnNull")]
    pub earnings: Vec<Earning>,
}

#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, Builder, PartialEq)]
pub struct RewardsConfig {
    pub asset_address: Address,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub rate_per_day: Decimal,
    pub total_rewards: Decimal,
}

#[non_exhaustive]
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize, Builder, PartialEq)]
#[builder(on(String, into))]
pub struct MarketRewardsConfig {
    #[serde_as(as = "StringFromAny")]
    pub id: String,
    pub asset_address: Address,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub rate_per_day: Decimal,
    pub total_rewards: Decimal,
    pub total_days: Decimal,
}

#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, Builder, PartialEq)]
pub struct Earning {
    pub asset_address: Address,
    pub earnings: Decimal,
    pub asset_rate: Decimal,
}

pub type RewardsPercentagesResponse = HashMap<String, Decimal>;

#[non_exhaustive]
#[serde_as]
#[derive(Debug, Clone, Deserialize, Builder, PartialEq)]
#[builder(on(String, into))]
pub struct CurrentRewardResponse {
    /// The market condition ID (unique market identifier).
    pub condition_id: B256,
    #[serde(default)]
    #[serde_as(deserialize_as = "DefaultOnNull")]
    pub rewards_config: Vec<RewardsConfig>,
    pub rewards_max_spread: Decimal,
    pub rewards_min_size: Decimal,
}

#[non_exhaustive]
#[serde_as]
#[derive(Debug, Clone, Deserialize, Builder, PartialEq)]
#[builder(on(String, into))]
pub struct MarketRewardResponse {
    /// The market condition ID (unique market identifier).
    pub condition_id: B256,
    pub question: String,
    pub market_slug: String,
    pub event_slug: String,
    pub image: String,
    pub rewards_max_spread: Decimal,
    pub rewards_min_size: Decimal,
    pub market_competitiveness: Decimal,
    #[serde(default)]
    #[serde_as(deserialize_as = "DefaultOnNull")]
    pub tokens: Vec<Token>,
    #[serde(default)]
    #[serde_as(deserialize_as = "DefaultOnNull")]
    pub rewards_config: Vec<MarketRewardsConfig>,
}

#[non_exhaustive]
#[serde_as]
#[derive(Debug, Clone, Deserialize, Builder, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BuilderApiKeyResponse {
    pub key: ApiKey,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub revoked_at: Option<DateTime<Utc>>,
}

#[non_exhaustive]
#[serde_as]
#[derive(Debug, Clone, Deserialize, Builder, PartialEq)]
#[serde(rename_all = "camelCase")]
#[builder(on(String, into))]
pub struct BuilderTradeResponse {
    pub id: String,
    pub trade_type: String,
    /// Hash of the taker order.
    pub taker_order_hash: B256,
    /// Address of the builder.
    pub builder: Address,
    /// The market condition ID.
    pub market: B256,
    pub asset_id: U256,
    pub side: Side,
    pub size: Decimal,
    pub size_usdc: Decimal,
    pub price: Decimal,
    pub status: TradeStatusType,
    pub outcome: String,
    pub outcome_index: u32,
    pub owner: ApiKey,
    /// Address of the maker.
    pub maker: Address,
    /// On-chain transaction hash.
    pub transaction_hash: B256,
    #[serde_as(as = "TimestampSeconds<String>")]
    pub match_time: DateTime<Utc>,
    pub bucket_index: u32,
    pub fee: Decimal,
    pub fee_usdc: Decimal,
    #[serde(alias = "err_msg")]
    pub err_msg: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[non_exhaustive]
#[derive(Debug, Clone, Deserialize, Builder, PartialEq)]
#[builder(on(String, into))]
pub struct HeartbeatResponse {
    pub heartbeat_id: Uuid,
    pub error: Option<String>,
}

/// Generic wrapper structure that holds inner `data` with metadata designating how to query for the
/// next page.
#[non_exhaustive]
#[derive(Clone, Debug, Serialize, Deserialize, Builder, PartialEq)]
#[builder(on(String, into))]
pub struct Page<T> {
    pub data: Vec<T>,
    /// The continuation token to supply to the API to trigger for the next [`Page<T>`].
    pub next_cursor: String,
    /// The maximum length of `data`.
    pub limit: u64,
    /// The length of `data`
    pub count: u64,
}

#[non_exhaustive]
#[derive(Clone, Debug, Deserialize, Builder, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReadonlyApiKeyResponse {
    pub api_key: String,
}

/// Cached V2 fee parameters keyed by token, sourced from `/clob-markets/{id}`'s `fd` field.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FeeInfo {
    pub rate: Decimal,
    pub exponent: u32,
}

/// Platform fee parameters for a V2 market. Applied as `rate × (price × (1 − price))^exponent`.
#[non_exhaustive]
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct FeeDetails {
    #[serde(rename = "r", default)]
    pub rate: Decimal,
    #[serde(rename = "e", default)]
    pub exponent: u32,
    #[serde(rename = "to", default)]
    pub taker_only: bool,
}

#[non_exhaustive]
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct ClobToken {
    #[serde(rename = "t")]
    pub token_id: U256,
    #[serde(rename = "o")]
    pub outcome: String,
}

/// Response from `GET /clob-markets/{condition_id}`. Uses the server's short wire
/// keys (`c`, `t`, `mts`, …) renamed to ergonomic Rust names.
#[non_exhaustive]
#[serde_as]
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct ClobMarketInfoResponse {
    #[serde(rename = "c")]
    pub condition_id: B256,
    #[serde(rename = "t", default)]
    #[serde_as(deserialize_as = "DefaultOnNull")]
    pub tokens: Vec<Option<ClobToken>>,
    #[serde(rename = "mts")]
    #[serde_as(as = "TryFromInto<Decimal>")]
    pub min_tick_size: TickSize,
    #[serde(rename = "mos", default)]
    pub min_order_size: Decimal,
    #[serde(rename = "nr", default)]
    pub neg_risk: bool,
    #[serde(rename = "fd", default)]
    pub fee_details: Option<FeeDetails>,
    /// Legacy V1 maker base fee. Unused in V2 settlement.
    #[serde(rename = "mbf", default)]
    pub maker_base_fee: Option<Decimal>,
    /// Legacy V1 taker base fee. Unused in V2 settlement.
    #[serde(rename = "tbf", default)]
    pub taker_base_fee: Option<Decimal>,
    #[serde(rename = "rfqe", default)]
    pub rfq_enabled: bool,
}

#[non_exhaustive]
#[derive(Clone, Debug, Deserialize, Builder, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BuilderFeeRateResponse {
    #[serde(alias = "builder_maker_fee_rate_bps")]
    pub builder_maker_fee_rate_bps: u32,
    #[serde(alias = "builder_taker_fee_rate_bps")]
    pub builder_taker_fee_rate_bps: u32,
}

/// Response from creating an RFQ request.
#[cfg(feature = "rfq")]
#[non_exhaustive]
#[derive(Debug, Clone, Deserialize, Builder, PartialEq)]
#[serde(rename_all = "camelCase")]
#[builder(on(String, into))]
pub struct CreateRfqRequestResponse {
    /// Unique identifier for the created request.
    pub request_id: String,
    /// Unix timestamp when the request expires.
    pub expiry: i64,
}

/// Response from creating an RFQ quote.
#[cfg(feature = "rfq")]
#[non_exhaustive]
#[derive(Debug, Clone, Deserialize, Builder, PartialEq)]
#[serde(rename_all = "camelCase")]
#[builder(on(String, into))]
pub struct CreateRfqQuoteResponse {
    /// Unique identifier for the created quote.
    pub quote_id: String,
}

/// Response from accepting an RFQ quote.
///
/// Returns "OK" as text, represented as unit type for deserialization.
#[cfg(feature = "rfq")]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptRfqQuoteResponse;

/// Response from approving an RFQ order.
#[cfg(feature = "rfq")]
#[non_exhaustive]
#[derive(Debug, Clone, Deserialize, Builder, PartialEq)]
#[serde(rename_all = "camelCase")]
#[builder(on(String, into))]
pub struct ApproveRfqOrderResponse {
    /// Trade IDs for the executed order.
    pub trade_ids: Vec<String>,
}

/// An RFQ request in the system.
#[cfg(feature = "rfq")]
#[non_exhaustive]
#[derive(Debug, Clone, Deserialize, Builder, PartialEq)]
#[serde(rename_all = "camelCase")]
#[builder(on(String, into))]
pub struct RfqRequest {
    /// Unique request identifier.
    pub request_id: String,
    /// User's address.
    pub user_address: Address,
    /// Proxy address (may be same as user).
    pub proxy_address: Address,
    /// Market condition ID.
    pub condition: B256,
    /// Token ID for the outcome token.
    pub token: U256,
    /// Complement token ID.
    pub complement: U256,
    /// Order side (BUY or SELL).
    pub side: Side,
    /// Size of tokens to receive.
    pub size_in: Decimal,
    /// Size of tokens to give.
    pub size_out: Decimal,
    /// Price for the request.
    pub price: Decimal,
    /// Unix timestamp when the request expires.
    pub expiry: i64,
}

/// An RFQ quote in the system.
#[cfg(feature = "rfq")]
#[non_exhaustive]
#[derive(Debug, Clone, Deserialize, Builder, PartialEq)]
#[serde(rename_all = "camelCase")]
#[builder(on(String, into))]
pub struct RfqQuote {
    /// Unique quote identifier.
    pub quote_id: String,
    /// Request ID this quote is for.
    pub request_id: String,
    /// Quoter's address.
    pub user_address: Address,
    /// Proxy address (may be same as user).
    pub proxy_address: Address,
    /// Market condition ID.
    pub condition: B256,
    /// Token ID for the outcome token.
    pub token: U256,
    /// Complement token ID.
    pub complement: U256,
    /// Order side (BUY or SELL).
    pub side: Side,
    /// Size of tokens to receive.
    pub size_in: Decimal,
    /// Size of tokens to give.
    pub size_out: Decimal,
    /// Quoted price.
    pub price: Decimal,
}

#[cfg(test)]
mod tests {
    use rust_decimal_macros::dec;

    use super::*;

    fn maker_order_json(fee_rate_bps: Option<&str>) -> serde_json::Value {
        let mut maker = serde_json::json!({
            "order_id": "maker-1",
            "owner": "ffffffff-ffff-ffff-ffff-ffffffffffff",
            "maker_address": "0x1111111111111111111111111111111111111111",
            "matched_amount": "4",
            "price": "0.4",
            "asset_id": "2",
            "outcome": "NO",
            "side": "SELL"
        });
        if let Some(value) = fee_rate_bps {
            maker
                .as_object_mut()
                .expect("maker fixture must be an object")
                .insert("fee_rate_bps".to_owned(), serde_json::json!(value));
        }
        maker
    }

    /// Reproduces the exact production shape from 2026-08-03 (axiom-node
    /// v678): a `Page<TradeResponse>` whose `data[0].maker_orders[0]` carries
    /// `"fee_rate_bps": ""`. Before this fix, deserializing the bare-`Decimal`
    /// field failed the whole page and closed Live admission.
    #[test]
    fn page_with_blank_maker_order_fee_rate_bps_now_decodes() {
        let json = serde_json::json!({
            "data": [{
                "id": "trade-1",
                "taker_order_id": "taker-1",
                "market": format!("0x{}", "3".repeat(64)),
                "asset_id": "1",
                "side": "BUY",
                "size": "4",
                "fee_rate_bps": "30",
                "price": "0.6",
                "status": "CONFIRMED",
                "match_time": "1700000000",
                "last_update": "1700000010",
                "outcome": "YES",
                "bucket_index": 0,
                "owner": "ffffffff-ffff-ffff-ffff-ffffffffffff",
                "maker_address": "0x0000000000000000000000000000000000000000",
                "maker_orders": [maker_order_json(Some(""))],
                "transaction_hash": "",
                "trader_side": "TAKER"
            }],
            "next_cursor": "LTE=",
            "limit": 100,
            "count": 1
        });

        let page: Page<TradeResponse> =
            serde_json::from_value(json).expect("blank maker-order fee_rate_bps must decode");
        assert_eq!(page.data[0].maker_orders[0].fee_rate_bps, None);
        // The trade-level field is unaffected by the maker leg's blank value.
        assert_eq!(page.data[0].fee_rate_bps, Some(dec!(30)));
    }

    #[test]
    fn maker_order_absent_fee_rate_bps_key_is_none() {
        let json = maker_order_json(None);
        let maker: MakerOrder = serde_json::from_value(json).expect("missing key must decode");
        assert_eq!(maker.fee_rate_bps, None);
    }

    #[test]
    fn maker_order_blank_fee_rate_bps_is_none_not_zero() {
        let json = maker_order_json(Some(""));
        let maker: MakerOrder = serde_json::from_value(json).expect("blank fee must decode");
        assert_eq!(maker.fee_rate_bps, None);
        assert_ne!(maker.fee_rate_bps, Some(Decimal::ZERO));
    }

    #[test]
    fn maker_order_valid_fee_rate_bps_round_trips_exactly() {
        let json = maker_order_json(Some("999"));
        let maker: MakerOrder = serde_json::from_value(json).expect("valid fee must decode");
        assert_eq!(maker.fee_rate_bps, Some(dec!(999)));
    }

    #[test]
    fn maker_order_quoted_zero_is_present_not_absent() {
        let json = maker_order_json(Some("0"));
        let maker: MakerOrder = serde_json::from_value(json).expect("quoted zero must decode");
        assert_eq!(maker.fee_rate_bps, Some(Decimal::ZERO));
    }

    #[test]
    fn maker_order_malformed_nonempty_fee_rate_bps_still_errors() {
        let json = maker_order_json(Some("abc"));
        let error = serde_json::from_value::<MakerOrder>(json)
            .expect_err("non-numeric, non-empty text must still reject");
        assert!(
            error.to_string().contains("Invalid decimal"),
            "unexpected error message: {error}"
        );
    }

    #[test]
    fn trade_response_blank_fee_rate_bps_is_none_not_zero() {
        let mut json = serde_json::json!({
            "id": "trade-2",
            "taker_order_id": "taker-2",
            "market": format!("0x{}", "4".repeat(64)),
            "asset_id": "1",
            "side": "BUY",
            "size": "4",
            "fee_rate_bps": "",
            "price": "0.6",
            "status": "CONFIRMED",
            "match_time": "1700000000",
            "last_update": "1700000010",
            "outcome": "YES",
            "bucket_index": 0,
            "owner": "ffffffff-ffff-ffff-ffff-ffffffffffff",
            "maker_address": "0x0000000000000000000000000000000000000000",
            "transaction_hash": "",
            "trader_side": "TAKER"
        });
        json.as_object_mut()
            .expect("trade fixture must be an object")
            .insert("maker_orders".to_owned(), serde_json::json!([]));

        let trade: TradeResponse =
            serde_json::from_value(json).expect("blank trade-level fee must decode");
        assert_eq!(trade.fee_rate_bps, None);
    }

    #[test]
    fn trade_response_malformed_nonempty_fee_rate_bps_still_errors() {
        let json = serde_json::json!({
            "id": "trade-3",
            "taker_order_id": "taker-3",
            "market": format!("0x{}", "5".repeat(64)),
            "asset_id": "1",
            "side": "BUY",
            "size": "4",
            "fee_rate_bps": "not-a-decimal",
            "price": "0.6",
            "status": "CONFIRMED",
            "match_time": "1700000000",
            "last_update": "1700000010",
            "outcome": "YES",
            "bucket_index": 0,
            "owner": "ffffffff-ffff-ffff-ffff-ffffffffffff",
            "maker_address": "0x0000000000000000000000000000000000000000",
            "maker_orders": [],
            "transaction_hash": "",
            "trader_side": "TAKER"
        });

        let error = serde_json::from_value::<TradeResponse>(json)
            .expect_err("malformed non-empty trade-level fee must still reject the record");
        assert!(
            error.to_string().contains("Invalid decimal"),
            "unexpected error message: {error}"
        );
    }
}

/// Contract inventory + regression detector for every `Decimal`-typed field
/// declared across the three venue-echoed response modules: `clob::types::response`
/// (this file), `clob::ws::types::response`, and `data::types::response`.
///
/// # Why a type-level table, not 122 struct-fixture round-trips
///
/// Rust has no runtime struct-field reflection, so "table-driven over every
/// `Decimal` field" cannot mean introspecting a struct's fields at runtime.
/// One hand-built valid JSON fixture per struct (54 of them), blanking one
/// field at a time, was considered and rejected: it needs every fixture to
/// already be exactly right (a fixture that fails to parse even *unmodified*
/// makes its row's assertion vacuously pass), it must track every
/// `#[serde(rename)]`/`alias` by hand, and it multiplies maintenance by 54
/// struct shapes for a fact that does not vary per struct.
///
/// The fact that actually varies is per-*field*, not per-struct: does this
/// field have a `deserialize_with`/`serde_as` wrapper that tolerates a blank
/// string, or not? Serde deserializes each struct field independently
/// through its own declared conversion — a struct's other fields being valid
/// or invalid has no bearing on how *this* field's own value is interpreted.
/// So the field's declared type (`Decimal` / `Option<Decimal>`) plus its
/// wrapper, fed the same synthetic `""` the wire would send, reproduces
/// exactly what happens when the containing struct sees that field blank.
/// [`FIELDS`] below is generated by walking every `pub <field>:
/// (Decimal|Option<Decimal>)` declaration in the three files and its
/// immediately-preceding attribute block, classifying by wrapper.
///
/// # Inventory (2026-08-05 walk — verify against this count before trusting it)
///
/// | module | fields | of which already resolve blank to `None` |
/// |---|---|---|
/// | `clob::types::response` (this file) | 71 | 3 (`TradeResponse`/`MakerOrder` `fee_rate_bps` via [`empty_string_as_none`]; `OrderBookSummaryResponse::last_trade_price` via the looser pre-existing `DefaultOnError`) |
/// | `clob::ws::types::response` | 23 | 2 (`LastTradePrice`/`TradeMessage` `fee_rate_bps` via [`empty_string_as_none`]) |
/// | `data::types::response` | 28 | 0 |
/// | **total** | **122** | **5** |
///
/// The other 117 fields reject a blank string outright (a clean `Err`, never
/// a panic, never a silently-wrong `Decimal::ZERO`) — that is `Decimal`'s own
/// stock `Deserialize` behavior (`rust_decimal`, `serde` feature,
/// `deserialize_any` -> `visit_str` ->
/// `Decimal::from_str("").or_else(from_scientific)` -> `Err`), inherited by
/// any `Option<Decimal>` field that has not been specifically retrofitted,
/// because serde's derived `Option<T>` forwards a *present* non-null value
/// straight to `T::deserialize` (only a *missing* key falls back to the
/// field's default). A future blank sighting on one of those 117 looks its
/// owner up in [`FIELDS`] and gets a one-line fix site — wire
/// `empty_string_as_none` the same way the four `fee_rate_bps` sites do —
/// instead of a fresh investigation across three files.
///
/// `OrderBookSummaryResponse::last_trade_price` is flagged
/// `BlankOutcome::DefaultOnErrorSwallowsToNone` rather than
/// `BlankOutcome::AbsentAsNone` on purpose: its `#[serde_as(deserialize_as =
/// "DefaultOnError")]` swallows *any* deserialize failure (a genuinely
/// malformed value too, not just blank) into `None`. That is strictly looser
/// than the sanctioned `empty_string_as_none` contract and predates this
/// step — flagged here as a pre-existing gap, not changed (this step's
/// NON-GOALS: only what counts as *absent* may widen, never what counts as
/// *valid*).
///
/// # Machine-checked vs. hand-walked columns (#2562 REDO 1 — 2026-08-05)
///
/// Two facts about the table above are tied to this file's own source text
/// by a runtime scan inside [`decimal_blank_string_contract`], re-checked on
/// every test run rather than trusted as a one-time hand count: the number
/// of sites wired to the shared tolerant deserializer (the row count behind
/// the "already resolve blank to `None` via `empty_string_as_none`"
/// fraction), and the per-module Decimal/`Option<Decimal>` field totals
/// (71/23/28, 122 overall). A source scan can prove *how many* declaration
/// sites exist; it cannot prove *which* [`BlankOutcome`] a given row
/// deserves — classifying each row as `Rejects`, `AbsentAsNone`, or
/// `DefaultOnErrorSwallowsToNone` is still a hand walk, exactly like the
/// row's `module`/`owner`/`field` columns. Read the per-row classification
/// in [`FIELDS`] as reviewed-by-eye evidence; only the two counts named
/// above carry an independent, re-runnable proof.
///
/// # Production-reachability trace (#2562 action 5 — traced 2026-08-05)
///
/// Does production's periodic authenticated-recovery path (named directly in
/// #2562's log line, `axiom_node::services::fills`) actually run through the
/// vendored `Page<TradeResponse>`/`MakerOrder` this file hardens, or through
/// `clients/rspm`'s own `WireTrade`/`WireMakerOrder`? Traced the call graph,
/// not assumed:
///
/// - `polymarket_client_sdk_v2::clob::Client::trades()` — the only method
///   that constructs a `Page<TradeResponse>` — has no call site anywhere in
///   the workspace outside this SDK's own tests/examples/benches, plus one
///   *private* `wait_for_resolved_trades` helper reachable only via the
///   public `post_order`. `clients/rspm`'s order-submission path
///   deliberately calls `post_order_initial` instead (its own doc comment:
///   "without performing any follow-up private reads... Settlement hashes
///   can be enriched later"), so that private path never fires in
///   production either.
/// - `bin/node`'s periodic authenticated recovery
///   (`crates/engine::exec::polymarket::recover_next_authenticated_fill_page`)
///   calls `self.clob.authenticated_trades(...)` where `self.clob:
///   Arc<rspm::ClobClient>` — `clients/rspm/src/clob/client.rs`'s
///   `authenticated_trades`, whose own doc comment states "rspm owns this
///   recovery request... The SDK remains responsible for order construction
///   and signing." It fetches raw HTTP itself and decodes via
///   `AuthenticatedTradePage::decode_json`
///   (`clients/rspm/src/clob/authenticated_trades.rs`'s own `WireTrade`/
///   `WireMakerOrder`, with dedicated `quoted_decimal`/
///   `optional_quoted_decimal` deserializers) — never touching this file's
///   `TradeResponse`/`MakerOrder`/`Page` at all.
/// - That rspm path already has its own comprehensive, independently-landed
///   blank-tolerance contract suite,
///   `clients/rspm/tests/empty_fee_rate_response.rs` (predates this step).
///
/// **Conclusion:** for the *current* worktree, this file's `TradeResponse`/
/// `MakerOrder` fix is unreachable from the REST fill-recovery path #2562's
/// log line names — that incident's "Live admission remains closed" symptom
/// is closed today by rspm's own Wire-type contract, not by this patch.
/// #2562's own second comment reaches the same reading independently ("the
/// coder flagged that production's actual authenticated-recovery path may
/// run through `clients/rspm`'s own... deserializers... The deployed binary
/// is older than the repo"), which corroborates rather than contradicts this
/// trace: `axiom-node` v678 (the build that logged the incident) most likely
/// predates the introduction of `rspm::ClobClient::authenticated_trades`'s
/// dedicated Wire-type decode path, and a redeploy is what actually confirms
/// resolution rather than reasoning about source alone.
///
/// This file's fix is not dead weight, though — it protects two paths that
/// ARE reachable today: (1) the SDK's own direct-consumer test/example/bench
/// surface, and (2) `clob::ws::types::response::LastTradePrice.fee_rate_bps`,
/// consumed by `clients/rspm/src/types/clob_trade.rs`'s `#[cfg(feature =
/// "watch")]` (not test-gated) `From<LastTradePrice> for ClobTrade`, which
/// feeds the public-market `last_trade_price` WS channel into QuestDB — a
/// blank `fee_rate_bps` there would have rejected the *entire* WS event
/// before this fix, even though `ClobTrade` never reads that field's value.
/// `clob::ws::types::response::TradeMessage`/`OrderMessage` remain reachable
/// only from `#[cfg(test)]`-gated conversion code in `crates/engine` today —
/// this fix protects that test's integrity and any future un-gating, but has
/// no live production exposure yet.
#[cfg(test)]
mod decimal_blank_string_contract {
    use serde::de::value::{Error as DeError, StrDeserializer};
    use serde_with::DeserializeAs;

    use super::*;

    /// How a *present*, blank (`""`) wire value resolves for one field today.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum BlankOutcome {
        /// Neither the field nor `Decimal` itself has any blank tolerance:
        /// `""` is a clean `Err`, never a panic, never `Ok(Decimal::ZERO)`.
        Rejects,
        /// Deserialized through the sanctioned [`empty_string_as_none`]:
        /// blank parses cleanly to `None`; a genuinely malformed non-blank
        /// value still `Err`s.
        AbsentAsNone,
        /// Tolerates blank only as a side effect of a pre-existing, looser
        /// `#[serde_as(deserialize_as = "DefaultOnError")]`, which swallows
        /// ANY deserialize failure (not just blank) into `None`. See the
        /// module doc comment.
        DefaultOnErrorSwallowsToNone,
    }

    /// One census row: a single `Decimal`/`Option<Decimal>` field declaration.
    struct Field {
        /// Rust module path, e.g. `"clob::types::response"`.
        module: &'static str,
        /// Owning struct name.
        owner: &'static str,
        /// Field name.
        field: &'static str,
        /// Declared field type: `"Decimal"` or `"Option<Decimal>"`.
        ty: &'static str,
        outcome: BlankOutcome,
    }

    /// The full census. One row per `pub <field>: (Decimal|Option<Decimal>)`
    /// declaration across the three venue-echoed response modules, in file
    /// order. See the module doc comment for the generation method and the
    /// per-module breakdown.
    const FIELDS: &[Field] = &[
        Field { module: "clob::types::response", owner: "MidpointResponse", field: "mid", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "PriceResponse", field: "price", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "SpreadResponse", field: "spread", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "PricePoint", field: "p", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "OrderBookSummaryResponse", field: "min_order_size", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "OrderBookSummaryResponse", field: "last_trade_price", ty: "Option<Decimal>", outcome: BlankOutcome::DefaultOnErrorSwallowsToNone },
        Field { module: "clob::types::response", owner: "OrderSummary", field: "price", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "OrderSummary", field: "size", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "LastTradePriceResponse", field: "price", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "LastTradesPricesResponse", field: "price", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "MarketResponse", field: "minimum_order_size", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "MarketResponse", field: "minimum_tick_size", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "MarketResponse", field: "maker_base_fee", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "MarketResponse", field: "taker_base_fee", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "Token", field: "price", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "PostOrderResponse", field: "making_amount", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "PostOrderResponse", field: "taking_amount", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "OpenOrderResponse", field: "original_size", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "OpenOrderResponse", field: "size_matched", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "OpenOrderResponse", field: "price", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "TradeResponse", field: "size", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "TradeResponse", field: "fee_rate_bps", ty: "Option<Decimal>", outcome: BlankOutcome::AbsentAsNone },
        Field { module: "clob::types::response", owner: "TradeResponse", field: "price", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "NotificationPayload", field: "matched_size", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "NotificationPayload", field: "original_size", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "NotificationPayload", field: "price", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "NotificationPayload", field: "remaining_size", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "BalanceAllowanceResponse", field: "balance", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "PriceSideResponse", field: "price", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "RewardRate", field: "rewards_daily_rate", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "Rewards", field: "min_size", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "Rewards", field: "max_spread", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "MakerOrder", field: "matched_amount", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "MakerOrder", field: "price", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "MakerOrder", field: "fee_rate_bps", ty: "Option<Decimal>", outcome: BlankOutcome::AbsentAsNone },
        Field { module: "clob::types::response", owner: "UserEarningResponse", field: "earnings", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "UserEarningResponse", field: "asset_rate", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "TotalUserEarningResponse", field: "earnings", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "TotalUserEarningResponse", field: "asset_rate", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "UserRewardsEarningResponse", field: "rewards_max_spread", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "UserRewardsEarningResponse", field: "rewards_min_size", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "UserRewardsEarningResponse", field: "market_competitiveness", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "UserRewardsEarningResponse", field: "earning_percentage", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "RewardsConfig", field: "rate_per_day", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "RewardsConfig", field: "total_rewards", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "MarketRewardsConfig", field: "rate_per_day", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "MarketRewardsConfig", field: "total_rewards", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "MarketRewardsConfig", field: "total_days", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "Earning", field: "earnings", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "Earning", field: "asset_rate", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "CurrentRewardResponse", field: "rewards_max_spread", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "CurrentRewardResponse", field: "rewards_min_size", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "MarketRewardResponse", field: "rewards_max_spread", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "MarketRewardResponse", field: "rewards_min_size", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "MarketRewardResponse", field: "market_competitiveness", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "BuilderTradeResponse", field: "size", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "BuilderTradeResponse", field: "size_usdc", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "BuilderTradeResponse", field: "price", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "BuilderTradeResponse", field: "fee", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "BuilderTradeResponse", field: "fee_usdc", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "FeeInfo", field: "rate", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "FeeDetails", field: "rate", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "ClobMarketInfoResponse", field: "min_order_size", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "ClobMarketInfoResponse", field: "maker_base_fee", ty: "Option<Decimal>", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "ClobMarketInfoResponse", field: "taker_base_fee", ty: "Option<Decimal>", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "RfqRequest", field: "size_in", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "RfqRequest", field: "size_out", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "RfqRequest", field: "price", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "RfqQuote", field: "size_in", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "RfqQuote", field: "size_out", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::types::response", owner: "RfqQuote", field: "price", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::ws::types::response", owner: "OrderBookLevel", field: "price", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::ws::types::response", owner: "OrderBookLevel", field: "size", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::ws::types::response", owner: "PriceChangeBatchEntry", field: "price", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::ws::types::response", owner: "PriceChangeBatchEntry", field: "size", ty: "Option<Decimal>", outcome: BlankOutcome::Rejects },
        Field { module: "clob::ws::types::response", owner: "PriceChangeBatchEntry", field: "best_bid", ty: "Option<Decimal>", outcome: BlankOutcome::Rejects },
        Field { module: "clob::ws::types::response", owner: "PriceChangeBatchEntry", field: "best_ask", ty: "Option<Decimal>", outcome: BlankOutcome::Rejects },
        Field { module: "clob::ws::types::response", owner: "TickSizeChange", field: "old_tick_size", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::ws::types::response", owner: "TickSizeChange", field: "new_tick_size", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::ws::types::response", owner: "LastTradePrice", field: "price", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::ws::types::response", owner: "LastTradePrice", field: "size", ty: "Option<Decimal>", outcome: BlankOutcome::Rejects },
        Field { module: "clob::ws::types::response", owner: "LastTradePrice", field: "fee_rate_bps", ty: "Option<Decimal>", outcome: BlankOutcome::AbsentAsNone },
        Field { module: "clob::ws::types::response", owner: "BestBidAsk", field: "best_bid", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::ws::types::response", owner: "BestBidAsk", field: "best_ask", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::ws::types::response", owner: "BestBidAsk", field: "spread", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::ws::types::response", owner: "MakerOrder", field: "matched_amount", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::ws::types::response", owner: "MakerOrder", field: "price", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::ws::types::response", owner: "TradeMessage", field: "size", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::ws::types::response", owner: "TradeMessage", field: "price", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::ws::types::response", owner: "TradeMessage", field: "fee_rate_bps", ty: "Option<Decimal>", outcome: BlankOutcome::AbsentAsNone },
        Field { module: "clob::ws::types::response", owner: "OrderMessage", field: "price", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "clob::ws::types::response", owner: "OrderMessage", field: "original_size", ty: "Option<Decimal>", outcome: BlankOutcome::Rejects },
        Field { module: "clob::ws::types::response", owner: "OrderMessage", field: "size_matched", ty: "Option<Decimal>", outcome: BlankOutcome::Rejects },
        Field { module: "clob::ws::types::response", owner: "MidpointUpdate", field: "midpoint", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "data::types::response", owner: "Position", field: "size", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "data::types::response", owner: "Position", field: "avg_price", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "data::types::response", owner: "Position", field: "initial_value", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "data::types::response", owner: "Position", field: "current_value", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "data::types::response", owner: "Position", field: "cash_pnl", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "data::types::response", owner: "Position", field: "percent_pnl", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "data::types::response", owner: "Position", field: "total_bought", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "data::types::response", owner: "Position", field: "realized_pnl", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "data::types::response", owner: "Position", field: "percent_realized_pnl", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "data::types::response", owner: "Position", field: "cur_price", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "data::types::response", owner: "ClosedPosition", field: "avg_price", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "data::types::response", owner: "ClosedPosition", field: "total_bought", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "data::types::response", owner: "ClosedPosition", field: "realized_pnl", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "data::types::response", owner: "ClosedPosition", field: "cur_price", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "data::types::response", owner: "Trade", field: "size", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "data::types::response", owner: "Trade", field: "price", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "data::types::response", owner: "Activity", field: "size", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "data::types::response", owner: "Activity", field: "usdc_size", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "data::types::response", owner: "Activity", field: "price", ty: "Option<Decimal>", outcome: BlankOutcome::Rejects },
        Field { module: "data::types::response", owner: "Holder", field: "amount", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "data::types::response", owner: "Value", field: "value", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "data::types::response", owner: "OpenInterest", field: "value", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "data::types::response", owner: "MarketVolume", field: "value", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "data::types::response", owner: "LiveVolume", field: "total", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "data::types::response", owner: "BuilderLeaderboardEntry", field: "volume", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "data::types::response", owner: "BuilderVolumeEntry", field: "volume", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "data::types::response", owner: "TraderLeaderboardEntry", field: "vol", ty: "Decimal", outcome: BlankOutcome::Rejects },
        Field { module: "data::types::response", owner: "TraderLeaderboardEntry", field: "pnl", ty: "Decimal", outcome: BlankOutcome::Rejects },
    ];

    #[test]
    fn census_matches_the_walked_inventory() {
        assert_eq!(
            FIELDS.len(),
            122,
            "the three response modules' Decimal-typed field count moved; re-walk \
             `rg -n \"pub [a-z_0-9]+: (Option<Decimal>|Decimal),\"` over \
             clob/types/response.rs, clob/ws/types/response.rs, and \
             data/types/response.rs, and update this census before trusting it"
        );
        assert!(
            FIELDS.len() >= 88,
            "acceptance floor for #2562's swept-class requirement"
        );

        let per_module = |m: &str| FIELDS.iter().filter(|f| f.module == m).count();
        assert_eq!(per_module("clob::types::response"), 71);
        assert_eq!(per_module("clob::ws::types::response"), 23);
        assert_eq!(per_module("data::types::response"), 28);

        let per_outcome = |o: BlankOutcome| FIELDS.iter().filter(|f| f.outcome == o).count();
        assert_eq!(per_outcome(BlankOutcome::Rejects), 117);
        assert_eq!(per_outcome(BlankOutcome::AbsentAsNone), 4);
        assert_eq!(per_outcome(BlankOutcome::DefaultOnErrorSwallowsToNone), 1);
    }

    /// Source-derives the per-module Decimal/`Option<Decimal>` field totals
    /// that back [`census_matches_the_walked_inventory`]'s 71/23/28 pins,
    /// instead of resting on that hand walk alone. Two needles, each split
    /// via `concat!` so this file's own source text (embedded below through
    /// `include_str!("response.rs")`) never matches its own needle
    /// definition — see the module doc comment's "Machine-checked vs.
    /// hand-walked columns" section. Confirmed independently (not just
    /// asserted): the `Option<Decimal>`-shaped needle does not contain the
    /// bare-`Decimal`-shaped needle as a substring, because the former's
    /// colon-space run is immediately followed by `Option<`, never by the
    /// bare type name — so a given field declaration can only ever match one
    /// of the two counters, never both.
    ///
    /// HONESTY VALVE: a derived total that disagrees with its hand-walked
    /// pin is a reportable finding. Neither number gets tuned to match the
    /// other — see #2562 REDO 1.
    #[test]
    fn per_module_decimal_field_totals_are_source_derived_or_flagged_hand_walked() {
        const NEEDLE_DECIMAL: &str = concat!(": ", "Decimal,");
        const NEEDLE_OPTION_DECIMAL: &str = concat!(": Option<Decimal", ">,");

        let count = |haystack: &str| {
            haystack.matches(NEEDLE_DECIMAL).count()
                + haystack.matches(NEEDLE_OPTION_DECIMAL).count()
        };

        let this_file = include_str!("response.rs");
        let ws_file = include_str!("../ws/types/response.rs");
        let data_file = include_str!("../../data/types/response.rs");

        assert_eq!(
            count(this_file),
            71,
            "clob::types::response: source-derived field total drifted from the \
             hand-walked pin of 71 — do not tune either number, report the discrepancy"
        );
        assert_eq!(
            count(ws_file),
            23,
            "clob::ws::types::response: source-derived field total drifted from the \
             hand-walked pin of 23 — do not tune either number, report the discrepancy"
        );
        assert_eq!(
            count(data_file),
            28,
            "data::types::response: source-derived field total drifted from the \
             hand-walked pin of 28 — do not tune either number, report the discrepancy"
        );
    }

    /// The bare-type half of the property the old, overclaiming
    /// `fields_without_a_tolerant_wrapper_reject_blank_never_coerce_to_zero`
    /// test used to assert 117 times over: what actually varies per census
    /// row is which *type* it holds, not which struct it belongs to, and
    /// there are only two types in play. No loop over [`FIELDS`] — that
    /// would just re-run the same two assertions under different failure
    /// messages, which is the exact shape #2562 REDO 1 flagged as a false
    /// green.
    #[test]
    fn bare_decimal_and_option_decimal_reject_blank_string() {
        assert!(
            serde_json::from_str::<Decimal>("\"\"").is_err(),
            "Decimal must reject a blank string as a clean Err, never silently coerce it to zero"
        );
        assert!(
            serde_json::from_str::<Option<Decimal>>("\"\"").is_err(),
            "Option<Decimal> must reject a blank string as a clean Err absent a tolerant wrapper"
        );
    }

    /// The census half of the same property: every [`FIELDS`] row flagged
    /// `Rejects` actually carries one of the two types
    /// [`bare_decimal_and_option_decimal_reject_blank_string`] proves the
    /// property for. This one legitimately loops — it is checking the
    /// census's own `ty` column, not re-deriving the type-level fact.
    /// Previously an `unreachable!` arm; an unreached branch proves nothing,
    /// so this is now an explicit, failing assertion.
    #[test]
    fn every_rejects_row_ty_is_covered_by_the_bare_type_proof() {
        for f in FIELDS.iter().filter(|f| f.outcome == BlankOutcome::Rejects) {
            assert!(
                f.ty == "Decimal" || f.ty == "Option<Decimal>",
                "{}::{}.{} has census ty `{}`, which \
                 bare_decimal_and_option_decimal_reject_blank_string does not cover — add a \
                 case there or fix the census",
                f.module, f.owner, f.field, f.ty
            );
        }
    }

    /// Source-derives the `AbsentAsNone` row count from the shared tolerant
    /// deserializer's actual wiring sites, then checks the one fact a source
    /// scan cannot prove by itself: that every row so flagged really is
    /// `Option<Decimal>` (the only type the deserializer is wired onto).
    /// Replaces the old `fields_wired_to_empty_string_as_none_treat_blank_as_absent_evidence`,
    /// which called the shared deserializer once per row and thereby
    /// duplicated [`fee_rate_bps_shared_deserializer_covers_all_four_behaviors`]
    /// without ever consulting the row's owning field.
    #[test]
    fn absent_as_none_rows_are_source_derived_and_all_option_decimal() {
        const NEEDLE: &str = concat!("deserialize_with = \"", "empty_string_as_none\"");

        let this_file = include_str!("response.rs");
        let ws_file = include_str!("../ws/types/response.rs");
        let data_file = include_str!("../../data/types/response.rs");

        let source_derived_total = this_file.matches(NEEDLE).count()
            + ws_file.matches(NEEDLE).count()
            + data_file.matches(NEEDLE).count();

        let census_rows: Vec<&Field> = FIELDS
            .iter()
            .filter(|f| f.outcome == BlankOutcome::AbsentAsNone)
            .collect();

        assert_eq!(
            source_derived_total,
            census_rows.len(),
            "source has {source_derived_total} sites wired to the shared tolerant \
             deserializer across the three census modules but FIELDS carries {} \
             AbsentAsNone rows — report the discrepancy, do not tune either number",
            census_rows.len()
        );

        for f in census_rows {
            assert_eq!(
                f.ty, "Option<Decimal>",
                "{}::{}.{} is flagged AbsentAsNone but its census ty is `{}`, not \
                 Option<Decimal>",
                f.module, f.owner, f.field, f.ty
            );
        }
    }

    #[test]
    fn the_one_default_on_error_field_also_swallows_blank_to_none() {
        let rows: Vec<&Field> = FIELDS
            .iter()
            .filter(|f| f.outcome == BlankOutcome::DefaultOnErrorSwallowsToNone)
            .collect();
        assert_eq!(
            rows.len(),
            1,
            "exactly one field carries this pre-existing looser tolerance; census drifted"
        );
        for f in rows {
            assert_eq!(f.ty, "Option<Decimal>");
            let result: Option<Decimal> =
                <DefaultOnError as DeserializeAs<'_, Option<Decimal>>>::deserialize_as(
                    StrDeserializer::<DeError>::new(""),
                )
                .expect("DefaultOnError never propagates a deserialize failure as Err");
            assert_eq!(
                result, None,
                "{}::{}.{} must still resolve blank to None via its pre-existing \
                 DefaultOnError wrapper",
                f.module, f.owner, f.field
            );
        }
    }

    /// The four sanctioned `fee_rate_bps` behaviors (#2562's explicit
    /// requirement), asserted once against the shared [`empty_string_as_none`]
    /// function all four `fee_rate_bps` sites are wired to. A fifth
    /// reimplementation per site would be a `DO-NOT-DUPLICATE` violation;
    /// per-site round-trips already exist at `clob/types/response.rs::tests`
    /// (`page_with_blank_maker_order_fee_rate_bps_now_decodes` and
    /// neighbors) and `clob/ws/types/response.rs::tests`
    /// (`parse_last_trade_price_with_blank_fee_rate_bps` and neighbors).
    #[test]
    fn fee_rate_bps_shared_deserializer_covers_all_four_behaviors() {
        // "" -> None (absent evidence, never a forged zero)
        assert_eq!(
            empty_string_as_none(StrDeserializer::<DeError>::new("")).unwrap(),
            None
        );
        // "abc" -> Err (a genuinely malformed value never widens to valid)
        assert!(empty_string_as_none(StrDeserializer::<DeError>::new("abc")).is_err());
        // "0" -> Some(ZERO) (a present zero is exact venue-reported evidence)
        assert_eq!(
            empty_string_as_none(StrDeserializer::<DeError>::new("0")).unwrap(),
            Some(Decimal::ZERO)
        );
        // a normal value round-trips exactly
        assert_eq!(
            empty_string_as_none(StrDeserializer::<DeError>::new("30")).unwrap(),
            Some(Decimal::from(30))
        );
    }
}
