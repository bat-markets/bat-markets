use serde::Deserialize;
use serde_json::Value;

#[derive(Clone, Debug, Deserialize)]
pub struct RestResponse<T> {
    pub success: bool,
    #[serde(default)]
    pub code: i64,
    #[serde(default)]
    pub message: Option<String>,
    pub data: T,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ServerTimeResponse {
    pub data: i64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContractInfo {
    pub symbol: String,
    pub base_coin: String,
    pub quote_coin: String,
    pub settle_coin: String,
    pub contract_size: Value,
    pub price_scale: u32,
    pub vol_scale: u32,
    #[serde(default)]
    pub amount_scale: Option<u32>,
    pub price_unit: Value,
    pub vol_unit: Value,
    pub min_vol: Value,
    #[serde(default)]
    pub max_leverage: Option<Value>,
    #[serde(default)]
    pub state: Option<i64>,
    #[serde(default)]
    pub future_type: Option<i64>,
    #[serde(default)]
    pub api_allowed: Option<bool>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PublicEnvelope {
    pub channel: String,
    #[serde(default)]
    pub symbol: Option<String>,
    #[serde(default)]
    pub data: Value,
    #[serde(default)]
    pub ts: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TickerData {
    pub symbol: String,
    pub last_price: Value,
    #[serde(default)]
    pub fair_price: Option<Value>,
    #[serde(default)]
    pub index_price: Option<Value>,
    #[serde(default)]
    pub volume24: Option<Value>,
    #[serde(default)]
    pub amount24: Option<Value>,
    #[serde(default)]
    pub hold_vol: Option<Value>,
    #[serde(default)]
    pub max_bid_price: Option<Value>,
    #[serde(default)]
    pub min_ask_price: Option<Value>,
    #[serde(default)]
    pub timestamp: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct DealData {
    pub p: Value,
    pub v: Value,
    #[serde(rename = "T")]
    pub side: i64,
    #[serde(default)]
    pub t: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct DepthData {
    #[serde(default)]
    pub asks: Vec<Vec<Value>>,
    #[serde(default)]
    pub bids: Vec<Vec<Value>>,
    #[serde(default)]
    pub version: Option<i64>,
    #[serde(default)]
    pub timestamp: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct KlineData {
    #[serde(default)]
    pub symbol: Option<String>,
    #[serde(default)]
    pub interval: Option<String>,
    pub o: Value,
    pub h: Value,
    pub l: Value,
    pub c: Value,
    #[serde(default)]
    pub a: Option<Value>,
    #[serde(default)]
    pub q: Option<Value>,
    #[serde(default)]
    pub t: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct KlineRestData {
    #[serde(default)]
    pub time: Vec<i64>,
    #[serde(default)]
    pub open: Vec<Value>,
    #[serde(default)]
    pub close: Vec<Value>,
    #[serde(default)]
    pub high: Vec<Value>,
    #[serde(default)]
    pub low: Vec<Value>,
    #[serde(default)]
    pub vol: Vec<Value>,
    #[serde(default)]
    pub amount: Vec<Value>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FundingRateData {
    pub symbol: String,
    pub funding_rate: Value,
    #[serde(default)]
    pub next_settle_time: Option<i64>,
    #[serde(default)]
    pub timestamp: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetData {
    pub currency: String,
    #[serde(default)]
    pub available_balance: Option<Value>,
    #[serde(default)]
    pub available: Option<Value>,
    #[serde(default)]
    pub wallet_balance: Option<Value>,
    #[serde(default)]
    pub equity: Option<Value>,
    #[serde(default)]
    pub unrealized: Option<Value>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PositionData {
    pub position_id: Value,
    pub symbol: String,
    pub position_type: i64,
    #[serde(default)]
    pub hold_vol: Option<Value>,
    #[serde(default)]
    pub open_avg_price: Option<Value>,
    #[serde(default)]
    pub mark_price: Option<Value>,
    #[serde(default)]
    pub unrealised: Option<Value>,
    #[serde(default)]
    pub unrealized: Option<Value>,
    #[serde(default)]
    pub leverage: Option<Value>,
    #[serde(default)]
    pub open_type: Option<i64>,
    #[serde(default)]
    pub position_mode: Option<i64>,
    #[serde(default)]
    pub update_time: Option<i64>,
    #[serde(default)]
    pub create_time: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderData {
    pub order_id: Value,
    pub symbol: String,
    pub side: i64,
    #[serde(default)]
    pub order_type: Option<i64>,
    #[serde(default)]
    pub category: Option<i64>,
    pub state: i64,
    pub price: Value,
    pub vol: Value,
    #[serde(default)]
    pub deal_vol: Option<Value>,
    #[serde(default)]
    pub deal_avg_price: Option<Value>,
    #[serde(default)]
    pub external_oid: Option<String>,
    #[serde(default)]
    pub reduce_only: Option<bool>,
    #[serde(default)]
    pub create_time: Option<i64>,
    #[serde(default)]
    pub update_time: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionData {
    pub id: Value,
    pub order_id: Value,
    pub symbol: String,
    pub side: i64,
    pub vol: Value,
    pub price: Value,
    #[serde(default)]
    pub fee: Option<Value>,
    #[serde(default)]
    pub fee_currency: Option<String>,
    #[serde(default)]
    pub is_taker: Option<bool>,
    #[serde(default)]
    pub timestamp: Option<Value>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitOrderResponse {
    pub data: Option<Value>,
    pub success: bool,
    #[serde(default)]
    pub code: i64,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelOrderItem {
    pub order_id: Value,
    #[serde(default)]
    pub error_code: i64,
    #[serde(default)]
    pub error_msg: Option<String>,
}
