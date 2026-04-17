use serde::Deserialize;

/// Binance public linear-futures market messages.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "e")]
pub enum PublicMessage {
    #[serde(rename = "24hrTicker")]
    Ticker(TickerEvent),
    #[serde(rename = "aggTrade")]
    AggTrade(AggTradeEvent),
    #[serde(rename = "bookTicker")]
    BookTicker(BookTickerEvent),
    #[serde(rename = "kline")]
    Kline(KlineEvent),
    #[serde(rename = "markPriceUpdate")]
    MarkPrice(MarkPriceEvent),
}

/// Binance private user-data messages.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "e")]
pub enum PrivateMessage {
    #[serde(rename = "ACCOUNT_UPDATE")]
    AccountUpdate(AccountUpdateEvent),
    #[serde(rename = "ORDER_TRADE_UPDATE")]
    OrderTradeUpdate(Box<OrderTradeUpdateEvent>),
}

#[derive(Clone, Debug, Deserialize)]
pub struct TickerEvent {
    #[serde(rename = "E")]
    pub event_time: i64,
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "c")]
    pub last_price: String,
    #[serde(rename = "v")]
    pub volume_24h: String,
    #[serde(rename = "q")]
    pub quote_volume_24h: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AggTradeEvent {
    #[serde(rename = "E")]
    pub event_time: i64,
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "a")]
    pub agg_trade_id: i64,
    #[serde(rename = "p")]
    pub price: String,
    #[serde(rename = "q")]
    pub quantity: String,
    #[serde(rename = "T")]
    pub trade_time: i64,
    #[serde(rename = "m")]
    pub is_buyer_maker: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct BookTickerEvent {
    #[serde(rename = "T")]
    pub transaction_time: i64,
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "b")]
    pub best_bid_price: String,
    #[serde(rename = "B")]
    pub best_bid_qty: String,
    #[serde(rename = "a")]
    pub best_ask_price: String,
    #[serde(rename = "A")]
    pub best_ask_qty: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct KlineEvent {
    #[serde(rename = "E")]
    pub event_time: i64,
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "k")]
    pub kline: KlineData,
}

#[derive(Clone, Debug, Deserialize)]
pub struct KlineData {
    #[serde(rename = "i")]
    pub interval: String,
    #[serde(rename = "t")]
    pub open_time: i64,
    #[serde(rename = "T")]
    pub close_time: i64,
    #[serde(rename = "o")]
    pub open: String,
    #[serde(rename = "h")]
    pub high: String,
    #[serde(rename = "l")]
    pub low: String,
    #[serde(rename = "c")]
    pub close: String,
    #[serde(rename = "v")]
    pub volume: String,
    #[serde(rename = "x")]
    pub closed: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MarkPriceEvent {
    #[serde(rename = "E")]
    pub event_time: i64,
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "p")]
    pub mark_price: String,
    #[serde(rename = "r")]
    pub funding_rate: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct OpenInterestSnapshot {
    pub symbol: String,
    #[serde(rename = "openInterest")]
    pub open_interest: String,
    pub time: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TickerSnapshot {
    pub symbol: String,
    #[serde(rename = "lastPrice")]
    pub last_price: String,
    pub volume: String,
    #[serde(rename = "quoteVolume")]
    pub quote_volume: String,
    #[serde(rename = "closeTime")]
    pub close_time: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AggTradeSnapshot {
    #[serde(rename = "a")]
    pub agg_trade_id: i64,
    #[serde(rename = "p")]
    pub price: String,
    #[serde(rename = "q")]
    pub quantity: String,
    #[serde(rename = "T")]
    pub trade_time: i64,
    #[serde(rename = "m")]
    pub is_buyer_maker: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct BookTickerSnapshot {
    pub symbol: String,
    #[serde(rename = "bidPrice")]
    pub bid_price: String,
    #[serde(rename = "bidQty")]
    pub bid_qty: String,
    #[serde(rename = "askPrice")]
    pub ask_price: String,
    #[serde(rename = "askQty")]
    pub ask_qty: String,
    pub time: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ServerTimeResponse {
    #[serde(rename = "serverTime")]
    pub server_time: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ExchangeInfoResponse {
    pub symbols: Vec<ExchangeSymbol>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ExchangeSymbol {
    pub symbol: String,
    #[serde(rename = "contractType")]
    pub contract_type: String,
    pub status: String,
    #[serde(rename = "baseAsset")]
    pub base_asset: String,
    #[serde(rename = "quoteAsset")]
    pub quote_asset: String,
    #[serde(rename = "marginAsset")]
    pub margin_asset: String,
    #[serde(rename = "quotePrecision")]
    pub quote_precision: u32,
    pub filters: Vec<ExchangeFilter>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ExchangeFilter {
    #[serde(rename = "filterType")]
    pub filter_type: String,
    #[serde(default, rename = "tickSize")]
    pub tick_size: Option<String>,
    #[serde(default, rename = "stepSize")]
    pub step_size: Option<String>,
    #[serde(default, rename = "minQty")]
    pub min_qty: Option<String>,
    #[serde(default)]
    pub notional: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AccountUpdateEvent {
    #[serde(rename = "E")]
    pub event_time: i64,
    #[serde(rename = "T")]
    pub transaction_time: i64,
    #[serde(rename = "a")]
    pub account: AccountUpdateData,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AccountUpdateData {
    #[serde(rename = "B")]
    pub balances: Vec<AccountBalance>,
    #[serde(rename = "P")]
    pub positions: Vec<AccountPosition>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AccountBalance {
    #[serde(rename = "a")]
    pub asset: String,
    #[serde(rename = "wb")]
    pub wallet_balance: String,
    #[serde(rename = "cw")]
    pub cross_wallet_balance: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AccountPosition {
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "pa")]
    pub position_amount: String,
    #[serde(default, rename = "ep")]
    pub entry_price: Option<String>,
    #[serde(rename = "up")]
    pub unrealized_pnl: String,
    #[serde(rename = "mt")]
    pub margin_type: String,
    #[serde(rename = "ps")]
    pub position_side: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AccountInfoResponse {
    #[serde(rename = "totalWalletBalance")]
    pub total_wallet_balance: String,
    #[serde(rename = "availableBalance")]
    pub available_balance: String,
    #[serde(rename = "totalUnrealizedProfit")]
    pub total_unrealized_profit: String,
    pub assets: Vec<AccountAssetSnapshot>,
    pub positions: Vec<AccountPositionSnapshot>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AccountAssetSnapshot {
    pub asset: String,
    #[serde(rename = "walletBalance")]
    pub wallet_balance: String,
    #[serde(rename = "availableBalance")]
    pub available_balance: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AccountPositionSnapshot {
    pub symbol: String,
    #[serde(rename = "positionAmt")]
    pub position_amount: String,
    #[serde(default, rename = "entryPrice")]
    pub entry_price: Option<String>,
    #[serde(default, rename = "unrealizedProfit")]
    pub unrealized_profit: Option<String>,
    #[serde(default)]
    pub leverage: Option<String>,
    #[serde(default, rename = "marginType")]
    pub margin_type: Option<String>,
    #[serde(default)]
    pub isolated: Option<bool>,
    #[serde(default, rename = "isolatedMargin")]
    pub isolated_margin: Option<String>,
    #[serde(default, rename = "isolatedWallet")]
    pub isolated_wallet: Option<String>,
    #[serde(rename = "positionSide")]
    pub position_side: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct OrderTradeUpdateEvent {
    #[serde(rename = "o")]
    pub order: OrderTradeData,
}

#[derive(Clone, Debug, Deserialize)]
pub struct OrderTradeData {
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "c")]
    pub client_order_id: String,
    #[serde(rename = "S")]
    pub side: String,
    #[serde(rename = "o")]
    pub order_type: String,
    #[serde(rename = "f")]
    pub time_in_force: String,
    #[serde(rename = "q")]
    pub original_quantity: String,
    #[serde(rename = "p")]
    pub price: String,
    #[serde(rename = "ap")]
    pub average_price: Option<String>,
    #[serde(rename = "x")]
    pub execution_type: String,
    #[serde(rename = "X")]
    pub order_status: String,
    #[serde(rename = "i")]
    pub order_id: i64,
    #[serde(rename = "l")]
    pub last_filled_qty: String,
    #[serde(rename = "z")]
    pub cumulative_filled_qty: String,
    #[serde(rename = "L")]
    pub last_filled_price: String,
    #[serde(rename = "n")]
    pub commission: Option<String>,
    #[serde(rename = "N")]
    pub commission_asset: Option<String>,
    #[serde(rename = "T")]
    pub trade_time: i64,
    #[serde(rename = "t")]
    pub trade_id: Option<i64>,
    #[serde(rename = "R")]
    pub reduce_only: bool,
    #[serde(rename = "O")]
    pub order_trade_time: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ErrorResponse {
    pub code: i64,
    #[serde(rename = "msg")]
    pub message: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct OrderResponse {
    pub symbol: String,
    #[serde(rename = "orderId")]
    pub order_id: i64,
    #[serde(rename = "clientOrderId")]
    pub client_order_id: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct OrderSnapshot {
    pub symbol: String,
    #[serde(rename = "orderId")]
    pub order_id: i64,
    #[serde(rename = "clientOrderId")]
    pub client_order_id: String,
    pub side: String,
    #[serde(rename = "type")]
    pub order_type: String,
    #[serde(rename = "timeInForce")]
    pub time_in_force: String,
    pub status: String,
    pub price: String,
    #[serde(rename = "origQty")]
    pub original_quantity: String,
    #[serde(rename = "executedQty")]
    pub executed_quantity: String,
    #[serde(rename = "avgPrice")]
    pub average_price: String,
    #[serde(rename = "reduceOnly")]
    pub reduce_only: bool,
    #[serde(rename = "updateTime")]
    pub update_time: i64,
    #[serde(rename = "time")]
    pub created_time: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct UserTradeSnapshot {
    pub symbol: String,
    pub id: i64,
    #[serde(rename = "orderId")]
    pub order_id: i64,
    pub side: String,
    pub price: String,
    pub qty: String,
    pub commission: String,
    #[serde(rename = "commissionAsset")]
    pub commission_asset: String,
    pub maker: bool,
    pub time: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SetLeverageResponse {
    pub symbol: String,
    pub leverage: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SuccessResponse {
    pub code: Option<i64>,
    #[serde(rename = "msg")]
    pub message: String,
}
