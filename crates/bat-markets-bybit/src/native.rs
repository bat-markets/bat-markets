use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Shared public envelope for Bybit websocket topics.
#[derive(Clone, Debug, Deserialize)]
pub struct PublicEnvelope {
    pub topic: String,
    #[serde(rename = "type", default)]
    pub message_type: Option<String>,
    #[serde(default)]
    pub ts: i64,
    pub data: Value,
}

/// Shared private envelope for Bybit websocket topics.
#[derive(Clone, Debug, Deserialize)]
pub struct PrivateEnvelope {
    pub topic: String,
    #[serde(rename = "creationTime", default)]
    pub creation_time: i64,
    pub data: Value,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TickerData {
    #[serde(rename = "symbol")]
    pub symbol: String,
    #[serde(rename = "lastPrice")]
    pub last_price: String,
    #[serde(rename = "markPrice")]
    pub mark_price: String,
    #[serde(rename = "indexPrice")]
    pub index_price: String,
    #[serde(rename = "openInterest")]
    pub open_interest: String,
    #[serde(rename = "fundingRate")]
    pub funding_rate: String,
    #[serde(rename = "volume24h")]
    pub volume_24h: String,
    #[serde(rename = "turnover24h")]
    pub turnover_24h: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PublicTradeData {
    #[serde(rename = "T")]
    pub trade_time: i64,
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "S")]
    pub side: String,
    #[serde(rename = "v")]
    pub quantity: String,
    #[serde(rename = "p")]
    pub price: String,
    #[serde(rename = "i")]
    pub trade_id: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct OrderBookData {
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "b")]
    pub bids: Vec<[String; 2]>,
    #[serde(rename = "a")]
    pub asks: Vec<[String; 2]>,
    #[serde(rename = "u", default)]
    pub update_id: Option<i64>,
    #[serde(default)]
    pub seq: Option<i64>,
    #[serde(default)]
    pub cts: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct KlineData {
    #[serde(rename = "symbol")]
    pub symbol: String,
    #[serde(rename = "start")]
    pub start: i64,
    #[serde(rename = "end")]
    pub end: i64,
    #[serde(rename = "interval")]
    pub interval: String,
    #[serde(rename = "open")]
    pub open: String,
    #[serde(rename = "close")]
    pub close: String,
    #[serde(rename = "high")]
    pub high: String,
    #[serde(rename = "low")]
    pub low: String,
    #[serde(rename = "volume")]
    pub volume: String,
    #[serde(rename = "confirm")]
    pub confirm: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ServerTimeResponse {
    #[serde(rename = "retCode")]
    pub ret_code: i64,
    #[serde(rename = "retMsg")]
    pub ret_msg: String,
    pub result: ServerTimeResult,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ServerTimeResult {
    #[serde(rename = "timeSecond")]
    pub time_second: String,
    #[serde(rename = "timeNano")]
    pub time_nano: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct InstrumentsInfoResponse {
    #[serde(rename = "retCode")]
    pub ret_code: i64,
    #[serde(rename = "retMsg")]
    pub ret_msg: String,
    pub result: InstrumentsInfoResult,
}

#[derive(Clone, Debug, Deserialize)]
pub struct InstrumentsInfoResult {
    pub list: Vec<InstrumentInfo>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MarketTickersResponse {
    #[serde(rename = "retCode")]
    pub ret_code: i64,
    #[serde(rename = "retMsg")]
    pub ret_msg: String,
    pub result: MarketTickersResult,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MarketTickersResult {
    pub list: Vec<TickerData>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct InstrumentInfo {
    pub symbol: String,
    pub status: String,
    #[serde(rename = "baseCoin")]
    pub base_coin: String,
    #[serde(rename = "quoteCoin")]
    pub quote_coin: String,
    #[serde(rename = "settleCoin")]
    pub settle_coin: String,
    #[serde(rename = "priceScale")]
    pub price_scale: String,
    #[serde(rename = "priceFilter")]
    pub price_filter: PriceFilter,
    #[serde(rename = "lotSizeFilter")]
    pub lot_size_filter: LotSizeFilter,
    #[serde(rename = "leverageFilter")]
    pub leverage_filter: LeverageFilter,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PriceFilter {
    #[serde(rename = "tickSize")]
    pub tick_size: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct LotSizeFilter {
    #[serde(rename = "qtyStep")]
    pub qty_step: String,
    #[serde(rename = "minOrderQty")]
    pub min_order_qty: String,
    #[serde(rename = "minNotionalValue")]
    pub min_notional_value: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct LeverageFilter {
    #[serde(rename = "maxLeverage")]
    pub max_leverage: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct WalletData {
    #[serde(rename = "accountType")]
    pub account_type: String,
    #[serde(rename = "coin")]
    pub coins: Vec<WalletCoin>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct WalletBalanceResponse {
    #[serde(rename = "retCode")]
    pub ret_code: i64,
    #[serde(rename = "retMsg")]
    pub ret_msg: String,
    pub result: WalletBalanceResult,
}

#[derive(Clone, Debug, Deserialize)]
pub struct WalletBalanceResult {
    pub list: Vec<WalletBalanceAccount>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct WalletBalanceAccount {
    #[serde(rename = "accountType")]
    pub account_type: String,
    #[serde(rename = "totalWalletBalance")]
    pub total_wallet_balance: String,
    #[serde(rename = "totalAvailableBalance")]
    pub total_available_balance: String,
    #[serde(rename = "totalPerpUPL")]
    pub total_perp_upl: Option<String>,
    pub coin: Vec<WalletCoin>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct WalletCoin {
    pub coin: String,
    #[serde(rename = "walletBalance")]
    pub wallet_balance: String,
    #[serde(rename = "availableToWithdraw")]
    pub available_to_withdraw: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PositionData {
    pub symbol: String,
    pub side: String,
    pub size: String,
    #[serde(rename = "entryPrice")]
    pub entry_price: String,
    #[serde(rename = "unrealisedPnl")]
    pub unrealised_pnl: String,
    #[serde(rename = "tradeMode")]
    pub trade_mode: u8,
    #[serde(rename = "positionIdx")]
    pub position_idx: u8,
    pub leverage: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_i64_from_value")]
    pub seq: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PositionListResponse {
    #[serde(rename = "retCode")]
    pub ret_code: i64,
    #[serde(rename = "retMsg")]
    pub ret_msg: String,
    pub result: PositionListResult,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PositionListResult {
    pub list: Vec<PositionData>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct OrderData {
    pub symbol: String,
    #[serde(rename = "orderId")]
    pub order_id: String,
    #[serde(rename = "orderLinkId")]
    pub order_link_id: Option<String>,
    pub side: String,
    #[serde(rename = "orderType")]
    pub order_type: String,
    pub price: String,
    #[serde(rename = "qty")]
    pub quantity: String,
    #[serde(rename = "orderStatus")]
    pub order_status: String,
    #[serde(rename = "cumExecQty")]
    pub cumulative_exec_qty: String,
    #[serde(rename = "avgPrice")]
    pub average_price: Option<String>,
    #[serde(rename = "timeInForce")]
    pub time_in_force: String,
    #[serde(rename = "reduceOnly")]
    pub reduce_only: bool,
    #[serde(
        rename = "createdTime",
        deserialize_with = "deserialize_i64_from_string"
    )]
    pub created_time: i64,
    #[serde(
        rename = "updatedTime",
        deserialize_with = "deserialize_i64_from_string"
    )]
    pub updated_time: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct OrderListResponse {
    #[serde(rename = "retCode")]
    pub ret_code: i64,
    #[serde(rename = "retMsg")]
    pub ret_msg: String,
    pub result: OrderListResult,
}

#[derive(Clone, Debug, Deserialize)]
pub struct OrderListResult {
    pub list: Vec<OrderData>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ExecutionListResponse {
    #[serde(rename = "retCode")]
    pub ret_code: i64,
    #[serde(rename = "retMsg")]
    pub ret_msg: String,
    pub result: ExecutionListResult,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ExecutionListResult {
    pub list: Vec<ExecutionData>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ExecutionData {
    pub symbol: String,
    #[serde(rename = "execId")]
    pub exec_id: String,
    #[serde(rename = "orderId")]
    pub order_id: String,
    #[serde(rename = "orderLinkId")]
    pub order_link_id: Option<String>,
    pub side: String,
    #[serde(rename = "execQty")]
    pub exec_qty: String,
    #[serde(rename = "execPrice")]
    pub exec_price: String,
    #[serde(rename = "execFee")]
    pub exec_fee: String,
    #[serde(rename = "feeCurrency")]
    pub fee_currency: Option<String>,
    #[serde(rename = "execTime", deserialize_with = "deserialize_i64_from_string")]
    pub exec_time: i64,
    #[serde(default, deserialize_with = "deserialize_optional_i64_from_value")]
    pub seq: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AccountInfoResponse {
    #[serde(rename = "retCode")]
    pub ret_code: i64,
    #[serde(rename = "retMsg")]
    pub ret_msg: String,
    pub result: AccountInfoResult,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct AccountInfoResult {
    #[serde(default, rename = "unifiedMarginStatus")]
    pub unified_margin_status: Option<u8>,
    #[serde(default, rename = "marginMode")]
    pub margin_mode: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct RetCodeResponse {
    #[serde(rename = "retCode")]
    pub ret_code: i64,
    #[serde(rename = "retMsg")]
    pub ret_msg: String,
    #[serde(default)]
    pub result: Option<Value>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct OrderResult {
    pub symbol: Option<String>,
    #[serde(rename = "orderId")]
    pub order_id: Option<String>,
    #[serde(rename = "orderLinkId")]
    pub order_link_id: Option<String>,
}

fn deserialize_i64_from_string<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    value.parse::<i64>().map_err(serde::de::Error::custom)
}

fn deserialize_optional_i64_from_value<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => number
            .as_i64()
            .ok_or_else(|| serde::de::Error::custom("invalid numeric i64 value"))
            .map(Some),
        Some(Value::String(raw)) => raw
            .parse::<i64>()
            .map(Some)
            .map_err(serde::de::Error::custom),
        Some(other) => Err(serde::de::Error::custom(format!(
            "unsupported i64 representation: {other}"
        ))),
    }
}
