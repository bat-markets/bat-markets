//! Bybit linear futures adapter.

pub mod native;

use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use parking_lot::RwLock;
use rust_decimal::Decimal;
use serde::Deserialize;

use bat_markets_core::{
    AccountCapabilities, AccountSnapshot, AggressorSide, AssetCapabilities, AssetCode, Balance,
    BatMarketsConfig, CapabilitySet, CommandOperation, CommandReceipt, CommandStatus, ErrorKind,
    Execution, FastBookTop, FastKline, FastTicker, FastTrade, FetchOhlcvRequest, FundingRate,
    InstrumentCatalog, InstrumentId, InstrumentSpec, InstrumentStatus, InstrumentSupport, Kline,
    KlineInterval, Leverage, MarginMode, MarketCapabilities, MarketError, MarketType,
    NativeCapabilities, Notional, OpenInterest, Order, OrderId, OrderStatus, OrderType, Position,
    PositionCapabilities, PositionDirection, PositionId, PositionMode, Price, PrivateLaneEvent,
    Product, PublicLaneEvent, Quantity, Rate, RequestId, Result, Side, TimeInForce, TimestampMs,
    TradeCapabilities, TradeId, Venue, VenueAdapter,
};

/// Bybit account context discovered from authenticated endpoints.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BybitAccountContext {
    pub wallet_account_type: Box<str>,
    pub margin_mode: Option<MarginMode>,
}

/// Bybit linear futures adapter with a handwritten, fixture-backed contract.
#[derive(Clone, Debug)]
pub struct BybitLinearFuturesAdapter {
    config: BatMarketsConfig,
    capabilities: CapabilitySet,
    lane_set: bat_markets_core::LaneSet,
    instruments: Arc<RwLock<InstrumentCatalog>>,
}

impl Default for BybitLinearFuturesAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl BybitLinearFuturesAdapter {
    #[must_use]
    pub fn new() -> Self {
        Self::with_config(BatMarketsConfig::new(Venue::Bybit, Product::LinearUsdt))
    }

    #[must_use]
    pub fn with_config(config: BatMarketsConfig) -> Self {
        Self {
            config,
            capabilities: CapabilitySet {
                market: MarketCapabilities {
                    ticker: true,
                    recent_trades: true,
                    book_top: true,
                    klines: true,
                    funding_rate: true,
                    open_interest: true,
                    public_streams: true,
                },
                trade: TradeCapabilities {
                    create: true,
                    cancel: true,
                    get: true,
                    list_open: true,
                    history: true,
                },
                position: PositionCapabilities {
                    read: true,
                    leverage_set: true,
                    margin_mode_set: true,
                    hedge_mode: true,
                },
                account: AccountCapabilities {
                    read_balances: true,
                    read_summary: true,
                    private_streams: true,
                },
                asset: AssetCapabilities::default(),
                native: NativeCapabilities {
                    fast_stream: true,
                    special_orders: true,
                },
            },
            lane_set: bat_markets_core::LaneSet {
                public: bat_markets_core::LanePolicy {
                    lossless: false,
                    coalescing_allowed: true,
                    buffer_capacity: 4_096,
                    reconnect_required: true,
                    idempotent: false,
                },
                private: bat_markets_core::LanePolicy {
                    lossless: true,
                    coalescing_allowed: false,
                    buffer_capacity: 8_192,
                    reconnect_required: true,
                    idempotent: true,
                },
                command: bat_markets_core::LanePolicy {
                    lossless: true,
                    coalescing_allowed: false,
                    buffer_capacity: 1_024,
                    reconnect_required: true,
                    idempotent: true,
                },
            },
            instruments: Arc::new(RwLock::new(InstrumentCatalog::new([
                btc_spec(),
                eth_spec(),
            ]))),
        }
    }

    pub fn replace_instruments(&self, instruments: Vec<InstrumentSpec>) {
        self.instruments.write().replace(instruments);
    }

    pub fn parse_native_public(&self, payload: &str) -> Result<native::PublicEnvelope> {
        serde_json::from_str(payload).map_err(|error| {
            MarketError::new(
                ErrorKind::DecodeError,
                format!("failed to parse bybit public payload: {error}"),
            )
            .with_venue(Venue::Bybit, Product::LinearUsdt)
            .with_operation("bybit.parse_native_public")
        })
    }

    pub fn parse_native_private(&self, payload: &str) -> Result<native::PrivateEnvelope> {
        serde_json::from_str(payload).map_err(|error| {
            MarketError::new(
                ErrorKind::DecodeError,
                format!("failed to parse bybit private payload: {error}"),
            )
            .with_venue(Venue::Bybit, Product::LinearUsdt)
            .with_operation("bybit.parse_native_private")
        })
    }

    pub fn parse_server_time(&self, payload: &str) -> Result<TimestampMs> {
        let response =
            serde_json::from_str::<native::ServerTimeResponse>(payload).map_err(decode_error)?;
        if response.ret_code != 0 {
            return Err(exchange_reject(response.ret_code, &response.ret_msg));
        }
        let nanos = response
            .result
            .time_nano
            .parse::<i128>()
            .map_err(|error| decode_string_error("bybit server time nano", error))?;
        Ok(TimestampMs::new((nanos / 1_000_000_i128) as i64))
    }

    pub fn parse_metadata_snapshot(&self, payload: &str) -> Result<Vec<InstrumentSpec>> {
        let response = serde_json::from_str::<native::InstrumentsInfoResponse>(payload)
            .map_err(decode_error)?;
        if response.ret_code != 0 {
            return Err(exchange_reject(response.ret_code, &response.ret_msg));
        }

        let mut instruments = Vec::new();
        for instrument in response.result.list {
            let tick_size = parse_decimal(&instrument.price_filter.tick_size)?;
            let qty_step = parse_decimal(&instrument.lot_size_filter.qty_step)?;
            let min_qty = parse_decimal(&instrument.lot_size_filter.min_order_qty)?;
            let min_notional = parse_decimal(&instrument.lot_size_filter.min_notional_value)?;
            let price_scale = decimal_scale(tick_size);
            let qty_scale = decimal_scale(qty_step);
            let quote_scale = price_scale
                .saturating_add(qty_scale)
                .max(decimal_scale(min_notional));

            instruments.push(InstrumentSpec {
                venue: Venue::Bybit,
                product: Product::LinearUsdt,
                market_type: MarketType::LinearPerpetual,
                instrument_id: InstrumentId::from(canonical_symbol(
                    &instrument.base_coin,
                    &instrument.quote_coin,
                    &instrument.settle_coin,
                )),
                canonical_symbol: canonical_symbol(
                    &instrument.base_coin,
                    &instrument.quote_coin,
                    &instrument.settle_coin,
                )
                .into(),
                native_symbol: instrument.symbol.into(),
                base: AssetCode::from(instrument.base_coin),
                quote: AssetCode::from(instrument.quote_coin),
                settle: AssetCode::from(instrument.settle_coin),
                contract_size: Quantity::new(Decimal::ONE),
                tick_size: Price::new(tick_size),
                step_size: Quantity::new(qty_step),
                min_qty: Quantity::new(min_qty),
                min_notional: Notional::new(min_notional),
                price_scale,
                qty_scale,
                quote_scale,
                max_leverage: Some(Leverage::new(parse_decimal(
                    &instrument.leverage_filter.max_leverage,
                )?)),
                support: InstrumentSupport {
                    public_streams: true,
                    private_trading: true,
                    leverage_set: true,
                    margin_mode_set: true,
                    funding_rate: true,
                    open_interest: true,
                },
                status: parse_instrument_status(&instrument.status),
            });
        }

        Ok(instruments)
    }

    pub fn parse_account_context(&self, payload: &str) -> Result<BybitAccountContext> {
        let response =
            serde_json::from_str::<native::AccountInfoResponse>(payload).map_err(decode_error)?;
        if response.ret_code != 0 {
            return Err(exchange_reject(response.ret_code, &response.ret_msg));
        }

        Ok(BybitAccountContext {
            wallet_account_type: if response.result.unified_margin_status.unwrap_or(0) > 0 {
                "UNIFIED".into()
            } else {
                "CONTRACT".into()
            },
            margin_mode: response
                .result
                .margin_mode
                .as_deref()
                .and_then(parse_margin_mode_name),
        })
    }

    pub fn parse_account_snapshot(
        &self,
        payload: &str,
        observed_at: TimestampMs,
    ) -> Result<AccountSnapshot> {
        let response =
            serde_json::from_str::<native::WalletBalanceResponse>(payload).map_err(decode_error)?;
        if response.ret_code != 0 {
            return Err(exchange_reject(response.ret_code, &response.ret_msg));
        }

        let account = response.result.list.into_iter().next().ok_or_else(|| {
            MarketError::new(
                ErrorKind::DecodeError,
                "missing bybit wallet balance account",
            )
        })?;
        let balances = account
            .coin
            .into_iter()
            .map(|coin| {
                Ok(Balance {
                    asset: AssetCode::from(coin.coin),
                    wallet_balance: parse_decimal(&coin.wallet_balance)?.into(),
                    available_balance: parse_decimal(&coin.available_to_withdraw)?.into(),
                    updated_at: observed_at,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(AccountSnapshot {
            balances,
            summary: Some(bat_markets_core::AccountSummary {
                total_wallet_balance: parse_decimal(&account.total_wallet_balance)?.into(),
                total_available_balance: parse_decimal(&account.total_available_balance)?.into(),
                total_unrealized_pnl: account
                    .total_perp_upl
                    .as_deref()
                    .map(parse_decimal)
                    .transpose()?
                    .unwrap_or(Decimal::ZERO)
                    .into(),
                updated_at: observed_at,
            }),
        })
    }

    pub fn parse_positions_snapshot(
        &self,
        payload: &str,
        observed_at: TimestampMs,
    ) -> Result<Vec<Position>> {
        let response =
            serde_json::from_str::<native::PositionListResponse>(payload).map_err(decode_error)?;
        if response.ret_code != 0 {
            return Err(exchange_reject(response.ret_code, &response.ret_msg));
        }

        response
            .result
            .list
            .into_iter()
            .filter(|position| position.size != "0")
            .map(|position| self.position_from_snapshot(position, observed_at))
            .collect()
    }

    pub fn parse_open_orders_snapshot(
        &self,
        payload: &str,
        observed_at: TimestampMs,
    ) -> Result<Vec<Order>> {
        let response =
            serde_json::from_str::<native::OrderListResponse>(payload).map_err(decode_error)?;
        if response.ret_code != 0 {
            return Err(exchange_reject(response.ret_code, &response.ret_msg));
        }

        response
            .result
            .list
            .into_iter()
            .map(|order| self.order_from_snapshot(order, observed_at))
            .collect()
    }

    pub fn parse_order_snapshot(&self, payload: &str, observed_at: TimestampMs) -> Result<Order> {
        let mut orders = self.parse_open_orders_snapshot(payload, observed_at)?;
        orders.pop().ok_or_else(|| {
            MarketError::new(
                ErrorKind::DecodeError,
                "missing bybit order snapshot entry in response",
            )
        })
    }

    pub fn parse_order_history_snapshot(
        &self,
        payload: &str,
        observed_at: TimestampMs,
    ) -> Result<Vec<Order>> {
        self.parse_open_orders_snapshot(payload, observed_at)
    }

    pub fn parse_executions_snapshot(&self, payload: &str) -> Result<Vec<Execution>> {
        let response =
            serde_json::from_str::<native::ExecutionListResponse>(payload).map_err(decode_error)?;
        if response.ret_code != 0 {
            return Err(exchange_reject(response.ret_code, &response.ret_msg));
        }

        response
            .result
            .list
            .into_iter()
            .map(|execution| self.execution_from_snapshot(execution))
            .collect()
    }

    pub fn parse_ohlcv_snapshot(
        &self,
        payload: &str,
        request: &FetchOhlcvRequest,
    ) -> Result<Vec<Kline>> {
        #[derive(Clone, Debug, Deserialize)]
        struct KlineListResponse {
            #[serde(rename = "retCode")]
            ret_code: i64,
            #[serde(rename = "retMsg")]
            ret_msg: String,
            result: KlineListResult,
        }

        #[derive(Clone, Debug, Deserialize)]
        struct KlineListResult {
            list: Vec<[String; 7]>,
        }

        let interval = KlineInterval::parse(request.interval.as_ref()).ok_or_else(|| {
            MarketError::new(
                ErrorKind::Unsupported,
                format!("unsupported bybit OHLCV interval '{}'", request.interval),
            )
            .with_venue(Venue::Bybit, Product::LinearUsdt)
        })?;
        let instrument_id = request.single_instrument_id()?;
        let spec = self.resolve_instrument(instrument_id).ok_or_else(|| {
            MarketError::new(
                ErrorKind::Unsupported,
                format!("unsupported bybit instrument '{}'", instrument_id),
            )
            .with_venue(Venue::Bybit, Product::LinearUsdt)
        })?;
        let response = serde_json::from_str::<KlineListResponse>(payload).map_err(decode_error)?;
        if response.ret_code != 0 {
            return Err(exchange_reject(response.ret_code, &response.ret_msg));
        }

        let mut klines = response
            .result
            .list
            .into_iter()
            .map(|row| {
                let open_time = row[0]
                    .parse::<i64>()
                    .map_err(|error| decode_string_error("bybit kline startTime", error))?;
                let close_time = interval.close_time_ms(open_time).ok_or_else(|| {
                    MarketError::new(
                        ErrorKind::DecodeError,
                        format!(
                            "failed to derive bybit kline close time for interval '{}'",
                            interval.as_ccxt_str()
                        ),
                    )
                    .with_venue(Venue::Bybit, Product::LinearUsdt)
                })?;
                Ok(Kline {
                    instrument_id: spec.instrument_id.clone(),
                    interval: interval.as_ccxt_str().into(),
                    open: spec.price_from_fast(
                        Price::new(parse_decimal(&row[1])?).quantize(spec.price_scale)?,
                    ),
                    high: spec.price_from_fast(
                        Price::new(parse_decimal(&row[2])?).quantize(spec.price_scale)?,
                    ),
                    low: spec.price_from_fast(
                        Price::new(parse_decimal(&row[3])?).quantize(spec.price_scale)?,
                    ),
                    close: spec.price_from_fast(
                        Price::new(parse_decimal(&row[4])?).quantize(spec.price_scale)?,
                    ),
                    volume: spec.quantity_from_fast(
                        Quantity::new(parse_decimal(&row[5])?).quantize(spec.qty_scale)?,
                    ),
                    open_time: TimestampMs::new(open_time),
                    close_time: TimestampMs::new(close_time),
                    closed: close_time < now_timestamp_ms(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        klines.sort_by_key(|kline| kline.open_time.value());
        Ok(klines)
    }

    fn position_from_snapshot(
        &self,
        position: native::PositionData,
        observed_at: TimestampMs,
    ) -> Result<Position> {
        let spec = self.require_native_symbol(&position.symbol)?;
        let side = parse_side(&position.side)?;
        Ok(Position {
            position_id: PositionId::from(format!(
                "bybit:{}:{}",
                position.symbol, position.position_idx
            )),
            instrument_id: spec.instrument_id.clone(),
            direction: match side {
                Side::Buy => PositionDirection::Long,
                Side::Sell => PositionDirection::Short,
            },
            size: Quantity::new(parse_decimal(&position.size)?),
            entry_price: Some(Price::new(parse_decimal(&position.entry_price)?)),
            mark_price: None,
            unrealized_pnl: Some(parse_decimal(&position.unrealised_pnl)?.into()),
            leverage: parse_optional_decimal(position.leverage.as_deref())?.map(Leverage::new),
            margin_mode: parse_trade_mode(position.trade_mode),
            position_mode: parse_position_mode(position.position_idx),
            updated_at: observed_at,
        })
    }

    fn order_from_snapshot(
        &self,
        order: native::OrderData,
        observed_at: TimestampMs,
    ) -> Result<Order> {
        let spec = self.require_native_symbol(&order.symbol)?;
        let created_at = if order.created_time > 0 {
            TimestampMs::new(order.created_time)
        } else {
            observed_at
        };
        let updated_at = if order.updated_time > 0 {
            TimestampMs::new(order.updated_time)
        } else {
            observed_at
        };
        Ok(Order {
            order_id: OrderId::from(order.order_id),
            client_order_id: order.order_link_id.map(Into::into),
            instrument_id: spec.instrument_id.clone(),
            side: parse_side(&order.side)?,
            order_type: parse_order_type(&order.order_type)?,
            time_in_force: Some(parse_time_in_force(&order.time_in_force)?),
            status: parse_order_status(&order.order_status)?,
            price: Some(Price::new(parse_decimal(&order.price)?)),
            quantity: Quantity::new(parse_decimal(&order.quantity)?),
            filled_quantity: Quantity::new(parse_decimal(&order.cumulative_exec_qty)?),
            average_fill_price: parse_optional_decimal(order.average_price.as_deref())?
                .map(Price::new),
            reduce_only: order.reduce_only,
            post_only: matches!(order.time_in_force.as_str(), "PostOnly"),
            created_at,
            updated_at,
            venue_status: Some(order.order_status.into()),
        })
    }

    fn execution_from_snapshot(&self, execution: native::ExecutionData) -> Result<Execution> {
        let spec = self.require_native_symbol(&execution.symbol)?;
        Ok(Execution {
            execution_id: TradeId::from(execution.exec_id),
            order_id: OrderId::from(execution.order_id),
            client_order_id: execution.order_link_id.map(Into::into),
            instrument_id: spec.instrument_id.clone(),
            side: parse_side(&execution.side)?,
            quantity: Quantity::new(parse_decimal(&execution.exec_qty)?),
            price: Price::new(parse_decimal(&execution.exec_price)?),
            fee: Some(parse_decimal(&execution.exec_fee)?.into()),
            fee_asset: execution.fee_currency.map(AssetCode::from),
            liquidity: None,
            executed_at: TimestampMs::new(execution.exec_time),
        })
    }

    fn require_native_symbol(&self, native_symbol: &str) -> Result<InstrumentSpec> {
        self.resolve_native_symbol(native_symbol).ok_or_else(|| {
            MarketError::new(
                ErrorKind::Unsupported,
                format!("unsupported bybit symbol '{native_symbol}'"),
            )
            .with_venue(Venue::Bybit, Product::LinearUsdt)
        })
    }
}

impl VenueAdapter for BybitLinearFuturesAdapter {
    fn venue(&self) -> Venue {
        Venue::Bybit
    }

    fn product(&self) -> Product {
        Product::LinearUsdt
    }

    fn config(&self) -> &BatMarketsConfig {
        &self.config
    }

    fn capabilities(&self) -> CapabilitySet {
        self.capabilities
    }

    fn lane_set(&self) -> bat_markets_core::LaneSet {
        self.lane_set
    }

    fn instrument_specs(&self) -> Vec<InstrumentSpec> {
        self.instruments.read().all()
    }

    fn resolve_instrument(&self, instrument_id: &InstrumentId) -> Option<InstrumentSpec> {
        self.instruments.read().get(instrument_id)
    }

    fn resolve_native_symbol(&self, native_symbol: &str) -> Option<InstrumentSpec> {
        self.instruments.read().by_native_symbol(native_symbol)
    }

    fn parse_public(&self, payload: &str) -> Result<Vec<PublicLaneEvent>> {
        let envelope = self.parse_native_public(payload)?;
        if envelope.topic.starts_with("tickers.") {
            let data: native::TickerData =
                serde_json::from_value(envelope.data).map_err(decode_error)?;
            let spec = self.require_native_symbol(&data.symbol)?;
            return Ok(vec![
                PublicLaneEvent::Ticker(FastTicker {
                    instrument_id: spec.instrument_id.clone(),
                    last_price: Price::new(parse_decimal(&data.last_price)?)
                        .quantize(spec.price_scale)?,
                    mark_price: Some(
                        Price::new(parse_decimal(&data.mark_price)?).quantize(spec.price_scale)?,
                    ),
                    index_price: Some(
                        Price::new(parse_decimal(&data.index_price)?).quantize(spec.price_scale)?,
                    ),
                    volume_24h: Some(
                        Quantity::new(parse_decimal(&data.volume_24h)?).quantize(spec.qty_scale)?,
                    ),
                    turnover_24h: Some(
                        Notional::new(parse_decimal(&data.turnover_24h)?)
                            .quantize(spec.quote_scale)?,
                    ),
                    event_time: TimestampMs::new(envelope.ts),
                }),
                PublicLaneEvent::FundingRate(FundingRate {
                    instrument_id: spec.instrument_id.clone(),
                    value: Rate::new(parse_decimal(&data.funding_rate)?),
                    mark_price: Some(Price::new(parse_decimal(&data.mark_price)?)),
                    event_time: TimestampMs::new(envelope.ts),
                }),
                PublicLaneEvent::OpenInterest(OpenInterest {
                    instrument_id: spec.instrument_id.clone(),
                    value: Quantity::new(parse_decimal(&data.open_interest)?),
                    event_time: TimestampMs::new(envelope.ts),
                }),
            ]);
        }

        if envelope.topic.starts_with("publicTrade.") {
            let data: Vec<native::PublicTradeData> =
                serde_json::from_value(envelope.data).map_err(decode_error)?;
            let mut events = Vec::with_capacity(data.len());
            for trade in data {
                let spec = self.require_native_symbol(&trade.symbol)?;
                events.push(PublicLaneEvent::Trade(FastTrade {
                    instrument_id: spec.instrument_id.clone(),
                    trade_id: TradeId::from(trade.trade_id),
                    price: Price::new(parse_decimal(&trade.price)?).quantize(spec.price_scale)?,
                    quantity: Quantity::new(parse_decimal(&trade.quantity)?)
                        .quantize(spec.qty_scale)?,
                    aggressor_side: parse_aggressor(&trade.side)?,
                    event_time: TimestampMs::new(trade.trade_time),
                }));
            }
            return Ok(events);
        }

        if envelope.topic.starts_with("orderbook.") {
            let data: native::OrderBookData =
                serde_json::from_value(envelope.data).map_err(decode_error)?;
            let spec = self.require_native_symbol(&data.symbol)?;
            let bid = data.bids.first().ok_or_else(|| {
                MarketError::new(ErrorKind::DecodeError, "missing bybit best bid")
            })?;
            let ask = data.asks.first().ok_or_else(|| {
                MarketError::new(ErrorKind::DecodeError, "missing bybit best ask")
            })?;
            return Ok(vec![PublicLaneEvent::BookTop(FastBookTop {
                instrument_id: spec.instrument_id.clone(),
                bid_price: Price::new(parse_decimal(&bid[0])?).quantize(spec.price_scale)?,
                bid_quantity: Quantity::new(parse_decimal(&bid[1])?).quantize(spec.qty_scale)?,
                ask_price: Price::new(parse_decimal(&ask[0])?).quantize(spec.price_scale)?,
                ask_quantity: Quantity::new(parse_decimal(&ask[1])?).quantize(spec.qty_scale)?,
                event_time: TimestampMs::new(envelope.ts),
            })]);
        }

        if envelope.topic.starts_with("kline.") {
            let topic_symbol = envelope.topic.rsplit('.').next().ok_or_else(|| {
                MarketError::new(
                    ErrorKind::DecodeError,
                    format!(
                        "failed to derive bybit kline symbol from topic '{}'",
                        envelope.topic
                    ),
                )
                .with_venue(Venue::Bybit, Product::LinearUsdt)
            })?;
            let data: Vec<native::KlineData> =
                serde_json::from_value(envelope.data).map_err(decode_error)?;
            let mut events = Vec::with_capacity(data.len());
            for kline in data {
                let symbol = kline.symbol.as_deref().unwrap_or(topic_symbol);
                let spec = self.require_native_symbol(symbol)?;
                events.push(PublicLaneEvent::Kline(FastKline {
                    instrument_id: spec.instrument_id.clone(),
                    interval: kline.interval.into(),
                    open: Price::new(parse_decimal(&kline.open)?).quantize(spec.price_scale)?,
                    high: Price::new(parse_decimal(&kline.high)?).quantize(spec.price_scale)?,
                    low: Price::new(parse_decimal(&kline.low)?).quantize(spec.price_scale)?,
                    close: Price::new(parse_decimal(&kline.close)?).quantize(spec.price_scale)?,
                    volume: Quantity::new(parse_decimal(&kline.volume)?)
                        .quantize(spec.qty_scale)?,
                    open_time: TimestampMs::new(kline.start),
                    close_time: TimestampMs::new(kline.end),
                    closed: kline.confirm,
                }));
            }
            return Ok(events);
        }

        Err(MarketError::new(
            ErrorKind::Unsupported,
            format!("unsupported bybit public topic '{}'", envelope.topic),
        ))
    }

    fn parse_private(&self, payload: &str) -> Result<Vec<PrivateLaneEvent>> {
        let envelope = self.parse_native_private(payload)?;
        if envelope.topic == "wallet" {
            let data: Vec<native::WalletData> =
                serde_json::from_value(envelope.data).map_err(decode_error)?;
            let mut events = Vec::new();
            for wallet in data {
                for coin in wallet.coins {
                    events.push(PrivateLaneEvent::Balance(Balance {
                        asset: AssetCode::from(coin.coin),
                        wallet_balance: parse_decimal(&coin.wallet_balance)?.into(),
                        available_balance: parse_decimal(&coin.available_to_withdraw)?.into(),
                        updated_at: TimestampMs::new(envelope.creation_time),
                    }));
                }
            }
            return Ok(events);
        }

        if envelope.topic == "position" {
            let data: Vec<native::PositionData> =
                serde_json::from_value(envelope.data).map_err(decode_error)?;
            let mut events = Vec::new();
            for position in data {
                if position.size == "0" {
                    continue;
                }
                let spec = self.require_native_symbol(&position.symbol)?;
                let side = parse_side(&position.side)?;
                events.push(PrivateLaneEvent::Position(Position {
                    position_id: PositionId::from(format!(
                        "bybit:{}:{}",
                        position.symbol, position.position_idx
                    )),
                    instrument_id: spec.instrument_id.clone(),
                    direction: match side {
                        Side::Buy => PositionDirection::Long,
                        Side::Sell => PositionDirection::Short,
                    },
                    size: Quantity::new(parse_decimal(&position.size)?),
                    entry_price: Some(Price::new(parse_decimal(&position.entry_price)?)),
                    mark_price: None,
                    unrealized_pnl: Some(parse_decimal(&position.unrealised_pnl)?.into()),
                    leverage: parse_optional_decimal(position.leverage.as_deref())?
                        .map(Leverage::new),
                    margin_mode: parse_trade_mode(position.trade_mode),
                    position_mode: parse_position_mode(position.position_idx),
                    updated_at: TimestampMs::new(envelope.creation_time),
                }));
            }
            return Ok(events);
        }

        if envelope.topic == "order" {
            let data: Vec<native::OrderData> =
                serde_json::from_value(envelope.data).map_err(decode_error)?;
            let mut events = Vec::new();
            for order in data {
                events.push(PrivateLaneEvent::Order(self.order_from_snapshot(
                    order,
                    TimestampMs::new(envelope.creation_time),
                )?));
            }
            return Ok(events);
        }

        if envelope.topic == "execution" {
            let data: Vec<native::ExecutionData> =
                serde_json::from_value(envelope.data).map_err(decode_error)?;
            let mut events = Vec::new();
            for execution in data {
                let spec = self.require_native_symbol(&execution.symbol)?;
                events.push(PrivateLaneEvent::Execution(Execution {
                    execution_id: TradeId::from(execution.exec_id),
                    order_id: OrderId::from(execution.order_id),
                    client_order_id: execution.order_link_id.map(Into::into),
                    instrument_id: spec.instrument_id.clone(),
                    side: parse_side(&execution.side)?,
                    quantity: Quantity::new(parse_decimal(&execution.exec_qty)?),
                    price: Price::new(parse_decimal(&execution.exec_price)?),
                    fee: Some(parse_decimal(&execution.exec_fee)?.into()),
                    fee_asset: execution.fee_currency.map(AssetCode::from),
                    liquidity: None,
                    executed_at: TimestampMs::new(execution.exec_time),
                }));
            }
            return Ok(events);
        }

        Err(MarketError::new(
            ErrorKind::Unsupported,
            format!("unsupported bybit private topic '{}'", envelope.topic),
        ))
    }

    fn classify_command(
        &self,
        operation: CommandOperation,
        payload: Option<&str>,
        request_id: Option<RequestId>,
    ) -> Result<CommandReceipt> {
        let Some(payload) = payload else {
            return Ok(CommandReceipt {
                operation,
                status: CommandStatus::UnknownExecution,
                venue: Venue::Bybit,
                product: Product::LinearUsdt,
                instrument_id: None,
                order_id: None,
                client_order_id: None,
                request_id,
                message: Some("command outcome requires reconcile".into()),
                native_code: None,
                retriable: true,
            });
        };

        let response =
            serde_json::from_str::<native::RetCodeResponse>(payload).map_err(|error| {
                MarketError::new(
                    ErrorKind::DecodeError,
                    format!("failed to classify bybit command response: {error}"),
                )
            })?;

        if response.ret_code != 0 {
            return Ok(CommandReceipt {
                operation,
                status: CommandStatus::Rejected,
                venue: Venue::Bybit,
                product: Product::LinearUsdt,
                instrument_id: None,
                order_id: None,
                client_order_id: None,
                request_id,
                message: Some(response.ret_msg.into()),
                native_code: Some(response.ret_code.to_string().into()),
                retriable: false,
            });
        }

        let result = response.result.unwrap_or_default();

        let (instrument_id, order_id, client_order_id) = match operation {
            CommandOperation::CreateOrder
            | CommandOperation::CancelOrder
            | CommandOperation::GetOrder => {
                let order = serde_json::from_value::<native::OrderResult>(result)
                    .unwrap_or_else(|_| native::OrderResult::default());
                let instrument_id = order
                    .symbol
                    .as_deref()
                    .and_then(|symbol| self.resolve_native_symbol(symbol))
                    .map(|spec| spec.instrument_id.clone());
                (
                    instrument_id,
                    order.order_id.map(OrderId::from),
                    order.order_link_id.map(Into::into),
                )
            }
            _ => (None, None, None),
        };

        Ok(CommandReceipt {
            operation,
            status: CommandStatus::Accepted,
            venue: Venue::Bybit,
            product: Product::LinearUsdt,
            instrument_id,
            order_id,
            client_order_id,
            request_id,
            message: Some(response.ret_msg.into()),
            native_code: Some(response.ret_code.to_string().into()),
            retriable: false,
        })
    }
}

fn btc_spec() -> InstrumentSpec {
    instrument_spec(("BTC", "USDT", "USDT"), "BTCUSDT", 2, 3, 5, 100)
}

fn eth_spec() -> InstrumentSpec {
    instrument_spec(("ETH", "USDT", "USDT"), "ETHUSDT", 2, 3, 5, 50)
}

fn instrument_spec(
    assets: (&str, &str, &str),
    native_symbol: &str,
    price_scale: u32,
    qty_scale: u32,
    quote_scale: u32,
    max_leverage: i64,
) -> InstrumentSpec {
    let (base, quote, settle) = assets;
    InstrumentSpec {
        venue: Venue::Bybit,
        product: Product::LinearUsdt,
        market_type: MarketType::LinearPerpetual,
        instrument_id: InstrumentId::from(canonical_symbol(base, quote, settle)),
        canonical_symbol: canonical_symbol(base, quote, settle).into(),
        native_symbol: native_symbol.into(),
        base: AssetCode::from(base),
        quote: AssetCode::from(quote),
        settle: AssetCode::from(settle),
        contract_size: Quantity::new(Decimal::ONE),
        tick_size: Price::new(Decimal::new(1, price_scale)),
        step_size: Quantity::new(Decimal::new(1, qty_scale)),
        min_qty: Quantity::new(Decimal::new(1, qty_scale)),
        min_notional: Notional::new(Decimal::new(5, quote_scale)),
        price_scale,
        qty_scale,
        quote_scale,
        max_leverage: Some(Leverage::new(Decimal::new(max_leverage, 0))),
        support: InstrumentSupport {
            public_streams: true,
            private_trading: true,
            leverage_set: true,
            margin_mode_set: true,
            funding_rate: true,
            open_interest: true,
        },
        status: InstrumentStatus::Active,
    }
}

fn canonical_symbol(base: &str, quote: &str, settle: &str) -> String {
    format!("{base}/{quote}:{settle}")
}

fn parse_decimal(raw: &str) -> Result<Decimal> {
    raw.parse::<Decimal>().map_err(|error| {
        MarketError::new(
            ErrorKind::DecodeError,
            format!("invalid decimal '{raw}': {error}"),
        )
        .with_venue(Venue::Bybit, Product::LinearUsdt)
    })
}

fn parse_optional_decimal(raw: Option<&str>) -> Result<Option<Decimal>> {
    raw.map(parse_decimal).transpose()
}

fn parse_side(raw: &str) -> Result<Side> {
    match raw {
        "Buy" => Ok(Side::Buy),
        "Sell" => Ok(Side::Sell),
        other => Err(MarketError::new(
            ErrorKind::DecodeError,
            format!("unsupported bybit side '{other}'"),
        )),
    }
}

fn parse_aggressor(raw: &str) -> Result<AggressorSide> {
    match raw {
        "Buy" => Ok(AggressorSide::Buyer),
        "Sell" => Ok(AggressorSide::Seller),
        other => Err(MarketError::new(
            ErrorKind::DecodeError,
            format!("unsupported bybit trade side '{other}'"),
        )),
    }
}

fn parse_order_type(raw: &str) -> Result<OrderType> {
    match raw {
        "Market" => Ok(OrderType::Market),
        "Limit" => Ok(OrderType::Limit),
        "Stop" => Ok(OrderType::StopLimit),
        "StopMarket" => Ok(OrderType::StopMarket),
        other => Err(MarketError::new(
            ErrorKind::DecodeError,
            format!("unsupported bybit order type '{other}'"),
        )),
    }
}

fn parse_time_in_force(raw: &str) -> Result<TimeInForce> {
    match raw {
        "GTC" => Ok(TimeInForce::Gtc),
        "IOC" => Ok(TimeInForce::Ioc),
        "FOK" => Ok(TimeInForce::Fok),
        "PostOnly" => Ok(TimeInForce::PostOnly),
        other => Err(MarketError::new(
            ErrorKind::DecodeError,
            format!("unsupported bybit time in force '{other}'"),
        )),
    }
}

fn parse_order_status(raw: &str) -> Result<OrderStatus> {
    match raw {
        "New" => Ok(OrderStatus::New),
        "PartiallyFilled" => Ok(OrderStatus::PartiallyFilled),
        "Filled" => Ok(OrderStatus::Filled),
        "Cancelled" | "Canceled" => Ok(OrderStatus::Canceled),
        "Rejected" | "Deactivated" => Ok(OrderStatus::Rejected),
        other => Err(MarketError::new(
            ErrorKind::DecodeError,
            format!("unsupported bybit order status '{other}'"),
        )),
    }
}

fn parse_trade_mode(mode: u8) -> MarginMode {
    if mode == 1 {
        MarginMode::Isolated
    } else {
        MarginMode::Cross
    }
}

fn parse_margin_mode_name(raw: &str) -> Option<MarginMode> {
    match raw {
        "ISOLATED_MARGIN" => Some(MarginMode::Isolated),
        "REGULAR_MARGIN" | "CROSS_MARGIN" => Some(MarginMode::Cross),
        _ => None,
    }
}

fn parse_position_mode(index: u8) -> PositionMode {
    if index == 0 {
        PositionMode::OneWay
    } else {
        PositionMode::Hedge
    }
}

fn parse_instrument_status(raw: &str) -> InstrumentStatus {
    match raw {
        "Trading" => InstrumentStatus::Active,
        "Settling" | "Closed" | "PreLaunch" => InstrumentStatus::Halted,
        "Deliverying" => InstrumentStatus::Settled,
        _ => InstrumentStatus::Halted,
    }
}

fn decimal_scale(value: Decimal) -> u32 {
    value.normalize().scale()
}

fn decode_error(error: serde_json::Error) -> MarketError {
    MarketError::new(
        ErrorKind::DecodeError,
        format!("failed to decode bybit topic payload: {error}"),
    )
    .with_venue(Venue::Bybit, Product::LinearUsdt)
}

fn decode_string_error(label: &str, error: impl std::fmt::Display) -> MarketError {
    MarketError::new(
        ErrorKind::DecodeError,
        format!("failed to decode {label}: {error}"),
    )
    .with_venue(Venue::Bybit, Product::LinearUsdt)
}

fn exchange_reject(code: i64, message: &str) -> MarketError {
    MarketError::new(ErrorKind::ExchangeReject, message)
        .with_venue(Venue::Bybit, Product::LinearUsdt)
        .with_native_code(code.to_string())
}

fn now_timestamp_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i128::from(i64::MAX) as u128) as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::BybitLinearFuturesAdapter;
    use bat_markets_core::{
        FetchOhlcvRequest, InstrumentId, OrderStatus, PublicLaneEvent, TimestampMs, VenueAdapter,
    };

    const EXECUTION_HISTORY: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/bybit/execution_history.json"
    ));
    const ORDER_HISTORY: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/bybit/order_history.json"
    ));

    #[test]
    fn parse_bybit_execution_history_snapshot() {
        let adapter = BybitLinearFuturesAdapter::new();
        let executions = adapter
            .parse_executions_snapshot(EXECUTION_HISTORY)
            .expect("bybit execution history fixture should parse");
        assert_eq!(executions.len(), 1);
        assert_eq!(executions[0].execution_id.to_string(), "bybit-exec-1");
    }

    #[test]
    fn parse_bybit_order_history_snapshot() {
        let adapter = BybitLinearFuturesAdapter::new();
        let orders = adapter
            .parse_order_history_snapshot(ORDER_HISTORY, TimestampMs::new(1))
            .expect("bybit order history fixture should parse");
        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].status, OrderStatus::Canceled);
    }

    #[test]
    fn parse_bybit_ohlcv_snapshot_sorts_ascending() {
        let adapter = BybitLinearFuturesAdapter::new();
        let klines = adapter
            .parse_ohlcv_snapshot(
                r#"{
                    "retCode":0,
                    "retMsg":"OK",
                    "result":{
                        "list":[
                            ["1710000060000","64050.0","64150.0","64000.0","64100.0","23.456","0"],
                            ["1710000000000","64000.1","64100.0","63950.0","64050.0","12.345","0"]
                        ]
                    }
                }"#,
                &FetchOhlcvRequest::for_instrument(
                    InstrumentId::from("BTC/USDT:USDT"),
                    "1m",
                    None,
                    None,
                    Some(2),
                ),
            )
            .expect("bybit klines snapshot should parse");

        assert_eq!(klines.len(), 2);
        assert_eq!(klines[0].open_time, TimestampMs::new(1710000000000));
        assert_eq!(klines[1].close.to_string(), "64100.00");
    }

    #[test]
    fn parse_bybit_public_kline_without_symbol_uses_topic_suffix() {
        let adapter = BybitLinearFuturesAdapter::new();
        let events = adapter
            .parse_public(
                r#"{
                    "topic":"kline.1.BTCUSDT",
                    "type":"snapshot",
                    "ts":1710000005000,
                    "data":[
                        {
                            "start":1710000000000,
                            "end":1710000059999,
                            "interval":"1",
                            "open":"64000.0",
                            "close":"64010.0",
                            "high":"64020.0",
                            "low":"63990.0",
                            "volume":"12.0",
                            "confirm":false
                        }
                    ]
                }"#,
            )
            .expect("kline payload without symbol should still parse");

        assert_eq!(events.len(), 1);
        let PublicLaneEvent::Kline(kline) = &events[0] else {
            panic!("expected kline event");
        };
        assert_eq!(kline.instrument_id, InstrumentId::from("BTC/USDT:USDT"));
        assert_eq!(kline.interval.as_ref(), "1");
    }
}
