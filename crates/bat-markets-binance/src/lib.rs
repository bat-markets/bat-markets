//! Binance linear futures adapter.

pub mod native;

use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use parking_lot::RwLock;
use rust_decimal::Decimal;

use bat_markets_core::{
    AccountCapabilities, AccountSnapshot, AggressorSide, AssetCapabilities, AssetCode, Balance,
    BatMarketsConfig, CapabilitySet, CommandOperation, CommandReceipt, CommandStatus, ErrorKind,
    Execution, FastBookTop, FastKline, FastTicker, FastTrade, FetchOhlcvRequest,
    FetchTradesRequest, FundingRate, InstrumentCatalog, InstrumentId, InstrumentSpec,
    InstrumentStatus, InstrumentSupport, Kline, KlineInterval, Leverage, MarginMode,
    MarketCapabilities, MarketError, MarketType, NativeCapabilities, Notional, OpenInterest, Order,
    OrderId, OrderStatus, OrderType, Position, PositionCapabilities, PositionDirection, PositionId,
    PositionMode, Price, PrivateLaneEvent, Product, PublicLaneEvent, Quantity, Rate, RequestId,
    Result, Side, Ticker, TimeInForce, TimestampMs, TradeCapabilities, TradeId, Venue,
    VenueAdapter,
};

/// Binance linear futures adapter with a handwritten, fixture-backed contract.
#[derive(Clone, Debug)]
pub struct BinanceLinearFuturesAdapter {
    config: BatMarketsConfig,
    capabilities: CapabilitySet,
    lane_set: bat_markets_core::LaneSet,
    instruments: Arc<RwLock<InstrumentCatalog>>,
}

impl Default for BinanceLinearFuturesAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl BinanceLinearFuturesAdapter {
    #[must_use]
    pub fn new() -> Self {
        Self::with_config(BatMarketsConfig::new(Venue::Binance, Product::LinearUsdt))
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

    pub fn parse_native_public(&self, payload: &str) -> Result<native::PublicMessage> {
        serde_json::from_str(payload).map_err(|error| {
            MarketError::new(
                ErrorKind::DecodeError,
                format!("failed to parse binance public payload: {error}"),
            )
            .with_venue(Venue::Binance, Product::LinearUsdt)
            .with_operation("binance.parse_native_public")
        })
    }

    pub fn parse_native_private(&self, payload: &str) -> Result<native::PrivateMessage> {
        serde_json::from_str(payload).map_err(|error| {
            MarketError::new(
                ErrorKind::DecodeError,
                format!("failed to parse binance private payload: {error}"),
            )
            .with_venue(Venue::Binance, Product::LinearUsdt)
            .with_operation("binance.parse_native_private")
        })
    }

    pub fn parse_server_time(&self, payload: &str) -> Result<TimestampMs> {
        let response =
            serde_json::from_str::<native::ServerTimeResponse>(payload).map_err(|error| {
                MarketError::new(
                    ErrorKind::DecodeError,
                    format!("failed to parse binance server-time response: {error}"),
                )
            })?;
        Ok(TimestampMs::new(response.server_time))
    }

    pub fn parse_metadata_snapshot(&self, payload: &str) -> Result<Vec<InstrumentSpec>> {
        let response =
            serde_json::from_str::<native::ExchangeInfoResponse>(payload).map_err(|error| {
                MarketError::new(
                    ErrorKind::DecodeError,
                    format!("failed to parse binance exchangeInfo response: {error}"),
                )
                .with_venue(Venue::Binance, Product::LinearUsdt)
                .with_operation("binance.parse_metadata_snapshot")
            })?;

        let mut instruments = Vec::new();
        for symbol in response.symbols {
            if symbol.contract_type != "PERPETUAL" || symbol.quote_asset != "USDT" {
                continue;
            }

            let tick_size = require_filter_decimal(&symbol.filters, "PRICE_FILTER", |filter| {
                filter.tick_size.as_deref()
            })?;
            let step_size = require_filter_decimal(&symbol.filters, "LOT_SIZE", |filter| {
                filter.step_size.as_deref()
            })?;
            let min_qty = require_filter_decimal(&symbol.filters, "LOT_SIZE", |filter| {
                filter.min_qty.as_deref()
            })?;
            let min_notional = require_filter_decimal(&symbol.filters, "MIN_NOTIONAL", |filter| {
                filter.notional.as_deref()
            })?;

            let price_scale = decimal_scale(tick_size);
            let qty_scale = decimal_scale(step_size);
            let quote_scale = symbol
                .quote_precision
                .max(price_scale.saturating_add(qty_scale))
                .max(decimal_scale(min_notional));

            instruments.push(InstrumentSpec {
                venue: Venue::Binance,
                product: Product::LinearUsdt,
                market_type: MarketType::LinearPerpetual,
                instrument_id: InstrumentId::from(canonical_symbol(
                    &symbol.base_asset,
                    &symbol.quote_asset,
                    &symbol.margin_asset,
                )),
                canonical_symbol: canonical_symbol(
                    &symbol.base_asset,
                    &symbol.quote_asset,
                    &symbol.margin_asset,
                )
                .into(),
                native_symbol: symbol.symbol.into(),
                base: AssetCode::from(symbol.base_asset),
                quote: AssetCode::from(symbol.quote_asset),
                settle: AssetCode::from(symbol.margin_asset),
                contract_size: Quantity::new(Decimal::ONE),
                tick_size: Price::new(tick_size),
                step_size: Quantity::new(step_size),
                min_qty: Quantity::new(min_qty),
                min_notional: Notional::new(min_notional),
                price_scale,
                qty_scale,
                quote_scale,
                max_leverage: None,
                support: InstrumentSupport {
                    public_streams: true,
                    private_trading: true,
                    leverage_set: true,
                    margin_mode_set: true,
                    funding_rate: true,
                    open_interest: true,
                },
                status: parse_instrument_status(&symbol.status),
            });
        }

        Ok(instruments)
    }

    pub fn parse_account_snapshot(
        &self,
        payload: &str,
        observed_at: TimestampMs,
    ) -> Result<(AccountSnapshot, Vec<Position>)> {
        let response =
            serde_json::from_str::<native::AccountInfoResponse>(payload).map_err(|error| {
                MarketError::new(
                    ErrorKind::DecodeError,
                    format!("failed to parse binance account snapshot: {error}"),
                )
                .with_venue(Venue::Binance, Product::LinearUsdt)
                .with_operation("binance.parse_account_snapshot")
            })?;

        let balances = response
            .assets
            .into_iter()
            .map(|asset| {
                Ok(Balance {
                    asset: AssetCode::from(asset.asset),
                    wallet_balance: balance_amount(&asset.wallet_balance)?,
                    available_balance: balance_amount(&asset.available_balance)?,
                    updated_at: observed_at,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        let positions = response
            .positions
            .into_iter()
            .filter_map(|position| match parse_decimal(&position.position_amount) {
                Ok(size) if size.is_zero() => None,
                Ok(size) => Some(self.position_from_account_snapshot(position, observed_at, size)),
                Err(error) => Some(Err(error)),
            })
            .collect::<Result<Vec<_>>>()?;

        let account = AccountSnapshot {
            balances,
            summary: Some(bat_markets_core::AccountSummary {
                total_wallet_balance: balance_amount(&response.total_wallet_balance)?,
                total_available_balance: balance_amount(&response.available_balance)?,
                total_unrealized_pnl: balance_amount(&response.total_unrealized_profit)?,
                updated_at: observed_at,
            }),
        };

        Ok((account, positions))
    }

    pub fn parse_open_orders_snapshot(
        &self,
        payload: &str,
        observed_at: TimestampMs,
    ) -> Result<Vec<Order>> {
        let snapshots =
            serde_json::from_str::<Vec<native::OrderSnapshot>>(payload).map_err(|error| {
                MarketError::new(
                    ErrorKind::DecodeError,
                    format!("failed to parse binance open-orders snapshot: {error}"),
                )
                .with_venue(Venue::Binance, Product::LinearUsdt)
                .with_operation("binance.parse_open_orders_snapshot")
            })?;

        snapshots
            .into_iter()
            .map(|snapshot| self.order_from_snapshot(snapshot, observed_at))
            .collect()
    }

    pub fn parse_order_snapshot(&self, payload: &str, observed_at: TimestampMs) -> Result<Order> {
        let snapshot = serde_json::from_str::<native::OrderSnapshot>(payload).map_err(|error| {
            MarketError::new(
                ErrorKind::DecodeError,
                format!("failed to parse binance order snapshot: {error}"),
            )
            .with_venue(Venue::Binance, Product::LinearUsdt)
            .with_operation("binance.parse_order_snapshot")
        })?;
        self.order_from_snapshot(snapshot, observed_at)
    }

    pub fn parse_order_history_snapshot(
        &self,
        payload: &str,
        observed_at: TimestampMs,
    ) -> Result<Vec<Order>> {
        self.parse_open_orders_snapshot(payload, observed_at)
    }

    pub fn parse_executions_snapshot(&self, payload: &str) -> Result<Vec<Execution>> {
        let snapshots =
            serde_json::from_str::<Vec<native::UserTradeSnapshot>>(payload).map_err(|error| {
                MarketError::new(
                    ErrorKind::DecodeError,
                    format!("failed to parse binance user-trades snapshot: {error}"),
                )
                .with_venue(Venue::Binance, Product::LinearUsdt)
                .with_operation("binance.parse_executions_snapshot")
            })?;

        snapshots
            .into_iter()
            .map(|snapshot| self.execution_from_snapshot(snapshot))
            .collect()
    }

    pub fn parse_ticker_snapshot(
        &self,
        payload: &str,
        instrument_id: &InstrumentId,
    ) -> Result<Ticker> {
        let snapshot =
            serde_json::from_str::<native::TickerSnapshot>(payload).map_err(|error| {
                MarketError::new(
                    ErrorKind::DecodeError,
                    format!("failed to parse binance ticker snapshot: {error}"),
                )
                .with_venue(Venue::Binance, Product::LinearUsdt)
                .with_operation("binance.parse_ticker_snapshot")
            })?;
        let spec = self.resolve_instrument(instrument_id).ok_or_else(|| {
            MarketError::new(
                ErrorKind::Unsupported,
                format!("unsupported binance instrument '{}'", instrument_id),
            )
            .with_venue(Venue::Binance, Product::LinearUsdt)
        })?;

        Ok(FastTicker {
            instrument_id: spec.instrument_id.clone(),
            last_price: Price::new(parse_decimal(&snapshot.last_price)?)
                .quantize(spec.price_scale)?,
            mark_price: None,
            index_price: None,
            volume_24h: Some(
                Quantity::new(parse_decimal(&snapshot.volume)?).quantize(spec.qty_scale)?,
            ),
            turnover_24h: quantize_optional_notional(
                parse_decimal(&snapshot.quote_volume)?,
                spec.quote_scale,
            ),
            event_time: TimestampMs::new(snapshot.close_time),
        }
        .to_unified(&spec))
    }

    pub fn parse_trades_snapshot(
        &self,
        payload: &str,
        request: &FetchTradesRequest,
    ) -> Result<Vec<bat_markets_core::TradeTick>> {
        let spec = self
            .resolve_instrument(&request.instrument_id)
            .ok_or_else(|| {
                MarketError::new(
                    ErrorKind::Unsupported,
                    format!("unsupported binance instrument '{}'", request.instrument_id),
                )
                .with_venue(Venue::Binance, Product::LinearUsdt)
            })?;
        let snapshots =
            serde_json::from_str::<Vec<native::AggTradeSnapshot>>(payload).map_err(|error| {
                MarketError::new(
                    ErrorKind::DecodeError,
                    format!("failed to parse binance trades snapshot: {error}"),
                )
                .with_venue(Venue::Binance, Product::LinearUsdt)
                .with_operation("binance.parse_trades_snapshot")
            })?;

        snapshots
            .into_iter()
            .map(|snapshot| {
                Ok(FastTrade {
                    instrument_id: spec.instrument_id.clone(),
                    trade_id: TradeId::from(snapshot.agg_trade_id.to_string()),
                    price: Price::new(parse_decimal(&snapshot.price)?)
                        .quantize(spec.price_scale)?,
                    quantity: Quantity::new(parse_decimal(&snapshot.quantity)?)
                        .quantize(spec.qty_scale)?,
                    aggressor_side: if snapshot.is_buyer_maker {
                        AggressorSide::Seller
                    } else {
                        AggressorSide::Buyer
                    },
                    event_time: TimestampMs::new(snapshot.trade_time),
                }
                .to_unified(&spec))
            })
            .collect()
    }

    pub fn parse_book_top_snapshot(
        &self,
        payload: &str,
        instrument_id: &InstrumentId,
    ) -> Result<bat_markets_core::BookTop> {
        let spec = self.resolve_instrument(instrument_id).ok_or_else(|| {
            MarketError::new(
                ErrorKind::Unsupported,
                format!("unsupported binance instrument '{}'", instrument_id),
            )
            .with_venue(Venue::Binance, Product::LinearUsdt)
        })?;
        let snapshot =
            serde_json::from_str::<native::BookTickerSnapshot>(payload).map_err(|error| {
                MarketError::new(
                    ErrorKind::DecodeError,
                    format!("failed to parse binance book-ticker snapshot: {error}"),
                )
                .with_venue(Venue::Binance, Product::LinearUsdt)
                .with_operation("binance.parse_book_top_snapshot")
            })?;

        Ok(FastBookTop {
            instrument_id: spec.instrument_id.clone(),
            bid_price: Price::new(parse_decimal(&snapshot.bid_price)?)
                .quantize(spec.price_scale)?,
            bid_quantity: Quantity::new(parse_decimal(&snapshot.bid_qty)?)
                .quantize(spec.qty_scale)?,
            ask_price: Price::new(parse_decimal(&snapshot.ask_price)?)
                .quantize(spec.price_scale)?,
            ask_quantity: Quantity::new(parse_decimal(&snapshot.ask_qty)?)
                .quantize(spec.qty_scale)?,
            event_time: TimestampMs::new(snapshot.time),
        }
        .to_unified(&spec))
    }

    pub fn parse_ohlcv_snapshot(
        &self,
        payload: &str,
        request: &FetchOhlcvRequest,
    ) -> Result<Vec<Kline>> {
        let interval = KlineInterval::parse(request.interval.as_ref()).ok_or_else(|| {
            MarketError::new(
                ErrorKind::Unsupported,
                format!("unsupported binance OHLCV interval '{}'", request.interval),
            )
            .with_venue(Venue::Binance, Product::LinearUsdt)
        })?;
        let instrument_id = request.single_instrument_id()?;
        let spec = self.resolve_instrument(instrument_id).ok_or_else(|| {
            MarketError::new(
                ErrorKind::Unsupported,
                format!("unsupported binance instrument '{}'", instrument_id),
            )
            .with_venue(Venue::Binance, Product::LinearUsdt)
        })?;
        let rows =
            serde_json::from_str::<Vec<Vec<serde_json::Value>>>(payload).map_err(|error| {
                MarketError::new(
                    ErrorKind::DecodeError,
                    format!("failed to parse binance klines snapshot: {error}"),
                )
                .with_venue(Venue::Binance, Product::LinearUsdt)
                .with_operation("binance.parse_ohlcv_snapshot")
            })?;

        let mut klines = rows
            .into_iter()
            .map(|row| parse_binance_kline_row(&spec, interval, row))
            .collect::<Result<Vec<_>>>()?;
        klines.sort_by_key(|kline| kline.open_time.value());
        Ok(klines)
    }

    fn position_from_account_snapshot(
        &self,
        position: native::AccountPositionSnapshot,
        observed_at: TimestampMs,
        size: Decimal,
    ) -> Result<Position> {
        let spec = self.require_native_symbol(&position.symbol)?;
        Ok(Position {
            position_id: PositionId::from(format!(
                "binance:{}:{}",
                position.symbol, position.position_side
            )),
            instrument_id: spec.instrument_id.clone(),
            direction: decimal_direction(size),
            size: Quantity::new(size.abs()),
            entry_price: parse_optional_decimal(position.entry_price.as_deref())?.map(Price::new),
            mark_price: None,
            unrealized_pnl: position
                .unrealized_profit
                .as_deref()
                .map(balance_amount)
                .transpose()?,
            leverage: parse_optional_decimal(position.leverage.as_deref())?.map(Leverage::new),
            margin_mode: parse_margin_mode_snapshot(
                position.margin_type.as_deref(),
                position.isolated,
                position.isolated_margin.as_deref(),
                position.isolated_wallet.as_deref(),
            )?,
            position_mode: parse_position_mode(&position.position_side),
            updated_at: observed_at,
        })
    }

    fn order_from_snapshot(
        &self,
        snapshot: native::OrderSnapshot,
        observed_at: TimestampMs,
    ) -> Result<Order> {
        let spec = self.require_native_symbol(&snapshot.symbol)?;
        let average_fill_price = if snapshot.average_price == "0" {
            None
        } else {
            Some(Price::new(parse_decimal(&snapshot.average_price)?))
        };
        Ok(Order {
            order_id: OrderId::from(snapshot.order_id.to_string()),
            client_order_id: Some(snapshot.client_order_id.into()),
            instrument_id: spec.instrument_id.clone(),
            side: parse_side(&snapshot.side)?,
            order_type: parse_order_type(&snapshot.order_type)?,
            time_in_force: Some(parse_time_in_force(&snapshot.time_in_force)?),
            status: parse_order_status(&snapshot.status)?,
            price: Some(Price::new(parse_decimal(&snapshot.price)?)),
            quantity: Quantity::new(parse_decimal(&snapshot.original_quantity)?),
            filled_quantity: Quantity::new(parse_decimal(&snapshot.executed_quantity)?),
            average_fill_price,
            reduce_only: snapshot.reduce_only,
            post_only: matches!(snapshot.time_in_force.as_str(), "GTX"),
            created_at: snapshot
                .created_time
                .map(TimestampMs::new)
                .unwrap_or(observed_at),
            updated_at: TimestampMs::new(snapshot.update_time),
            venue_status: Some(snapshot.status.into()),
        })
    }

    fn execution_from_snapshot(&self, snapshot: native::UserTradeSnapshot) -> Result<Execution> {
        let spec = self.require_native_symbol(&snapshot.symbol)?;
        Ok(Execution {
            execution_id: TradeId::from(snapshot.id.to_string()),
            order_id: OrderId::from(snapshot.order_id.to_string()),
            client_order_id: None,
            instrument_id: spec.instrument_id.clone(),
            side: parse_side(&snapshot.side)?,
            quantity: Quantity::new(parse_decimal(&snapshot.qty)?),
            price: Price::new(parse_decimal(&snapshot.price)?),
            fee: Some(balance_amount(&snapshot.commission)?),
            fee_asset: Some(AssetCode::from(snapshot.commission_asset)),
            liquidity: Some(if snapshot.maker {
                bat_markets_core::Liquidity::Maker
            } else {
                bat_markets_core::Liquidity::Taker
            }),
            executed_at: TimestampMs::new(snapshot.time),
        })
    }

    fn require_native_symbol(&self, native_symbol: &str) -> Result<InstrumentSpec> {
        self.resolve_native_symbol(native_symbol).ok_or_else(|| {
            MarketError::new(
                ErrorKind::Unsupported,
                format!("unsupported binance symbol '{native_symbol}'"),
            )
            .with_venue(Venue::Binance, Product::LinearUsdt)
        })
    }
}

impl VenueAdapter for BinanceLinearFuturesAdapter {
    fn venue(&self) -> Venue {
        Venue::Binance
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
        if let Ok(snapshot) = serde_json::from_str::<native::OpenInterestSnapshot>(payload) {
            let spec = self.require_native_symbol(&snapshot.symbol)?;
            let value = Quantity::new(parse_decimal(&snapshot.open_interest)?);
            return Ok(vec![PublicLaneEvent::OpenInterest(OpenInterest {
                instrument_id: spec.instrument_id.clone(),
                value,
                event_time: TimestampMs::new(snapshot.time),
            })]);
        }

        let message = self.parse_native_public(payload)?;
        match message {
            native::PublicMessage::Ticker(event) => {
                let spec = self.require_native_symbol(&event.symbol)?;
                Ok(vec![PublicLaneEvent::Ticker(FastTicker {
                    instrument_id: spec.instrument_id.clone(),
                    last_price: Price::new(parse_decimal(&event.last_price)?)
                        .quantize(spec.price_scale)?,
                    mark_price: None,
                    index_price: None,
                    volume_24h: Some(
                        Quantity::new(parse_decimal(&event.volume_24h)?)
                            .quantize(spec.qty_scale)?,
                    ),
                    turnover_24h: quantize_optional_notional(
                        parse_decimal(&event.quote_volume_24h)?,
                        spec.quote_scale,
                    ),
                    event_time: TimestampMs::new(event.event_time),
                })])
            }
            native::PublicMessage::AggTrade(event) => {
                let spec = self.require_native_symbol(&event.symbol)?;
                Ok(vec![PublicLaneEvent::Trade(FastTrade {
                    instrument_id: spec.instrument_id.clone(),
                    trade_id: TradeId::from(event.agg_trade_id.to_string()),
                    price: Price::new(parse_decimal(&event.price)?).quantize(spec.price_scale)?,
                    quantity: Quantity::new(parse_decimal(&event.quantity)?)
                        .quantize(spec.qty_scale)?,
                    aggressor_side: if event.is_buyer_maker {
                        AggressorSide::Seller
                    } else {
                        AggressorSide::Buyer
                    },
                    event_time: TimestampMs::new(event.trade_time),
                })])
            }
            native::PublicMessage::BookTicker(event) => {
                let spec = self.require_native_symbol(&event.symbol)?;
                Ok(vec![PublicLaneEvent::BookTop(FastBookTop {
                    instrument_id: spec.instrument_id.clone(),
                    bid_price: Price::new(parse_decimal(&event.best_bid_price)?)
                        .quantize(spec.price_scale)?,
                    bid_quantity: Quantity::new(parse_decimal(&event.best_bid_qty)?)
                        .quantize(spec.qty_scale)?,
                    ask_price: Price::new(parse_decimal(&event.best_ask_price)?)
                        .quantize(spec.price_scale)?,
                    ask_quantity: Quantity::new(parse_decimal(&event.best_ask_qty)?)
                        .quantize(spec.qty_scale)?,
                    event_time: TimestampMs::new(event.transaction_time),
                })])
            }
            native::PublicMessage::Kline(event) => {
                let spec = self.require_native_symbol(&event.symbol)?;
                Ok(vec![PublicLaneEvent::Kline(FastKline {
                    instrument_id: spec.instrument_id.clone(),
                    interval: event.kline.interval.into(),
                    open: Price::new(parse_decimal(&event.kline.open)?)
                        .quantize(spec.price_scale)?,
                    high: Price::new(parse_decimal(&event.kline.high)?)
                        .quantize(spec.price_scale)?,
                    low: Price::new(parse_decimal(&event.kline.low)?).quantize(spec.price_scale)?,
                    close: Price::new(parse_decimal(&event.kline.close)?)
                        .quantize(spec.price_scale)?,
                    volume: Quantity::new(parse_decimal(&event.kline.volume)?)
                        .quantize(spec.qty_scale)?,
                    open_time: TimestampMs::new(event.kline.open_time),
                    close_time: TimestampMs::new(event.kline.close_time),
                    closed: event.kline.closed,
                })])
            }
            native::PublicMessage::MarkPrice(event) => {
                let spec = self.require_native_symbol(&event.symbol)?;
                Ok(vec![PublicLaneEvent::FundingRate(FundingRate {
                    instrument_id: spec.instrument_id.clone(),
                    value: Rate::new(parse_decimal(&event.funding_rate)?),
                    mark_price: Some(Price::new(parse_decimal(&event.mark_price)?)),
                    event_time: TimestampMs::new(event.event_time),
                })])
            }
        }
    }

    fn parse_private(&self, payload: &str) -> Result<Vec<PrivateLaneEvent>> {
        let message = self.parse_native_private(payload)?;
        match message {
            native::PrivateMessage::AccountUpdate(event) => {
                let mut events = Vec::new();

                for balance in event.account.balances {
                    events.push(PrivateLaneEvent::Balance(Balance {
                        asset: AssetCode::from(balance.asset),
                        wallet_balance: balance_amount(&balance.wallet_balance)?,
                        available_balance: balance_amount(&balance.cross_wallet_balance)?,
                        updated_at: TimestampMs::new(event.transaction_time),
                    }));
                }

                for position in event.account.positions {
                    let spec = self.require_native_symbol(&position.symbol)?;
                    let size = parse_decimal(&position.position_amount)?;
                    events.push(PrivateLaneEvent::Position(Position {
                        position_id: PositionId::from(format!(
                            "binance:{}:{}",
                            position.symbol, position.position_side
                        )),
                        instrument_id: spec.instrument_id.clone(),
                        direction: decimal_direction(size),
                        size: Quantity::new(size.abs()),
                        entry_price: parse_optional_decimal(position.entry_price.as_deref())?
                            .map(Price::new),
                        mark_price: None,
                        unrealized_pnl: Some(balance_amount(&position.unrealized_pnl)?),
                        leverage: None,
                        margin_mode: parse_margin_mode(&position.margin_type)?,
                        position_mode: parse_position_mode(&position.position_side),
                        updated_at: TimestampMs::new(event.event_time),
                    }));
                }

                Ok(events)
            }
            native::PrivateMessage::OrderTradeUpdate(event) => {
                let spec = self.require_native_symbol(&event.order.symbol)?;
                let mut events = Vec::new();
                let order_id = OrderId::from(event.order.order_id.to_string());
                let client_order_id = Some(event.order.client_order_id.clone().into());
                let average_fill_price =
                    parse_optional_decimal(event.order.average_price.as_deref())?.map(Price::new);
                let updated_at = TimestampMs::new(event.order.trade_time);

                events.push(PrivateLaneEvent::Order(Order {
                    order_id: order_id.clone(),
                    client_order_id: client_order_id.clone(),
                    instrument_id: spec.instrument_id.clone(),
                    side: parse_side(&event.order.side)?,
                    order_type: parse_order_type(&event.order.order_type)?,
                    time_in_force: Some(parse_time_in_force(&event.order.time_in_force)?),
                    status: parse_order_status(&event.order.order_status)?,
                    price: Some(Price::new(parse_decimal(&event.order.price)?)),
                    quantity: Quantity::new(parse_decimal(&event.order.original_quantity)?),
                    filled_quantity: Quantity::new(parse_decimal(
                        &event.order.cumulative_filled_qty,
                    )?),
                    average_fill_price,
                    reduce_only: event.order.reduce_only,
                    post_only: false,
                    created_at: TimestampMs::new(
                        event
                            .order
                            .order_trade_time
                            .unwrap_or(event.order.trade_time),
                    ),
                    updated_at,
                    venue_status: Some(event.order.execution_type.into()),
                }));

                if parse_decimal(&event.order.last_filled_qty)? > Decimal::ZERO {
                    events.push(PrivateLaneEvent::Execution(Execution {
                        execution_id: TradeId::from(
                            event
                                .order
                                .trade_id
                                .unwrap_or(event.order.trade_time)
                                .to_string(),
                        ),
                        order_id,
                        client_order_id,
                        instrument_id: spec.instrument_id.clone(),
                        side: parse_side(&event.order.side)?,
                        quantity: Quantity::new(parse_decimal(&event.order.last_filled_qty)?),
                        price: Price::new(parse_decimal(&event.order.last_filled_price)?),
                        fee: parse_optional_decimal(event.order.commission.as_deref())?
                            .map(Into::into),
                        fee_asset: event.order.commission_asset.map(AssetCode::from),
                        liquidity: None,
                        executed_at: updated_at,
                    }));
                }

                Ok(events)
            }
        }
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
                venue: Venue::Binance,
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

        if let Ok(error) = serde_json::from_str::<native::ErrorResponse>(payload) {
            return Ok(CommandReceipt {
                operation,
                status: CommandStatus::Rejected,
                venue: Venue::Binance,
                product: Product::LinearUsdt,
                instrument_id: None,
                order_id: None,
                client_order_id: None,
                request_id,
                message: Some(error.message.into()),
                native_code: Some(error.code.to_string().into()),
                retriable: false,
            });
        }

        match operation {
            CommandOperation::CreateOrder
            | CommandOperation::CancelOrder
            | CommandOperation::GetOrder => {
                let response =
                    serde_json::from_str::<native::OrderResponse>(payload).map_err(|error| {
                        MarketError::new(
                            ErrorKind::DecodeError,
                            format!("failed to classify binance order response: {error}"),
                        )
                        .with_venue(Venue::Binance, Product::LinearUsdt)
                        .with_operation("binance.classify_command")
                    })?;
                let spec = self.require_native_symbol(&response.symbol)?;
                Ok(CommandReceipt {
                    operation,
                    status: CommandStatus::Accepted,
                    venue: Venue::Binance,
                    product: Product::LinearUsdt,
                    instrument_id: Some(spec.instrument_id.clone()),
                    order_id: Some(OrderId::from(response.order_id.to_string())),
                    client_order_id: Some(response.client_order_id.into()),
                    request_id,
                    message: Some("accepted".into()),
                    native_code: None,
                    retriable: false,
                })
            }
            CommandOperation::SetLeverage => {
                let response = serde_json::from_str::<native::SetLeverageResponse>(payload)
                    .map_err(|error| {
                        MarketError::new(
                            ErrorKind::DecodeError,
                            format!("failed to classify binance leverage response: {error}"),
                        )
                    })?;
                let spec = self.require_native_symbol(&response.symbol)?;
                Ok(CommandReceipt {
                    operation,
                    status: CommandStatus::Accepted,
                    venue: Venue::Binance,
                    product: Product::LinearUsdt,
                    instrument_id: Some(spec.instrument_id.clone()),
                    order_id: None,
                    client_order_id: None,
                    request_id,
                    message: Some(format!("leverage set to {}", response.leverage).into()),
                    native_code: None,
                    retriable: false,
                })
            }
            CommandOperation::SetMarginMode => {
                let response =
                    serde_json::from_str::<native::SuccessResponse>(payload).map_err(|error| {
                        MarketError::new(
                            ErrorKind::DecodeError,
                            format!("failed to classify binance margin-mode response: {error}"),
                        )
                    })?;
                Ok(CommandReceipt {
                    operation,
                    status: CommandStatus::Accepted,
                    venue: Venue::Binance,
                    product: Product::LinearUsdt,
                    instrument_id: None,
                    order_id: None,
                    client_order_id: None,
                    request_id,
                    message: Some(response.message.into()),
                    native_code: response.code.map(|value| value.to_string().into()),
                    retriable: false,
                })
            }
        }
    }
}

fn btc_spec() -> InstrumentSpec {
    instrument_spec(("BTC", "USDT", "USDT"), "BTCUSDT", 2, 3, 5, Some(125))
}

fn eth_spec() -> InstrumentSpec {
    instrument_spec(("ETH", "USDT", "USDT"), "ETHUSDT", 2, 3, 5, Some(100))
}

fn instrument_spec(
    assets: (&str, &str, &str),
    native_symbol: &str,
    price_scale: u32,
    qty_scale: u32,
    quote_scale: u32,
    max_leverage: Option<i64>,
) -> InstrumentSpec {
    let (base, quote, settle) = assets;
    InstrumentSpec {
        venue: Venue::Binance,
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
        max_leverage: max_leverage.map(|value| Leverage::new(Decimal::new(value, 0))),
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
        .with_venue(Venue::Binance, Product::LinearUsdt)
    })
}

fn parse_optional_decimal(raw: Option<&str>) -> Result<Option<Decimal>> {
    raw.map(parse_decimal).transpose()
}

fn parse_side(raw: &str) -> Result<Side> {
    match raw {
        "BUY" => Ok(Side::Buy),
        "SELL" => Ok(Side::Sell),
        other => Err(MarketError::new(
            ErrorKind::DecodeError,
            format!("unsupported binance side '{other}'"),
        )),
    }
}

fn parse_order_type(raw: &str) -> Result<OrderType> {
    match raw {
        "MARKET" => Ok(OrderType::Market),
        "LIMIT" => Ok(OrderType::Limit),
        "STOP_MARKET" => Ok(OrderType::StopMarket),
        "STOP" => Ok(OrderType::StopLimit),
        other => Err(MarketError::new(
            ErrorKind::DecodeError,
            format!("unsupported binance order type '{other}'"),
        )),
    }
}

fn parse_time_in_force(raw: &str) -> Result<TimeInForce> {
    match raw {
        "GTC" => Ok(TimeInForce::Gtc),
        "IOC" => Ok(TimeInForce::Ioc),
        "FOK" => Ok(TimeInForce::Fok),
        "GTX" => Ok(TimeInForce::PostOnly),
        other => Err(MarketError::new(
            ErrorKind::DecodeError,
            format!("unsupported binance time in force '{other}'"),
        )),
    }
}

fn parse_order_status(raw: &str) -> Result<OrderStatus> {
    match raw {
        "NEW" => Ok(OrderStatus::New),
        "PARTIALLY_FILLED" => Ok(OrderStatus::PartiallyFilled),
        "FILLED" => Ok(OrderStatus::Filled),
        "CANCELED" => Ok(OrderStatus::Canceled),
        "REJECTED" => Ok(OrderStatus::Rejected),
        "EXPIRED" => Ok(OrderStatus::Expired),
        "PENDING_CANCEL" => Ok(OrderStatus::PendingCancel),
        other => Err(MarketError::new(
            ErrorKind::DecodeError,
            format!("unsupported binance order status '{other}'"),
        )),
    }
}

fn parse_margin_mode(raw: &str) -> Result<MarginMode> {
    match raw {
        "isolated" | "ISOLATED" => Ok(MarginMode::Isolated),
        "cross" | "crossed" | "CROSSED" => Ok(MarginMode::Cross),
        other => Err(MarketError::new(
            ErrorKind::DecodeError,
            format!("unsupported binance margin type '{other}'"),
        )),
    }
}

fn parse_margin_mode_snapshot(
    raw: Option<&str>,
    isolated: Option<bool>,
    isolated_margin: Option<&str>,
    isolated_wallet: Option<&str>,
) -> Result<MarginMode> {
    if let Some(raw) = raw {
        return parse_margin_mode(raw);
    }

    if let Some(isolated) = isolated {
        return Ok(if isolated {
            MarginMode::Isolated
        } else {
            MarginMode::Cross
        });
    }

    let isolated_margin = parse_optional_decimal(isolated_margin)?;
    let isolated_wallet = parse_optional_decimal(isolated_wallet)?;
    if isolated_margin.is_some() || isolated_wallet.is_some() {
        return Ok(
            if isolated_margin.unwrap_or_default().is_zero()
                && isolated_wallet.unwrap_or_default().is_zero()
            {
                MarginMode::Cross
            } else {
                MarginMode::Isolated
            },
        );
    }

    Err(MarketError::new(
        ErrorKind::DecodeError,
        "missing binance margin mode in account snapshot",
    ))
}

fn parse_position_mode(raw: &str) -> PositionMode {
    match raw {
        "LONG" | "SHORT" => PositionMode::Hedge,
        _ => PositionMode::OneWay,
    }
}

fn parse_instrument_status(raw: &str) -> InstrumentStatus {
    match raw {
        "TRADING" => InstrumentStatus::Active,
        "SETTLING" | "CLOSE" | "PENDING_TRADING" => InstrumentStatus::Halted,
        _ => InstrumentStatus::Halted,
    }
}

fn decimal_direction(value: Decimal) -> PositionDirection {
    if value > Decimal::ZERO {
        PositionDirection::Long
    } else if value < Decimal::ZERO {
        PositionDirection::Short
    } else {
        PositionDirection::Flat
    }
}

fn balance_amount(raw: &str) -> Result<bat_markets_core::Amount> {
    parse_decimal(raw).map(Into::into)
}

fn quantize_optional_notional(
    value: Decimal,
    scale: u32,
) -> Option<bat_markets_core::FastNotional> {
    Notional::new(value).quantize(scale).ok()
}

fn decimal_scale(value: Decimal) -> u32 {
    value.normalize().scale()
}

fn require_filter_decimal(
    filters: &[native::ExchangeFilter],
    filter_type: &str,
    select: impl Fn(&native::ExchangeFilter) -> Option<&str>,
) -> Result<Decimal> {
    let raw = filters
        .iter()
        .find(|filter| filter.filter_type == filter_type)
        .and_then(select)
        .ok_or_else(|| {
            MarketError::new(
                ErrorKind::DecodeError,
                format!("missing binance {filter_type} filter"),
            )
        })?;
    parse_decimal(raw)
}

fn parse_binance_kline_row(
    spec: &InstrumentSpec,
    interval: KlineInterval,
    row: Vec<serde_json::Value>,
) -> Result<Kline> {
    if row.len() < 7 {
        return Err(MarketError::new(
            ErrorKind::DecodeError,
            format!(
                "binance kline row has {} fields, expected at least 7",
                row.len()
            ),
        )
        .with_venue(Venue::Binance, Product::LinearUsdt));
    }

    let open_time = parse_i64_value(&row[0], "open_time")?;
    let close_time = parse_i64_value(&row[6], "close_time")?;

    Ok(Kline {
        instrument_id: spec.instrument_id.clone(),
        interval: interval.as_ccxt_str().into(),
        open: spec.price_from_fast(
            Price::new(parse_decimal(parse_str_value(&row[1], "open")?)?)
                .quantize(spec.price_scale)?,
        ),
        high: spec.price_from_fast(
            Price::new(parse_decimal(parse_str_value(&row[2], "high")?)?)
                .quantize(spec.price_scale)?,
        ),
        low: spec.price_from_fast(
            Price::new(parse_decimal(parse_str_value(&row[3], "low")?)?)
                .quantize(spec.price_scale)?,
        ),
        close: spec.price_from_fast(
            Price::new(parse_decimal(parse_str_value(&row[4], "close")?)?)
                .quantize(spec.price_scale)?,
        ),
        volume: spec.quantity_from_fast(
            Quantity::new(parse_decimal(parse_str_value(&row[5], "volume")?)?)
                .quantize(spec.qty_scale)?,
        ),
        open_time: TimestampMs::new(open_time),
        close_time: TimestampMs::new(close_time),
        closed: close_time < now_timestamp_ms(),
    })
}

fn parse_i64_value(value: &serde_json::Value, label: &str) -> Result<i64> {
    match value {
        serde_json::Value::Number(number) => number.as_i64().ok_or_else(|| {
            MarketError::new(
                ErrorKind::DecodeError,
                format!("invalid numeric value for binance {label}"),
            )
            .with_venue(Venue::Binance, Product::LinearUsdt)
        }),
        serde_json::Value::String(raw) => raw.parse::<i64>().map_err(|error| {
            MarketError::new(
                ErrorKind::DecodeError,
                format!("invalid i64 '{raw}' for binance {label}: {error}"),
            )
            .with_venue(Venue::Binance, Product::LinearUsdt)
        }),
        other => Err(MarketError::new(
            ErrorKind::DecodeError,
            format!("unsupported binance {label} representation: {other}"),
        )
        .with_venue(Venue::Binance, Product::LinearUsdt)),
    }
}

fn parse_str_value<'a>(value: &'a serde_json::Value, label: &str) -> Result<&'a str> {
    match value {
        serde_json::Value::String(raw) => Ok(raw),
        other => Err(MarketError::new(
            ErrorKind::DecodeError,
            format!("unsupported binance {label} representation: {other}"),
        )
        .with_venue(Venue::Binance, Product::LinearUsdt)),
    }
}

fn now_timestamp_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i128::from(i64::MAX) as u128) as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::BinanceLinearFuturesAdapter;
    use bat_markets_core::{
        FetchOhlcvRequest, FetchTradesRequest, InstrumentId, OrderStatus, TimestampMs, VenueAdapter,
    };

    const USER_TRADES: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/binance/user_trades.json"
    ));
    const ORDER_HISTORY: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/binance/order_history.json"
    ));

    #[test]
    fn parse_binance_execution_history_snapshot() {
        let adapter = BinanceLinearFuturesAdapter::new();
        let executions = adapter
            .parse_executions_snapshot(USER_TRADES)
            .expect("binance user trades fixture should parse");
        assert_eq!(executions.len(), 1);
        assert_eq!(executions[0].execution_id.to_string(), "880001");
    }

    #[test]
    fn parse_binance_order_history_snapshot() {
        let adapter = BinanceLinearFuturesAdapter::new();
        let orders = adapter
            .parse_order_history_snapshot(ORDER_HISTORY, TimestampMs::new(1))
            .expect("binance order history fixture should parse");
        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].status, OrderStatus::Filled);
    }

    #[test]
    fn parse_binance_rest_ticker_snapshot() {
        let adapter = BinanceLinearFuturesAdapter::new();
        let ticker = adapter
            .parse_ticker_snapshot(
                r#"{
                    "symbol":"BTCUSDT",
                    "lastPrice":"70100.50",
                    "volume":"1234.567",
                    "quoteVolume":"86500000.12",
                    "closeTime":1710000000000
                }"#,
                &InstrumentId::from("BTC/USDT:USDT"),
            )
            .expect("binance rest ticker should parse");
        assert_eq!(ticker.last_price.to_string(), "70100.50");
    }

    #[test]
    fn parse_binance_rest_trades_snapshot() {
        let adapter = BinanceLinearFuturesAdapter::new();
        let trades = adapter
            .parse_trades_snapshot(
                r#"[{"a":1,"p":"70100.10","q":"0.500","T":1710000000001,"m":true}]"#,
                &FetchTradesRequest::new(InstrumentId::from("BTC/USDT:USDT"), Some(1)),
            )
            .expect("binance rest trades should parse");
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].price.to_string(), "70100.10");
    }

    #[test]
    fn parse_binance_rest_book_top_snapshot() {
        let adapter = BinanceLinearFuturesAdapter::new();
        let book_top = adapter
            .parse_book_top_snapshot(
                r#"{
                    "symbol":"BTCUSDT",
                    "bidPrice":"70100.90",
                    "bidQty":"1.250",
                    "askPrice":"70101.10",
                    "askQty":"0.900",
                    "time":1710000000200
                }"#,
                &InstrumentId::from("BTC/USDT:USDT"),
            )
            .expect("binance rest book top should parse");
        assert_eq!(book_top.bid.price.to_string(), "70100.90");
        assert_eq!(book_top.ask.price.to_string(), "70101.10");
    }

    #[test]
    fn parse_binance_ticker_drops_unrepresentable_turnover_instead_of_failing() {
        let adapter = BinanceLinearFuturesAdapter::new();
        let events = adapter
            .parse_public(
                r#"{
                    "e":"24hrTicker",
                    "E":1710000000000,
                    "s":"BTCUSDT",
                    "c":"64000.10",
                    "v":"12345.678",
                    "q":"100000000000000000000.00"
                }"#,
            )
            .expect("binance ticker with large quote turnover should still parse");

        let ticker = match &events[0] {
            bat_markets_core::PublicLaneEvent::Ticker(ticker) => ticker,
            other => panic!("expected ticker event, got {other:?}"),
        };
        assert!(ticker.turnover_24h.is_none());
    }

    #[test]
    fn parse_binance_private_order_update_without_order_create_time() {
        let adapter = BinanceLinearFuturesAdapter::new();
        let events = adapter
            .parse_private(
                r#"{
                    "e":"ORDER_TRADE_UPDATE",
                    "o":{
                        "s":"BTCUSDT",
                        "c":"codex-demo",
                        "S":"BUY",
                        "o":"LIMIT",
                        "f":"GTC",
                        "q":"0.002",
                        "p":"64000.10",
                        "ap":"0",
                        "x":"CANCELED",
                        "X":"CANCELED",
                        "i":123456,
                        "l":"0",
                        "z":"0",
                        "L":"0",
                        "n":null,
                        "N":null,
                        "T":1710000001234,
                        "t":null,
                        "R":false
                    }
                }"#,
            )
            .expect("private order update without O should parse");

        let order = match &events[0] {
            bat_markets_core::PrivateLaneEvent::Order(order) => order,
            other => panic!("expected order event, got {other:?}"),
        };
        assert_eq!(order.created_at, TimestampMs::new(1710000001234));
        assert_eq!(order.updated_at, TimestampMs::new(1710000001234));
    }

    #[test]
    fn parse_binance_account_snapshot_tolerates_missing_optional_position_fields() {
        let adapter = BinanceLinearFuturesAdapter::new();
        let (account, positions) = adapter
            .parse_account_snapshot(
                r#"{
                    "totalWalletBalance":"5000.0",
                    "availableBalance":"5000.0",
                    "totalUnrealizedProfit":"0.0",
                    "assets":[
                        {
                            "asset":"USDT",
                            "walletBalance":"5000.0",
                            "availableBalance":"5000.0"
                        }
                    ],
                    "positions":[
                        {
                            "symbol":"BTCUSDT",
                            "positionAmt":"0.0",
                            "positionSide":"BOTH"
                        },
                        {
                            "symbol":"BTCUSDT",
                            "positionAmt":"0.001",
                            "unrealizedProfit":"0.0",
                            "isolatedMargin":"0",
                            "isolatedWallet":"0",
                            "positionSide":"BOTH"
                        }
                    ]
                }"#,
                TimestampMs::new(42),
            )
            .expect("account snapshot with sparse position fields should still parse");

        assert_eq!(account.balances.len(), 1);
        assert_eq!(positions.len(), 1);
        assert!(positions[0].entry_price.is_none());
        assert!(positions[0].leverage.is_none());
        assert_eq!(
            positions[0].margin_mode,
            bat_markets_core::MarginMode::Cross
        );
    }

    #[test]
    fn parse_binance_ohlcv_snapshot() {
        let adapter = BinanceLinearFuturesAdapter::new();
        let klines = adapter
            .parse_ohlcv_snapshot(
                r#"[
                    [1710000000000,"64000.1","64100.0","63950.0","64050.0","12.345","1710000059999","0","0","0","0","0"],
                    [1710000060000,"64050.0","64150.0","64000.0","64100.0","23.456","1710000119999","0","0","0","0","0"]
                ]"#,
                &FetchOhlcvRequest::for_instrument(
                    InstrumentId::from("BTC/USDT:USDT"),
                    "1m",
                    None,
                    None,
                    Some(2),
                ),
            )
            .expect("binance klines snapshot should parse");

        assert_eq!(klines.len(), 2);
        assert_eq!(klines[0].interval.as_ref(), "1m");
        assert_eq!(klines[0].open.to_string(), "64000.10");
        assert_eq!(klines[1].close.to_string(), "64100.00");
    }
}
