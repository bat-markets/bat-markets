//! MEXC USDT-M futures adapter.

pub mod native;

use std::sync::Arc;

use parking_lot::RwLock;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::Value;

use bat_markets_core::{
    AccountSnapshot, AggressorSide, AssetCode, Balance, BatMarketsConfig, CapabilitySet,
    ClientOrderId, CommandOperation, CommandReceipt, CommandStatus, ErrorKind, Execution,
    FastKline, FastOrderBookDelta, FastTicker, FastTrade, FundingRate, InstrumentCatalog,
    InstrumentId, InstrumentSpec, InstrumentStatus, InstrumentSupport, Kline, KlineInterval,
    Leverage, Liquidity, MarginMode, MarketError, MarketType, Notional, OpenInterest, Order,
    OrderId, OrderStatus, OrderType, Position, PositionDirection, PositionId, PositionMode, Price,
    PrivateLaneEvent, Product, PublicLaneEvent, Quantity, Rate, RequestId, Result, Side, Ticker,
    TimeInForce, TimestampMs, TradeId, Venue, VenueAdapter,
};

/// MEXC USDT-M futures adapter with fixture-backed protocol parsing.
#[derive(Clone, Debug)]
pub struct MexcLinearFuturesAdapter {
    config: BatMarketsConfig,
    capabilities: CapabilitySet,
    lane_set: bat_markets_core::LaneSet,
    instruments: Arc<RwLock<InstrumentCatalog>>,
}

impl Default for MexcLinearFuturesAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl MexcLinearFuturesAdapter {
    #[must_use]
    pub fn new() -> Self {
        Self::with_config(BatMarketsConfig::new(Venue::Mexc, Product::LinearUsdt))
    }

    #[must_use]
    pub fn with_config(config: BatMarketsConfig) -> Self {
        Self {
            config,
            capabilities: mexc_capabilities(),
            lane_set: bat_markets_core::LaneSet::linear_futures_defaults(),
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
            decode_message(format!("failed to parse mexc public payload: {error}"))
        })
    }

    pub fn parse_metadata_snapshot(&self, payload: &str) -> Result<Vec<InstrumentSpec>> {
        let response =
            serde_json::from_str::<native::RestResponse<Vec<native::ContractInfo>>>(payload)
                .map_err(|error| {
                    decode_message(format!(
                        "failed to parse mexc contract detail response: {error}"
                    ))
                })?;
        ensure_success(response.success, response.code, response.message.as_deref())?;

        response
            .data
            .into_iter()
            .filter(|contract| contract.quote_coin == "USDT" && contract.settle_coin == "USDT")
            .map(contract_to_spec)
            .collect()
    }

    pub fn parse_server_time(&self, payload: &str) -> Result<TimestampMs> {
        let response =
            serde_json::from_str::<native::ServerTimeResponse>(payload).map_err(|error| {
                decode_message(format!(
                    "failed to parse mexc server-time response: {error}"
                ))
            })?;
        Ok(TimestampMs::new(response.data))
    }

    pub fn parse_ticker_snapshot(&self, payload: &str, spec: &InstrumentSpec) -> Result<Ticker> {
        let response = serde_json::from_str::<native::RestResponse<Value>>(payload)
            .map_err(|error| decode_message(format!("failed to parse mexc ticker: {error}")))?;
        ensure_success(response.success, response.code, response.message.as_deref())?;
        let data = find_symbol_object(response.data, spec.native_symbol.as_ref())?;
        let ticker = serde_json::from_value::<native::TickerData>(data).map_err(|error| {
            decode_message(format!("failed to decode mexc ticker data: {error}"))
        })?;
        ticker_to_unified(ticker, spec)
    }

    pub fn parse_tickers_snapshot(
        &self,
        payload: &str,
        specs: &[InstrumentSpec],
    ) -> Result<Vec<Ticker>> {
        let response = serde_json::from_str::<native::RestResponse<Value>>(payload)
            .map_err(|error| decode_message(format!("failed to parse mexc tickers: {error}")))?;
        ensure_success(response.success, response.code, response.message.as_deref())?;
        let items = if let Some(items) = response.data.as_array() {
            items.clone()
        } else if response.data.is_object() {
            vec![response.data.clone()]
        } else {
            Vec::new()
        };
        items
            .into_iter()
            .filter_map(|item| {
                let ticker = serde_json::from_value::<native::TickerData>(item).ok()?;
                let spec = specs
                    .iter()
                    .find(|spec| spec.native_symbol.as_ref() == ticker.symbol.as_str())?;
                Some(ticker_to_unified(ticker, spec))
            })
            .collect()
    }

    pub fn parse_trades_snapshot(
        &self,
        payload: &str,
        spec: &InstrumentSpec,
    ) -> Result<Vec<bat_markets_core::TradeTick>> {
        let response = serde_json::from_str::<native::RestResponse<Vec<native::DealData>>>(payload)
            .map_err(|error| {
                decode_message(format!("failed to parse mexc deals response: {error}"))
            })?;
        ensure_success(response.success, response.code, response.message.as_deref())?;
        response
            .data
            .into_iter()
            .enumerate()
            .map(|(index, deal)| trade_to_unified(deal, spec, index))
            .collect()
    }

    pub fn parse_order_book_snapshot(
        &self,
        payload: &str,
        spec: &InstrumentSpec,
    ) -> Result<bat_markets_core::OrderBookSnapshot> {
        let response = serde_json::from_str::<native::RestResponse<native::DepthData>>(payload)
            .map_err(|error| {
                decode_message(format!("failed to parse mexc order book response: {error}"))
            })?;
        ensure_success(response.success, response.code, response.message.as_deref())?;
        Ok(bat_markets_core::OrderBookSnapshot {
            instrument_id: spec.instrument_id.clone(),
            bids: response
                .data
                .bids
                .into_iter()
                .map(level_to_book_level)
                .collect::<Result<Vec<_>>>()?,
            asks: response
                .data
                .asks
                .into_iter()
                .map(level_to_book_level)
                .collect::<Result<Vec<_>>>()?,
            event_time: TimestampMs::new(response.data.timestamp.unwrap_or_else(now_ms)),
        })
    }

    pub fn parse_ohlcv_snapshot(
        &self,
        payload: &str,
        request: &bat_markets_core::FetchOhlcvRequest,
    ) -> Result<Vec<Kline>> {
        let instrument_id = request.single_instrument_id()?.clone();
        self.resolve_instrument(&instrument_id).ok_or_else(|| {
            MarketError::new(
                ErrorKind::Unsupported,
                format!("unknown mexc instrument {instrument_id}"),
            )
        })?;
        let interval = parse_kline_interval(&request.interval)?;
        let response = serde_json::from_str::<native::RestResponse<native::KlineRestData>>(payload)
            .map_err(|error| {
                decode_message(format!("failed to parse mexc kline response: {error}"))
            })?;
        ensure_success(response.success, response.code, response.message.as_deref())?;

        let len = response.data.time.len();
        if response.data.open.len() != len
            || response.data.close.len() != len
            || response.data.high.len() != len
            || response.data.low.len() != len
            || response.data.vol.len() != len
        {
            return Err(decode_message(
                "mexc kline arrays have inconsistent lengths".to_owned(),
            ));
        }

        (0..len)
            .map(|index| {
                let open_time = response.data.time[index] * 1_000;
                Ok(Kline {
                    instrument_id: instrument_id.clone(),
                    interval: interval.into(),
                    open: Price::new(decimal_from_value(&response.data.open[index])?),
                    high: Price::new(decimal_from_value(&response.data.high[index])?),
                    low: Price::new(decimal_from_value(&response.data.low[index])?),
                    close: Price::new(decimal_from_value(&response.data.close[index])?),
                    volume: Quantity::new(decimal_from_value(&response.data.vol[index])?),
                    open_time: TimestampMs::new(open_time),
                    close_time: TimestampMs::new(
                        interval.close_time_ms(open_time).unwrap_or(open_time),
                    ),
                    closed: true,
                })
            })
            .collect()
    }

    pub fn parse_funding_rate_snapshot(
        &self,
        payload: &str,
        spec: &InstrumentSpec,
    ) -> Result<FundingRate> {
        let response =
            serde_json::from_str::<native::RestResponse<native::FundingRateData>>(payload)
                .map_err(|error| {
                    decode_message(format!("failed to parse mexc funding rate: {error}"))
                })?;
        ensure_success(response.success, response.code, response.message.as_deref())?;
        Ok(FundingRate {
            instrument_id: spec.instrument_id.clone(),
            value: Rate::new(decimal_from_value(&response.data.funding_rate)?),
            mark_price: None,
            event_time: TimestampMs::new(response.data.timestamp.unwrap_or_else(now_ms)),
        })
    }

    pub fn parse_open_interest_snapshot(
        &self,
        payload: &str,
        spec: &InstrumentSpec,
    ) -> Result<OpenInterest> {
        let response =
            serde_json::from_str::<native::RestResponse<Value>>(payload).map_err(|error| {
                decode_message(format!(
                    "failed to parse mexc open interest ticker: {error}"
                ))
            })?;
        ensure_success(response.success, response.code, response.message.as_deref())?;
        let data = find_symbol_object(response.data, spec.native_symbol.as_ref())?;
        let ticker = serde_json::from_value::<native::TickerData>(data).map_err(|error| {
            decode_message(format!(
                "failed to decode mexc open interest ticker data: {error}"
            ))
        })?;
        let value = ticker
            .hold_vol
            .as_ref()
            .map(decimal_from_value)
            .transpose()?
            .map(Quantity::new)
            .unwrap_or_else(|| Quantity::new(Decimal::ZERO));
        Ok(OpenInterest {
            instrument_id: spec.instrument_id.clone(),
            value,
            event_time: TimestampMs::new(ticker.timestamp.unwrap_or_else(now_ms)),
        })
    }

    pub fn parse_account_snapshot(
        &self,
        payload: &str,
        observed_at: TimestampMs,
    ) -> Result<AccountSnapshot> {
        let response = serde_json::from_str::<native::RestResponse<Vec<native::AssetData>>>(
            payload,
        )
        .map_err(|error| decode_message(format!("failed to parse mexc account assets: {error}")))?;
        ensure_success(response.success, response.code, response.message.as_deref())?;
        let mut total_wallet = Decimal::ZERO;
        let mut total_available = Decimal::ZERO;
        let mut total_unrealized = Decimal::ZERO;
        let balances = response
            .data
            .into_iter()
            .map(|asset| {
                let wallet = decimal_from_optional_value(
                    asset.wallet_balance.as_ref().or(asset.equity.as_ref()),
                )?;
                let available = decimal_from_optional_value(
                    asset
                        .available_balance
                        .as_ref()
                        .or(asset.available.as_ref()),
                )?;
                let unrealized = decimal_from_optional_value(
                    asset.unrealized.as_ref().or(asset.unrealised_value()),
                )?;
                total_wallet += wallet;
                total_available += available;
                total_unrealized += unrealized;
                Ok(Balance {
                    asset: AssetCode::from(asset.currency),
                    wallet_balance: bat_markets_core::Amount::new(wallet),
                    available_balance: bat_markets_core::Amount::new(available),
                    updated_at: observed_at,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(AccountSnapshot {
            balances,
            summary: Some(bat_markets_core::AccountSummary {
                total_wallet_balance: bat_markets_core::Amount::new(total_wallet),
                total_available_balance: bat_markets_core::Amount::new(total_available),
                total_unrealized_pnl: bat_markets_core::Amount::new(total_unrealized),
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
            serde_json::from_str::<native::RestResponse<Vec<native::PositionData>>>(payload)
                .map_err(|error| {
                    decode_message(format!("failed to parse mexc positions: {error}"))
                })?;
        ensure_success(response.success, response.code, response.message.as_deref())?;
        response
            .data
            .into_iter()
            .filter_map(
                |position| match decimal_from_optional_value(position.hold_vol.as_ref()) {
                    Ok(size) if size.is_zero() => None,
                    Ok(size) => Some(self.position_from_data(position, observed_at, size)),
                    Err(error) => Some(Err(error)),
                },
            )
            .collect()
    }

    pub fn parse_open_orders_snapshot(
        &self,
        payload: &str,
        observed_at: TimestampMs,
    ) -> Result<Vec<Order>> {
        parse_order_list_payload(payload)?
            .into_iter()
            .map(|order| self.order_from_data(order, observed_at))
            .collect()
    }

    pub fn parse_order_snapshot(&self, payload: &str, observed_at: TimestampMs) -> Result<Order> {
        let response = serde_json::from_str::<native::RestResponse<native::OrderData>>(payload)
            .map_err(|error| decode_message(format!("failed to parse mexc order: {error}")))?;
        ensure_success(response.success, response.code, response.message.as_deref())?;
        self.order_from_data(response.data, observed_at)
    }

    pub fn parse_executions_snapshot(&self, payload: &str) -> Result<Vec<Execution>> {
        let response =
            serde_json::from_str::<native::RestResponse<Vec<native::ExecutionData>>>(payload)
                .map_err(|error| {
                    decode_message(format!("failed to parse mexc executions: {error}"))
                })?;
        ensure_success(response.success, response.code, response.message.as_deref())?;
        response
            .data
            .into_iter()
            .map(|execution| self.execution_from_data(execution))
            .collect()
    }

    fn position_from_data(
        &self,
        position: native::PositionData,
        observed_at: TimestampMs,
        size: Decimal,
    ) -> Result<Position> {
        let spec = require_native_symbol(self, &position.symbol)?;
        Ok(Position {
            position_id: PositionId::from(value_to_id_string(&position.position_id)),
            instrument_id: spec.instrument_id,
            direction: parse_position_direction(position.position_type),
            size: Quantity::new(size),
            entry_price: decimal_from_optional_value(position.open_avg_price.as_ref())
                .ok()
                .filter(|value| !value.is_zero())
                .map(Price::new),
            mark_price: decimal_from_optional_value(position.mark_price.as_ref())
                .ok()
                .map(Price::new),
            unrealized_pnl: decimal_from_optional_value(
                position
                    .unrealized
                    .as_ref()
                    .or(position.unrealised.as_ref()),
            )
            .ok()
            .map(bat_markets_core::Amount::new),
            leverage: decimal_from_optional_value(position.leverage.as_ref())
                .ok()
                .map(Leverage::new),
            margin_mode: parse_margin_mode(position.open_type),
            position_mode: parse_position_mode(position.position_mode),
            updated_at: TimestampMs::new(
                position
                    .update_time
                    .or(position.create_time)
                    .unwrap_or(observed_at.value()),
            ),
        })
    }

    fn order_from_data(&self, order: native::OrderData, observed_at: TimestampMs) -> Result<Order> {
        let spec = require_native_symbol(self, &order.symbol)?;
        Ok(Order {
            order_id: OrderId::from(value_to_id_string(&order.order_id)),
            client_order_id: order
                .external_oid
                .as_deref()
                .filter(|value| !value.is_empty())
                .map(ClientOrderId::from),
            instrument_id: spec.instrument_id,
            side: parse_order_side(order.side),
            order_type: parse_order_type(order.order_type.or(order.category).unwrap_or(1)),
            time_in_force: Some(parse_time_in_force(order.order_type.or(order.category))),
            status: parse_order_status(order.state),
            price: Some(Price::new(decimal_from_value(&order.price)?)),
            quantity: Quantity::new(decimal_from_value(&order.vol)?),
            filled_quantity: Quantity::new(decimal_from_optional_value(order.deal_vol.as_ref())?),
            average_fill_price: decimal_from_optional_value(order.deal_avg_price.as_ref())
                .ok()
                .filter(|value| !value.is_zero())
                .map(Price::new),
            reduce_only: order.reduce_only.unwrap_or(matches!(order.side, 2 | 4)),
            post_only: matches!(order.order_type.or(order.category), Some(2)),
            created_at: TimestampMs::new(order.create_time.unwrap_or(observed_at.value())),
            updated_at: TimestampMs::new(order.update_time.unwrap_or(observed_at.value())),
            venue_status: Some(order.state.to_string().into()),
        })
    }

    fn execution_from_data(&self, execution: native::ExecutionData) -> Result<Execution> {
        let spec = require_native_symbol(self, &execution.symbol)?;
        Ok(Execution {
            execution_id: TradeId::from(value_to_id_string(&execution.id)),
            order_id: OrderId::from(value_to_id_string(&execution.order_id)),
            client_order_id: None,
            instrument_id: spec.instrument_id,
            side: parse_order_side(execution.side),
            quantity: Quantity::new(decimal_from_value(&execution.vol)?),
            price: Price::new(decimal_from_value(&execution.price)?),
            fee: execution
                .fee
                .as_ref()
                .map(decimal_from_value)
                .transpose()?
                .map(bat_markets_core::Amount::new),
            fee_asset: execution.fee_currency.map(AssetCode::from),
            liquidity: execution.is_taker.map(|is_taker| {
                if is_taker {
                    Liquidity::Taker
                } else {
                    Liquidity::Maker
                }
            }),
            executed_at: TimestampMs::new(
                execution
                    .timestamp
                    .as_ref()
                    .and_then(value_to_i64)
                    .unwrap_or_else(now_ms),
            ),
        })
    }
}

impl VenueAdapter for MexcLinearFuturesAdapter {
    fn venue(&self) -> Venue {
        Venue::Mexc
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
        self.instruments.read().all().to_vec()
    }

    fn resolve_instrument(&self, instrument_id: &InstrumentId) -> Option<InstrumentSpec> {
        self.instruments.read().get(instrument_id)
    }

    fn resolve_native_symbol(&self, native_symbol: &str) -> Option<InstrumentSpec> {
        self.instruments.read().by_native_symbol(native_symbol)
    }

    fn parse_public(&self, payload: &str) -> Result<Vec<PublicLaneEvent>> {
        let envelope = self.parse_native_public(payload)?;
        match envelope.channel.as_str() {
            "push.ticker" => {
                let ticker = serde_json::from_value::<native::TickerData>(envelope.data).map_err(
                    |error| decode_message(format!("failed to decode mexc ticker ws: {error}")),
                )?;
                let spec = require_native_symbol(self, &ticker.symbol)?;
                Ok(vec![PublicLaneEvent::Ticker(fast_ticker(ticker, &spec)?)])
            }
            "push.tickers" => {
                let tickers = serde_json::from_value::<Vec<native::TickerData>>(envelope.data)
                    .map_err(|error| {
                        decode_message(format!("failed to decode mexc tickers ws: {error}"))
                    })?;
                tickers
                    .into_iter()
                    .map(|ticker| {
                        let spec = require_native_symbol(self, &ticker.symbol)?;
                        Ok(PublicLaneEvent::Ticker(fast_ticker(ticker, &spec)?))
                    })
                    .collect()
            }
            "push.deal" => {
                let symbol = envelope.symbol.as_deref().ok_or_else(|| {
                    decode_message("mexc deal websocket payload missing symbol".to_owned())
                })?;
                let spec = require_native_symbol(self, symbol)?;
                decode_mexc_ws_deals(envelope.data)?
                    .into_iter()
                    .enumerate()
                    .map(|(index, deal)| {
                        Ok(PublicLaneEvent::Trade(fast_trade(deal, &spec, index)?))
                    })
                    .collect()
            }
            "push.depth" | "push.depth.full" => {
                let depth = serde_json::from_value::<native::DepthData>(envelope.data).map_err(
                    |error| decode_message(format!("failed to decode mexc depth ws: {error}")),
                )?;
                let symbol = envelope.symbol.as_deref().ok_or_else(|| {
                    decode_message("mexc depth websocket payload missing symbol".to_owned())
                })?;
                let spec = require_native_symbol(self, symbol)?;
                Ok(vec![PublicLaneEvent::OrderBookDelta(FastOrderBookDelta {
                    instrument_id: spec.instrument_id.clone(),
                    bids: depth
                        .bids
                        .into_iter()
                        .map(|level| fast_book_tuple(level, &spec))
                        .collect::<Result<Vec<_>>>()?,
                    asks: depth
                        .asks
                        .into_iter()
                        .map(|level| fast_book_tuple(level, &spec))
                        .collect::<Result<Vec<_>>>()?,
                    event_time: TimestampMs::new(
                        depth.timestamp.or(envelope.ts).unwrap_or_else(now_ms),
                    ),
                })])
            }
            "push.kline" => {
                let kline = serde_json::from_value::<native::KlineData>(envelope.data).map_err(
                    |error| decode_message(format!("failed to decode mexc kline ws: {error}")),
                )?;
                let symbol = kline
                    .symbol
                    .as_deref()
                    .or(envelope.symbol.as_deref())
                    .ok_or_else(|| {
                        decode_message("mexc kline websocket payload missing symbol".to_owned())
                    })?;
                let spec = require_native_symbol(self, symbol)?;
                Ok(vec![PublicLaneEvent::Kline(fast_kline(kline, &spec)?)])
            }
            "pong" | "rs.sub.ticker" | "rs.sub.tickers" | "rs.sub.deal" | "rs.sub.depth"
            | "rs.sub.kline" => Ok(Vec::new()),
            other => Err(MarketError::new(
                ErrorKind::Unsupported,
                format!("unsupported mexc public channel '{other}'"),
            )
            .with_venue(Venue::Mexc, Product::LinearUsdt)),
        }
    }

    fn parse_private(&self, payload: &str) -> Result<Vec<PrivateLaneEvent>> {
        let envelope = self.parse_native_public(payload)?;
        match envelope.channel.as_str() {
            "push.personal.asset" => {
                let asset = serde_json::from_value::<native::AssetData>(envelope.data).map_err(
                    |error| decode_message(format!("failed to decode mexc asset ws: {error}")),
                )?;
                Ok(vec![PrivateLaneEvent::Balance(Balance {
                    asset: AssetCode::from(asset.currency),
                    wallet_balance: bat_markets_core::Amount::new(decimal_from_optional_value(
                        asset.wallet_balance.as_ref().or(asset.equity.as_ref()),
                    )?),
                    available_balance: bat_markets_core::Amount::new(decimal_from_optional_value(
                        asset
                            .available_balance
                            .as_ref()
                            .or(asset.available.as_ref()),
                    )?),
                    updated_at: TimestampMs::new(envelope.ts.unwrap_or_else(now_ms)),
                })])
            }
            "push.personal.position" => {
                let position = serde_json::from_value::<native::PositionData>(envelope.data)
                    .map_err(|error| {
                        decode_message(format!("failed to decode mexc position ws: {error}"))
                    })?;
                let size = decimal_from_optional_value(position.hold_vol.as_ref())?;
                Ok(vec![PrivateLaneEvent::Position(self.position_from_data(
                    position,
                    TimestampMs::new(envelope.ts.unwrap_or_else(now_ms)),
                    size,
                )?)])
            }
            "push.personal.order" => {
                let order = serde_json::from_value::<native::OrderData>(envelope.data).map_err(
                    |error| decode_message(format!("failed to decode mexc order ws: {error}")),
                )?;
                Ok(vec![PrivateLaneEvent::Order(self.order_from_data(
                    order,
                    TimestampMs::new(envelope.ts.unwrap_or_else(now_ms)),
                )?)])
            }
            "push.personal.order.deal" => {
                let execution = serde_json::from_value::<native::ExecutionData>(envelope.data)
                    .map_err(|error| {
                        decode_message(format!("failed to decode mexc execution ws: {error}"))
                    })?;
                Ok(vec![PrivateLaneEvent::Execution(
                    self.execution_from_data(execution)?,
                )])
            }
            "rs.login" | "pong" => Ok(Vec::new()),
            other => Err(MarketError::new(
                ErrorKind::Unsupported,
                format!("unsupported mexc private channel '{other}'"),
            )
            .with_venue(Venue::Mexc, Product::LinearUsdt)),
        }
    }

    fn classify_command(
        &self,
        operation: CommandOperation,
        payload: Option<&str>,
        request_id: Option<RequestId>,
    ) -> Result<CommandReceipt> {
        let Some(payload) = payload else {
            return Ok(command_receipt(MexcCommandReceipt {
                operation,
                status: CommandStatus::UnknownExecution,
                instrument_id: None,
                order_id: None,
                request_id,
                message: Some("command outcome requires reconcile".into()),
                native_code: None,
                retriable: true,
            }));
        };
        let value = serde_json::from_str::<Value>(payload).map_err(|error| {
            decode_message(format!("failed to parse mexc command payload: {error}"))
        })?;
        let success = value
            .get("success")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let code = value.get("code").and_then(value_to_i64).unwrap_or_default();
        let message = value
            .get("message")
            .and_then(Value::as_str)
            .or_else(|| value.get("msg").and_then(Value::as_str))
            .map(Box::<str>::from);
        let item_error = mexc_command_item_error(value.get("data"));
        let item_error_code = item_error
            .as_ref()
            .map(|(error_code, _)| *error_code)
            .unwrap_or(code);
        let order_id = mexc_command_order_id(value.get("data"));
        Ok(command_receipt(MexcCommandReceipt {
            operation,
            status: if success && code == 0 && item_error.is_none() {
                CommandStatus::Accepted
            } else {
                CommandStatus::Rejected
            },
            instrument_id: None,
            order_id,
            request_id,
            message: item_error
                .as_ref()
                .and_then(|(_, error_msg)| error_msg.clone())
                .or(message)
                .or_else(|| Some(if success { "accepted" } else { "rejected" }.into())),
            native_code: Some(item_error_code.to_string().into()),
            retriable: matches!(item_error_code, 500 | 501 | 510 | 603 | 2037),
        }))
    }
}

trait AssetDataExt {
    fn unrealised_value(&self) -> Option<&Value>;
}

impl AssetDataExt for native::AssetData {
    fn unrealised_value(&self) -> Option<&Value> {
        self.unrealized.as_ref()
    }
}

fn mexc_capabilities() -> CapabilitySet {
    let mut capabilities = CapabilitySet::linear_futures_defaults();
    capabilities.market.liquidations = false;
    capabilities.trade.create = true;
    capabilities.trade.batch_create = true;
    capabilities.trade.amend = false;
    capabilities.trade.cancel = true;
    capabilities.trade.batch_cancel = true;
    capabilities.trade.cancel_all = true;
    capabilities.trade.validate = false;
    capabilities.position.leverage_set = true;
    capabilities.position.margin_mode_set = true;
    capabilities.native.ws_order_entry = false;
    capabilities.native.special_orders = false;
    capabilities
}

fn contract_to_spec(contract: native::ContractInfo) -> Result<InstrumentSpec> {
    let tick_size = decimal_from_value(&contract.price_unit)?;
    let step_size = decimal_from_value(&contract.vol_unit)?;
    let min_qty = decimal_from_value(&contract.min_vol)?;
    let contract_size = decimal_from_value(&contract.contract_size)?;
    let price_scale = contract.price_scale;
    let qty_scale = contract.vol_scale;
    let quote_scale = contract
        .amount_scale
        .unwrap_or_else(|| price_scale.saturating_add(qty_scale));
    Ok(InstrumentSpec {
        venue: Venue::Mexc,
        product: Product::LinearUsdt,
        market_type: MarketType::LinearPerpetual,
        instrument_id: InstrumentId::from(canonical_symbol(
            &contract.base_coin,
            &contract.quote_coin,
            &contract.settle_coin,
        )),
        canonical_symbol: canonical_symbol(
            &contract.base_coin,
            &contract.quote_coin,
            &contract.settle_coin,
        )
        .into(),
        native_symbol: contract.symbol.into(),
        base: AssetCode::from(contract.base_coin),
        quote: AssetCode::from(contract.quote_coin),
        settle: AssetCode::from(contract.settle_coin),
        contract_size: Quantity::new(contract_size),
        tick_size: Price::new(tick_size),
        step_size: Quantity::new(step_size),
        min_qty: Quantity::new(min_qty),
        min_notional: Notional::new(tick_size * min_qty * contract_size),
        price_scale,
        qty_scale,
        quote_scale,
        max_leverage: contract
            .max_leverage
            .as_ref()
            .map(decimal_from_value)
            .transpose()?
            .map(Leverage::new),
        support: InstrumentSupport {
            public_streams: true,
            private_trading: contract.api_allowed.unwrap_or(false),
            leverage_set: false,
            margin_mode_set: false,
            funding_rate: true,
            open_interest: true,
        },
        status: if contract.state.unwrap_or(0) == 0 {
            InstrumentStatus::Active
        } else {
            InstrumentStatus::Halted
        },
    })
}

fn ticker_to_unified(ticker: native::TickerData, spec: &InstrumentSpec) -> Result<Ticker> {
    Ok(Ticker {
        instrument_id: spec.instrument_id.clone(),
        last_price: Price::new(decimal_from_value(&ticker.last_price)?),
        mark_price: ticker
            .fair_price
            .as_ref()
            .map(decimal_from_value)
            .transpose()?
            .map(Price::new),
        index_price: ticker
            .index_price
            .as_ref()
            .map(decimal_from_value)
            .transpose()?
            .map(Price::new),
        volume_24h: ticker
            .volume24
            .as_ref()
            .map(decimal_from_value)
            .transpose()?
            .map(Quantity::new),
        turnover_24h: ticker
            .amount24
            .as_ref()
            .map(decimal_from_value)
            .transpose()?
            .map(Notional::new),
        event_time: TimestampMs::new(ticker.timestamp.unwrap_or_else(now_ms)),
    })
}

fn fast_ticker(ticker: native::TickerData, spec: &InstrumentSpec) -> Result<FastTicker> {
    let event_time = TimestampMs::new(ticker.timestamp.unwrap_or_else(now_ms));
    Ok(FastTicker {
        instrument_id: spec.instrument_id.clone(),
        last_price: Price::new(decimal_from_value(&ticker.last_price)?)
            .quantize(spec.price_scale)?,
        mark_price: ticker
            .fair_price
            .as_ref()
            .map(decimal_from_value)
            .transpose()?
            .map(Price::new)
            .map(|price| price.quantize(spec.price_scale))
            .transpose()?,
        index_price: ticker
            .index_price
            .as_ref()
            .map(decimal_from_value)
            .transpose()?
            .map(Price::new)
            .map(|price| price.quantize(spec.price_scale))
            .transpose()?,
        volume_24h: ticker
            .volume24
            .as_ref()
            .map(decimal_from_value)
            .transpose()?
            .map(Quantity::new)
            .map(|quantity| quantity.quantize(spec.qty_scale))
            .transpose()?,
        turnover_24h: ticker
            .amount24
            .as_ref()
            .map(decimal_from_value)
            .transpose()?
            .map(Notional::new)
            .map(|notional| notional.quantize(spec.quote_scale))
            .transpose()
            .unwrap_or(None),
        event_time,
    })
}

fn trade_to_unified(
    deal: native::DealData,
    spec: &InstrumentSpec,
    index: usize,
) -> Result<bat_markets_core::TradeTick> {
    Ok(bat_markets_core::TradeTick {
        instrument_id: spec.instrument_id.clone(),
        trade_id: TradeId::from(format!("{}-{}", deal.t.unwrap_or_else(now_ms), index)),
        price: Price::new(decimal_from_value(&deal.p)?),
        quantity: Quantity::new(decimal_from_value(&deal.v)?),
        aggressor_side: if deal.side == 1 {
            AggressorSide::Buyer
        } else {
            AggressorSide::Seller
        },
        event_time: TimestampMs::new(deal.t.unwrap_or_else(now_ms)),
    })
}

fn fast_trade(deal: native::DealData, spec: &InstrumentSpec, index: usize) -> Result<FastTrade> {
    let trade = trade_to_unified(deal, spec, index)?;
    Ok(FastTrade {
        instrument_id: trade.instrument_id,
        trade_id: trade.trade_id,
        price: trade.price.quantize(spec.price_scale)?,
        quantity: trade.quantity.quantize(spec.qty_scale)?,
        aggressor_side: trade.aggressor_side,
        event_time: trade.event_time,
    })
}

fn decode_mexc_ws_deals(value: Value) -> Result<Vec<native::DealData>> {
    if value.is_array() {
        serde_json::from_value::<Vec<native::DealData>>(value).map_err(|error| {
            decode_message(format!("failed to decode mexc deal ws array: {error}"))
        })
    } else {
        serde_json::from_value::<native::DealData>(value)
            .map(|deal| vec![deal])
            .map_err(|error| decode_message(format!("failed to decode mexc deal ws: {error}")))
    }
}

fn fast_kline(kline: native::KlineData, spec: &InstrumentSpec) -> Result<FastKline> {
    let interval = kline
        .interval
        .as_deref()
        .and_then(mexc_interval_to_core)
        .ok_or_else(|| {
            decode_message(format!(
                "unsupported or missing mexc kline interval '{}'",
                kline.interval.as_deref().unwrap_or("<missing>")
            ))
        })?;
    let open_time = kline.t.unwrap_or_else(|| now_ms() / 1_000) * 1_000;
    Ok(FastKline {
        instrument_id: spec.instrument_id.clone(),
        interval: interval.into(),
        open: Price::new(decimal_from_value(&kline.o)?).quantize(spec.price_scale)?,
        high: Price::new(decimal_from_value(&kline.h)?).quantize(spec.price_scale)?,
        low: Price::new(decimal_from_value(&kline.l)?).quantize(spec.price_scale)?,
        close: Price::new(decimal_from_value(&kline.c)?).quantize(spec.price_scale)?,
        volume: Quantity::new(decimal_from_optional_value(
            kline.a.as_ref().or(kline.q.as_ref()),
        )?)
        .quantize(spec.qty_scale)?,
        open_time: TimestampMs::new(open_time),
        close_time: TimestampMs::new(interval.close_time_ms(open_time).unwrap_or(open_time)),
        closed: false,
    })
}

fn level_to_book_level(level: Vec<Value>) -> Result<bat_markets_core::OrderBookLevel> {
    if level.len() < 2 {
        return Err(decode_message(
            "mexc depth level has fewer than two fields".to_owned(),
        ));
    }
    Ok(bat_markets_core::OrderBookLevel {
        price: Price::new(decimal_from_value(&level[0])?),
        quantity: Quantity::new(decimal_from_value(&level[1])?),
    })
}

fn fast_book_tuple(
    level: Vec<Value>,
    spec: &InstrumentSpec,
) -> Result<(bat_markets_core::FastPrice, bat_markets_core::FastQuantity)> {
    let level = level_to_book_level(level)?;
    Ok((
        level.price.quantize(spec.price_scale)?,
        level.quantity.quantize(spec.qty_scale)?,
    ))
}

fn parse_order_list_payload(payload: &str) -> Result<Vec<native::OrderData>> {
    #[derive(Deserialize)]
    struct Page {
        #[serde(default)]
        result_list: Vec<native::OrderData>,
    }
    let response = serde_json::from_str::<native::RestResponse<Value>>(payload)
        .map_err(|error| decode_message(format!("failed to parse mexc order list: {error}")))?;
    ensure_success(response.success, response.code, response.message.as_deref())?;
    if response.data.is_array() {
        return serde_json::from_value(response.data)
            .map_err(|error| decode_message(format!("failed to decode mexc order list: {error}")));
    }
    let page = serde_json::from_value::<Page>(response.data)
        .map_err(|error| decode_message(format!("failed to decode mexc order page: {error}")))?;
    Ok(page.result_list)
}

fn find_symbol_object(data: Value, native_symbol: &str) -> Result<Value> {
    if let Some(items) = data.as_array() {
        return items
            .iter()
            .find(|item| item.get("symbol").and_then(Value::as_str) == Some(native_symbol))
            .cloned()
            .ok_or_else(|| {
                decode_message(format!("mexc ticker response missing {native_symbol}"))
            });
    }
    Ok(data)
}

fn parse_kline_interval(raw: &str) -> Result<KlineInterval> {
    KlineInterval::parse(raw)
        .or_else(|| mexc_interval_to_core(raw))
        .ok_or_else(|| {
            MarketError::new(
                ErrorKind::Unsupported,
                format!("unsupported mexc interval '{raw}'"),
            )
        })
}

fn mexc_interval_to_core(raw: &str) -> Option<KlineInterval> {
    match raw {
        "Min1" => Some(KlineInterval::Minute1),
        "Min5" => Some(KlineInterval::Minute5),
        "Min15" => Some(KlineInterval::Minute15),
        "Min30" => Some(KlineInterval::Minute30),
        "Min60" => Some(KlineInterval::Hour1),
        "Hour4" => Some(KlineInterval::Hour4),
        "Day1" => Some(KlineInterval::Day1),
        "Week1" => Some(KlineInterval::Week1),
        "Month1" => Some(KlineInterval::Month1),
        _ => None,
    }
}

fn parse_order_side(value: i64) -> Side {
    match value {
        1 | 4 => Side::Buy,
        _ => Side::Sell,
    }
}

fn parse_position_direction(value: i64) -> PositionDirection {
    match value {
        1 => PositionDirection::Long,
        2 => PositionDirection::Short,
        _ => PositionDirection::Flat,
    }
}

fn parse_order_type(value: i64) -> OrderType {
    match value {
        5 | 6 => OrderType::Market,
        _ => OrderType::Limit,
    }
}

fn parse_time_in_force(value: Option<i64>) -> TimeInForce {
    match value {
        Some(2) => TimeInForce::PostOnly,
        Some(3) => TimeInForce::Ioc,
        Some(4) => TimeInForce::Fok,
        _ => TimeInForce::Gtc,
    }
}

fn parse_order_status(value: i64) -> OrderStatus {
    match value {
        1 | 2 => OrderStatus::New,
        3 => OrderStatus::Filled,
        4 => OrderStatus::Canceled,
        5 => OrderStatus::Rejected,
        _ => OrderStatus::Expired,
    }
}

fn parse_margin_mode(value: Option<i64>) -> MarginMode {
    match value {
        Some(1) => MarginMode::Isolated,
        _ => MarginMode::Cross,
    }
}

fn parse_position_mode(value: Option<i64>) -> PositionMode {
    match value {
        Some(1) => PositionMode::Hedge,
        _ => PositionMode::OneWay,
    }
}

struct MexcCommandReceipt {
    operation: CommandOperation,
    status: CommandStatus,
    instrument_id: Option<InstrumentId>,
    order_id: Option<OrderId>,
    request_id: Option<RequestId>,
    message: Option<Box<str>>,
    native_code: Option<Box<str>>,
    retriable: bool,
}

fn command_receipt(parts: MexcCommandReceipt) -> CommandReceipt {
    CommandReceipt {
        operation: parts.operation,
        status: parts.status,
        venue: Venue::Mexc,
        product: Product::LinearUsdt,
        instrument_id: parts.instrument_id,
        order_id: parts.order_id,
        client_order_id: None,
        request_id: parts.request_id,
        message: parts.message,
        native_code: parts.native_code,
        retriable: parts.retriable,
    }
}

fn require_native_symbol(
    adapter: &MexcLinearFuturesAdapter,
    symbol: &str,
) -> Result<InstrumentSpec> {
    adapter.resolve_native_symbol(symbol).ok_or_else(|| {
        MarketError::new(
            ErrorKind::Unsupported,
            format!("unknown mexc symbol {symbol}"),
        )
        .with_venue(Venue::Mexc, Product::LinearUsdt)
    })
}

fn decimal_from_optional_value(value: Option<&Value>) -> Result<Decimal> {
    value
        .map(decimal_from_value)
        .transpose()
        .map(|value| value.unwrap_or(Decimal::ZERO))
}

fn decimal_from_value(value: &Value) -> Result<Decimal> {
    match value {
        Value::Number(number) => decimal_from_str(&number.to_string()),
        Value::String(value) => decimal_from_str(value),
        Value::Null => Ok(Decimal::ZERO),
        other => decimal_from_str(&other.to_string()),
    }
    .map_err(|error| decode_message(format!("invalid mexc decimal '{value}': {error}")))
}

fn decimal_from_str(value: &str) -> std::result::Result<Decimal, String> {
    let Some((mantissa, exponent)) = value.split_once(['e', 'E']) else {
        return Decimal::from_str_exact(value).map_err(|error| error.to_string());
    };
    let mut parsed = Decimal::from_str_exact(mantissa).map_err(|error| error.to_string())?;
    let exponent = exponent
        .parse::<i32>()
        .map_err(|error| format!("invalid exponent: {error}"))?;
    let ten = Decimal::from(10);
    for _ in 0..exponent.unsigned_abs() {
        if exponent >= 0 {
            parsed *= ten;
        } else {
            parsed /= ten;
        }
    }
    Ok(parsed)
}

fn value_to_id_string(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        Value::Null => String::new(),
        value => value.to_string(),
    }
}

fn value_to_i64(value: &Value) -> Option<i64> {
    value.as_i64().or_else(|| value.as_str()?.parse().ok())
}

fn mexc_command_order_id(data: Option<&Value>) -> Option<OrderId> {
    let value = match data? {
        Value::Array(items) => items.first()?.get("orderId")?,
        Value::Object(map) => map.get("orderId").or_else(|| map.get("order_id"))?,
        value => value,
    };
    Some(value_to_id_string(value))
        .filter(|id| !id.is_empty() && id != "null")
        .map(OrderId::from)
}

fn mexc_command_item_error(data: Option<&Value>) -> Option<(i64, Option<Box<str>>)> {
    let item = match data? {
        Value::Array(items) => items.first()?,
        Value::Object(_) => data?,
        _ => return None,
    };
    let error_code = item
        .get("errorCode")
        .or_else(|| item.get("error_code"))
        .and_then(value_to_i64)?;
    if error_code == 0 {
        return None;
    }
    let error_msg = item
        .get("errorMsg")
        .or_else(|| item.get("error_msg"))
        .and_then(Value::as_str)
        .map(Box::<str>::from);
    Some((error_code, error_msg))
}

fn canonical_symbol(base: &str, quote: &str, settle: &str) -> String {
    format!("{base}/{quote}:{settle}")
}

fn ensure_success(success: bool, code: i64, message: Option<&str>) -> Result<()> {
    if success && code == 0 {
        return Ok(());
    }
    Err(MarketError::new(
        ErrorKind::ExchangeReject,
        message.unwrap_or("mexc request rejected"),
    )
    .with_venue(Venue::Mexc, Product::LinearUsdt))
}

fn decode_message(message: String) -> MarketError {
    MarketError::new(ErrorKind::DecodeError, message).with_venue(Venue::Mexc, Product::LinearUsdt)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

fn btc_spec() -> InstrumentSpec {
    fixture_spec("BTC", 2, 0, "0.1", "1", 125)
}

fn eth_spec() -> InstrumentSpec {
    fixture_spec("ETH", 2, 0, "0.01", "1", 100)
}

fn fixture_spec(
    base: &str,
    price_scale: u32,
    qty_scale: u32,
    tick: &str,
    step: &str,
    leverage: i64,
) -> InstrumentSpec {
    let native = format!("{base}_USDT");
    InstrumentSpec {
        venue: Venue::Mexc,
        product: Product::LinearUsdt,
        market_type: MarketType::LinearPerpetual,
        instrument_id: InstrumentId::from(canonical_symbol(base, "USDT", "USDT")),
        canonical_symbol: canonical_symbol(base, "USDT", "USDT").into(),
        native_symbol: native.into(),
        base: AssetCode::from(base),
        quote: AssetCode::from("USDT"),
        settle: AssetCode::from("USDT"),
        contract_size: Quantity::new(Decimal::ONE),
        tick_size: Price::new(Decimal::new(
            tick.replace('.', "").parse::<i64>().unwrap_or(1),
            decimal_places(tick),
        )),
        step_size: Quantity::new(Decimal::new(
            step.replace('.', "").parse::<i64>().unwrap_or(1),
            decimal_places(step),
        )),
        min_qty: Quantity::new(Decimal::new(
            step.replace('.', "").parse::<i64>().unwrap_or(1),
            decimal_places(step),
        )),
        min_notional: Notional::new(Decimal::new(
            tick.replace('.', "").parse::<i64>().unwrap_or(1),
            decimal_places(tick),
        )),
        price_scale,
        qty_scale,
        quote_scale: price_scale + qty_scale,
        max_leverage: Some(Leverage::new(Decimal::from(leverage))),
        support: InstrumentSupport {
            public_streams: true,
            private_trading: false,
            leverage_set: false,
            margin_mode_set: false,
            funding_rate: true,
            open_interest: true,
        },
        status: InstrumentStatus::Active,
    }
}

fn decimal_places(value: &str) -> u32 {
    value
        .split_once('.')
        .map(|(_, fraction)| fraction.len() as u32)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bat_markets_core::{PrivateLaneEvent, PublicLaneEvent};

    const CONTRACT_DETAIL: &str = include_str!("../../../fixtures/mexc/contract_detail.json");
    const PUBLIC_TICKER: &str = include_str!("../../../fixtures/mexc/public_ticker.json");
    const PUBLIC_DEAL: &str = include_str!("../../../fixtures/mexc/public_deal.json");
    const PUBLIC_DEPTH: &str = include_str!("../../../fixtures/mexc/public_depth.json");
    const PUBLIC_KLINE: &str = include_str!("../../../fixtures/mexc/public_kline.json");
    const PRIVATE_ORDER: &str = include_str!("../../../fixtures/mexc/private_order.json");

    #[test]
    fn parse_mexc_contract_metadata() {
        let adapter = MexcLinearFuturesAdapter::new();
        let specs = adapter
            .parse_metadata_snapshot(CONTRACT_DETAIL)
            .unwrap_or_else(|error| panic!("mexc metadata should parse: {error}"));
        assert_eq!(specs[0].native_symbol.as_ref(), "BTC_USDT");
        assert_eq!(specs[0].instrument_id.as_ref(), "BTC/USDT:USDT");
        assert_eq!(specs[0].tick_size.to_string(), "0.1");
        assert!(!specs[0].support.private_trading);
    }

    #[test]
    fn parse_mexc_scientific_decimal_values() {
        let value = serde_json::json!(1e-8);
        let parsed = decimal_from_value(&value)
            .unwrap_or_else(|error| panic!("scientific decimal should parse: {error}"));
        assert_eq!(parsed, Decimal::new(1, 8));
    }

    #[test]
    fn parse_mexc_public_ticker_ws() {
        let adapter = MexcLinearFuturesAdapter::new();
        let events = adapter
            .parse_public(PUBLIC_TICKER)
            .unwrap_or_else(|error| panic!("mexc ticker should parse: {error}"));
        let PublicLaneEvent::Ticker(ticker) = &events[0] else {
            panic!("expected ticker");
        };
        assert_eq!(ticker.instrument_id.as_ref(), "BTC/USDT:USDT");
        assert_eq!(ticker.last_price.value(), 6543210);
    }

    #[test]
    fn parse_mexc_open_interest_uses_hold_volume() {
        let adapter = MexcLinearFuturesAdapter::new();
        let spec = adapter
            .resolve_native_symbol("BTC_USDT")
            .unwrap_or_else(|| panic!("mexc fixture symbol should exist"));
        let payload = r#"{
            "success": true,
            "code": 0,
            "data": {
                "symbol": "BTC_USDT",
                "lastPrice": 65432.1,
                "volume24": 1200,
                "holdVol": 4321,
                "timestamp": 1761879567135
            }
        }"#;
        let open_interest = adapter
            .parse_open_interest_snapshot(payload, &spec)
            .unwrap_or_else(|error| panic!("mexc open interest should parse: {error}"));
        assert_eq!(open_interest.value.value(), Decimal::new(4321, 0));
    }

    #[test]
    fn parse_mexc_public_deal_ws_array() {
        let adapter = MexcLinearFuturesAdapter::new();
        let events = adapter
            .parse_public(PUBLIC_DEAL)
            .unwrap_or_else(|error| panic!("mexc deal should parse: {error}"));
        let PublicLaneEvent::Trade(trade) = &events[0] else {
            panic!("expected trade");
        };
        assert_eq!(trade.instrument_id.as_ref(), "BTC/USDT:USDT");
        assert_eq!(trade.price.value(), 8104690);
        assert_eq!(trade.quantity.value(), 2);
    }

    #[test]
    fn parse_mexc_public_depth_ws() {
        let adapter = MexcLinearFuturesAdapter::new();
        let events = adapter
            .parse_public(PUBLIC_DEPTH)
            .unwrap_or_else(|error| panic!("mexc depth should parse: {error}"));
        let PublicLaneEvent::OrderBookDelta(delta) = &events[0] else {
            panic!("expected depth");
        };
        assert_eq!(delta.bids[0].0.value(), 6543200);
        assert_eq!(delta.asks[0].1.value(), 2);
    }

    #[test]
    fn parse_mexc_public_kline_ws() {
        let adapter = MexcLinearFuturesAdapter::new();
        let events = adapter
            .parse_public(PUBLIC_KLINE)
            .unwrap_or_else(|error| panic!("mexc kline should parse: {error}"));
        let PublicLaneEvent::Kline(kline) = &events[0] else {
            panic!("expected kline");
        };
        assert_eq!(kline.interval.as_ref(), "1m");
        assert_eq!(kline.close.value(), 6544500);
    }

    #[test]
    fn parse_mexc_private_order_ws() {
        let adapter = MexcLinearFuturesAdapter::new();
        let events = adapter
            .parse_private(PRIVATE_ORDER)
            .unwrap_or_else(|error| panic!("mexc private order should parse: {error}"));
        let PrivateLaneEvent::Order(order) = &events[0] else {
            panic!("expected order");
        };
        assert_eq!(order.order_id.as_ref(), "123456789");
        assert_eq!(order.status, OrderStatus::New);
        assert_eq!(order.side, Side::Buy);
    }

    #[test]
    fn mexc_write_capabilities_expose_documented_rest_order_paths() {
        let adapter = MexcLinearFuturesAdapter::new();
        let capabilities = adapter.capabilities();
        assert!(capabilities.trade.create);
        assert!(capabilities.trade.batch_create);
        assert!(capabilities.trade.cancel);
        assert!(capabilities.trade.batch_cancel);
        assert!(capabilities.trade.cancel_all);
        assert!(capabilities.position.leverage_set);
        assert!(capabilities.position.margin_mode_set);
        assert!(!capabilities.trade.amend);
        assert!(!capabilities.trade.validate);
        assert!(!capabilities.native.ws_order_entry);
        assert!(capabilities.trade.get);
        assert!(capabilities.market.public_streams);
    }

    #[test]
    fn mexc_command_classification_reads_nested_order_id_and_item_errors() {
        let adapter = MexcLinearFuturesAdapter::new();
        let accepted = adapter
            .classify_command(
                CommandOperation::CreateOrder,
                Some(r#"{"success":true,"code":0,"data":{"orderId":"739113577038255616","ts":1761888808839}}"#),
                None,
            )
            .unwrap_or_else(|error| panic!("mexc command should parse: {error}"));
        assert_eq!(accepted.status, CommandStatus::Accepted);
        assert_eq!(
            accepted.order_id.as_ref().map(OrderId::as_ref),
            Some("739113577038255616")
        );

        let rejected = adapter
            .classify_command(
                CommandOperation::CancelOrder,
                Some(r#"{"success":true,"code":0,"data":[{"orderId":101716841474621953,"errorCode":2040,"errorMsg":"order not exist"}]}"#),
                None,
            )
            .unwrap_or_else(|error| panic!("mexc cancel command should parse: {error}"));
        assert_eq!(rejected.status, CommandStatus::Rejected);
        assert_eq!(rejected.native_code.as_deref(), Some("2040"));
    }
}
