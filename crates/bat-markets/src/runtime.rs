use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[cfg(feature = "binance")]
use futures_util::future::try_join_all;
use futures_util::{SinkExt, StreamExt};
#[cfg(feature = "binance")]
use reqwest::Method;
use reqwest::StatusCode;
#[cfg(feature = "bybit")]
use rust_decimal::Decimal;
use serde_json::{Value, json};
use tokio::{
    sync::{Mutex, oneshot},
    time::{Instant, interval, sleep},
};
use tokio_tungstenite::tungstenite::Message;
use url::form_urlencoded::Serializer;

#[cfg(feature = "binance")]
use bat_markets_core::OrderType;
use bat_markets_core::{
    AccountSnapshot, AmendOrderRequest, AmendOrdersRequest, BookTop, CancelAllOrdersRequest,
    CancelOrderRequest, CancelOrdersRequest, ClientOrderId, ClosePositionRequest, CommandAck,
    CommandLaneEvent, CommandLifecycleEvent, CommandOperation, CommandReceipt, CommandStatus,
    CommandTransport, CreateOrderRequest, CreateOrdersRequest, DegradedReason, ErrorKind,
    Execution, FetchOhlcvRequest, FetchOrderBookRequest, FetchTickersRequest, FetchTradesRequest,
    GetOrderRequest, HealthReport, InstrumentId, InstrumentSpec, Kline, KlineInterval, Liquidation,
    ListExecutionsRequest, ListOpenOrdersRequest, MarginMode, MarkPrice, MarketError, OpenInterest,
    Order, OrderBookLevel, OrderBookSnapshot, OrderId, OrderTarget, Position, Price,
    PrivateLaneEvent, Product, PublicLaneEvent, Quantity, ReconcileOutcome, ReconcileReport,
    ReconcileTrigger, RequestId, Result, SequenceNumber, SetLeverageRequest, SetMarginModeRequest,
    SetPositionModeRequest, Ticker, TimestampMs, TradeTick, ValidateOrderRequest, Venue,
    VenueAdapter,
};

#[cfg(feature = "binance")]
use bat_markets_binance::BinanceLinearFuturesAdapter;
#[cfg(feature = "binance")]
use bat_markets_binance::native as binance_native;
#[cfg(feature = "bybit")]
use bat_markets_bybit::BybitLinearFuturesAdapter;
#[cfg(feature = "bybit")]
use bat_markets_bybit::native as bybit_native;

use crate::{
    client::{AdapterHandle, LiveContext},
    diagnostics::{RuntimeDiagnosticsState, RuntimeOperation},
    stream::{LiveStreamHandle, PublicSubscription},
    transport::CommandWsRequestError,
};

#[derive(Debug)]
struct RateLimiterState {
    tokens: f64,
    last_refill: Instant,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PendingUnknownCommand {
    operation: CommandOperation,
    instrument_id: InstrumentId,
    order_id: Option<OrderId>,
    client_order_id: Option<ClientOrderId>,
    recorded_at: TimestampMs,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct TopicKey(Box<str>);

#[derive(Clone, Copy, Debug)]
struct Watermark {
    value: i64,
    strict_gap: bool,
}

#[derive(Debug, Default)]
struct SequenceTracker {
    watermarks: BTreeMap<TopicKey, Watermark>,
}

#[derive(Clone, Debug)]
struct SequenceObservation {
    topic: TopicKey,
    value: i64,
    strict_gap: bool,
    reset: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PrivateReconcileMode {
    SnapshotOnly,
    RecentHistoryRepair,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CommandTransportMode {
    Auto,
    WebSocketOnly,
}

impl CommandTransportMode {
    fn is_websocket_only(self) -> bool {
        matches!(self, Self::WebSocketOnly)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExecutionRepairScope {
    PendingUnknownOnly,
    LocalEvidence,
}

const HISTORY_REPAIR_REWIND_MS: i64 = 60_000;
const HISTORY_REPAIR_MAX_LOOKBACK_MS: i64 = 7 * 24 * 60 * 60 * 1_000;

#[derive(Debug)]
pub(crate) struct LiveRuntimeState {
    pending_unknown_commands: Mutex<Vec<PendingUnknownCommand>>,
    pub(crate) diagnostics: RuntimeDiagnosticsState,
    #[cfg(feature = "bybit")]
    bybit_account_context: Mutex<Option<bat_markets_bybit::BybitAccountContext>>,
}

impl Default for LiveRuntimeState {
    fn default() -> Self {
        Self {
            pending_unknown_commands: Mutex::new(Vec::new()),
            diagnostics: RuntimeDiagnosticsState::default(),
            #[cfg(feature = "bybit")]
            bybit_account_context: Mutex::new(None),
        }
    }
}

impl LiveRuntimeState {
    async fn cache_pending_unknown(&self, pending: PendingUnknownCommand) {
        let mut commands = self.pending_unknown_commands.lock().await;
        if commands.iter().any(|existing| existing == &pending) {
            return;
        }
        commands.push(pending);
    }

    async fn pending_unknown_commands(&self) -> Vec<PendingUnknownCommand> {
        self.pending_unknown_commands.lock().await.clone()
    }

    async fn replace_pending_unknown_commands(&self, pending: Vec<PendingUnknownCommand>) {
        *self.pending_unknown_commands.lock().await = pending;
    }

    #[cfg(feature = "bybit")]
    async fn cached_bybit_account_context(&self) -> Option<bat_markets_bybit::BybitAccountContext> {
        self.bybit_account_context.lock().await.clone()
    }

    #[cfg(feature = "bybit")]
    async fn cache_bybit_account_context(&self, context: bat_markets_bybit::BybitAccountContext) {
        *self.bybit_account_context.lock().await = Some(context);
    }

    #[cfg(feature = "bybit")]
    async fn invalidate_bybit_account_context(&self) {
        *self.bybit_account_context.lock().await = None;
    }
}

/// Cheap leaky-bucket limiter for command writes.
#[derive(Debug)]
pub(crate) struct CommandRateLimiter {
    burst: f64,
    refill_per_second: f64,
    state: Mutex<RateLimiterState>,
}

impl CommandRateLimiter {
    pub(crate) fn new(command_burst: u32, command_refill_per_second: u32) -> Self {
        Self {
            burst: f64::from(command_burst.max(1)),
            refill_per_second: f64::from(command_refill_per_second.max(1)),
            state: Mutex::new(RateLimiterState {
                tokens: f64::from(command_burst.max(1)),
                last_refill: Instant::now(),
            }),
        }
    }

    pub(crate) async fn acquire(&self) {
        loop {
            let wait_for = {
                let mut state = self.state.lock().await;
                let now = Instant::now();
                let elapsed = now.duration_since(state.last_refill).as_secs_f64();
                state.tokens = (state.tokens + elapsed * self.refill_per_second).min(self.burst);
                state.last_refill = now;
                if state.tokens >= 1.0 {
                    state.tokens -= 1.0;
                    None
                } else {
                    let missing = 1.0 - state.tokens;
                    Some(Duration::from_secs_f64(
                        missing / self.refill_per_second.max(1.0),
                    ))
                }
            };

            if let Some(wait_for) = wait_for {
                sleep(wait_for).await;
            } else {
                return;
            }
        }
    }
}

impl SequenceTracker {
    fn observe(&mut self, observation: SequenceObservation) -> std::result::Result<(), i64> {
        let current = Watermark {
            value: observation.value,
            strict_gap: observation.strict_gap,
        };

        if observation.reset {
            self.watermarks.insert(observation.topic, current);
            return std::result::Result::Ok(());
        }

        match self.watermarks.get_mut(&observation.topic) {
            None => {
                self.watermarks.insert(observation.topic, current);
                std::result::Result::Ok(())
            }
            Some(existing) if observation.value == existing.value => std::result::Result::Ok(()),
            Some(existing) if observation.value < existing.value => {
                std::result::Result::Err(observation.value)
            }
            Some(existing) if existing.strict_gap && observation.value > existing.value + 1 => {
                std::result::Result::Err(existing.value + 1)
            }
            Some(existing) => {
                *existing = current;
                std::result::Result::Ok(())
            }
        }
    }
}

fn record_runtime_latency(context: &LiveContext, operation: RuntimeOperation, started_at: Instant) {
    context
        .runtime_state
        .diagnostics
        .observe(operation, started_at.elapsed());
}

fn unsupported_ws_command(context: &LiveContext, operation: &str) -> MarketError {
    MarketError::new(
        ErrorKind::Unsupported,
        format!("{operation} requires websocket command transport for this venue/path"),
    )
    .with_venue(context.config.venue, context.config.product)
}

pub(crate) async fn bootstrap_live(context: &LiveContext) -> Result<()> {
    sync_server_time(context).await?;
    refresh_metadata(context).await?;
    #[cfg(feature = "bybit")]
    if matches!(context.adapter, AdapterHandle::Bybit(_))
        && context.api_key.is_some()
        && context.signer.is_some()
    {
        let _ = refresh_bybit_account_context(context).await?;
    }
    Ok(())
}

pub(crate) async fn refresh_metadata(context: &LiveContext) -> Result<Vec<InstrumentSpec>> {
    let started_at = Instant::now();
    let result = async {
        let specs = match &context.adapter {
            #[cfg(feature = "binance")]
            AdapterHandle::Binance(adapter) => {
                let payload = public_get_with_retry(
                    context,
                    "/fapi/v1/exchangeInfo",
                    &[],
                    "binance.metadata",
                )
                .await?;
                adapter.parse_metadata_snapshot(&payload)?
            }
            #[cfg(feature = "bybit")]
            AdapterHandle::Bybit(adapter) => {
                let payload = public_get_with_retry(
                    context,
                    "/v5/market/instruments-info",
                    &[("category", "linear")],
                    "bybit.metadata",
                )
                .await?;
                adapter.parse_metadata_snapshot(&payload)?
            }
        };

        context.adapter.replace_instruments(specs.clone());
        let specs_for_state = specs.clone();
        context.shared.write(|state| {
            state.replace_instruments(specs_for_state);
            state.mark_rest_success(None);
        });
        Ok(specs)
    }
    .await;
    record_runtime_latency(context, RuntimeOperation::RefreshMetadata, started_at);
    result
}

pub(crate) async fn fetch_balance(context: &LiveContext) -> Result<AccountSnapshot> {
    let started_at = Instant::now();
    let result = async {
        let snapshot = match &context.adapter {
            #[cfg(feature = "binance")]
            AdapterHandle::Binance(adapter) => {
                let payload = binance_signed_request_text(
                    context,
                    Method::GET,
                    "/fapi/v3/account",
                    &[],
                    "binance.account",
                )
                .await?;
                let observed_at = timestamp_now_ms();
                let (account, positions) = adapter.parse_account_snapshot(&payload, observed_at)?;
                context.shared.write(|state| {
                    state.replace_account_snapshot(account.clone());
                    state.replace_positions(positions);
                    state.mark_rest_success(None);
                });
                account
            }
            #[cfg(feature = "bybit")]
            AdapterHandle::Bybit(adapter) => {
                let account_context =
                    refresh_bybit_account_context_with_adapter(context, adapter).await?;
                let payload = bybit_signed_get_text(
                    context,
                    "/v5/account/wallet-balance",
                    &[("accountType", account_context.wallet_account_type.as_ref())],
                    "bybit.account.wallet_balance",
                )
                .await?;
                let observed_at = timestamp_now_ms();
                let account = adapter.parse_account_snapshot(&payload, observed_at)?;
                context.shared.write(|state| {
                    state.replace_account_snapshot(account.clone());
                    state.mark_rest_success(None);
                });
                account
            }
        };

        Ok(snapshot)
    }
    .await;
    record_runtime_latency(context, RuntimeOperation::RefreshAccount, started_at);
    result
}

pub(crate) async fn refresh_account(
    context: &LiveContext,
) -> Result<Option<bat_markets_core::AccountSummary>> {
    Ok(fetch_balance(context).await?.summary)
}

pub(crate) async fn refresh_positions(
    context: &LiveContext,
) -> Result<Vec<bat_markets_core::Position>> {
    let started_at = Instant::now();
    let result = async {
        let positions = match &context.adapter {
            #[cfg(feature = "binance")]
            AdapterHandle::Binance(adapter) => {
                let payload = binance_signed_request_text(
                    context,
                    Method::GET,
                    "/fapi/v3/account",
                    &[],
                    "binance.positions",
                )
                .await?;
                let observed_at = timestamp_now_ms();
                let (account, positions) = adapter.parse_account_snapshot(&payload, observed_at)?;
                let positions_for_state = positions.clone();
                context.shared.write(|state| {
                    state.replace_account_snapshot(account);
                    state.replace_positions(positions_for_state);
                    state.mark_rest_success(None);
                });
                positions
            }
            #[cfg(feature = "bybit")]
            AdapterHandle::Bybit(adapter) => {
                let payload = bybit_signed_get_text(
                    context,
                    "/v5/position/list",
                    &[("category", "linear"), ("settleCoin", "USDT")],
                    "bybit.positions",
                )
                .await?;
                let positions = adapter.parse_positions_snapshot(&payload, timestamp_now_ms())?;
                let positions_for_state = positions.clone();
                context.shared.write(|state| {
                    state.replace_positions(positions_for_state);
                    state.mark_rest_success(None);
                });
                positions
            }
        };

        Ok(positions)
    }
    .await;
    record_runtime_latency(context, RuntimeOperation::RefreshPositions, started_at);
    result
}

pub(crate) async fn refresh_open_orders(
    context: &LiveContext,
    request: Option<&ListOpenOrdersRequest>,
) -> Result<Vec<Order>> {
    let started_at = Instant::now();
    let result = async {
        let orders = match &context.adapter {
            #[cfg(feature = "binance")]
            AdapterHandle::Binance(adapter) => {
                let mut query = Vec::new();
                if let Some(request) = request.and_then(|request| request.instrument_id.as_ref()) {
                    let spec = require_spec(context, request)?;
                    query.push(("symbol".to_owned(), spec.native_symbol.to_string()));
                }
                let pairs = query
                    .iter()
                    .map(|(key, value)| (key.as_str(), value.as_str()))
                    .collect::<Vec<_>>();
                let payload = binance_signed_request_text(
                    context,
                    Method::GET,
                    "/fapi/v1/openOrders",
                    &pairs,
                    "binance.open_orders",
                )
                .await?;
                let mut orders =
                    adapter.parse_open_orders_snapshot(&payload, timestamp_now_ms())?;
                let algo_payload = binance_signed_request_text(
                    context,
                    Method::GET,
                    "/fapi/v1/openAlgoOrders",
                    &pairs,
                    "binance.open_algo_orders",
                )
                .await?;
                orders.extend(
                    adapter.parse_open_algo_orders_snapshot(&algo_payload, timestamp_now_ms())?,
                );
                orders
            }
            #[cfg(feature = "bybit")]
            AdapterHandle::Bybit(adapter) => {
                let mut owned = vec![("category".to_owned(), "linear".to_owned())];
                if let Some(request) = request.and_then(|request| request.instrument_id.as_ref()) {
                    let spec = require_spec(context, request)?;
                    owned.push(("symbol".to_owned(), spec.native_symbol.to_string()));
                } else {
                    owned.push(("settleCoin".to_owned(), "USDT".to_owned()));
                }
                let pairs = owned
                    .iter()
                    .map(|(key, value)| (key.as_str(), value.as_str()))
                    .collect::<Vec<_>>();
                let payload = bybit_signed_get_text(
                    context,
                    "/v5/order/realtime",
                    &pairs,
                    "bybit.open_orders",
                )
                .await?;
                adapter.parse_open_orders_snapshot(&payload, timestamp_now_ms())?
            }
        };

        let orders_for_state = orders.clone();
        context.shared.write(|state| {
            state.replace_open_orders(orders_for_state);
            state.mark_rest_success(None);
        });
        Ok(orders)
    }
    .await;
    record_runtime_latency(context, RuntimeOperation::RefreshOpenOrders, started_at);
    result
}

pub(crate) async fn refresh_executions(
    context: &LiveContext,
    request: Option<&ListExecutionsRequest>,
) -> Result<Vec<Execution>> {
    let started_at = Instant::now();
    let result = async {
        let executions = match &context.adapter {
            #[cfg(feature = "binance")]
            AdapterHandle::Binance(adapter) => {
                let mut all = Vec::new();
                for spec in execution_specs(context, request).await? {
                    let limit = request
                        .and_then(|request| request.limit)
                        .unwrap_or(100)
                        .to_string();
                    let payload = binance_signed_request_text(
                        context,
                        Method::GET,
                        "/fapi/v1/userTrades",
                        &[
                            ("symbol", spec.native_symbol.as_ref()),
                            ("limit", limit.as_str()),
                        ],
                        "binance.executions",
                    )
                    .await?;
                    all.extend(adapter.parse_executions_snapshot(&payload)?);
                }
                all
            }
            #[cfg(feature = "bybit")]
            AdapterHandle::Bybit(adapter) => {
                let mut query = vec![("category".to_owned(), "linear".to_owned())];
                if let Some(request) = request {
                    if let Some(instrument_id) = &request.instrument_id {
                        let spec = require_spec(context, instrument_id)?;
                        query.push(("symbol".to_owned(), spec.native_symbol.to_string()));
                    } else {
                        query.push(("settleCoin".to_owned(), "USDT".to_owned()));
                    }
                    if let Some(limit) = request.limit {
                        query.push(("limit".to_owned(), limit.to_string()));
                    }
                } else {
                    query.push(("settleCoin".to_owned(), "USDT".to_owned()));
                    query.push(("limit".to_owned(), "100".to_owned()));
                }
                let pairs = query
                    .iter()
                    .map(|(key, value)| (key.as_str(), value.as_str()))
                    .collect::<Vec<_>>();
                let payload = bybit_signed_get_text(
                    context,
                    "/v5/execution/list",
                    &pairs,
                    "bybit.executions",
                )
                .await?;
                adapter.parse_executions_snapshot(&payload)?
            }
        };

        let executions_for_state = executions.clone();
        context.shared.write(|state| {
            state.merge_executions(executions_for_state);
            state.mark_rest_success(None);
        });
        Ok(executions)
    }
    .await;
    record_runtime_latency(context, RuntimeOperation::RefreshExecutions, started_at);
    result
}

pub(crate) async fn get_order(context: &LiveContext, request: &GetOrderRequest) -> Result<Order> {
    let started_at = Instant::now();
    let result = async {
        let spec = require_spec(context, &request.instrument_id)?;
        let order = match &context.adapter {
            #[cfg(feature = "binance")]
            AdapterHandle::Binance(adapter) => {
                let (path, operation, query) =
                    if is_binance_algo_order_id(request.order_id.as_ref()) {
                        (
                            "/fapi/v1/algoOrder",
                            "binance.get_algo_order",
                            binance_algo_identity_query(
                                request.order_id.as_ref(),
                                request.client_order_id.as_ref(),
                            )?,
                        )
                    } else {
                        (
                            "/fapi/v1/order",
                            "binance.get_order",
                            order_identity_query(
                                &spec.native_symbol,
                                request.order_id.as_ref(),
                                request.client_order_id.as_ref(),
                            )?,
                        )
                    };
                let pairs = query
                    .iter()
                    .map(|(key, value)| (key.as_str(), value.as_str()))
                    .collect::<Vec<_>>();
                let payload =
                    binance_signed_request_text(context, Method::GET, path, &pairs, operation)
                        .await?;
                adapter.parse_order_snapshot(&payload, timestamp_now_ms())?
            }
            #[cfg(feature = "bybit")]
            AdapterHandle::Bybit(adapter) => {
                let mut query = vec![
                    ("category".to_owned(), "linear".to_owned()),
                    ("symbol".to_owned(), spec.native_symbol.to_string()),
                ];
                append_bybit_order_identity(
                    &mut query,
                    request.order_id.as_ref(),
                    request.client_order_id.as_ref(),
                )?;
                let pairs = query
                    .iter()
                    .map(|(key, value)| (key.as_str(), value.as_str()))
                    .collect::<Vec<_>>();
                let payload =
                    bybit_signed_get_text(context, "/v5/order/realtime", &pairs, "bybit.get_order")
                        .await?;
                adapter.parse_order_snapshot(&payload, timestamp_now_ms())?
            }
        };

        context
            .shared
            .apply_private_event(PrivateLaneEvent::Order(order.clone()));
        context.shared.write(|state| {
            state.mark_rest_success(None);
        });
        Ok(order)
    }
    .await;
    record_runtime_latency(context, RuntimeOperation::GetOrder, started_at);
    result
}

pub(crate) async fn create_order(
    context: &LiveContext,
    request: &CreateOrderRequest,
) -> Result<CommandAck> {
    let started_at = Instant::now();
    let result = async {
        validate_create_order(context, request)?;
        context.command_limiter.acquire().await;
        let (receipt, transport) = match &context.adapter {
        #[cfg(feature = "binance")]
        AdapterHandle::Binance(adapter) => {
            let spec = require_spec(context, &request.instrument_id)?;
            if binance_create_uses_algo_order_endpoint(request) {
                if context.command_transport_mode.is_websocket_only() {
                    return Err(unsupported_ws_command(context, "create_order_ws"));
                }
                let pairs =
                    build_binance_algo_create_pairs(&spec.native_symbol, request);
                let pairs = pairs
                    .iter()
                    .map(|(key, value)| (key.as_str(), value.as_str()))
                    .collect::<Vec<_>>();
                match binance_signed_request_text(
                    context,
                    Method::POST,
                    "/fapi/v1/algoOrder",
                    &pairs,
                    "binance.create_algo_order",
                )
                .await
                {
                    Ok(payload) => (
                        adapter.classify_command(
                            CommandOperation::CreateOrder,
                            Some(&payload),
                            request.request_id.clone(),
                        )?,
                        CommandTransport::Rest,
                    ),
                    Err(error) if is_uncertain_command_error(&error) => (
                        adapter.classify_command(
                            CommandOperation::CreateOrder,
                            None,
                            request.request_id.clone(),
                        )?,
                        CommandTransport::Rest,
                    ),
                    Err(error) => return Err(error),
                }
            } else {
                let mut owned = vec![
                    ("symbol".to_owned(), spec.native_symbol.to_string()),
                    ("side".to_owned(), binance_side(request.side).to_owned()),
                    (
                        "type".to_owned(),
                        binance_order_type(request.order_type).to_owned(),
                    ),
                    ("quantity".to_owned(), format_quantity(request.quantity)),
                ];
                if let Some(client_order_id) = &request.client_order_id {
                    owned.push(("newClientOrderId".to_owned(), client_order_id.to_string()));
                }
                if let Some(price) = request.price {
                    owned.push(("price".to_owned(), format_price(price)));
                }
                if let Some(trigger_price) = request.trigger_price {
                    owned.push(("stopPrice".to_owned(), format_price(trigger_price)));
                }
                if let Some(trigger_type) = request.trigger_type {
                    owned.push((
                        "workingType".to_owned(),
                        binance_trigger_type(trigger_type).to_owned(),
                    ));
                }
                if let Some(time_in_force) = request.time_in_force {
                    owned.push((
                        "timeInForce".to_owned(),
                        binance_time_in_force(time_in_force, request.post_only).to_owned(),
                    ));
                }
                if request.reduce_only {
                    owned.push(("reduceOnly".to_owned(), "true".to_owned()));
                }
                if context.adapter.capabilities().native.ws_order_entry
                    && binance_create_prefers_ws(request)
                {
                match binance_signed_ws_request_text(
                    context,
                    "order.place",
                    owned.clone(),
                    "binance.create_order_ws",
                )
                .await
                {
                    Ok(payload) => (
                        adapter.classify_command(
                            CommandOperation::CreateOrder,
                            Some(&payload),
                            request.request_id.clone(),
                        )?,
                        CommandTransport::WebSocket,
                    ),
                    Err(CommandWsRequestError::Uncertain(_)) => (
                        adapter.classify_command(
                            CommandOperation::CreateOrder,
                            None,
                            request.request_id.clone(),
                        )?,
                        CommandTransport::WebSocket,
                    ),
                    Err(CommandWsRequestError::Unavailable(error)) => {
                        if context.command_transport_mode.is_websocket_only() {
                            return Err(error);
                        }
                        let pairs = owned
                            .iter()
                            .map(|(key, value)| (key.as_str(), value.as_str()))
                            .collect::<Vec<_>>();
                        match binance_signed_request_text(
                            context,
                            Method::POST,
                            "/fapi/v1/order",
                            &pairs,
                            "binance.create_order",
                        )
                        .await
                        {
                            Ok(payload) => (
                                adapter.classify_command(
                                    CommandOperation::CreateOrder,
                                    Some(&payload),
                                    request.request_id.clone(),
                                )?,
                                CommandTransport::Rest,
                            ),
                            Err(error) if is_uncertain_command_error(&error) => (
                                adapter.classify_command(
                                    CommandOperation::CreateOrder,
                                    None,
                                    request.request_id.clone(),
                                )?,
                                CommandTransport::Rest,
                            ),
                            Err(error) => return Err(error),
                        }
                    }
                }
                } else {
                    if context.command_transport_mode.is_websocket_only() {
                        return Err(unsupported_ws_command(context, "create_order_ws"));
                    }
                    let pairs = owned
                        .iter()
                        .map(|(key, value)| (key.as_str(), value.as_str()))
                        .collect::<Vec<_>>();
                    match binance_signed_request_text(
                        context,
                        Method::POST,
                        "/fapi/v1/order",
                        &pairs,
                        "binance.create_order",
                    )
                    .await
                    {
                        Ok(payload) => (
                            adapter.classify_command(
                                CommandOperation::CreateOrder,
                                Some(&payload),
                                request.request_id.clone(),
                            )?,
                            CommandTransport::Rest,
                        ),
                        Err(error) if is_uncertain_command_error(&error) => (
                            adapter.classify_command(
                                CommandOperation::CreateOrder,
                                None,
                                request.request_id.clone(),
                            )?,
                            CommandTransport::Rest,
                        ),
                        Err(error) => return Err(error),
                    }
                }
            }
        }
        #[cfg(feature = "bybit")]
        AdapterHandle::Bybit(adapter) => {
            let spec = require_spec(context, &request.instrument_id)?;
            let body_value = json!({
                "category": "linear",
                "symbol": spec.native_symbol,
                "side": bybit_side(request.side),
                "orderType": bybit_order_type(request.order_type),
                "qty": format_quantity(request.quantity),
                "price": request.price.map(format_price),
                "triggerPrice": request.trigger_price.map(format_price),
                "triggerBy": request.trigger_type.map(bybit_trigger_type),
                "timeInForce": request.time_in_force.map(|value| bybit_time_in_force(value, request.post_only)),
                "orderLinkId": request.client_order_id.as_ref().map(ToString::to_string),
                "reduceOnly": request.reduce_only,
            });
            if context.adapter.capabilities().native.ws_order_entry {
                match bybit_trade_ws_request_text(
                    context,
                    "order.create",
                    vec![body_value.clone()],
                    "bybit.create_order_ws",
                )
                .await
                {
                    Ok(payload) => (
                        adapter.classify_command(
                            CommandOperation::CreateOrder,
                            Some(&payload),
                            request.request_id.clone(),
                        )?,
                        CommandTransport::WebSocket,
                    ),
                    Err(CommandWsRequestError::Uncertain(_)) => (
                        adapter.classify_command(
                            CommandOperation::CreateOrder,
                            None,
                            request.request_id.clone(),
                        )?,
                        CommandTransport::WebSocket,
                    ),
                    Err(CommandWsRequestError::Unavailable(error)) => {
                        if context.command_transport_mode.is_websocket_only() {
                            return Err(error);
                        }
                        let body = serde_json::to_string(&body_value).map_err(|error| {
                            MarketError::new(
                                ErrorKind::ConfigError,
                                format!("failed to serialize bybit create order body: {error}"),
                            )
                        })?;
                        match bybit_signed_post_text(
                            context,
                            "/v5/order/create",
                            &body,
                            "bybit.create_order",
                        )
                        .await
                        {
                            Ok(payload) => (
                                adapter.classify_command(
                                    CommandOperation::CreateOrder,
                                    Some(&payload),
                                    request.request_id.clone(),
                                )?,
                                CommandTransport::Rest,
                            ),
                            Err(error) if is_uncertain_command_error(&error) => (
                                adapter.classify_command(
                                    CommandOperation::CreateOrder,
                                    None,
                                    request.request_id.clone(),
                                )?,
                                CommandTransport::Rest,
                            ),
                            Err(error) => return Err(error),
                        }
                    }
                }
            } else {
                if context.command_transport_mode.is_websocket_only() {
                    return Err(unsupported_ws_command(context, "create_order_ws"));
                }
                let body = serde_json::to_string(&body_value).map_err(|error| {
                    MarketError::new(
                        ErrorKind::ConfigError,
                        format!("failed to serialize bybit create order body: {error}"),
                    )
                })?;
                match bybit_signed_post_text(context, "/v5/order/create", &body, "bybit.create_order")
                    .await
                {
                    Ok(payload) => (
                        adapter.classify_command(
                            CommandOperation::CreateOrder,
                            Some(&payload),
                            request.request_id.clone(),
                        )?,
                        CommandTransport::Rest,
                    ),
                    Err(error) if is_uncertain_command_error(&error) => (
                        adapter.classify_command(
                            CommandOperation::CreateOrder,
                            None,
                            request.request_id.clone(),
                        )?,
                        CommandTransport::Rest,
                    ),
                    Err(error) => return Err(error),
                }
            }
        }
    };

    let receipt = hydrate_create_receipt(receipt, request);
    if receipt.status == CommandStatus::UnknownExecution {
        context
            .runtime_state
            .cache_pending_unknown(PendingUnknownCommand {
                operation: CommandOperation::CreateOrder,
                instrument_id: request.instrument_id.clone(),
                order_id: None,
                client_order_id: request.client_order_id.clone(),
                recorded_at: timestamp_now_ms(),
            })
                .await;
    }
    let ack = apply_command_receipt(context, receipt.clone(), transport).await;
        Ok(ack)
    }
    .await;
    record_runtime_latency(context, RuntimeOperation::CreateOrder, started_at);
    result
}

pub(crate) async fn create_orders(
    context: &LiveContext,
    request: &CreateOrdersRequest,
) -> Result<Vec<CommandAck>> {
    let started_at = Instant::now();
    let result = async {
        validate_create_orders(context, request)?;
        if request.orders.len() == 1 {
            let single = request.orders[0].clone();
            let single = hydrate_create_request_with_batch_id(single, request.request_id.clone());
            return Ok(vec![create_order(context, &single).await?]);
        }

        let mut acks = vec![None; request.orders.len()];
        match &context.adapter {
            #[cfg(feature = "binance")]
            AdapterHandle::Binance(adapter) => {
                if context.adapter.capabilities().native.ws_order_entry {
                    let indexed = request.orders.iter().enumerate().collect::<Vec<_>>();
                    for chunk in indexed.chunks(5) {
                        let chunk_requests = chunk
                            .iter()
                            .map(|(_, order)| {
                                hydrate_create_request_with_batch_id(
                                    (*order).clone(),
                                    request.request_id.clone(),
                                )
                            })
                            .collect::<Vec<_>>();
                        let chunk_acks = try_join_all(
                            chunk_requests
                                .iter()
                                .map(|order| create_order(context, order)),
                        )
                        .await?;
                        for ((index, _), ack) in chunk.iter().zip(chunk_acks) {
                            acks[*index] = Some(ack);
                        }
                    }
                } else {
                    if context.command_transport_mode.is_websocket_only() {
                        return Err(unsupported_ws_command(context, "create_orders_ws"));
                    }
                    for chunk in request
                        .orders
                        .iter()
                        .enumerate()
                        .collect::<Vec<_>>()
                        .chunks(5)
                    {
                        context.command_limiter.acquire().await;
                        let chunk_requests = chunk
                            .iter()
                            .map(|(_, order)| {
                                hydrate_create_request_with_batch_id(
                                    (*order).clone(),
                                    request.request_id.clone(),
                                )
                            })
                            .collect::<Vec<_>>();
                        let batch_orders = chunk_requests
                            .iter()
                            .map(|order| build_binance_batch_create_object(context, order))
                            .collect::<Result<Vec<_>>>()?;
                        let batch_json = serde_json::to_string(&batch_orders).map_err(|error| {
                            MarketError::new(
                                ErrorKind::ConfigError,
                                format!("failed to serialize binance batch create body: {error}"),
                            )
                        })?;
                        let pairs = [("batchOrders", batch_json.as_str())];
                        let chunk_receipts = match binance_signed_request_text(
                            context,
                            Method::POST,
                            "/fapi/v1/batchOrders",
                            &pairs,
                            "binance.create_orders",
                        )
                        .await
                        {
                            Ok(payload) => classify_binance_batch_payload(
                                adapter,
                                CommandOperation::CreateOrder,
                                &payload,
                                &chunk_requests
                                    .iter()
                                    .map(|order| order.request_id.clone())
                                    .collect::<Vec<_>>(),
                            )?,
                            Err(error) if is_uncertain_command_error(&error) => chunk_requests
                                .iter()
                                .map(|order| {
                                    unknown_command_receipt(
                                        context,
                                        CommandOperation::CreateOrder,
                                        Some(order.instrument_id.clone()),
                                        None,
                                        order.client_order_id.clone(),
                                        order.request_id.clone(),
                                    )
                                })
                                .collect(),
                            Err(error) => return Err(error),
                        };
                        if chunk_receipts.len() != chunk.len() {
                            return Err(MarketError::new(
                                ErrorKind::DecodeError,
                                format!(
                                    "binance batch create returned {} receipts for {} requests",
                                    chunk_receipts.len(),
                                    chunk.len()
                                ),
                            ));
                        }
                        let chunk_acks = cache_and_emit_create_receipts(
                            context,
                            &chunk_receipts,
                            &chunk_requests,
                            CommandTransport::Rest,
                        )
                        .await;
                        for ((index, _), ack) in chunk.iter().zip(chunk_acks) {
                            acks[*index] = Some(ack);
                        }
                    }
                }
            }
            #[cfg(feature = "bybit")]
            AdapterHandle::Bybit(_) => {
                for chunk in request
                    .orders
                    .iter()
                    .enumerate()
                    .collect::<Vec<_>>()
                    .chunks(20)
                {
                    context.command_limiter.acquire().await;
                    let chunk_requests = chunk
                        .iter()
                        .map(|(_, order)| {
                            hydrate_create_request_with_batch_id(
                                (*order).clone(),
                                request.request_id.clone(),
                            )
                        })
                        .collect::<Vec<_>>();
                    let batch = chunk_requests
                        .iter()
                        .map(|order| build_bybit_batch_create_object(context, order))
                        .collect::<Result<Vec<_>>>()?;
                    let batch_body = json!({
                        "category": "linear",
                        "request": batch,
                    });
                    let body = serde_json::to_string(&batch_body).map_err(|error| {
                        MarketError::new(
                            ErrorKind::ConfigError,
                            format!("failed to serialize bybit batch create body: {error}"),
                        )
                    })?;
                    let identities = chunk_requests
                        .iter()
                        .map(|order| BatchIdentity {
                            instrument_id: Some(order.instrument_id.clone()),
                            order_id: None,
                            client_order_id: order.client_order_id.clone(),
                            #[cfg(feature = "bybit")]
                            request_id: order.request_id.clone(),
                        })
                        .collect::<Vec<_>>();
                    let (chunk_receipts, transport) =
                        if context.adapter.capabilities().native.ws_order_entry {
                            match bybit_trade_ws_request_text(
                                context,
                                "order.create-batch",
                                vec![batch_body.clone()],
                                "bybit.create_orders_ws",
                            )
                            .await
                            {
                                Ok(payload) => (
                                    classify_bybit_batch_payload(
                                        context,
                                        CommandOperation::CreateOrder,
                                        &payload,
                                        &identities,
                                    )?,
                                    CommandTransport::WebSocket,
                                ),
                                Err(CommandWsRequestError::Uncertain(_)) => (
                                    chunk_requests
                                        .iter()
                                        .map(|order| {
                                            unknown_command_receipt(
                                                context,
                                                CommandOperation::CreateOrder,
                                                Some(order.instrument_id.clone()),
                                                None,
                                                order.client_order_id.clone(),
                                                order.request_id.clone(),
                                            )
                                        })
                                        .collect(),
                                    CommandTransport::WebSocket,
                                ),
                                Err(CommandWsRequestError::Unavailable(error)) => {
                                    if context.command_transport_mode.is_websocket_only() {
                                        return Err(error);
                                    }
                                    match bybit_signed_post_text(
                                        context,
                                        "/v5/order/create-batch",
                                        &body,
                                        "bybit.create_orders",
                                    )
                                    .await
                                    {
                                        Ok(payload) => (
                                            classify_bybit_batch_payload(
                                                context,
                                                CommandOperation::CreateOrder,
                                                &payload,
                                                &identities,
                                            )?,
                                            CommandTransport::Rest,
                                        ),
                                        Err(error) if is_uncertain_command_error(&error) => (
                                            chunk_requests
                                                .iter()
                                                .map(|order| {
                                                    unknown_command_receipt(
                                                        context,
                                                        CommandOperation::CreateOrder,
                                                        Some(order.instrument_id.clone()),
                                                        None,
                                                        order.client_order_id.clone(),
                                                        order.request_id.clone(),
                                                    )
                                                })
                                                .collect(),
                                            CommandTransport::Rest,
                                        ),
                                        Err(error) => return Err(error),
                                    }
                                }
                            }
                        } else {
                            if context.command_transport_mode.is_websocket_only() {
                                return Err(unsupported_ws_command(context, "create_orders_ws"));
                            }
                            match bybit_signed_post_text(
                                context,
                                "/v5/order/create-batch",
                                &body,
                                "bybit.create_orders",
                            )
                            .await
                            {
                                Ok(payload) => (
                                    classify_bybit_batch_payload(
                                        context,
                                        CommandOperation::CreateOrder,
                                        &payload,
                                        &identities,
                                    )?,
                                    CommandTransport::Rest,
                                ),
                                Err(error) if is_uncertain_command_error(&error) => (
                                    chunk_requests
                                        .iter()
                                        .map(|order| {
                                            unknown_command_receipt(
                                                context,
                                                CommandOperation::CreateOrder,
                                                Some(order.instrument_id.clone()),
                                                None,
                                                order.client_order_id.clone(),
                                                order.request_id.clone(),
                                            )
                                        })
                                        .collect(),
                                    CommandTransport::Rest,
                                ),
                                Err(error) => return Err(error),
                            }
                        };
                    if chunk_receipts.len() != chunk.len() {
                        return Err(MarketError::new(
                            ErrorKind::DecodeError,
                            format!(
                                "bybit batch create returned {} receipts for {} requests",
                                chunk_receipts.len(),
                                chunk.len()
                            ),
                        ));
                    }
                    let chunk_acks = cache_and_emit_create_receipts(
                        context,
                        &chunk_receipts,
                        &chunk_requests,
                        transport,
                    )
                    .await;
                    for ((index, _), ack) in chunk.iter().zip(chunk_acks) {
                        acks[*index] = Some(ack);
                    }
                }
            }
        }

        collect_batch_acks(acks, "create_orders")
    }
    .await;
    record_runtime_latency(context, RuntimeOperation::CreateOrders, started_at);
    result
}

#[cfg(feature = "binance")]
fn binance_create_prefers_ws(request: &CreateOrderRequest) -> bool {
    // On live USDⓈ-M testing subaccounts, conditional algo orders can still be rejected on the
    // websocket trade path despite the current public docs listing them under `order.place`.
    // Keep the hot path on WS for Market/Limit, but route slower protective/conditional flow
    // through the stable REST path until the live behavior is proven end-to-end.
    !binance_create_uses_algo_order_endpoint(request)
}

#[cfg(feature = "binance")]
fn binance_create_uses_algo_order_endpoint(request: &CreateOrderRequest) -> bool {
    matches!(
        request.order_type,
        OrderType::StopMarket
            | OrderType::StopLimit
            | OrderType::TakeProfitMarket
            | OrderType::TakeProfitLimit
    )
}

#[cfg(feature = "binance")]
fn build_binance_algo_create_pairs(
    native_symbol: &str,
    request: &CreateOrderRequest,
) -> Vec<(String, String)> {
    let mut owned = vec![
        ("algoType".to_owned(), "CONDITIONAL".to_owned()),
        ("symbol".to_owned(), native_symbol.to_owned()),
        ("side".to_owned(), binance_side(request.side).to_owned()),
        (
            "type".to_owned(),
            binance_order_type(request.order_type).to_owned(),
        ),
        ("quantity".to_owned(), format_quantity(request.quantity)),
    ];
    if let Some(client_order_id) = &request.client_order_id {
        owned.push(("clientAlgoId".to_owned(), client_order_id.to_string()));
    }
    if let Some(price) = request.price {
        owned.push(("price".to_owned(), format_price(price)));
    }
    if let Some(trigger_price) = request.trigger_price {
        owned.push(("triggerPrice".to_owned(), format_price(trigger_price)));
    }
    if let Some(trigger_type) = request.trigger_type {
        owned.push((
            "workingType".to_owned(),
            binance_trigger_type(trigger_type).to_owned(),
        ));
    }
    if let Some(time_in_force) = request.time_in_force {
        owned.push((
            "timeInForce".to_owned(),
            binance_time_in_force(time_in_force, request.post_only).to_owned(),
        ));
    }
    if request.reduce_only {
        owned.push(("reduceOnly".to_owned(), "true".to_owned()));
    }
    owned
}

pub(crate) async fn amend_order(
    context: &LiveContext,
    request: &AmendOrderRequest,
) -> Result<CommandAck> {
    let started_at = Instant::now();
    let result = async {
        validate_amend_order(context, request).await?;
        context.command_limiter.acquire().await;
        let (receipt, transport) = match &context.adapter {
            #[cfg(feature = "binance")]
            AdapterHandle::Binance(adapter) => {
                let spec = require_spec(context, &request.instrument_id)?;
                let current_order = resolve_order_for_amend(context, request).await?;
                if current_order.order_type != OrderType::Limit {
                    return Err(MarketError::new(
                        ErrorKind::Unsupported,
                        "binance amend currently supports only LIMIT orders",
                    ));
                }
                if request.trigger_price.is_some() {
                    return Err(MarketError::new(
                        ErrorKind::Unsupported,
                        "binance amend does not support modifying trigger_price on linear futures",
                    ));
                }
                let price = request.price.or(current_order.price).ok_or_else(|| {
                    MarketError::new(
                        ErrorKind::ConfigError,
                        "binance amend requires a limit price or a cached order price",
                    )
                })?;
                let quantity = request.quantity.unwrap_or(current_order.quantity);
                let mut owned = vec![
                    ("symbol".to_owned(), spec.native_symbol.to_string()),
                    (
                        "side".to_owned(),
                        binance_side(current_order.side).to_owned(),
                    ),
                    ("quantity".to_owned(), format_quantity(quantity)),
                    ("price".to_owned(), format_price(price)),
                ];
                append_order_identity(
                    &mut owned,
                    request.order_id.as_ref(),
                    request.client_order_id.as_ref(),
                )?;
                if context.adapter.capabilities().native.ws_order_entry {
                    let mut ws_params = owned.clone();
                    ws_params.push((
                        "origType".to_owned(),
                        binance_order_type(current_order.order_type).to_owned(),
                    ));
                    match binance_signed_ws_request_text(
                        context,
                        "order.modify",
                        ws_params,
                        "binance.amend_order_ws",
                    )
                    .await
                    {
                        Ok(payload) => (
                            adapter.classify_command(
                                CommandOperation::AmendOrder,
                                Some(&payload),
                                request.request_id.clone(),
                            )?,
                            CommandTransport::WebSocket,
                        ),
                        Err(CommandWsRequestError::Uncertain(_)) => (
                            adapter.classify_command(
                                CommandOperation::AmendOrder,
                                None,
                                request.request_id.clone(),
                            )?,
                            CommandTransport::WebSocket,
                        ),
                        Err(CommandWsRequestError::Unavailable(error)) => {
                            if context.command_transport_mode.is_websocket_only() {
                                return Err(error);
                            }
                            let pairs = owned
                                .iter()
                                .map(|(key, value)| (key.as_str(), value.as_str()))
                                .collect::<Vec<_>>();
                            match binance_signed_request_text(
                                context,
                                Method::PUT,
                                "/fapi/v1/order",
                                &pairs,
                                "binance.amend_order",
                            )
                            .await
                            {
                                Ok(payload) => (
                                    adapter.classify_command(
                                        CommandOperation::AmendOrder,
                                        Some(&payload),
                                        request.request_id.clone(),
                                    )?,
                                    CommandTransport::Rest,
                                ),
                                Err(error) if is_uncertain_command_error(&error) => (
                                    adapter.classify_command(
                                        CommandOperation::AmendOrder,
                                        None,
                                        request.request_id.clone(),
                                    )?,
                                    CommandTransport::Rest,
                                ),
                                Err(error) => return Err(error),
                            }
                        }
                    }
                } else {
                    if context.command_transport_mode.is_websocket_only() {
                        return Err(unsupported_ws_command(context, "edit_order_ws"));
                    }
                    let pairs = owned
                        .iter()
                        .map(|(key, value)| (key.as_str(), value.as_str()))
                        .collect::<Vec<_>>();
                    match binance_signed_request_text(
                        context,
                        Method::PUT,
                        "/fapi/v1/order",
                        &pairs,
                        "binance.amend_order",
                    )
                    .await
                    {
                        Ok(payload) => (
                            adapter.classify_command(
                                CommandOperation::AmendOrder,
                                Some(&payload),
                                request.request_id.clone(),
                            )?,
                            CommandTransport::Rest,
                        ),
                        Err(error) if is_uncertain_command_error(&error) => (
                            adapter.classify_command(
                                CommandOperation::AmendOrder,
                                None,
                                request.request_id.clone(),
                            )?,
                            CommandTransport::Rest,
                        ),
                        Err(error) => return Err(error),
                    }
                }
            }
            #[cfg(feature = "bybit")]
            AdapterHandle::Bybit(adapter) => {
                let spec = require_spec(context, &request.instrument_id)?;
                let mut body = vec![
                    ("category".to_owned(), "linear".to_owned()),
                    ("symbol".to_owned(), spec.native_symbol.to_string()),
                ];
                append_bybit_order_identity(
                    &mut body,
                    request.order_id.as_ref(),
                    request.client_order_id.as_ref(),
                )?;
                if let Some(quantity) = request.quantity {
                    body.push(("qty".to_owned(), format_quantity(quantity)));
                }
                if let Some(price) = request.price {
                    body.push(("price".to_owned(), format_price(price)));
                }
                if let Some(trigger_price) = request.trigger_price {
                    body.push(("triggerPrice".to_owned(), format_price(trigger_price)));
                }
                let body = body_to_object(body);
                if context.adapter.capabilities().native.ws_order_entry {
                    match bybit_trade_ws_request_text(
                        context,
                        "order.amend",
                        vec![Value::Object(body.clone())],
                        "bybit.amend_order_ws",
                    )
                    .await
                    {
                        Ok(payload) => (
                            adapter.classify_command(
                                CommandOperation::AmendOrder,
                                Some(&payload),
                                request.request_id.clone(),
                            )?,
                            CommandTransport::WebSocket,
                        ),
                        Err(CommandWsRequestError::Uncertain(_)) => (
                            adapter.classify_command(
                                CommandOperation::AmendOrder,
                                None,
                                request.request_id.clone(),
                            )?,
                            CommandTransport::WebSocket,
                        ),
                        Err(CommandWsRequestError::Unavailable(error)) => {
                            if context.command_transport_mode.is_websocket_only() {
                                return Err(error);
                            }
                            let body = serde_json::to_string(&body).map_err(|error| {
                                MarketError::new(
                                    ErrorKind::ConfigError,
                                    format!("failed to serialize bybit amend order body: {error}"),
                                )
                            })?;
                            match bybit_signed_post_text(
                                context,
                                "/v5/order/amend",
                                &body,
                                "bybit.amend_order",
                            )
                            .await
                            {
                                Ok(payload) => (
                                    adapter.classify_command(
                                        CommandOperation::AmendOrder,
                                        Some(&payload),
                                        request.request_id.clone(),
                                    )?,
                                    CommandTransport::Rest,
                                ),
                                Err(error) if is_uncertain_command_error(&error) => (
                                    adapter.classify_command(
                                        CommandOperation::AmendOrder,
                                        None,
                                        request.request_id.clone(),
                                    )?,
                                    CommandTransport::Rest,
                                ),
                                Err(error) => return Err(error),
                            }
                        }
                    }
                } else {
                    if context.command_transport_mode.is_websocket_only() {
                        return Err(unsupported_ws_command(context, "edit_order_ws"));
                    }
                    let body = serde_json::to_string(&body).map_err(|error| {
                        MarketError::new(
                            ErrorKind::ConfigError,
                            format!("failed to serialize bybit amend order body: {error}"),
                        )
                    })?;
                    match bybit_signed_post_text(
                        context,
                        "/v5/order/amend",
                        &body,
                        "bybit.amend_order",
                    )
                    .await
                    {
                        Ok(payload) => (
                            adapter.classify_command(
                                CommandOperation::AmendOrder,
                                Some(&payload),
                                request.request_id.clone(),
                            )?,
                            CommandTransport::Rest,
                        ),
                        Err(error) if is_uncertain_command_error(&error) => (
                            adapter.classify_command(
                                CommandOperation::AmendOrder,
                                None,
                                request.request_id.clone(),
                            )?,
                            CommandTransport::Rest,
                        ),
                        Err(error) => return Err(error),
                    }
                }
            }
        };

        let receipt = hydrate_amend_receipt(receipt, request);
        if receipt.status == CommandStatus::UnknownExecution {
            context
                .runtime_state
                .cache_pending_unknown(PendingUnknownCommand {
                    operation: CommandOperation::AmendOrder,
                    instrument_id: request.instrument_id.clone(),
                    order_id: request.order_id.clone(),
                    client_order_id: request.client_order_id.clone(),
                    recorded_at: timestamp_now_ms(),
                })
                .await;
        }
        let ack = apply_command_receipt(context, receipt.clone(), transport).await;
        Ok(ack)
    }
    .await;
    record_runtime_latency(context, RuntimeOperation::AmendOrder, started_at);
    result
}

pub(crate) async fn amend_orders(
    context: &LiveContext,
    request: &AmendOrdersRequest,
) -> Result<Vec<CommandAck>> {
    let started_at = Instant::now();
    let result = async {
        validate_amend_orders(context, request).await?;
        if request.orders.len() == 1 {
            let single = request.orders[0].clone();
            let single = hydrate_amend_request_with_batch_id(single, request.request_id.clone());
            return Ok(vec![amend_order(context, &single).await?]);
        }

        let mut acks = vec![None; request.orders.len()];
        match &context.adapter {
            #[cfg(feature = "binance")]
            AdapterHandle::Binance(adapter) => {
                let indexed = request.orders.iter().enumerate().collect::<Vec<_>>();
                if context.adapter.capabilities().native.ws_order_entry {
                    for chunk in indexed.chunks(5) {
                        let chunk_requests = chunk
                            .iter()
                            .map(|(_, order)| {
                                hydrate_amend_request_with_batch_id(
                                    (*order).clone(),
                                    request.request_id.clone(),
                                )
                            })
                            .collect::<Vec<_>>();
                        let chunk_acks = try_join_all(
                            chunk_requests
                                .iter()
                                .map(|order| amend_order(context, order)),
                        )
                        .await?;
                        for ((index, _), ack) in chunk.iter().zip(chunk_acks) {
                            acks[*index] = Some(ack);
                        }
                    }
                } else {
                    if context.command_transport_mode.is_websocket_only() {
                        return Err(unsupported_ws_command(context, "edit_orders_ws"));
                    }
                    for chunk in indexed.chunks(5) {
                        context.command_limiter.acquire().await;
                        let chunk_requests = chunk
                            .iter()
                            .map(|(_, order)| {
                                hydrate_amend_request_with_batch_id(
                                    (*order).clone(),
                                    request.request_id.clone(),
                                )
                            })
                            .collect::<Vec<_>>();
                        let batch_orders = chunk_requests
                            .iter()
                            .map(|order| build_binance_batch_amend_object(context, order))
                            .collect::<Result<Vec<_>>>()?;
                        let batch_json = serde_json::to_string(&batch_orders).map_err(|error| {
                            MarketError::new(
                                ErrorKind::ConfigError,
                                format!("failed to serialize binance batch amend body: {error}"),
                            )
                        })?;
                        let pairs = [("batchOrders", batch_json.as_str())];
                        let chunk_receipts = match binance_signed_request_text(
                            context,
                            Method::PUT,
                            "/fapi/v1/batchOrders",
                            &pairs,
                            "binance.amend_orders",
                        )
                        .await
                        {
                            Ok(payload) => classify_binance_batch_payload(
                                adapter,
                                CommandOperation::AmendOrder,
                                &payload,
                                &chunk_requests
                                    .iter()
                                    .map(|order| order.request_id.clone())
                                    .collect::<Vec<_>>(),
                            )?,
                            Err(error) if is_uncertain_command_error(&error) => chunk_requests
                                .iter()
                                .map(|order| {
                                    unknown_command_receipt(
                                        context,
                                        CommandOperation::AmendOrder,
                                        Some(order.instrument_id.clone()),
                                        order.order_id.clone(),
                                        order.client_order_id.clone(),
                                        order.request_id.clone(),
                                    )
                                })
                                .collect(),
                            Err(error) => return Err(error),
                        };
                        if chunk_receipts.len() != chunk.len() {
                            return Err(MarketError::new(
                                ErrorKind::DecodeError,
                                format!(
                                    "binance batch amend returned {} receipts for {} requests",
                                    chunk_receipts.len(),
                                    chunk.len()
                                ),
                            ));
                        }
                        let chunk_acks = cache_and_emit_amend_receipts(
                            context,
                            &chunk_receipts,
                            &chunk_requests,
                            CommandTransport::Rest,
                        )
                        .await;
                        for ((index, _), ack) in chunk.iter().zip(chunk_acks) {
                            acks[*index] = Some(ack);
                        }
                    }
                }
            }
            #[cfg(feature = "bybit")]
            AdapterHandle::Bybit(_) => {
                let indexed = request.orders.iter().enumerate().collect::<Vec<_>>();
                for chunk in indexed.chunks(20) {
                    context.command_limiter.acquire().await;
                    let chunk_requests = chunk
                        .iter()
                        .map(|(_, order)| {
                            hydrate_amend_request_with_batch_id(
                                (*order).clone(),
                                request.request_id.clone(),
                            )
                        })
                        .collect::<Vec<_>>();
                    let batch = chunk_requests
                        .iter()
                        .map(|order| build_bybit_batch_amend_object(context, order))
                        .collect::<Result<Vec<_>>>()?;
                    let batch_body = json!({
                        "category": "linear",
                        "request": batch,
                    });
                    let body = serde_json::to_string(&batch_body).map_err(|error| {
                        MarketError::new(
                            ErrorKind::ConfigError,
                            format!("failed to serialize bybit batch amend body: {error}"),
                        )
                    })?;
                    let identities = chunk_requests
                        .iter()
                        .map(|order| BatchIdentity {
                            instrument_id: Some(order.instrument_id.clone()),
                            order_id: order.order_id.clone(),
                            client_order_id: order.client_order_id.clone(),
                            #[cfg(feature = "bybit")]
                            request_id: order.request_id.clone(),
                        })
                        .collect::<Vec<_>>();
                    let (chunk_receipts, transport) =
                        if context.adapter.capabilities().native.ws_order_entry {
                            match bybit_trade_ws_request_text(
                                context,
                                "order.amend-batch",
                                vec![batch_body.clone()],
                                "bybit.amend_orders_ws",
                            )
                            .await
                            {
                                Ok(payload) => (
                                    classify_bybit_batch_payload(
                                        context,
                                        CommandOperation::AmendOrder,
                                        &payload,
                                        &identities,
                                    )?,
                                    CommandTransport::WebSocket,
                                ),
                                Err(CommandWsRequestError::Uncertain(_)) => (
                                    chunk_requests
                                        .iter()
                                        .map(|order| {
                                            unknown_command_receipt(
                                                context,
                                                CommandOperation::AmendOrder,
                                                Some(order.instrument_id.clone()),
                                                order.order_id.clone(),
                                                order.client_order_id.clone(),
                                                order.request_id.clone(),
                                            )
                                        })
                                        .collect(),
                                    CommandTransport::WebSocket,
                                ),
                                Err(CommandWsRequestError::Unavailable(error)) => {
                                    if context.command_transport_mode.is_websocket_only() {
                                        return Err(error);
                                    }
                                    match bybit_signed_post_text(
                                        context,
                                        "/v5/order/amend-batch",
                                        &body,
                                        "bybit.amend_orders",
                                    )
                                    .await
                                    {
                                        Ok(payload) => (
                                            classify_bybit_batch_payload(
                                                context,
                                                CommandOperation::AmendOrder,
                                                &payload,
                                                &identities,
                                            )?,
                                            CommandTransport::Rest,
                                        ),
                                        Err(error) if is_uncertain_command_error(&error) => (
                                            chunk_requests
                                                .iter()
                                                .map(|order| {
                                                    unknown_command_receipt(
                                                        context,
                                                        CommandOperation::AmendOrder,
                                                        Some(order.instrument_id.clone()),
                                                        order.order_id.clone(),
                                                        order.client_order_id.clone(),
                                                        order.request_id.clone(),
                                                    )
                                                })
                                                .collect(),
                                            CommandTransport::Rest,
                                        ),
                                        Err(error) => return Err(error),
                                    }
                                }
                            }
                        } else {
                            if context.command_transport_mode.is_websocket_only() {
                                return Err(unsupported_ws_command(context, "edit_orders_ws"));
                            }
                            match bybit_signed_post_text(
                                context,
                                "/v5/order/amend-batch",
                                &body,
                                "bybit.amend_orders",
                            )
                            .await
                            {
                                Ok(payload) => (
                                    classify_bybit_batch_payload(
                                        context,
                                        CommandOperation::AmendOrder,
                                        &payload,
                                        &identities,
                                    )?,
                                    CommandTransport::Rest,
                                ),
                                Err(error) if is_uncertain_command_error(&error) => (
                                    chunk_requests
                                        .iter()
                                        .map(|order| {
                                            unknown_command_receipt(
                                                context,
                                                CommandOperation::AmendOrder,
                                                Some(order.instrument_id.clone()),
                                                order.order_id.clone(),
                                                order.client_order_id.clone(),
                                                order.request_id.clone(),
                                            )
                                        })
                                        .collect(),
                                    CommandTransport::Rest,
                                ),
                                Err(error) => return Err(error),
                            }
                        };
                    if chunk_receipts.len() != chunk.len() {
                        return Err(MarketError::new(
                            ErrorKind::DecodeError,
                            format!(
                                "bybit batch amend returned {} receipts for {} requests",
                                chunk_receipts.len(),
                                chunk.len()
                            ),
                        ));
                    }
                    let chunk_acks = cache_and_emit_amend_receipts(
                        context,
                        &chunk_receipts,
                        &chunk_requests,
                        transport,
                    )
                    .await;
                    for ((index, _), ack) in chunk.iter().zip(chunk_acks) {
                        acks[*index] = Some(ack);
                    }
                }
            }
        }

        collect_batch_acks(acks, "amend_orders")
    }
    .await;
    record_runtime_latency(context, RuntimeOperation::AmendOrders, started_at);
    result
}

pub(crate) async fn cancel_order(
    context: &LiveContext,
    request: &CancelOrderRequest,
) -> Result<CommandAck> {
    let started_at = Instant::now();
    let result = async {
        let spec = require_spec(context, &request.instrument_id)?;
        context.command_limiter.acquire().await;
        let (receipt, transport) = match &context.adapter {
            #[cfg(feature = "binance")]
            AdapterHandle::Binance(adapter) => {
                if is_binance_algo_order_id(request.order_id.as_ref()) {
                    if context.command_transport_mode.is_websocket_only() {
                        return Err(unsupported_ws_command(context, "cancel_order_ws"));
                    }
                    let pairs = binance_algo_identity_query(
                        request.order_id.as_ref(),
                        request.client_order_id.as_ref(),
                    )?;
                    let pairs = pairs
                        .iter()
                        .map(|(key, value)| (key.as_str(), value.as_str()))
                        .collect::<Vec<_>>();
                    match binance_signed_request_text(
                        context,
                        Method::DELETE,
                        "/fapi/v1/algoOrder",
                        &pairs,
                        "binance.cancel_algo_order",
                    )
                    .await
                    {
                        Ok(payload) => (
                            adapter.classify_command(
                                CommandOperation::CancelOrder,
                                Some(&payload),
                                request.request_id.clone(),
                            )?,
                            CommandTransport::Rest,
                        ),
                        Err(error) if is_uncertain_command_error(&error) => (
                            adapter.classify_command(
                                CommandOperation::CancelOrder,
                                None,
                                request.request_id.clone(),
                            )?,
                            CommandTransport::Rest,
                        ),
                        Err(error) => return Err(error),
                    }
                } else {
                    let query = order_identity_query(
                        &spec.native_symbol,
                        request.order_id.as_ref(),
                        request.client_order_id.as_ref(),
                    )?;
                    if context.adapter.capabilities().native.ws_order_entry {
                        match binance_signed_ws_request_text(
                            context,
                            "order.cancel",
                            query.clone(),
                            "binance.cancel_order_ws",
                        )
                        .await
                        {
                            Ok(payload) => (
                                adapter.classify_command(
                                    CommandOperation::CancelOrder,
                                    Some(&payload),
                                    request.request_id.clone(),
                                )?,
                                CommandTransport::WebSocket,
                            ),
                            Err(CommandWsRequestError::Uncertain(_)) => (
                                adapter.classify_command(
                                    CommandOperation::CancelOrder,
                                    None,
                                    request.request_id.clone(),
                                )?,
                                CommandTransport::WebSocket,
                            ),
                            Err(CommandWsRequestError::Unavailable(error)) => {
                                if context.command_transport_mode.is_websocket_only() {
                                    return Err(error);
                                }
                                let pairs = query
                                    .iter()
                                    .map(|(key, value)| (key.as_str(), value.as_str()))
                                    .collect::<Vec<_>>();
                                match binance_signed_request_text(
                                    context,
                                    Method::DELETE,
                                    "/fapi/v1/order",
                                    &pairs,
                                    "binance.cancel_order",
                                )
                                .await
                                {
                                    Ok(payload) => (
                                        adapter.classify_command(
                                            CommandOperation::CancelOrder,
                                            Some(&payload),
                                            request.request_id.clone(),
                                        )?,
                                        CommandTransport::Rest,
                                    ),
                                    Err(error) if is_uncertain_command_error(&error) => (
                                        adapter.classify_command(
                                            CommandOperation::CancelOrder,
                                            None,
                                            request.request_id.clone(),
                                        )?,
                                        CommandTransport::Rest,
                                    ),
                                    Err(error) => return Err(error),
                                }
                            }
                        }
                    } else {
                        if context.command_transport_mode.is_websocket_only() {
                            return Err(unsupported_ws_command(context, "cancel_order_ws"));
                        }
                        let pairs = query
                            .iter()
                            .map(|(key, value)| (key.as_str(), value.as_str()))
                            .collect::<Vec<_>>();
                        match binance_signed_request_text(
                            context,
                            Method::DELETE,
                            "/fapi/v1/order",
                            &pairs,
                            "binance.cancel_order",
                        )
                        .await
                        {
                            Ok(payload) => (
                                adapter.classify_command(
                                    CommandOperation::CancelOrder,
                                    Some(&payload),
                                    request.request_id.clone(),
                                )?,
                                CommandTransport::Rest,
                            ),
                            Err(error) if is_uncertain_command_error(&error) => (
                                adapter.classify_command(
                                    CommandOperation::CancelOrder,
                                    None,
                                    request.request_id.clone(),
                                )?,
                                CommandTransport::Rest,
                            ),
                            Err(error) => return Err(error),
                        }
                    }
                }
            }
            #[cfg(feature = "bybit")]
            AdapterHandle::Bybit(adapter) => {
                let mut body = vec![
                    ("category".to_owned(), "linear".to_owned()),
                    ("symbol".to_owned(), spec.native_symbol.to_string()),
                ];
                append_bybit_order_identity(
                    &mut body,
                    request.order_id.as_ref(),
                    request.client_order_id.as_ref(),
                )?;
                let body = body_to_object(body);
                if context.adapter.capabilities().native.ws_order_entry {
                    match bybit_trade_ws_request_text(
                        context,
                        "order.cancel",
                        vec![Value::Object(body.clone())],
                        "bybit.cancel_order_ws",
                    )
                    .await
                    {
                        Ok(payload) => (
                            adapter.classify_command(
                                CommandOperation::CancelOrder,
                                Some(&payload),
                                request.request_id.clone(),
                            )?,
                            CommandTransport::WebSocket,
                        ),
                        Err(CommandWsRequestError::Uncertain(_)) => (
                            adapter.classify_command(
                                CommandOperation::CancelOrder,
                                None,
                                request.request_id.clone(),
                            )?,
                            CommandTransport::WebSocket,
                        ),
                        Err(CommandWsRequestError::Unavailable(error)) => {
                            if context.command_transport_mode.is_websocket_only() {
                                return Err(error);
                            }
                            let body = serde_json::to_string(&body).map_err(|error| {
                                MarketError::new(
                                    ErrorKind::ConfigError,
                                    format!("failed to serialize bybit cancel order body: {error}"),
                                )
                            })?;
                            match bybit_signed_post_text(
                                context,
                                "/v5/order/cancel",
                                &body,
                                "bybit.cancel_order",
                            )
                            .await
                            {
                                Ok(payload) => (
                                    adapter.classify_command(
                                        CommandOperation::CancelOrder,
                                        Some(&payload),
                                        request.request_id.clone(),
                                    )?,
                                    CommandTransport::Rest,
                                ),
                                Err(error) if is_uncertain_command_error(&error) => (
                                    adapter.classify_command(
                                        CommandOperation::CancelOrder,
                                        None,
                                        request.request_id.clone(),
                                    )?,
                                    CommandTransport::Rest,
                                ),
                                Err(error) => return Err(error),
                            }
                        }
                    }
                } else {
                    if context.command_transport_mode.is_websocket_only() {
                        return Err(unsupported_ws_command(context, "cancel_order_ws"));
                    }
                    let body = serde_json::to_string(&body).map_err(|error| {
                        MarketError::new(
                            ErrorKind::ConfigError,
                            format!("failed to serialize bybit cancel order body: {error}"),
                        )
                    })?;
                    match bybit_signed_post_text(
                        context,
                        "/v5/order/cancel",
                        &body,
                        "bybit.cancel_order",
                    )
                    .await
                    {
                        Ok(payload) => (
                            adapter.classify_command(
                                CommandOperation::CancelOrder,
                                Some(&payload),
                                request.request_id.clone(),
                            )?,
                            CommandTransport::Rest,
                        ),
                        Err(error) if is_uncertain_command_error(&error) => (
                            adapter.classify_command(
                                CommandOperation::CancelOrder,
                                None,
                                request.request_id.clone(),
                            )?,
                            CommandTransport::Rest,
                        ),
                        Err(error) => return Err(error),
                    }
                }
            }
        };

        let receipt = hydrate_cancel_receipt(receipt, request);
        if receipt.status == CommandStatus::UnknownExecution {
            context
                .runtime_state
                .cache_pending_unknown(PendingUnknownCommand {
                    operation: CommandOperation::CancelOrder,
                    instrument_id: request.instrument_id.clone(),
                    order_id: request.order_id.clone(),
                    client_order_id: request.client_order_id.clone(),
                    recorded_at: timestamp_now_ms(),
                })
                .await;
        }
        let ack = apply_command_receipt(context, receipt.clone(), transport).await;
        Ok(ack)
    }
    .await;
    record_runtime_latency(context, RuntimeOperation::CancelOrder, started_at);
    result
}

pub(crate) async fn cancel_orders(
    context: &LiveContext,
    request: &CancelOrdersRequest,
) -> Result<Vec<CommandAck>> {
    let started_at = Instant::now();
    let result = async {
        validate_cancel_orders(request)?;
        if request.orders.len() == 1 {
            let target = &request.orders[0];
            return Ok(vec![cancel_order(
                context,
                &CancelOrderRequest {
                    request_id: request.request_id.clone(),
                    instrument_id: target.instrument_id.clone(),
                    order_id: target.order_id.clone(),
                    client_order_id: target.client_order_id.clone(),
                },
            )
            .await?]);
        }

        let mut acks = vec![None; request.orders.len()];
        match &context.adapter {
            #[cfg(feature = "binance")]
            AdapterHandle::Binance(adapter) => {
                let indexed = request.orders.iter().enumerate().collect::<Vec<_>>();
                if context.adapter.capabilities().native.ws_order_entry {
                    for chunk in indexed.chunks(10) {
                        let chunk_requests = chunk
                            .iter()
                            .map(|(_, target)| CancelOrderRequest {
                                request_id: request.request_id.clone(),
                                instrument_id: target.instrument_id.clone(),
                                order_id: target.order_id.clone(),
                                client_order_id: target.client_order_id.clone(),
                            })
                            .collect::<Vec<_>>();
                        let chunk_acks =
                            try_join_all(chunk_requests.iter().map(|order| cancel_order(context, order)))
                                .await?;
                        for ((index, _), ack) in chunk.iter().zip(chunk_acks) {
                            acks[*index] = Some(ack);
                        }
                    }
                } else {
                    if context.command_transport_mode.is_websocket_only() {
                        return Err(unsupported_ws_command(context, "cancel_orders_ws"));
                    }
                    let mut order_id_groups =
                        BTreeMap::<InstrumentId, Vec<(usize, &OrderTarget)>>::new();
                    let mut client_id_groups =
                        BTreeMap::<InstrumentId, Vec<(usize, &OrderTarget)>>::new();
                    for (index, target) in indexed {
                        match (&target.order_id, &target.client_order_id) {
                            (Some(_), _) => order_id_groups
                                .entry(target.instrument_id.clone())
                                .or_default()
                                .push((index, target)),
                            (None, Some(_)) => client_id_groups
                                .entry(target.instrument_id.clone())
                                .or_default()
                                .push((index, target)),
                            (None, None) => {}
                        }
                    }

                    for groups in [&order_id_groups, &client_id_groups] {
                        for (instrument_id, targets) in groups {
                            let spec = require_spec(context, instrument_id)?;
                            for chunk in targets.chunks(10) {
                                context.command_limiter.acquire().await;
                                let mut owned =
                                    vec![("symbol".to_owned(), spec.native_symbol.to_string())];
                                if chunk[0].1.order_id.is_some() {
                                    owned.push((
                                        "orderIdList".to_owned(),
                                        format!(
                                            "[{}]",
                                            chunk
                                                .iter()
                                                .filter_map(|(_, target)| {
                                                    target.order_id.as_ref().map(ToString::to_string)
                                                })
                                                .collect::<Vec<_>>()
                                                .join(",")
                                        ),
                                    ));
                                } else {
                                    owned.push((
                                        "origClientOrderIdList".to_owned(),
                                        serde_json::to_string(
                                            &chunk
                                                .iter()
                                                .filter_map(|(_, target)| {
                                                    target.client_order_id.as_ref().map(ToString::to_string)
                                                })
                                                .collect::<Vec<_>>(),
                                        )
                                        .map_err(|error| {
                                            MarketError::new(
                                                ErrorKind::ConfigError,
                                                format!(
                                                    "failed to serialize binance cancel batch client ids: {error}"
                                                ),
                                            )
                                        })?,
                                    ));
                                }
                                let pairs = owned
                                    .iter()
                                    .map(|(key, value)| (key.as_str(), value.as_str()))
                                    .collect::<Vec<_>>();
                                let chunk_receipts = match binance_signed_request_text(
                                    context,
                                    Method::DELETE,
                                    "/fapi/v1/batchOrders",
                                    &pairs,
                                    "binance.cancel_orders",
                                )
                                .await
                                {
                                    Ok(payload) => classify_binance_batch_payload(
                                        adapter,
                                        CommandOperation::CancelOrder,
                                        &payload,
                                        &vec![None; chunk.len()],
                                    )?,
                                    Err(error) if is_uncertain_command_error(&error) => chunk
                                        .iter()
                                        .map(|(_, target)| {
                                            unknown_command_receipt(
                                                context,
                                                CommandOperation::CancelOrder,
                                                Some(target.instrument_id.clone()),
                                                target.order_id.clone(),
                                                target.client_order_id.clone(),
                                                None,
                                            )
                                        })
                                        .collect(),
                                    Err(error) => return Err(error),
                                };
                                if chunk_receipts.len() != chunk.len() {
                                    return Err(MarketError::new(
                                        ErrorKind::DecodeError,
                                        format!(
                                            "binance batch cancel returned {} receipts for {} requests",
                                            chunk_receipts.len(),
                                            chunk.len()
                                        ),
                                    ));
                                }
                                let chunk_targets = chunk
                                    .iter()
                                    .map(|(_, target)| (*target).clone())
                                    .collect::<Vec<_>>();
                                let chunk_acks = cache_and_emit_cancel_receipts(
                                    context,
                                    &chunk_receipts,
                                    &chunk_targets,
                                    CommandTransport::Rest,
                                )
                                .await;
                                for ((index, _), ack) in chunk.iter().zip(chunk_acks) {
                                    acks[*index] = Some(ack);
                                }
                            }
                        }
                    }
                }
            }
            #[cfg(feature = "bybit")]
            AdapterHandle::Bybit(_) => {
                let indexed = request.orders.iter().enumerate().collect::<Vec<_>>();
                for chunk in indexed.chunks(20) {
                    context.command_limiter.acquire().await;
                    let chunk_targets = chunk
                        .iter()
                        .map(|(_, target)| (*target).clone())
                        .collect::<Vec<_>>();
                    let batch = chunk_targets
                        .iter()
                        .map(|target| build_bybit_batch_cancel_object(context, target))
                        .collect::<Result<Vec<_>>>()?;
                    let batch_body = json!({
                        "category": "linear",
                        "request": batch,
                    });
                    let body = serde_json::to_string(&batch_body)
                    .map_err(|error| {
                        MarketError::new(
                            ErrorKind::ConfigError,
                            format!("failed to serialize bybit batch cancel body: {error}"),
                        )
                    })?;
                    let identities = chunk_targets
                        .iter()
                        .map(|target| BatchIdentity {
                            instrument_id: Some(target.instrument_id.clone()),
                            order_id: target.order_id.clone(),
                            client_order_id: target.client_order_id.clone(),
                            #[cfg(feature = "bybit")]
                            request_id: None,
                        })
                        .collect::<Vec<_>>();
                    let (chunk_receipts, transport) = if context
                        .adapter
                        .capabilities()
                        .native
                        .ws_order_entry
                    {
                        match bybit_trade_ws_request_text(
                            context,
                            "order.cancel-batch",
                            vec![batch_body.clone()],
                            "bybit.cancel_orders_ws",
                        )
                        .await
                        {
                            Ok(payload) => (
                                classify_bybit_batch_payload(
                                    context,
                                    CommandOperation::CancelOrder,
                                    &payload,
                                    &identities,
                                )?,
                                CommandTransport::WebSocket,
                            ),
                            Err(CommandWsRequestError::Uncertain(_)) => (
                                chunk_targets
                                    .iter()
                                    .map(|target| {
                                        unknown_command_receipt(
                                            context,
                                            CommandOperation::CancelOrder,
                                            Some(target.instrument_id.clone()),
                                            target.order_id.clone(),
                                            target.client_order_id.clone(),
                                            None,
                                        )
                                    })
                                    .collect(),
                                CommandTransport::WebSocket,
                            ),
                            Err(CommandWsRequestError::Unavailable(error)) => {
                                if context.command_transport_mode.is_websocket_only() {
                                    return Err(error);
                                }
                                match bybit_signed_post_text(
                                    context,
                                    "/v5/order/cancel-batch",
                                    &body,
                                    "bybit.cancel_orders",
                                )
                                .await
                                {
                                    Ok(payload) => (
                                        classify_bybit_batch_payload(
                                            context,
                                            CommandOperation::CancelOrder,
                                            &payload,
                                            &identities,
                                        )?,
                                        CommandTransport::Rest,
                                    ),
                                    Err(error) if is_uncertain_command_error(&error) => (
                                        chunk_targets
                                            .iter()
                                            .map(|target| {
                                                unknown_command_receipt(
                                                    context,
                                                    CommandOperation::CancelOrder,
                                                    Some(target.instrument_id.clone()),
                                                    target.order_id.clone(),
                                                    target.client_order_id.clone(),
                                                    None,
                                                )
                                            })
                                            .collect(),
                                        CommandTransport::Rest,
                                    ),
                                    Err(error) => return Err(error),
                                }
                            }
                        }
                    } else {
                        if context.command_transport_mode.is_websocket_only() {
                            return Err(unsupported_ws_command(context, "cancel_orders_ws"));
                        }
                        match bybit_signed_post_text(
                            context,
                            "/v5/order/cancel-batch",
                            &body,
                            "bybit.cancel_orders",
                        )
                        .await
                        {
                            Ok(payload) => (
                                classify_bybit_batch_payload(
                                    context,
                                    CommandOperation::CancelOrder,
                                    &payload,
                                    &identities,
                                )?,
                                CommandTransport::Rest,
                            ),
                            Err(error) if is_uncertain_command_error(&error) => (
                                chunk_targets
                                    .iter()
                                    .map(|target| {
                                        unknown_command_receipt(
                                            context,
                                            CommandOperation::CancelOrder,
                                            Some(target.instrument_id.clone()),
                                            target.order_id.clone(),
                                            target.client_order_id.clone(),
                                            None,
                                        )
                                    })
                                    .collect(),
                                CommandTransport::Rest,
                            ),
                            Err(error) => return Err(error),
                        }
                    };
                    if chunk_receipts.len() != chunk.len() {
                        return Err(MarketError::new(
                            ErrorKind::DecodeError,
                            format!(
                                "bybit batch cancel returned {} receipts for {} requests",
                                chunk_receipts.len(),
                                chunk.len()
                            ),
                        ));
                    }
                    let chunk_acks =
                        cache_and_emit_cancel_receipts(context, &chunk_receipts, &chunk_targets, transport)
                            .await;
                    for ((index, _), ack) in chunk.iter().zip(chunk_acks) {
                        acks[*index] = Some(ack);
                    }
                }
            }
        }

        collect_batch_acks(acks, "cancel_orders")
    }
    .await;
    record_runtime_latency(context, RuntimeOperation::CancelOrders, started_at);
    result
}

pub(crate) async fn cancel_all_orders(
    context: &LiveContext,
    request: &CancelAllOrdersRequest,
) -> Result<CommandReceipt> {
    let started_at = Instant::now();
    let result = async {
        if context.command_transport_mode.is_websocket_only() {
            return Err(unsupported_ws_command(context, "cancel_all_orders_ws"));
        }
        context.command_limiter.acquire().await;
        let receipt = match (&context.adapter, &request.instrument_id) {
            #[cfg(feature = "binance")]
            (AdapterHandle::Binance(adapter), Some(instrument_id)) => {
                let spec = require_spec(context, instrument_id)?;
                let pairs = [("symbol", spec.native_symbol.as_ref())];
                let payload = binance_signed_request_text(
                    context,
                    Method::DELETE,
                    "/fapi/v1/allOpenOrders",
                    &pairs,
                    "binance.cancel_all_orders",
                )
                .await?;
                let _ = binance_signed_request_text(
                    context,
                    Method::DELETE,
                    "/fapi/v1/algoOpenOrders",
                    &pairs,
                    "binance.cancel_all_algo_orders",
                )
                .await?;
                adapter.classify_command(
                    CommandOperation::CancelAllOrders,
                    Some(&payload),
                    request.request_id.clone(),
                )?
            }
            #[cfg(feature = "binance")]
            (AdapterHandle::Binance(_), None) => {
                let instruments = context.shared.read(|state| {
                    state
                        .open_orders()
                        .into_iter()
                        .map(|order| order.instrument_id)
                        .collect::<std::collections::BTreeSet<_>>()
                });
                for instrument_id in instruments {
                    let spec = require_spec(context, &instrument_id)?;
                    let pairs = [("symbol", spec.native_symbol.as_ref())];
                    let _ = binance_signed_request_text(
                        context,
                        Method::DELETE,
                        "/fapi/v1/allOpenOrders",
                        &pairs,
                        "binance.cancel_all_orders",
                    )
                    .await?;
                    let _ = binance_signed_request_text(
                        context,
                        Method::DELETE,
                        "/fapi/v1/algoOpenOrders",
                        &pairs,
                        "binance.cancel_all_algo_orders",
                    )
                    .await?;
                }
                CommandReceipt {
                    operation: CommandOperation::CancelAllOrders,
                    status: CommandStatus::Accepted,
                    venue: context.config.venue,
                    product: context.config.product,
                    instrument_id: None,
                    order_id: None,
                    client_order_id: None,
                    request_id: request.request_id.clone(),
                    message: Some("accepted".into()),
                    native_code: None,
                    retriable: false,
                }
            }
            #[cfg(feature = "bybit")]
            (AdapterHandle::Bybit(adapter), Some(instrument_id)) => {
                let spec = require_spec(context, instrument_id)?;
                let body = serde_json::to_string(&json!({
                    "category": "linear",
                    "symbol": spec.native_symbol,
                }))
                .map_err(|error| {
                    MarketError::new(
                        ErrorKind::ConfigError,
                        format!("failed to serialize bybit cancel-all body: {error}"),
                    )
                })?;
                let payload = bybit_signed_post_text(
                    context,
                    "/v5/order/cancel-all",
                    &body,
                    "bybit.cancel_all_orders",
                )
                .await?;
                adapter.classify_command(
                    CommandOperation::CancelAllOrders,
                    Some(&payload),
                    request.request_id.clone(),
                )?
            }
            #[cfg(feature = "bybit")]
            (AdapterHandle::Bybit(adapter), None) => {
                let body = serde_json::to_string(&json!({
                    "category": "linear",
                    "coin": "USDT",
                }))
                .map_err(|error| {
                    MarketError::new(
                        ErrorKind::ConfigError,
                        format!("failed to serialize bybit cancel-all body: {error}"),
                    )
                })?;
                let payload = bybit_signed_post_text(
                    context,
                    "/v5/order/cancel-all",
                    &body,
                    "bybit.cancel_all_orders",
                )
                .await?;
                adapter.classify_command(
                    CommandOperation::CancelAllOrders,
                    Some(&payload),
                    request.request_id.clone(),
                )?
            }
        };

        apply_command_receipt(context, receipt.clone(), CommandTransport::Rest).await;
        Ok(receipt)
    }
    .await;
    record_runtime_latency(context, RuntimeOperation::CancelAllOrders, started_at);
    result
}

pub(crate) async fn close_position(
    context: &LiveContext,
    request: &ClosePositionRequest,
) -> Result<CommandAck> {
    let started_at = Instant::now();
    let result = async {
        let position = resolve_close_position(context, request).await?;
        let position_size = position.size;
        if position_size.value().is_zero() {
            return Err(MarketError::new(
                ErrorKind::ConfigError,
                format!("position {} is already flat", request.instrument_id),
            ));
        }
        let quantity = request.quantity.unwrap_or(position_size);
        if quantity.value() > position_size.value() {
            return Err(MarketError::new(
                ErrorKind::ConfigError,
                "close_position quantity exceeds current position size",
            ));
        }

        let derived_request = CreateOrderRequest {
            request_id: request.request_id.clone(),
            instrument_id: request.instrument_id.clone(),
            client_order_id: request.client_order_id.clone(),
            side: match position.direction {
                bat_markets_core::PositionDirection::Long => bat_markets_core::Side::Sell,
                bat_markets_core::PositionDirection::Short => bat_markets_core::Side::Buy,
                bat_markets_core::PositionDirection::Flat => {
                    return Err(MarketError::new(
                        ErrorKind::ConfigError,
                        format!("position {} is already flat", request.instrument_id),
                    ));
                }
            },
            order_type: if request.price.is_some() {
                bat_markets_core::OrderType::Limit
            } else {
                bat_markets_core::OrderType::Market
            },
            time_in_force: request.time_in_force,
            quantity,
            price: request.price,
            trigger_price: None,
            trigger_type: None,
            reduce_only: true,
            post_only: request.post_only,
        };

        validate_create_order(context, &derived_request)?;
        context.command_limiter.acquire().await;
        let (receipt, transport) = match &context.adapter {
            #[cfg(feature = "binance")]
            AdapterHandle::Binance(adapter) => {
                let spec = require_spec(context, &derived_request.instrument_id)?;
                let mut owned = vec![
                    ("symbol".to_owned(), spec.native_symbol.to_string()),
                    ("side".to_owned(), binance_side(derived_request.side).to_owned()),
                    (
                        "type".to_owned(),
                        binance_order_type(derived_request.order_type).to_owned(),
                    ),
                    ("quantity".to_owned(), format_quantity(derived_request.quantity)),
                ];
                if let Some(client_order_id) = &derived_request.client_order_id {
                    owned.push(("newClientOrderId".to_owned(), client_order_id.to_string()));
                }
                if let Some(price) = derived_request.price {
                    owned.push(("price".to_owned(), format_price(price)));
                }
                if let Some(time_in_force) = derived_request.time_in_force {
                    owned.push((
                        "timeInForce".to_owned(),
                        binance_time_in_force(time_in_force, derived_request.post_only).to_owned(),
                    ));
                }
                owned.push(("reduceOnly".to_owned(), "true".to_owned()));
                if context.adapter.capabilities().native.ws_order_entry {
                    match binance_signed_ws_request_text(
                        context,
                        "order.place",
                        owned.clone(),
                        "binance.close_position_ws",
                    )
                    .await
                    {
                        Ok(payload) => (
                            adapter.classify_command(
                                CommandOperation::ClosePosition,
                                Some(&payload),
                                derived_request.request_id.clone(),
                            )?,
                            CommandTransport::WebSocket,
                        ),
                        Err(CommandWsRequestError::Uncertain(_)) => (
                            adapter.classify_command(
                                CommandOperation::ClosePosition,
                                None,
                                derived_request.request_id.clone(),
                            )?,
                            CommandTransport::WebSocket,
                        ),
                        Err(CommandWsRequestError::Unavailable(_)) => {
                            let pairs = owned
                                .iter()
                                .map(|(key, value)| (key.as_str(), value.as_str()))
                                .collect::<Vec<_>>();
                            match binance_signed_request_text(
                                context,
                                Method::POST,
                                "/fapi/v1/order",
                                &pairs,
                                "binance.close_position",
                            )
                            .await
                            {
                                Ok(payload) => (
                                    adapter.classify_command(
                                        CommandOperation::ClosePosition,
                                        Some(&payload),
                                        derived_request.request_id.clone(),
                                    )?,
                                    CommandTransport::Rest,
                                ),
                                Err(error) if is_uncertain_command_error(&error) => (
                                    adapter.classify_command(
                                        CommandOperation::ClosePosition,
                                        None,
                                        derived_request.request_id.clone(),
                                    )?,
                                    CommandTransport::Rest,
                                ),
                                Err(error) => return Err(error),
                            }
                        }
                    }
                } else {
                    let pairs = owned
                        .iter()
                        .map(|(key, value)| (key.as_str(), value.as_str()))
                        .collect::<Vec<_>>();
                    match binance_signed_request_text(
                        context,
                        Method::POST,
                        "/fapi/v1/order",
                        &pairs,
                        "binance.close_position",
                    )
                    .await
                    {
                        Ok(payload) => (
                            adapter.classify_command(
                                CommandOperation::ClosePosition,
                                Some(&payload),
                                derived_request.request_id.clone(),
                            )?,
                            CommandTransport::Rest,
                        ),
                        Err(error) if is_uncertain_command_error(&error) => (
                            adapter.classify_command(
                                CommandOperation::ClosePosition,
                                None,
                                derived_request.request_id.clone(),
                            )?,
                            CommandTransport::Rest,
                        ),
                        Err(error) => return Err(error),
                    }
                }
            }
            #[cfg(feature = "bybit")]
            AdapterHandle::Bybit(adapter) => {
                let spec = require_spec(context, &derived_request.instrument_id)?;
                let body = json!({
                    "category": "linear",
                    "symbol": spec.native_symbol,
                    "side": bybit_side(derived_request.side),
                    "orderType": bybit_order_type(derived_request.order_type),
                    "qty": format_quantity(derived_request.quantity),
                    "price": derived_request.price.map(format_price),
                    "timeInForce": derived_request.time_in_force.map(|value| bybit_time_in_force(value, derived_request.post_only)),
                    "orderLinkId": derived_request.client_order_id.as_ref().map(ToString::to_string),
                    "reduceOnly": true,
                });
                if context.adapter.capabilities().native.ws_order_entry {
                    match bybit_trade_ws_request_text(
                        context,
                        "order.create",
                        vec![body.clone()],
                        "bybit.close_position_ws",
                    )
                    .await
                    {
                        Ok(payload) => (
                            adapter.classify_command(
                                CommandOperation::ClosePosition,
                                Some(&payload),
                                derived_request.request_id.clone(),
                            )?,
                            CommandTransport::WebSocket,
                        ),
                        Err(CommandWsRequestError::Uncertain(_)) => (
                            adapter.classify_command(
                                CommandOperation::ClosePosition,
                                None,
                                derived_request.request_id.clone(),
                            )?,
                            CommandTransport::WebSocket,
                        ),
                        Err(CommandWsRequestError::Unavailable(_)) => {
                            let body = serde_json::to_string(&body).map_err(|error| {
                                MarketError::new(
                                    ErrorKind::ConfigError,
                                    format!("failed to serialize bybit close-position body: {error}"),
                                )
                            })?;
                            match bybit_signed_post_text(
                                context,
                                "/v5/order/create",
                                &body,
                                "bybit.close_position",
                            )
                            .await
                            {
                                Ok(payload) => (
                                    adapter.classify_command(
                                        CommandOperation::ClosePosition,
                                        Some(&payload),
                                        derived_request.request_id.clone(),
                                    )?,
                                    CommandTransport::Rest,
                                ),
                                Err(error) if is_uncertain_command_error(&error) => (
                                    adapter.classify_command(
                                        CommandOperation::ClosePosition,
                                        None,
                                        derived_request.request_id.clone(),
                                    )?,
                                    CommandTransport::Rest,
                                ),
                                Err(error) => return Err(error),
                            }
                        }
                    }
                } else {
                    let body = serde_json::to_string(&body).map_err(|error| {
                        MarketError::new(
                            ErrorKind::ConfigError,
                            format!("failed to serialize bybit close-position body: {error}"),
                        )
                    })?;
                    match bybit_signed_post_text(
                        context,
                        "/v5/order/create",
                        &body,
                        "bybit.close_position",
                    )
                    .await
                    {
                        Ok(payload) => (
                            adapter.classify_command(
                                CommandOperation::ClosePosition,
                                Some(&payload),
                                derived_request.request_id.clone(),
                            )?,
                            CommandTransport::Rest,
                        ),
                        Err(error) if is_uncertain_command_error(&error) => (
                            adapter.classify_command(
                                CommandOperation::ClosePosition,
                                None,
                                derived_request.request_id.clone(),
                            )?,
                            CommandTransport::Rest,
                        ),
                        Err(error) => return Err(error),
                    }
                }
            }
        };

        if receipt.status == CommandStatus::UnknownExecution {
            context
                .runtime_state
                .cache_pending_unknown(PendingUnknownCommand {
                    operation: CommandOperation::ClosePosition,
                    instrument_id: derived_request.instrument_id.clone(),
                    order_id: None,
                    client_order_id: derived_request.client_order_id.clone(),
                    recorded_at: timestamp_now_ms(),
                })
                .await;
        }
        let ack = apply_command_receipt(context, receipt.clone(), transport).await;
        Ok(ack)
    }
    .await;
    record_runtime_latency(context, RuntimeOperation::ClosePosition, started_at);
    result
}

async fn resolve_close_position(
    context: &LiveContext,
    request: &ClosePositionRequest,
) -> Result<Position> {
    if let Some(position) = cached_position(context, &request.instrument_id)
        && close_position_cache_is_actionable(&position, request)
    {
        return Ok(position);
    }

    let refreshed = refresh_positions(context).await?;
    refreshed
        .into_iter()
        .find(|position| position.instrument_id == request.instrument_id)
        .ok_or_else(|| {
            MarketError::new(
                ErrorKind::ConfigError,
                format!(
                    "no refreshed position found for {}; start private stream or call refresh_positions before close_position",
                    request.instrument_id
                ),
            )
        })
}

fn cached_position(context: &LiveContext, instrument_id: &InstrumentId) -> Option<Position> {
    context.shared.read(|state| {
        state
            .positions()
            .into_iter()
            .find(|position| &position.instrument_id == instrument_id)
    })
}

fn close_position_cache_is_actionable(position: &Position, request: &ClosePositionRequest) -> bool {
    if position.size.value().is_zero() {
        return false;
    }
    request
        .quantity
        .is_none_or(|quantity| quantity.value() <= position.size.value())
}

pub(crate) async fn set_leverage(
    context: &LiveContext,
    request: &SetLeverageRequest,
) -> Result<CommandReceipt> {
    let started_at = Instant::now();
    let result = async {
        let spec = require_spec(context, &request.instrument_id)?;
        context.command_limiter.acquire().await;
        let leverage = request.leverage.value().normalize().to_string();
        let receipt = match &context.adapter {
            #[cfg(feature = "binance")]
            AdapterHandle::Binance(adapter) => {
                let pairs = [
                    ("symbol", spec.native_symbol.as_ref()),
                    ("leverage", leverage.as_str()),
                ];
                match binance_signed_request_text(
                    context,
                    Method::POST,
                    "/fapi/v1/leverage",
                    &pairs,
                    "binance.set_leverage",
                )
                .await
                {
                    Ok(payload) => adapter.classify_command(
                        CommandOperation::SetLeverage,
                        Some(&payload),
                        request.request_id.clone(),
                    )?,
                    Err(error) if is_uncertain_command_error(&error) => adapter.classify_command(
                        CommandOperation::SetLeverage,
                        None,
                        request.request_id.clone(),
                    )?,
                    Err(error) => return Err(error),
                }
            }
            #[cfg(feature = "bybit")]
            AdapterHandle::Bybit(adapter) => {
                let body = json!({
                    "category": "linear",
                    "symbol": spec.native_symbol,
                    "buyLeverage": leverage,
                    "sellLeverage": leverage,
                });
                let body = serde_json::to_string(&body).map_err(|error| {
                    MarketError::new(
                        ErrorKind::ConfigError,
                        format!("failed to serialize bybit set-leverage body: {error}"),
                    )
                })?;
                match bybit_signed_post_text(
                    context,
                    "/v5/position/set-leverage",
                    &body,
                    "bybit.set_leverage",
                )
                .await
                {
                    Ok(payload) => adapter.classify_command(
                        CommandOperation::SetLeverage,
                        Some(&payload),
                        request.request_id.clone(),
                    )?,
                    Err(error) if is_uncertain_command_error(&error) => adapter.classify_command(
                        CommandOperation::SetLeverage,
                        None,
                        request.request_id.clone(),
                    )?,
                    Err(error) => return Err(error),
                }
            }
        };
        apply_command_receipt(context, receipt.clone(), CommandTransport::Rest).await;
        Ok(receipt)
    }
    .await;
    record_runtime_latency(context, RuntimeOperation::SetLeverage, started_at);
    result
}

pub(crate) async fn set_margin_mode(
    context: &LiveContext,
    request: &SetMarginModeRequest,
) -> Result<CommandReceipt> {
    let started_at = Instant::now();
    let result = async {
        context.command_limiter.acquire().await;
        let receipt = match &context.adapter {
            #[cfg(feature = "binance")]
            AdapterHandle::Binance(adapter) => {
                let spec = require_spec(context, &request.instrument_id)?;
                let margin_type = match request.margin_mode {
                    MarginMode::Cross => "CROSSED",
                    MarginMode::Isolated => "ISOLATED",
                };
                let pairs = [
                    ("symbol", spec.native_symbol.as_ref()),
                    ("marginType", margin_type),
                ];
                match binance_signed_request_text(
                    context,
                    Method::POST,
                    "/fapi/v1/marginType",
                    &pairs,
                    "binance.set_margin_mode",
                )
                .await
                {
                    Ok(payload) => adapter.classify_command(
                        CommandOperation::SetMarginMode,
                        Some(&payload),
                        request.request_id.clone(),
                    )?,
                    Err(error) if is_uncertain_command_error(&error) => adapter.classify_command(
                        CommandOperation::SetMarginMode,
                        None,
                        request.request_id.clone(),
                    )?,
                    Err(error) => return Err(error),
                }
            }
            #[cfg(feature = "bybit")]
            AdapterHandle::Bybit(adapter) => {
                let set_margin_mode = match request.margin_mode {
                    MarginMode::Cross => "REGULAR_MARGIN",
                    MarginMode::Isolated => "ISOLATED_MARGIN",
                };
                let body = serde_json::to_string(&json!({
                    "setMarginMode": set_margin_mode,
                }))
                .map_err(|error| {
                    MarketError::new(
                        ErrorKind::ConfigError,
                        format!("failed to serialize bybit set-margin-mode body: {error}"),
                    )
                })?;
                match bybit_signed_post_text(
                    context,
                    "/v5/account/set-margin-mode",
                    &body,
                    "bybit.set_margin_mode",
                )
                .await
                {
                    Ok(payload) => adapter.classify_command(
                        CommandOperation::SetMarginMode,
                        Some(&payload),
                        request.request_id.clone(),
                    )?,
                    Err(error) if is_uncertain_command_error(&error) => adapter.classify_command(
                        CommandOperation::SetMarginMode,
                        None,
                        request.request_id.clone(),
                    )?,
                    Err(error) => return Err(error),
                }
            }
        };

        #[cfg(feature = "bybit")]
        if matches!(context.adapter, AdapterHandle::Bybit(_)) {
            context
                .runtime_state
                .invalidate_bybit_account_context()
                .await;
        }
        apply_command_receipt(context, receipt.clone(), CommandTransport::Rest).await;
        Ok(receipt)
    }
    .await;
    record_runtime_latency(context, RuntimeOperation::SetMarginMode, started_at);
    result
}

pub(crate) async fn validate_order(
    context: &LiveContext,
    request: &ValidateOrderRequest,
) -> Result<CommandReceipt> {
    let started_at = Instant::now();
    let result = async {
    validate_create_order(context, &request.order)?;
    context.command_limiter.acquire().await;
    let spec = require_spec(context, &request.order.instrument_id)?;

    match &context.adapter {
        #[cfg(feature = "binance")]
        AdapterHandle::Binance(_) => {
            let mut owned = vec![
                ("symbol".to_owned(), spec.native_symbol.to_string()),
                (
                    "side".to_owned(),
                    binance_side(request.order.side).to_owned(),
                ),
                (
                    "type".to_owned(),
                    binance_order_type(request.order.order_type).to_owned(),
                ),
                (
                    "quantity".to_owned(),
                    format_quantity(request.order.quantity),
                ),
            ];
            if let Some(client_order_id) = &request.order.client_order_id {
                owned.push(("newClientOrderId".to_owned(), client_order_id.to_string()));
            }
            if let Some(price) = request.order.price {
                owned.push(("price".to_owned(), format_price(price)));
            }
            if let Some(trigger_price) = request.order.trigger_price {
                owned.push(("stopPrice".to_owned(), format_price(trigger_price)));
            }
            if let Some(trigger_type) = request.order.trigger_type {
                owned.push((
                    "workingType".to_owned(),
                    binance_trigger_type(trigger_type).to_owned(),
                ));
            }
            if let Some(time_in_force) = request.order.time_in_force {
                owned.push((
                    "timeInForce".to_owned(),
                    binance_time_in_force(time_in_force, request.order.post_only).to_owned(),
                ));
            }
            if request.order.reduce_only {
                owned.push(("reduceOnly".to_owned(), "true".to_owned()));
            }
            let pairs = owned
                .iter()
                .map(|(key, value)| (key.as_str(), value.as_str()))
                .collect::<Vec<_>>();
            binance_signed_request_text(
                context,
                Method::POST,
                "/fapi/v1/order/test",
                &pairs,
                "binance.validate_order",
            )
            .await?;
        }
        #[cfg(feature = "bybit")]
        AdapterHandle::Bybit(_) => {
            let body = json!({
                "category": "linear",
                "symbol": spec.native_symbol,
                "side": bybit_side(request.order.side),
                "orderType": bybit_order_type(request.order.order_type),
                "qty": format_quantity(request.order.quantity),
                "price": request.order.price.map(format_price),
                "triggerPrice": request.order.trigger_price.map(format_price),
                "triggerBy": request.order.trigger_type.map(bybit_trigger_type),
                "timeInForce": request.order.time_in_force.map(|value| bybit_time_in_force(value, request.order.post_only)),
                "orderLinkId": request.order.client_order_id.as_ref().map(ToString::to_string),
                "reduceOnly": request.order.reduce_only,
            });
            let body = serde_json::to_string(&body).map_err(|error| {
                MarketError::new(
                    ErrorKind::ConfigError,
                    format!("failed to serialize bybit validate order body: {error}"),
                )
            })?;
            bybit_signed_post_text(
                context,
                "/v5/order/pre-check",
                &body,
                "bybit.validate_order",
            )
            .await?;
        }
    }

    let receipt = CommandReceipt {
        operation: CommandOperation::ValidateOrder,
        status: CommandStatus::Accepted,
        venue: context.config.venue,
        product: context.config.product,
        instrument_id: Some(spec.instrument_id.clone()),
        order_id: None,
        client_order_id: request.order.client_order_id.clone(),
        request_id: request
            .request_id
            .clone()
            .or(request.order.request_id.clone()),
        message: Some("validated".into()),
        native_code: None,
        retriable: false,
    };
    apply_command_receipt(context, receipt.clone(), CommandTransport::Rest).await;
    Ok(receipt)
    }
    .await;
    record_runtime_latency(context, RuntimeOperation::ValidateOrder, started_at);
    result
}

pub(crate) async fn set_position_mode(
    context: &LiveContext,
    request: &SetPositionModeRequest,
) -> Result<CommandReceipt> {
    let started_at = Instant::now();
    let result = async {
        context.command_limiter.acquire().await;
        let receipt = match &context.adapter {
            #[cfg(feature = "binance")]
            AdapterHandle::Binance(adapter) => {
                let dual_side_position = match request.position_mode {
                    bat_markets_core::PositionMode::OneWay => "false",
                    bat_markets_core::PositionMode::Hedge => "true",
                };
                let pairs = [("dualSidePosition", dual_side_position)];
                let payload = binance_signed_request_text(
                    context,
                    Method::POST,
                    "/fapi/v1/positionSide/dual",
                    &pairs,
                    "binance.set_position_mode",
                )
                .await?;
                adapter.classify_command(
                    CommandOperation::SetPositionMode,
                    Some(&payload),
                    request.request_id.clone(),
                )?
            }
            #[cfg(feature = "bybit")]
            AdapterHandle::Bybit(adapter) => {
                let mut body = json!({
                    "category": "linear",
                    "mode": match request.position_mode {
                        bat_markets_core::PositionMode::OneWay => 0,
                        bat_markets_core::PositionMode::Hedge => 3,
                    },
                });
                if let Some(instrument_id) = &request.instrument_id {
                    let spec = require_spec(context, instrument_id)?;
                    body["symbol"] = Value::String(spec.native_symbol.to_string());
                } else {
                    body["coin"] = Value::String("USDT".to_owned());
                }
                let body = serde_json::to_string(&body).map_err(|error| {
                    MarketError::new(
                        ErrorKind::ConfigError,
                        format!("failed to serialize bybit set-position-mode body: {error}"),
                    )
                })?;
                let payload = bybit_signed_post_text(
                    context,
                    "/v5/position/switch-mode",
                    &body,
                    "bybit.set_position_mode",
                )
                .await?;
                adapter.classify_command(
                    CommandOperation::SetPositionMode,
                    Some(&payload),
                    request.request_id.clone(),
                )?
            }
        };

        apply_command_receipt(context, receipt.clone(), CommandTransport::Rest).await;
        Ok(receipt)
    }
    .await;
    record_runtime_latency(context, RuntimeOperation::SetPositionMode, started_at);
    result
}

pub(crate) async fn refresh_open_interest(
    context: &LiveContext,
    instrument_id: &InstrumentId,
) -> Result<OpenInterest> {
    let started_at = Instant::now();
    let result = async {
        let spec = require_spec(context, instrument_id)?;
        let open_interest = match &context.adapter {
            #[cfg(feature = "binance")]
            AdapterHandle::Binance(_) => {
                let payload = public_get_with_retry(
                    context,
                    "/fapi/v1/openInterest",
                    &[("symbol", spec.native_symbol.as_ref())],
                    "binance.open_interest",
                )
                .await?;
                let events = context.adapter.as_adapter().parse_public(&payload)?;
                let event = events.into_iter().find_map(|event| match event {
                    bat_markets_core::PublicLaneEvent::OpenInterest(open_interest) => {
                        Some(open_interest)
                    }
                    _ => None,
                });
                event.ok_or_else(|| {
                    MarketError::new(
                        ErrorKind::DecodeError,
                        "missing binance open-interest event in snapshot response",
                    )
                })?
            }
            #[cfg(feature = "bybit")]
            AdapterHandle::Bybit(_) => {
                let payload = public_get_with_retry(
                    context,
                    "/v5/market/tickers",
                    &[
                        ("category", "linear"),
                        ("symbol", spec.native_symbol.as_ref()),
                    ],
                    "bybit.open_interest",
                )
                .await?;
                let response = serde_json::from_str::<bybit_native::MarketTickersResponse>(
                    &payload,
                )
                .map_err(|error| {
                    MarketError::new(
                        ErrorKind::DecodeError,
                        format!("failed to parse bybit tickers response: {error}"),
                    )
                })?;
                if response.ret_code != 0 {
                    return Err(MarketError::new(
                        ErrorKind::ExchangeReject,
                        response.ret_msg,
                    ));
                }
                let ticker = response.result.list.into_iter().next().ok_or_else(|| {
                    MarketError::new(
                        ErrorKind::DecodeError,
                        "missing bybit ticker entry in market/tickers response",
                    )
                })?;
                OpenInterest {
                    instrument_id: spec.instrument_id.clone(),
                    value: Quantity::new(parse_decimal(&ticker.open_interest, Venue::Bybit)?),
                    event_time: timestamp_now_ms(),
                }
            }
        };

        context
            .shared
            .apply_public_event(bat_markets_core::PublicLaneEvent::OpenInterest(
                open_interest.clone(),
            ));
        context.shared.write(|state| {
            state.mark_rest_success(None);
        });
        Ok(open_interest)
    }
    .await;
    record_runtime_latency(context, RuntimeOperation::RefreshOpenInterest, started_at);
    result
}

pub(crate) async fn fetch_ohlcv(
    context: &LiveContext,
    request: &FetchOhlcvRequest,
) -> Result<Vec<Kline>> {
    let instrument_ids = request.instrument_ids()?.to_vec();
    if instrument_ids.len() == 1 {
        return fetch_ohlcv_single(context, request).await;
    }

    let mut klines = Vec::new();
    for instrument_id in instrument_ids {
        let single_request = FetchOhlcvRequest::for_instrument(
            instrument_id,
            request.interval.clone(),
            request.start_time,
            request.end_time,
            request.limit,
        );
        klines.extend(fetch_ohlcv_single(context, &single_request).await?);
    }
    Ok(klines)
}

pub(crate) async fn fetch_ticker(
    context: &LiveContext,
    instrument_id: &InstrumentId,
) -> Result<Ticker> {
    let started_at = Instant::now();
    let result = async {
        let spec = require_spec(context, instrument_id)?;
        let ticker = match &context.adapter {
            #[cfg(feature = "binance")]
            AdapterHandle::Binance(adapter) => {
                let payload = public_get_with_retry(
                    context,
                    "/fapi/v1/ticker/24hr",
                    &[("symbol", spec.native_symbol.as_ref())],
                    "binance.fetch_ticker",
                )
                .await?;
                adapter.parse_ticker_snapshot(&payload, instrument_id)?
            }
            #[cfg(feature = "bybit")]
            AdapterHandle::Bybit(adapter) => {
                let payload = public_get_with_retry(
                    context,
                    "/v5/market/tickers",
                    &[
                        ("category", "linear"),
                        ("symbol", spec.native_symbol.as_ref()),
                    ],
                    "bybit.fetch_ticker",
                )
                .await?;
                adapter.parse_ticker_snapshot(&payload, instrument_id)?
            }
        };

        context.shared.write(|state| state.mark_rest_success(None));
        Ok(ticker)
    }
    .await;
    record_runtime_latency(context, RuntimeOperation::FetchTicker, started_at);
    result
}

pub(crate) async fn fetch_tickers(
    context: &LiveContext,
    request: &FetchTickersRequest,
) -> Result<Vec<Ticker>> {
    let instrument_ids = request.instrument_ids()?.to_vec();
    let mut tickers = Vec::with_capacity(instrument_ids.len());
    for instrument_id in instrument_ids {
        tickers.push(fetch_ticker(context, &instrument_id).await?);
    }
    Ok(tickers)
}

pub(crate) async fn fetch_mark_price(
    context: &LiveContext,
    instrument_id: &InstrumentId,
) -> Result<MarkPrice> {
    let started_at = Instant::now();
    let result = async {
        let spec = require_spec(context, instrument_id)?;
        let mark_price = match &context.adapter {
            #[cfg(feature = "binance")]
            AdapterHandle::Binance(_) => {
                let payload = public_get_with_retry(
                    context,
                    "/fapi/v1/premiumIndex",
                    &[("symbol", spec.native_symbol.as_ref())],
                    "binance.fetch_mark_price",
                )
                .await?;
                let snapshot = serde_json::from_str::<Value>(&payload).map_err(|error| {
                    MarketError::new(
                        ErrorKind::DecodeError,
                        format!("failed to parse binance premium index snapshot: {error}"),
                    )
                })?;
                let mark_price = snapshot
                    .get("markPrice")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        MarketError::new(
                            ErrorKind::DecodeError,
                            "missing markPrice in binance premium index snapshot",
                        )
                    })?;
                let funding_rate = snapshot
                    .get("lastFundingRate")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        MarketError::new(
                            ErrorKind::DecodeError,
                            "missing lastFundingRate in binance premium index snapshot",
                        )
                    })?;
                let time = snapshot
                    .get("time")
                    .and_then(Value::as_i64)
                    .ok_or_else(|| {
                        MarketError::new(
                            ErrorKind::DecodeError,
                            "missing time in binance premium index snapshot",
                        )
                    })?;
                MarkPrice {
                    instrument_id: spec.instrument_id.clone(),
                    price: Price::new(mark_price.parse::<rust_decimal::Decimal>().map_err(
                        |error| {
                            MarketError::new(
                                ErrorKind::DecodeError,
                                format!("invalid decimal '{mark_price}': {error}"),
                            )
                        },
                    )?),
                    funding_rate: Some(bat_markets_core::Rate::new(
                        funding_rate
                            .parse::<rust_decimal::Decimal>()
                            .map_err(|error| {
                                MarketError::new(
                                    ErrorKind::DecodeError,
                                    format!("invalid decimal '{funding_rate}': {error}"),
                                )
                            })?,
                    )),
                    event_time: TimestampMs::new(time),
                }
            }
            #[cfg(feature = "bybit")]
            AdapterHandle::Bybit(_) => {
                let payload = public_get_with_retry(
                    context,
                    "/v5/market/tickers",
                    &[
                        ("category", "linear"),
                        ("symbol", spec.native_symbol.as_ref()),
                    ],
                    "bybit.fetch_mark_price",
                )
                .await?;
                let response = serde_json::from_str::<bybit_native::MarketTickersResponse>(
                    &payload,
                )
                .map_err(|error| {
                    MarketError::new(
                        ErrorKind::DecodeError,
                        format!("failed to parse bybit tickers response: {error}"),
                    )
                })?;
                if response.ret_code != 0 {
                    return Err(MarketError::new(
                        ErrorKind::ExchangeReject,
                        response.ret_msg,
                    ));
                }
                let ticker = response.result.list.into_iter().next().ok_or_else(|| {
                    MarketError::new(
                        ErrorKind::DecodeError,
                        "missing bybit ticker entry in market/tickers response",
                    )
                })?;
                MarkPrice {
                    instrument_id: spec.instrument_id.clone(),
                    price: Price::new(parse_decimal(&ticker.mark_price, Venue::Bybit)?),
                    funding_rate: Some(bat_markets_core::Rate::new(parse_decimal(
                        &ticker.funding_rate,
                        Venue::Bybit,
                    )?)),
                    event_time: timestamp_now_ms(),
                }
            }
        };

        context
            .shared
            .apply_public_event(PublicLaneEvent::MarkPrice(
                bat_markets_core::FastMarkPrice {
                    instrument_id: mark_price.instrument_id.clone(),
                    price: mark_price.price.quantize(spec.price_scale)?,
                    funding_rate: mark_price.funding_rate,
                    event_time: mark_price.event_time,
                },
            ));
        context.shared.write(|state| state.mark_rest_success(None));
        Ok(mark_price)
    }
    .await;
    record_runtime_latency(context, RuntimeOperation::FetchMarkPrice, started_at);
    result
}

pub(crate) async fn fetch_funding_rate(
    context: &LiveContext,
    instrument_id: &InstrumentId,
) -> Result<bat_markets_core::FundingRate> {
    let started_at = Instant::now();
    let result = async {
        let spec = require_spec(context, instrument_id)?;
        let funding_rate = match &context.adapter {
            #[cfg(feature = "binance")]
            AdapterHandle::Binance(_) => {
                let payload = public_get_with_retry(
                    context,
                    "/fapi/v1/premiumIndex",
                    &[("symbol", spec.native_symbol.as_ref())],
                    "binance.fetch_funding_rate",
                )
                .await?;
                let snapshot = serde_json::from_str::<Value>(&payload).map_err(|error| {
                    MarketError::new(
                        ErrorKind::DecodeError,
                        format!("failed to parse binance premium index snapshot: {error}"),
                    )
                })?;
                let mark_price = snapshot
                    .get("markPrice")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        MarketError::new(
                            ErrorKind::DecodeError,
                            "missing markPrice in binance premium index snapshot",
                        )
                    })?;
                let funding_rate = snapshot
                    .get("lastFundingRate")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        MarketError::new(
                            ErrorKind::DecodeError,
                            "missing lastFundingRate in binance premium index snapshot",
                        )
                    })?;
                let time = snapshot
                    .get("time")
                    .and_then(Value::as_i64)
                    .ok_or_else(|| {
                        MarketError::new(
                            ErrorKind::DecodeError,
                            "missing time in binance premium index snapshot",
                        )
                    })?;
                bat_markets_core::FundingRate {
                    instrument_id: spec.instrument_id.clone(),
                    value: bat_markets_core::Rate::new(
                        funding_rate
                            .parse::<rust_decimal::Decimal>()
                            .map_err(|error| {
                                MarketError::new(
                                    ErrorKind::DecodeError,
                                    format!("invalid decimal '{funding_rate}': {error}"),
                                )
                            })?,
                    ),
                    mark_price: Some(Price::new(
                        mark_price
                            .parse::<rust_decimal::Decimal>()
                            .map_err(|error| {
                                MarketError::new(
                                    ErrorKind::DecodeError,
                                    format!("invalid decimal '{mark_price}': {error}"),
                                )
                            })?,
                    )),
                    event_time: TimestampMs::new(time),
                }
            }
            #[cfg(feature = "bybit")]
            AdapterHandle::Bybit(_) => {
                let payload = public_get_with_retry(
                    context,
                    "/v5/market/tickers",
                    &[
                        ("category", "linear"),
                        ("symbol", spec.native_symbol.as_ref()),
                    ],
                    "bybit.fetch_funding_rate",
                )
                .await?;
                let response = serde_json::from_str::<bybit_native::MarketTickersResponse>(
                    &payload,
                )
                .map_err(|error| {
                    MarketError::new(
                        ErrorKind::DecodeError,
                        format!("failed to parse bybit tickers response: {error}"),
                    )
                })?;
                if response.ret_code != 0 {
                    return Err(MarketError::new(
                        ErrorKind::ExchangeReject,
                        response.ret_msg,
                    ));
                }
                let ticker = response.result.list.into_iter().next().ok_or_else(|| {
                    MarketError::new(
                        ErrorKind::DecodeError,
                        "missing bybit ticker entry in market/tickers response",
                    )
                })?;
                bat_markets_core::FundingRate {
                    instrument_id: spec.instrument_id.clone(),
                    value: bat_markets_core::Rate::new(parse_decimal(
                        &ticker.funding_rate,
                        Venue::Bybit,
                    )?),
                    mark_price: Some(Price::new(parse_decimal(&ticker.mark_price, Venue::Bybit)?)),
                    event_time: timestamp_now_ms(),
                }
            }
        };

        context
            .shared
            .apply_public_event(PublicLaneEvent::FundingRate(funding_rate.clone()));
        context.shared.write(|state| state.mark_rest_success(None));
        Ok(funding_rate)
    }
    .await;
    record_runtime_latency(context, RuntimeOperation::FetchFundingRate, started_at);
    result
}

pub(crate) async fn fetch_trades(
    context: &LiveContext,
    request: &FetchTradesRequest,
) -> Result<Vec<TradeTick>> {
    let started_at = Instant::now();
    let result = async {
        let spec = require_spec(context, &request.instrument_id)?;
        let limit = request.validated_limit()?;
        let limit_string = limit.map(|value| value.to_string());

        let trades = match &context.adapter {
            #[cfg(feature = "binance")]
            AdapterHandle::Binance(adapter) => {
                let mut query = vec![("symbol", spec.native_symbol.as_ref())];
                if let Some(limit) = &limit_string {
                    query.push(("limit", limit.as_str()));
                }
                let payload = public_get_with_retry(
                    context,
                    "/fapi/v1/aggTrades",
                    &query,
                    "binance.fetch_trades",
                )
                .await?;
                adapter.parse_trades_snapshot(&payload, request)?
            }
            #[cfg(feature = "bybit")]
            AdapterHandle::Bybit(adapter) => {
                let mut query = vec![
                    ("category", "linear"),
                    ("symbol", spec.native_symbol.as_ref()),
                ];
                if let Some(limit) = &limit_string {
                    query.push(("limit", limit.as_str()));
                }
                let payload = public_get_with_retry(
                    context,
                    "/v5/market/recent-trade",
                    &query,
                    "bybit.fetch_trades",
                )
                .await?;
                adapter.parse_trades_snapshot(&payload, request)?
            }
        };

        context.shared.write(|state| state.mark_rest_success(None));
        Ok(trades)
    }
    .await;
    record_runtime_latency(context, RuntimeOperation::FetchTrades, started_at);
    result
}

pub(crate) async fn fetch_book_top(
    context: &LiveContext,
    instrument_id: &InstrumentId,
) -> Result<BookTop> {
    let started_at = Instant::now();
    let result = async {
        let spec = require_spec(context, instrument_id)?;
        let book_top = match &context.adapter {
            #[cfg(feature = "binance")]
            AdapterHandle::Binance(adapter) => {
                let payload = public_get_with_retry(
                    context,
                    "/fapi/v1/ticker/bookTicker",
                    &[("symbol", spec.native_symbol.as_ref())],
                    "binance.fetch_book_top",
                )
                .await?;
                adapter.parse_book_top_snapshot(&payload, instrument_id)?
            }
            #[cfg(feature = "bybit")]
            AdapterHandle::Bybit(adapter) => {
                let payload = public_get_with_retry(
                    context,
                    "/v5/market/orderbook",
                    &[
                        ("category", "linear"),
                        ("symbol", spec.native_symbol.as_ref()),
                        ("limit", "1"),
                    ],
                    "bybit.fetch_book_top",
                )
                .await?;
                adapter.parse_book_top_snapshot(&payload, instrument_id)?
            }
        };

        context.shared.write(|state| state.mark_rest_success(None));
        Ok(book_top)
    }
    .await;
    record_runtime_latency(context, RuntimeOperation::FetchBookTop, started_at);
    result
}

pub(crate) async fn fetch_order_book(
    context: &LiveContext,
    request: &FetchOrderBookRequest,
) -> Result<OrderBookSnapshot> {
    let started_at = Instant::now();
    let result = async {
        let spec = require_spec(context, &request.instrument_id)?;
        let limit = request.validated_limit()?;
        let limit_string = limit.map(|value| value.to_string());
        let snapshot = match &context.adapter {
            #[cfg(feature = "binance")]
            AdapterHandle::Binance(_) => {
                let mut query = vec![("symbol", spec.native_symbol.as_ref())];
                if let Some(limit) = &limit_string {
                    query.push(("limit", limit.as_str()));
                }
                let payload = public_get_with_retry(
                    context,
                    "/fapi/v1/depth",
                    &query,
                    "binance.fetch_order_book",
                )
                .await?;
                let snapshot = serde_json::from_str::<Value>(&payload).map_err(|error| {
                    MarketError::new(
                        ErrorKind::DecodeError,
                        format!("failed to parse binance order book snapshot: {error}"),
                    )
                })?;
                let bids = snapshot
                    .get("bids")
                    .and_then(Value::as_array)
                    .ok_or_else(|| {
                        MarketError::new(
                            ErrorKind::DecodeError,
                            "missing bids in binance order book snapshot",
                        )
                    })?;
                let asks = snapshot
                    .get("asks")
                    .and_then(Value::as_array)
                    .ok_or_else(|| {
                        MarketError::new(
                            ErrorKind::DecodeError,
                            "missing asks in binance order book snapshot",
                        )
                    })?;
                OrderBookSnapshot {
                    instrument_id: spec.instrument_id.clone(),
                    bids: bids
                        .iter()
                        .map(|level| {
                            let level = level.as_array().ok_or_else(|| {
                                MarketError::new(
                                    ErrorKind::DecodeError,
                                    "invalid bid level in binance order book snapshot",
                                )
                            })?;
                            Ok(OrderBookLevel {
                            price: Price::new(
                                level[0]
                                    .as_str()
                                    .ok_or_else(|| {
                                        MarketError::new(
                                            ErrorKind::DecodeError,
                                            "invalid bid price in binance order book snapshot",
                                        )
                                    })?
                                    .parse::<rust_decimal::Decimal>()
                                    .map_err(|error| {
                                        MarketError::new(
                                            ErrorKind::DecodeError,
                                            format!("invalid bid price decimal: {error}"),
                                        )
                                    })?,
                            ),
                            quantity: Quantity::new(
                                level[1]
                                    .as_str()
                                    .ok_or_else(|| {
                                        MarketError::new(
                                            ErrorKind::DecodeError,
                                            "invalid bid quantity in binance order book snapshot",
                                        )
                                    })?
                                    .parse::<rust_decimal::Decimal>()
                                    .map_err(|error| {
                                        MarketError::new(
                                            ErrorKind::DecodeError,
                                            format!("invalid bid quantity decimal: {error}"),
                                        )
                                    })?,
                            ),
                        })
                        })
                        .collect::<Result<Vec<_>>>()?,
                    asks: asks
                        .iter()
                        .map(|level| {
                            let level = level.as_array().ok_or_else(|| {
                                MarketError::new(
                                    ErrorKind::DecodeError,
                                    "invalid ask level in binance order book snapshot",
                                )
                            })?;
                            Ok(OrderBookLevel {
                            price: Price::new(
                                level[0]
                                    .as_str()
                                    .ok_or_else(|| {
                                        MarketError::new(
                                            ErrorKind::DecodeError,
                                            "invalid ask price in binance order book snapshot",
                                        )
                                    })?
                                    .parse::<rust_decimal::Decimal>()
                                    .map_err(|error| {
                                        MarketError::new(
                                            ErrorKind::DecodeError,
                                            format!("invalid ask price decimal: {error}"),
                                        )
                                    })?,
                            ),
                            quantity: Quantity::new(
                                level[1]
                                    .as_str()
                                    .ok_or_else(|| {
                                        MarketError::new(
                                            ErrorKind::DecodeError,
                                            "invalid ask quantity in binance order book snapshot",
                                        )
                                    })?
                                    .parse::<rust_decimal::Decimal>()
                                    .map_err(|error| {
                                        MarketError::new(
                                            ErrorKind::DecodeError,
                                            format!("invalid ask quantity decimal: {error}"),
                                        )
                                    })?,
                            ),
                        })
                        })
                        .collect::<Result<Vec<_>>>()?,
                    event_time: TimestampMs::new(
                        snapshot
                            .get("E")
                            .and_then(Value::as_i64)
                            .unwrap_or_else(|| timestamp_now_ms().value()),
                    ),
                }
            }
            #[cfg(feature = "bybit")]
            AdapterHandle::Bybit(_) => {
                let mut query = vec![
                    ("category", "linear"),
                    ("symbol", spec.native_symbol.as_ref()),
                ];
                if let Some(limit) = &limit_string {
                    query.push(("limit", limit.as_str()));
                }
                let payload = public_get_with_retry(
                    context,
                    "/v5/market/orderbook",
                    &query,
                    "bybit.fetch_order_book",
                )
                .await?;
                let response = serde_json::from_str::<bybit_native::OrderBookResponse>(&payload)
                    .map_err(|error| {
                        MarketError::new(
                            ErrorKind::DecodeError,
                            format!("failed to parse bybit order book snapshot: {error}"),
                        )
                    })?;
                if response.ret_code != 0 {
                    return Err(MarketError::new(
                        ErrorKind::ExchangeReject,
                        response.ret_msg,
                    ));
                }
                OrderBookSnapshot {
                    instrument_id: spec.instrument_id.clone(),
                    bids: response
                        .result
                        .bids
                        .iter()
                        .map(|level| {
                            Ok(OrderBookLevel {
                                price: Price::new(parse_decimal(&level[0], Venue::Bybit)?),
                                quantity: Quantity::new(parse_decimal(&level[1], Venue::Bybit)?),
                            })
                        })
                        .collect::<Result<Vec<_>>>()?,
                    asks: response
                        .result
                        .asks
                        .iter()
                        .map(|level| {
                            Ok(OrderBookLevel {
                                price: Price::new(parse_decimal(&level[0], Venue::Bybit)?),
                                quantity: Quantity::new(parse_decimal(&level[1], Venue::Bybit)?),
                            })
                        })
                        .collect::<Result<Vec<_>>>()?,
                    event_time: TimestampMs::new(
                        response
                            .result
                            .cts
                            .or(response.result.ts)
                            .unwrap_or_else(|| timestamp_now_ms().value()),
                    ),
                }
            }
        };
        Ok(snapshot)
    }
    .await;
    record_runtime_latency(context, RuntimeOperation::FetchOrderBook, started_at);
    result
}

pub(crate) async fn fetch_liquidations(
    context: &LiveContext,
    instrument_id: &InstrumentId,
    limit: Option<usize>,
) -> Result<Vec<Liquidation>> {
    let started_at = Instant::now();
    let result = async {
        let liquidations = context
            .shared
            .read(|state| state.liquidations(instrument_id))
            .ok_or_else(|| {
                MarketError::new(
                    ErrorKind::TemporaryUnavailable,
                    format!(
                        "no cached liquidation events for {}; subscribe to watch_liquidations() first",
                        instrument_id
                    ),
                )
            })?;
        let limit = limit.unwrap_or(liquidations.len());
        let mut recent = liquidations.into_iter().rev().take(limit).collect::<Vec<_>>();
        recent.reverse();
        Ok(recent)
    }
    .await;
    record_runtime_latency(context, RuntimeOperation::FetchLiquidations, started_at);
    result
}

async fn fetch_ohlcv_single(
    context: &LiveContext,
    request: &FetchOhlcvRequest,
) -> Result<Vec<Kline>> {
    let started_at = Instant::now();
    let result = async {
        let instrument_id = request.single_instrument_id()?;
        let spec = require_spec(context, instrument_id)?;
        let interval = parse_kline_interval(
            request.interval.as_ref(),
            context.config.venue,
            "fetch_ohlcv.interval",
        )?;
        let limit_string = request.limit.map(|limit| limit.to_string());
        let start_string = request.start_time.map(|value| value.value().to_string());
        let end_string = request.end_time.map(|value| value.value().to_string());

        let mut query = vec![
            ("symbol", spec.native_symbol.as_ref()),
            ("interval", interval.as_binance_str()),
        ];
        if let Some(limit) = &limit_string {
            query.push(("limit", limit.as_str()));
        }

        let klines = match &context.adapter {
            #[cfg(feature = "binance")]
            AdapterHandle::Binance(adapter) => {
                if let Some(start) = &start_string {
                    query.push(("startTime", start.as_str()));
                }
                if let Some(end) = &end_string {
                    query.push(("endTime", end.as_str()));
                }
                let payload = public_get_with_retry(
                    context,
                    "/fapi/v1/klines",
                    &query,
                    "binance.fetch_ohlcv",
                )
                .await?;
                adapter.parse_ohlcv_snapshot(&payload, request)?
            }
            #[cfg(feature = "bybit")]
            AdapterHandle::Bybit(adapter) => {
                query[1] = ("interval", interval.as_bybit_str());
                query.push(("category", "linear"));
                if let Some(start) = &start_string {
                    query.push(("start", start.as_str()));
                }
                if let Some(end) = &end_string {
                    query.push(("end", end.as_str()));
                }
                let payload =
                    public_get_with_retry(context, "/v5/market/kline", &query, "bybit.fetch_ohlcv")
                        .await?;
                adapter.parse_ohlcv_snapshot(&payload, request)?
            }
        };

        context.shared.write(|state| {
            state.mark_rest_success(None);
        });
        Ok(klines)
    }
    .await;
    record_runtime_latency(context, RuntimeOperation::FetchOhlcv, started_at);
    result
}

pub(crate) async fn reconcile_private(
    context: &LiveContext,
    trigger: ReconcileTrigger,
) -> Result<ReconcileReport> {
    let started_at = Instant::now();
    let repaired_at = timestamp_now_ms();
    let mode = reconcile_mode(context, trigger).await;
    let result = async {
        let _ = refresh_account(context).await?;
        let _ = refresh_positions(context).await?;
        let _ = refresh_open_orders(context, None).await?;
        match mode {
            PrivateReconcileMode::SnapshotOnly => Ok::<usize, MarketError>(0),
            PrivateReconcileMode::RecentHistoryRepair => {
                if matches!(
                    execution_repair_scope(trigger, &context.shared.health_snapshot()),
                    ExecutionRepairScope::LocalEvidence
                ) {
                    refresh_execution_repair_evidence(context).await?;
                }
                let unresolved = resolve_pending_unknown_commands(context).await?;
                Ok::<usize, MarketError>(unresolved)
            }
        }
    }
    .await;
    record_runtime_latency(context, RuntimeOperation::ReconcilePrivate, started_at);

    match result {
        Ok(0) => {
            let report = ReconcileReport {
                trigger,
                outcome: ReconcileOutcome::Synchronized,
                repaired_at,
                note: Some(match mode {
                    PrivateReconcileMode::SnapshotOnly => "snapshot reconcile completed".into(),
                    PrivateReconcileMode::RecentHistoryRepair => {
                        "snapshot reconcile with recent-history repair completed".into()
                    }
                }),
            };
            let report_for_state = report.clone();
            context
                .shared
                .write(|state| state.apply_reconcile_report(&report_for_state));
            Ok(report)
        }
        Ok(unresolved) => {
            let report = ReconcileReport {
                trigger,
                outcome: ReconcileOutcome::StillUncertain,
                repaired_at,
                note: Some(
                    format!("{unresolved} pending command outcomes still unresolved").into(),
                ),
            };
            let report_for_state = report.clone();
            context
                .shared
                .write(|state| state.apply_reconcile_report(&report_for_state));
            Ok(report)
        }
        Err(error) => {
            let report = ReconcileReport {
                trigger,
                outcome: ReconcileOutcome::StillUncertain,
                repaired_at,
                note: Some("snapshot reconcile failed".into()),
            };
            let report_for_state = report.clone();
            context.shared.write(|state| {
                state.apply_reconcile_report(&report_for_state);
                state.mark_snapshot_age(
                    context.config.health.snapshot_stale_after_ms,
                    context.config.health.snapshot_stale_after_ms,
                );
            });
            Err(error)
        }
    }
}

async fn reconcile_mode(context: &LiveContext, trigger: ReconcileTrigger) -> PrivateReconcileMode {
    match trigger {
        ReconcileTrigger::Manual
        | ReconcileTrigger::Reconnect
        | ReconcileTrigger::SequenceGap
        | ReconcileTrigger::UnknownExecution => PrivateReconcileMode::RecentHistoryRepair,
        ReconcileTrigger::Periodic => {
            let pending_unknown_count =
                context.runtime_state.pending_unknown_commands().await.len();
            let health = context.shared.health_snapshot();
            if needs_recent_history_repair(&health, pending_unknown_count) {
                PrivateReconcileMode::RecentHistoryRepair
            } else {
                PrivateReconcileMode::SnapshotOnly
            }
        }
    }
}

fn needs_recent_history_repair(health: &HealthReport, pending_unknown_count: usize) -> bool {
    if pending_unknown_count > 0 || health.state_divergence {
        return true;
    }

    matches!(
        health.degraded_reason,
        Some(
            DegradedReason::PrivateStreamGap
                | DegradedReason::ReconcileRequired
                | DegradedReason::CommandUncertain
                | DegradedReason::StateDivergence
        )
    )
}

fn execution_repair_scope(
    trigger: ReconcileTrigger,
    health: &HealthReport,
) -> ExecutionRepairScope {
    match trigger {
        ReconcileTrigger::Manual | ReconcileTrigger::Reconnect | ReconcileTrigger::SequenceGap => {
            ExecutionRepairScope::LocalEvidence
        }
        ReconcileTrigger::UnknownExecution => ExecutionRepairScope::PendingUnknownOnly,
        ReconcileTrigger::Periodic => {
            if health.state_divergence
                || matches!(
                    health.degraded_reason,
                    Some(
                        DegradedReason::PrivateStreamGap
                            | DegradedReason::ReconcileRequired
                            | DegradedReason::StateDivergence
                    )
                )
            {
                ExecutionRepairScope::LocalEvidence
            } else {
                ExecutionRepairScope::PendingUnknownOnly
            }
        }
    }
}

pub(crate) async fn spawn_public_stream(
    context: LiveContext,
    subscription: PublicSubscription,
) -> Result<LiveStreamHandle> {
    let (shutdown, shutdown_rx) = oneshot::channel();
    let join =
        tokio::spawn(async move { run_public_stream(context, subscription, shutdown_rx).await });
    Ok(LiveStreamHandle {
        shutdown: Some(shutdown),
        join,
    })
}

pub(crate) async fn spawn_private_stream(context: LiveContext) -> Result<LiveStreamHandle> {
    let (shutdown, shutdown_rx) = oneshot::channel();
    let join = tokio::spawn(async move { run_private_stream(context, shutdown_rx).await });
    Ok(LiveStreamHandle {
        shutdown: Some(shutdown),
        join,
    })
}

async fn apply_command_receipt(
    context: &LiveContext,
    receipt: CommandReceipt,
    transport: CommandTransport,
) -> CommandAck {
    let needs_reconcile = matches!(receipt.status, CommandStatus::UnknownExecution);
    let ack = CommandAck {
        receipt: receipt.clone(),
        transport,
        acknowledged_at: timestamp_now_ms(),
    };
    context
        .shared
        .write(|state| state.apply_command_receipt(&receipt));
    context
        .shared
        .emit_command_event(CommandLaneEvent::Lifecycle(CommandLifecycleEvent::Ack(
            ack.clone(),
        )));
    context
        .shared
        .emit_command_event(CommandLaneEvent::Receipt(receipt));
    if needs_reconcile {
        context
            .shared
            .emit_command_event(CommandLaneEvent::Lifecycle(
                CommandLifecycleEvent::RecoveryScheduled(ack.clone()),
            ));
        let reconcile_context = context.clone();
        let recovery_ack = ack.clone();
        tokio::spawn(async move {
            if let Ok(report) =
                reconcile_private(&reconcile_context, ReconcileTrigger::UnknownExecution).await
            {
                reconcile_context
                    .shared
                    .emit_command_event(CommandLaneEvent::Lifecycle(
                        CommandLifecycleEvent::RecoveryCompleted {
                            ack: recovery_ack,
                            report,
                        },
                    ));
            }
        });
    }
    ack
}

async fn sync_server_time(context: &LiveContext) -> Result<()> {
    let server_time = match &context.adapter {
        #[cfg(feature = "binance")]
        AdapterHandle::Binance(adapter) => {
            let payload =
                public_get_with_retry(context, "/fapi/v1/time", &[], "binance.server_time").await?;
            adapter.parse_server_time(&payload)?
        }
        #[cfg(feature = "bybit")]
        AdapterHandle::Bybit(adapter) => {
            let payload =
                public_get_with_retry(context, "/v5/market/time", &[], "bybit.server_time").await?;
            adapter.parse_server_time(&payload)?
        }
    };

    let local_time = timestamp_now_ms();
    let skew = server_time.value() - local_time.value();
    context
        .shared
        .write(|state| state.mark_rest_success(Some(skew)));
    Ok(())
}

async fn run_public_stream(
    context: LiveContext,
    subscription: PublicSubscription,
    mut shutdown: oneshot::Receiver<()>,
) -> Result<()> {
    let mut reconnect_attempt = 0_u32;
    loop {
        let loop_result = match &context.adapter {
            #[cfg(feature = "binance")]
            AdapterHandle::Binance(_) => {
                run_binance_public_stream(&context, &subscription, &mut shutdown).await
            }
            #[cfg(feature = "bybit")]
            AdapterHandle::Bybit(_) => {
                run_bybit_public_stream(&context, &subscription, &mut shutdown).await
            }
        };

        match loop_result {
            Ok(()) => return Ok(()),
            Err(error) if should_reconnect(&context, reconnect_attempt, &error) => {
                reconnect_attempt += 1;
                context.shared.write(|state| {
                    state.mark_public_disconnect();
                    state.mark_reconnect();
                });
                if context.config.reconnect.refresh_metadata_on_reconnect {
                    let _ = refresh_metadata(&context).await;
                }
                sleep(reconnect_backoff(&context, reconnect_attempt)).await;
            }
            Err(error) => return Err(error),
        }
    }
}

async fn run_private_stream(
    context: LiveContext,
    mut shutdown: oneshot::Receiver<()>,
) -> Result<()> {
    let mut reconnect_attempt = 0_u32;
    loop {
        let loop_result = match &context.adapter {
            #[cfg(feature = "binance")]
            AdapterHandle::Binance(_) => run_binance_private_stream(&context, &mut shutdown).await,
            #[cfg(feature = "bybit")]
            AdapterHandle::Bybit(_) => run_bybit_private_stream(&context, &mut shutdown).await,
        };

        match loop_result {
            Ok(()) => return Ok(()),
            Err(error) if should_reconnect(&context, reconnect_attempt, &error) => {
                reconnect_attempt += 1;
                context.shared.write(|state| {
                    state.mark_private_disconnect();
                    state.mark_reconnect();
                });
                #[cfg(feature = "bybit")]
                if matches!(context.adapter, AdapterHandle::Bybit(_)) {
                    context
                        .runtime_state
                        .invalidate_bybit_account_context()
                        .await;
                }
                if context.config.reconnect.reconcile_private_on_reconnect {
                    let _ = reconcile_private(&context, ReconcileTrigger::Reconnect).await;
                }
                sleep(reconnect_backoff(&context, reconnect_attempt)).await;
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(feature = "binance")]
async fn run_binance_public_stream(
    context: &LiveContext,
    subscription: &PublicSubscription,
    shutdown: &mut oneshot::Receiver<()>,
) -> Result<()> {
    let mut streams = Vec::new();
    for instrument_id in &subscription.instrument_ids {
        let spec = require_spec(context, instrument_id)?;
        let symbol = spec.native_symbol.to_ascii_lowercase();
        if subscription.ticker {
            streams.push(format!("{symbol}@ticker"));
        }
        if subscription.trades {
            streams.push(format!("{symbol}@aggTrade"));
        }
        if subscription.book_top {
            streams.push(format!("{symbol}@bookTicker"));
        }
        if subscription.order_book {
            streams.push(format!("{symbol}@depth20@100ms"));
        }
        if subscription.liquidations {
            streams.push(format!("{symbol}@forceOrder"));
        }
        if subscription.mark_price || subscription.funding_rate {
            streams.push(format!("{symbol}@markPrice@1s"));
        }
        for interval_value in &subscription.kline_intervals {
            let interval = parse_kline_interval(
                interval_value.as_ref(),
                context.config.venue,
                "binance.public_ws.kline_interval",
            )?;
            streams.push(format!("{symbol}@kline_{}", interval.as_binance_str()));
        }
    }

    let (mut ws, _) =
        tokio_tungstenite::connect_async(context.config.endpoints.public_ws_base.as_ref())
            .await
            .map_err(|error| {
                transport_error(context.config.venue, "binance.public_ws.connect", error)
            })?;
    let subscribe = serde_json::to_string(&json!({
        "method": "SUBSCRIBE",
        "params": streams,
        "id": 1_i64,
    }))
    .map_err(|error| {
        MarketError::new(
            ErrorKind::ConfigError,
            format!("failed to serialize binance public subscribe frame: {error}"),
        )
    })?;
    ws.send(Message::Text(subscribe.into()))
        .await
        .map_err(|error| {
            transport_error(context.config.venue, "binance.public_ws.subscribe", error)
        })?;
    let mut ping = interval(Duration::from_secs(20));
    let mut oi_poll = interval(Duration::from_secs(10));
    let mut maintenance = interval(Duration::from_millis(
        context.config.health.health_check_interval_ms.max(1),
    ));
    let mut last_frame_at = Instant::now();
    let mut last_metadata_refresh = Instant::now();
    let mut sequence = SequenceTracker::default();

    loop {
        tokio::select! {
            _ = &mut *shutdown => {
                let _ = ws.close(None).await;
                return Ok(());
            }
            _ = ping.tick() => {
                ws.send(Message::Ping(Vec::new().into())).await.map_err(|error| transport_error(context.config.venue, "binance.public_ws.ping", error))?;
            }
            _ = oi_poll.tick(), if subscription.open_interest => {
                for instrument_id in &subscription.instrument_ids {
                    let _ = refresh_open_interest(context, instrument_id).await;
                }
            }
            _ = maintenance.tick() => {
                if last_frame_at.elapsed() >= Duration::from_millis(context.config.timeouts.ws_idle_ms.max(1)) {
                    context.shared.apply_public_event(PublicLaneEvent::Divergence(
                        bat_markets_core::DivergenceEvent::SequenceGap { at: None },
                    ));
                    return Err(sequence_gap_error(
                        context.config.venue,
                        "binance.public_ws.idle",
                        None,
                    ));
                }

                if let Some(refresh_ms) = context.config.health.periodic_metadata_refresh_ms
                    && last_metadata_refresh.elapsed() >= Duration::from_millis(refresh_ms.max(1))
                {
                    let _ = sync_server_time(context).await;
                    let _ = refresh_metadata(context).await;
                    last_metadata_refresh = Instant::now();
                }
            }
            message = ws.next() => {
                let Some(message) = message else {
                    return Err(MarketError::new(ErrorKind::TransportError, "binance public stream closed"));
                };
                let message = message.map_err(|error| transport_error(context.config.venue, "binance.public_ws.read", error))?;
                if let Message::Text(payload) = message {
                    if is_binance_ack(&payload) {
                        continue;
                    }
                    for observation in binance_public_sequence_observations(context, &payload)? {
                        if let Err(at) = sequence.observe(observation) {
                            context.shared.apply_public_event(PublicLaneEvent::Divergence(
                                bat_markets_core::DivergenceEvent::SequenceGap {
                                    at: Some(SequenceNumber::new(at.max(0) as u64)),
                                },
                            ));
                            return Err(sequence_gap_error(context.config.venue, "binance.public_ws.sequence", Some(at)));
                        }
                    }
                        let mut events = context.adapter.as_adapter().parse_public(&payload)?;
                        retain_public_events_for_subscription(&mut events, subscription);
                        context.shared.apply_public_events(&events);
                    last_frame_at = Instant::now();
                }
            }
        }
    }
}

#[cfg(feature = "binance")]
async fn run_binance_private_stream(
    context: &LiveContext,
    shutdown: &mut oneshot::Receiver<()>,
) -> Result<()> {
    let listen_key = binance_start_listen_key(context).await?;
    let ws_url = format!(
        "{}/{}",
        context
            .config
            .endpoints
            .private_ws_base
            .trim_end_matches('/'),
        listen_key
    );
    let (mut ws, _) = tokio_tungstenite::connect_async(ws_url)
        .await
        .map_err(|error| {
            transport_error(context.config.venue, "binance.private_ws.connect", error)
        })?;
    let mut ping = interval(Duration::from_secs(20));
    let mut keepalive = interval(Duration::from_secs(30 * 60));
    let mut maintenance = interval(Duration::from_millis(
        context.config.health.health_check_interval_ms.max(1),
    ));
    let mut last_frame_at = Instant::now();
    let mut last_periodic_reconcile = Instant::now();
    let mut sequence = SequenceTracker::default();

    loop {
        tokio::select! {
            _ = &mut *shutdown => {
                let _ = ws.close(None).await;
                return Ok(());
            }
            _ = ping.tick() => {
                ws.send(Message::Ping(Vec::new().into())).await.map_err(|error| transport_error(context.config.venue, "binance.private_ws.ping", error))?;
            }
            _ = keepalive.tick() => {
                let _ = binance_keepalive_listen_key(context, &listen_key).await;
            }
            _ = maintenance.tick() => {
                if last_frame_at.elapsed() >= Duration::from_millis(context.config.timeouts.ws_idle_ms.max(1)) {
                    context.shared.apply_private_event(PrivateLaneEvent::Divergence(
                        bat_markets_core::DivergenceEvent::SequenceGap { at: None },
                    ));
                    return Err(sequence_gap_error(
                        context.config.venue,
                        "binance.private_ws.idle",
                        None,
                    ));
                }

                let age_ms = last_frame_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
                context.shared.write(|state| {
                    state.mark_snapshot_age(age_ms, context.config.health.snapshot_stale_after_ms);
                });

                if let Some(reconcile_ms) = context.config.health.periodic_private_reconcile_ms
                    && last_periodic_reconcile.elapsed() >= Duration::from_millis(reconcile_ms.max(1))
                {
                    let _ = reconcile_private(context, ReconcileTrigger::Periodic).await;
                    last_periodic_reconcile = Instant::now();
                }
            }
            message = ws.next() => {
                let Some(message) = message else {
                    return Err(MarketError::new(ErrorKind::TransportError, "binance private stream closed"));
                };
                let message = message.map_err(|error| transport_error(context.config.venue, "binance.private_ws.read", error))?;
                if let Message::Text(payload) = message {
                    if ws_frame_debug_enabled() && context.config.venue == Venue::Binance {
                        eprintln!("binance private frame: {payload}");
                    }
                    for observation in binance_private_sequence_observations(context, &payload)? {
                        if let Err(at) = sequence.observe(observation) {
                            context.shared.apply_private_event(PrivateLaneEvent::Divergence(
                                bat_markets_core::DivergenceEvent::SequenceGap {
                                    at: Some(SequenceNumber::new(at.max(0) as u64)),
                                },
                            ));
                            return Err(sequence_gap_error(context.config.venue, "binance.private_ws.sequence", Some(at)));
                        }
                    }
                    let events = context
                        .adapter
                        .as_adapter()
                        .parse_private(&payload)
                        .inspect_err(|error| {
                            if ws_frame_debug_enabled() && context.config.venue == Venue::Binance
                            {
                                eprintln!("binance private parse error: {error}");
                            }
                        })?;
                    if ws_frame_debug_enabled() && context.config.venue == Venue::Binance {
                        eprintln!("binance private events: {}", events.len());
                    }
                    context.shared.apply_private_events(&events);
                    last_frame_at = Instant::now();
                }
            }
        }
    }
}

#[cfg(feature = "bybit")]
async fn run_bybit_public_stream(
    context: &LiveContext,
    subscription: &PublicSubscription,
    shutdown: &mut oneshot::Receiver<()>,
) -> Result<()> {
    let mut args = Vec::new();
    for instrument_id in &subscription.instrument_ids {
        let spec = require_spec(context, instrument_id)?;
        if subscription.ticker
            || subscription.mark_price
            || subscription.funding_rate
            || subscription.open_interest
        {
            args.push(format!("tickers.{}", spec.native_symbol));
        }
        if subscription.trades {
            args.push(format!("publicTrade.{}", spec.native_symbol));
        }
        if subscription.book_top {
            args.push(format!("orderbook.1.{}", spec.native_symbol));
        }
        if subscription.order_book {
            args.push(format!("orderbook.50.{}", spec.native_symbol));
        }
        if subscription.liquidations {
            args.push(format!("allLiquidation.{}", spec.native_symbol));
        }
        for interval_value in &subscription.kline_intervals {
            let interval = parse_kline_interval(
                interval_value.as_ref(),
                context.config.venue,
                "bybit.public_ws.kline_interval",
            )?;
            args.push(format!(
                "kline.{}.{}",
                interval.as_bybit_str(),
                spec.native_symbol
            ));
        }
    }

    let (mut ws, _) =
        tokio_tungstenite::connect_async(context.config.endpoints.public_ws_base.as_ref())
            .await
            .map_err(|error| {
                transport_error(context.config.venue, "bybit.public_ws.connect", error)
            })?;
    let subscribe = serde_json::to_string(&json!({
        "op": "subscribe",
        "args": args,
    }))
    .map_err(|error| {
        MarketError::new(
            ErrorKind::ConfigError,
            format!("failed to serialize bybit public subscribe frame: {error}"),
        )
    })?;
    ws.send(Message::Text(subscribe.into()))
        .await
        .map_err(|error| {
            transport_error(context.config.venue, "bybit.public_ws.subscribe", error)
        })?;
    let mut ping = interval(Duration::from_secs(20));
    let mut maintenance = interval(Duration::from_millis(
        context.config.health.health_check_interval_ms.max(1),
    ));
    let mut last_frame_at = Instant::now();
    let mut last_metadata_refresh = Instant::now();
    let mut sequence = SequenceTracker::default();

    loop {
        tokio::select! {
            _ = &mut *shutdown => {
                let _ = ws.close(None).await;
                return Ok(());
            }
            _ = ping.tick() => {
                let ping_frame = serde_json::to_string(&json!({"op":"ping"})).map_err(|error| {
                    MarketError::new(ErrorKind::ConfigError, format!("failed to serialize bybit ping frame: {error}"))
                })?;
                ws.send(Message::Text(ping_frame.into())).await.map_err(|error| transport_error(context.config.venue, "bybit.public_ws.ping", error))?;
            }
            _ = maintenance.tick() => {
                if last_frame_at.elapsed() >= Duration::from_millis(context.config.timeouts.ws_idle_ms.max(1)) {
                    context.shared.apply_public_event(PublicLaneEvent::Divergence(
                        bat_markets_core::DivergenceEvent::SequenceGap { at: None },
                    ));
                    return Err(sequence_gap_error(
                        context.config.venue,
                        "bybit.public_ws.idle",
                        None,
                    ));
                }

                if let Some(refresh_ms) = context.config.health.periodic_metadata_refresh_ms
                    && last_metadata_refresh.elapsed() >= Duration::from_millis(refresh_ms.max(1))
                {
                    let _ = sync_server_time(context).await;
                    let _ = refresh_metadata(context).await;
                    last_metadata_refresh = Instant::now();
                }
            }
            message = ws.next() => {
                let Some(message) = message else {
                    return Err(MarketError::new(ErrorKind::TransportError, "bybit public stream closed"));
                };
                let message = message.map_err(|error| transport_error(context.config.venue, "bybit.public_ws.read", error))?;
                match message {
                    Message::Text(payload) => {
                        last_frame_at = Instant::now();
                        if ws_frame_debug_enabled() && context.config.venue == Venue::Bybit {
                            eprintln!("bybit public frame: {payload}");
                        }
                        if is_bybit_control_message(&payload) {
                            continue;
                        }
                        for observation in bybit_public_sequence_observations(context, &payload)? {
                            if let Err(at) = sequence.observe(observation) {
                                context.shared.apply_public_event(PublicLaneEvent::Divergence(
                                    bat_markets_core::DivergenceEvent::SequenceGap {
                                        at: Some(SequenceNumber::new(at.max(0) as u64)),
                                    },
                                ));
                                return Err(sequence_gap_error(context.config.venue, "bybit.public_ws.sequence", Some(at)));
                            }
                        }
                        let mut events = match context.adapter.as_adapter().parse_public(&payload) {
                            Ok(events) => events,
                            Err(error) => {
                                if ws_frame_debug_enabled() && context.config.venue == Venue::Bybit
                                {
                                    eprintln!("bybit public parse error: {error}");
                                }
                                return Err(error);
                            }
                        };
                        retain_public_events_for_subscription(&mut events, subscription);
                        context.shared.apply_public_events(&events);
                    }
                    Message::Ping(_) | Message::Pong(_) | Message::Binary(_) => {
                        last_frame_at = Instant::now();
                    }
                    _ => {}
                }
            }
        }
    }
}

#[cfg(feature = "bybit")]
async fn run_bybit_private_stream(
    context: &LiveContext,
    shutdown: &mut oneshot::Receiver<()>,
) -> Result<()> {
    let (api_key, signer) = require_credentials(context)?;
    let (mut ws, _) =
        tokio_tungstenite::connect_async(context.config.endpoints.private_ws_base.as_ref())
            .await
            .map_err(|error| {
                transport_error(context.config.venue, "bybit.private_ws.connect", error)
            })?;
    let expires = timestamp_now_ms().value() + 10_000;
    let signature = signer.sign_hex(format!("GET/realtime{expires}").as_bytes())?;
    let auth = serde_json::to_string(&json!({
        "op": "auth",
        "args": [api_key.as_ref(), expires, signature],
    }))
    .map_err(|error| {
        MarketError::new(
            ErrorKind::ConfigError,
            format!("failed to serialize bybit private auth frame: {error}"),
        )
    })?;
    ws.send(Message::Text(auth.into()))
        .await
        .map_err(|error| transport_error(context.config.venue, "bybit.private_ws.auth", error))?;
    let subscribe = serde_json::to_string(&json!({
        "op": "subscribe",
        "args": ["wallet", "position", "order", "execution"],
    }))
    .map_err(|error| {
        MarketError::new(
            ErrorKind::ConfigError,
            format!("failed to serialize bybit private subscribe frame: {error}"),
        )
    })?;
    ws.send(Message::Text(subscribe.into()))
        .await
        .map_err(|error| {
            transport_error(context.config.venue, "bybit.private_ws.subscribe", error)
        })?;
    let mut ping = interval(Duration::from_secs(20));
    let mut maintenance = interval(Duration::from_millis(
        context.config.health.health_check_interval_ms.max(1),
    ));
    let mut last_frame_at = Instant::now();
    let mut last_periodic_reconcile = Instant::now();
    let mut sequence = SequenceTracker::default();

    loop {
        tokio::select! {
            _ = &mut *shutdown => {
                let _ = ws.close(None).await;
                return Ok(());
            }
            _ = ping.tick() => {
                let ping_frame = serde_json::to_string(&json!({"op":"ping"})).map_err(|error| {
                    MarketError::new(ErrorKind::ConfigError, format!("failed to serialize bybit ping frame: {error}"))
                })?;
                ws.send(Message::Text(ping_frame.into())).await.map_err(|error| transport_error(context.config.venue, "bybit.private_ws.ping", error))?;
            }
            _ = maintenance.tick() => {
                if last_frame_at.elapsed() >= Duration::from_millis(context.config.timeouts.ws_idle_ms.max(1)) {
                    context.shared.apply_private_event(PrivateLaneEvent::Divergence(
                        bat_markets_core::DivergenceEvent::SequenceGap { at: None },
                    ));
                    return Err(sequence_gap_error(
                        context.config.venue,
                        "bybit.private_ws.idle",
                        None,
                    ));
                }

                let age_ms = last_frame_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
                context.shared.write(|state| {
                    state.mark_snapshot_age(age_ms, context.config.health.snapshot_stale_after_ms);
                });

                if let Some(reconcile_ms) = context.config.health.periodic_private_reconcile_ms
                    && last_periodic_reconcile.elapsed() >= Duration::from_millis(reconcile_ms.max(1))
                {
                    let _ = reconcile_private(context, ReconcileTrigger::Periodic).await;
                    last_periodic_reconcile = Instant::now();
                }
            }
            message = ws.next() => {
                let Some(message) = message else {
                    return Err(MarketError::new(ErrorKind::TransportError, "bybit private stream closed"));
                };
                let message = message.map_err(|error| transport_error(context.config.venue, "bybit.private_ws.read", error))?;
                match message {
                    Message::Text(payload) => {
                        last_frame_at = Instant::now();
                        if is_bybit_control_message(&payload) {
                            continue;
                        }
                        for observation in bybit_private_sequence_observations(context, &payload)? {
                            if let Err(at) = sequence.observe(observation) {
                                context.shared.apply_private_event(PrivateLaneEvent::Divergence(
                                    bat_markets_core::DivergenceEvent::SequenceGap {
                                        at: Some(SequenceNumber::new(at.max(0) as u64)),
                                    },
                                ));
                                return Err(sequence_gap_error(context.config.venue, "bybit.private_ws.sequence", Some(at)));
                            }
                        }
                        let events = context.adapter.as_adapter().parse_private(&payload)?;
                        context.shared.apply_private_events(&events);
                    }
                    Message::Ping(_) | Message::Pong(_) | Message::Binary(_) => {
                        last_frame_at = Instant::now();
                    }
                    _ => {}
                }
            }
        }
    }
}

fn should_reconnect(context: &LiveContext, reconnect_attempt: u32, error: &MarketError) -> bool {
    if !matches!(
        error.kind,
        ErrorKind::TransportError | ErrorKind::Timeout | ErrorKind::TemporaryUnavailable
    ) {
        return false;
    }

    match context.config.reconnect.max_reconnect_attempts {
        Some(max_attempts) => reconnect_attempt < max_attempts,
        None => true,
    }
}

fn reconnect_backoff(context: &LiveContext, reconnect_attempt: u32) -> Duration {
    let initial = context.config.reconnect.ws_initial_backoff_ms.max(1);
    let max = context.config.reconnect.ws_max_backoff_ms.max(initial);
    let delay = initial.saturating_mul(2_u64.saturating_pow(reconnect_attempt.min(8)));
    Duration::from_millis(delay.min(max))
}

fn require_spec(context: &LiveContext, instrument_id: &InstrumentId) -> Result<InstrumentSpec> {
    context
        .adapter
        .as_adapter()
        .resolve_instrument(instrument_id)
        .ok_or_else(|| {
            MarketError::new(
                ErrorKind::Unsupported,
                format!("unknown instrument {instrument_id}"),
            )
            .with_venue(context.config.venue, context.config.product)
        })
}

fn require_credentials(
    context: &LiveContext,
) -> Result<(Arc<str>, Arc<dyn bat_markets_core::Signer>)> {
    let api_key = context.api_key.clone().ok_or_else(|| {
        MarketError::new(
            ErrorKind::AuthError,
            "missing API key in configured environment variables",
        )
        .with_venue(context.config.venue, context.config.product)
    })?;
    let signer = context.signer.clone().ok_or_else(|| {
        MarketError::new(
            ErrorKind::AuthError,
            "missing signer for configured private API flow",
        )
        .with_venue(context.config.venue, context.config.product)
    })?;
    Ok((api_key, signer))
}

#[cfg(feature = "binance")]
async fn binance_signed_ws_request_text(
    context: &LiveContext,
    method: &str,
    params: Vec<(String, String)>,
    operation: &str,
) -> std::result::Result<String, CommandWsRequestError> {
    let (api_key, signer) =
        require_credentials(context).map_err(CommandWsRequestError::Unavailable)?;
    let timestamp = timestamp_now_ms().value();
    let mut signable = BTreeMap::new();
    for (key, value) in params {
        signable.insert(key, value);
    }
    signable.insert("apiKey".to_owned(), api_key.to_string());
    signable.insert("recvWindow".to_owned(), "5000".to_owned());
    signable.insert("timestamp".to_owned(), timestamp.to_string());
    let query = {
        let mut serializer = Serializer::new(String::new());
        for (key, value) in &signable {
            serializer.append_pair(key, value);
        }
        serializer.finish()
    };
    let signature = signer
        .sign_hex(query.as_bytes())
        .map_err(CommandWsRequestError::Unavailable)?;

    let mut payload = serde_json::Map::new();
    for (key, value) in signable {
        match key.as_str() {
            "timestamp" | "recvWindow" => {
                let parsed = value.parse::<i64>().map_err(|error| {
                    CommandWsRequestError::Unavailable(MarketError::new(
                        ErrorKind::ConfigError,
                        format!(
                            "failed to encode binance websocket numeric parameter '{key}': {error}"
                        ),
                    ))
                })?;
                payload.insert(key, Value::from(parsed));
            }
            _ => {
                payload.insert(key, Value::String(value));
            }
        }
    }
    payload.insert("signature".to_owned(), Value::String(signature));

    let request_id = context.command_transport.next_request_id("binance");
    let frame = serde_json::to_string(&json!({
        "id": request_id,
        "method": method,
        "params": payload,
    }))
    .map_err(|error| {
        CommandWsRequestError::Unavailable(MarketError::new(
            ErrorKind::ConfigError,
            format!("failed to serialize binance websocket command frame: {error}"),
        ))
    })?;
    context
        .command_transport
        .request_text(request_id, operation.to_owned(), frame)
        .await
}

#[cfg(feature = "bybit")]
async fn bybit_trade_ws_request_text(
    context: &LiveContext,
    op: &str,
    args: Vec<Value>,
    operation: &str,
) -> std::result::Result<String, CommandWsRequestError> {
    require_credentials(context).map_err(CommandWsRequestError::Unavailable)?;
    let request_id = context.command_transport.next_request_id("bybit");
    let frame = serde_json::to_string(&json!({
        "reqId": request_id,
        "header": {
            "X-BAPI-TIMESTAMP": timestamp_now_ms().value().to_string(),
            "X-BAPI-RECV-WINDOW": "5000",
        },
        "op": op,
        "args": args,
    }))
    .map_err(|error| {
        CommandWsRequestError::Unavailable(MarketError::new(
            ErrorKind::ConfigError,
            format!("failed to serialize bybit websocket command frame: {error}"),
        ))
    })?;
    context
        .command_transport
        .request_text(request_id, operation.to_owned(), frame)
        .await
}

async fn public_get_with_retry(
    context: &LiveContext,
    path: &str,
    query: &[(&str, &str)],
    operation: &str,
) -> Result<String> {
    with_rest_retries(context, operation, || async move {
        public_get_text(context, path, query, operation).await
    })
    .await
}

fn parse_kline_interval(raw: &str, venue: Venue, operation: &str) -> Result<KlineInterval> {
    KlineInterval::parse(raw).ok_or_else(|| {
        MarketError::new(
            ErrorKind::Unsupported,
            format!("unsupported OHLCV interval '{raw}'"),
        )
        .with_venue(venue, Product::LinearUsdt)
        .with_operation(operation)
    })
}

async fn public_get_text(
    context: &LiveContext,
    path: &str,
    query: &[(&str, &str)],
    operation: &str,
) -> Result<String> {
    let url = format!("{}{}", context.config.endpoints.rest_base, path);
    let response = context
        .http
        .get(url)
        .query(query)
        .send()
        .await
        .map_err(|error| classify_reqwest_error(context.config.venue, operation, error))?;
    response_text(context.config.venue, response, operation).await
}

#[cfg(feature = "binance")]
async fn binance_signed_request_text(
    context: &LiveContext,
    method: Method,
    path: &str,
    pairs: &[(&str, &str)],
    operation: &str,
) -> Result<String> {
    let (api_key, signer) = require_credentials(context)?;
    let timestamp = timestamp_now_ms().value().to_string();
    let query = {
        let mut serializer = Serializer::new(String::new());
        for (key, value) in pairs {
            serializer.append_pair(key, value);
        }
        serializer.append_pair("timestamp", &timestamp);
        serializer.append_pair("recvWindow", "5000");
        serializer.finish()
    };
    let signature = signer.sign_hex(query.as_bytes())?;
    let url = format!(
        "{}{}?{}&signature={}",
        context.config.endpoints.rest_base, path, query, signature
    );
    let request = context
        .http
        .request(method, url)
        .header("X-MBX-APIKEY", api_key.as_ref());
    let response = request
        .send()
        .await
        .map_err(|error| classify_reqwest_error(context.config.venue, operation, error))?;
    response_text(context.config.venue, response, operation).await
}

#[cfg(feature = "bybit")]
async fn bybit_signed_get_text(
    context: &LiveContext,
    path: &str,
    query: &[(&str, &str)],
    operation: &str,
) -> Result<String> {
    let (api_key, signer) = require_credentials(context)?;
    let timestamp = timestamp_now_ms().value().to_string();
    let query = {
        let mut serializer = Serializer::new(String::new());
        for (key, value) in query {
            serializer.append_pair(key, value);
        }
        serializer.finish()
    };
    let payload = format!(
        "{timestamp}{}{recv_window}{query}",
        api_key.as_ref(),
        recv_window = 5000
    );
    let signature = signer.sign_hex(payload.as_bytes())?;
    let url = if query.is_empty() {
        format!("{}{}", context.config.endpoints.rest_base, path)
    } else {
        format!("{}{}?{}", context.config.endpoints.rest_base, path, query)
    };
    let response = context
        .http
        .get(url)
        .header("X-BAPI-API-KEY", api_key.as_ref())
        .header("X-BAPI-SIGN", signature)
        .header("X-BAPI-TIMESTAMP", timestamp)
        .header("X-BAPI-RECV-WINDOW", "5000")
        .send()
        .await
        .map_err(|error| classify_reqwest_error(context.config.venue, operation, error))?;
    response_text(context.config.venue, response, operation).await
}

#[cfg(feature = "bybit")]
async fn bybit_signed_post_text(
    context: &LiveContext,
    path: &str,
    body: &str,
    operation: &str,
) -> Result<String> {
    let (api_key, signer) = require_credentials(context)?;
    let timestamp = timestamp_now_ms().value().to_string();
    let payload = format!(
        "{timestamp}{}{recv_window}{body}",
        api_key.as_ref(),
        recv_window = 5000
    );
    let signature = signer.sign_hex(payload.as_bytes())?;
    let url = format!("{}{}", context.config.endpoints.rest_base, path);
    let response = context
        .http
        .post(url)
        .header("content-type", "application/json")
        .header("X-BAPI-API-KEY", api_key.as_ref())
        .header("X-BAPI-SIGN", signature)
        .header("X-BAPI-TIMESTAMP", timestamp)
        .header("X-BAPI-RECV-WINDOW", "5000")
        .body(body.to_owned())
        .send()
        .await
        .map_err(|error| classify_reqwest_error(context.config.venue, operation, error))?;
    response_text(context.config.venue, response, operation).await
}

#[cfg(feature = "binance")]
async fn binance_start_listen_key(context: &LiveContext) -> Result<String> {
    let (api_key, _) = require_credentials(context)?;
    let url = format!(
        "{}{}",
        context.config.endpoints.rest_base, "/fapi/v1/listenKey"
    );
    let response = context
        .http
        .post(url)
        .header("X-MBX-APIKEY", api_key.as_ref())
        .send()
        .await
        .map_err(|error| {
            classify_reqwest_error(context.config.venue, "binance.start_listen_key", error)
        })?;
    let payload = response_text(context.config.venue, response, "binance.start_listen_key").await?;
    let value: Value = serde_json::from_str(&payload).map_err(|error| {
        MarketError::new(
            ErrorKind::DecodeError,
            format!("failed to decode binance listen-key response: {error}"),
        )
    })?;
    value
        .get("listenKey")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            MarketError::new(
                ErrorKind::DecodeError,
                "missing listenKey in binance response",
            )
        })
}

#[cfg(feature = "binance")]
async fn binance_keepalive_listen_key(context: &LiveContext, listen_key: &str) -> Result<()> {
    let (api_key, _) = require_credentials(context)?;
    let url = format!(
        "{}{}",
        context.config.endpoints.rest_base, "/fapi/v1/listenKey"
    );
    let response = context
        .http
        .put(url)
        .header("X-MBX-APIKEY", api_key.as_ref())
        .query(&[("listenKey", listen_key)])
        .send()
        .await
        .map_err(|error| {
            classify_reqwest_error(context.config.venue, "binance.keepalive_listen_key", error)
        })?;
    let _ = response_text(
        context.config.venue,
        response,
        "binance.keepalive_listen_key",
    )
    .await?;
    Ok(())
}

#[cfg(feature = "bybit")]
async fn refresh_bybit_account_context(
    context: &LiveContext,
) -> Result<bat_markets_bybit::BybitAccountContext> {
    let Some(adapter) = bybit_adapter(context) else {
        return Err(MarketError::new(
            ErrorKind::Unsupported,
            "bybit account context requested for non-bybit adapter",
        ));
    };
    refresh_bybit_account_context_with_adapter(context, adapter).await
}

#[cfg(feature = "bybit")]
async fn refresh_bybit_account_context_with_adapter(
    context: &LiveContext,
    adapter: &bat_markets_bybit::BybitLinearFuturesAdapter,
) -> Result<bat_markets_bybit::BybitAccountContext> {
    if let Some(cached) = context.runtime_state.cached_bybit_account_context().await {
        return Ok(cached);
    }
    let payload =
        bybit_signed_get_text(context, "/v5/account/info", &[], "bybit.account_info").await?;
    let parsed = adapter.parse_account_context(&payload)?;
    context
        .runtime_state
        .cache_bybit_account_context(parsed.clone())
        .await;
    Ok(parsed)
}

fn hydrate_create_receipt(
    mut receipt: CommandReceipt,
    request: &CreateOrderRequest,
) -> CommandReceipt {
    if receipt.instrument_id.is_none() {
        receipt.instrument_id = Some(request.instrument_id.clone());
    }
    if receipt.client_order_id.is_none() {
        receipt.client_order_id = request.client_order_id.clone();
    }
    receipt
}

fn hydrate_cancel_receipt(
    mut receipt: CommandReceipt,
    request: &CancelOrderRequest,
) -> CommandReceipt {
    if receipt.instrument_id.is_none() {
        receipt.instrument_id = Some(request.instrument_id.clone());
    }
    if receipt.order_id.is_none() {
        receipt.order_id = request.order_id.clone();
    }
    if receipt.client_order_id.is_none() {
        receipt.client_order_id = request.client_order_id.clone();
    }
    receipt
}

fn hydrate_amend_receipt(
    mut receipt: CommandReceipt,
    request: &AmendOrderRequest,
) -> CommandReceipt {
    if receipt.instrument_id.is_none() {
        receipt.instrument_id = Some(request.instrument_id.clone());
    }
    if receipt.order_id.is_none() {
        receipt.order_id = request.order_id.clone();
    }
    if receipt.client_order_id.is_none() {
        receipt.client_order_id = request.client_order_id.clone();
    }
    receipt
}

async fn validate_amend_order(context: &LiveContext, request: &AmendOrderRequest) -> Result<()> {
    require_spec(context, &request.instrument_id)?;
    append_order_identity(
        &mut Vec::new(),
        request.order_id.as_ref(),
        request.client_order_id.as_ref(),
    )?;
    if request.quantity.is_none() && request.price.is_none() && request.trigger_price.is_none() {
        return Err(MarketError::new(
            ErrorKind::ConfigError,
            "amend_order requires at least one of quantity, price, or trigger_price",
        ));
    }
    Ok(())
}

#[cfg(feature = "binance")]
async fn resolve_order_for_amend(
    context: &LiveContext,
    request: &AmendOrderRequest,
) -> Result<Order> {
    if let Some(order) = context.shared.read(|state| {
        state.orders().into_iter().find(|order| {
            order.instrument_id == request.instrument_id
                && match (&request.order_id, &request.client_order_id) {
                    (Some(order_id), _) => &order.order_id == order_id,
                    (None, Some(client_order_id)) => {
                        order.client_order_id.as_ref() == Some(client_order_id)
                    }
                    (None, None) => false,
                }
        })
    }) {
        return Ok(order);
    }

    get_order(
        context,
        &GetOrderRequest {
            request_id: request.request_id.clone(),
            instrument_id: request.instrument_id.clone(),
            order_id: request.order_id.clone(),
            client_order_id: request.client_order_id.clone(),
        },
    )
    .await
}

#[derive(Clone, Debug)]
struct BatchIdentity {
    instrument_id: Option<InstrumentId>,
    order_id: Option<OrderId>,
    client_order_id: Option<ClientOrderId>,
    #[cfg(feature = "bybit")]
    request_id: Option<RequestId>,
}

fn hydrate_create_request_with_batch_id(
    mut request: CreateOrderRequest,
    batch_request_id: Option<RequestId>,
) -> CreateOrderRequest {
    if request.request_id.is_none() {
        request.request_id = batch_request_id;
    }
    request
}

fn hydrate_amend_request_with_batch_id(
    mut request: AmendOrderRequest,
    batch_request_id: Option<RequestId>,
) -> AmendOrderRequest {
    if request.request_id.is_none() {
        request.request_id = batch_request_id;
    }
    request
}

fn validate_create_orders(context: &LiveContext, request: &CreateOrdersRequest) -> Result<()> {
    if request.orders.is_empty() {
        return Err(MarketError::new(
            ErrorKind::ConfigError,
            "create_orders requires at least one order",
        ));
    }
    let mut client_order_ids = BTreeMap::<ClientOrderId, usize>::new();
    for (index, order) in request.orders.iter().enumerate() {
        validate_create_order(context, order)?;
        if request.orders.len() > 1 {
            let client_order_id = order.client_order_id.clone().ok_or_else(|| {
                MarketError::new(
                    ErrorKind::ConfigError,
                    "batch create requires client_order_id on every order for deterministic tracking",
                )
            })?;
            if let Some(previous) = client_order_ids.insert(client_order_id.clone(), index) {
                return Err(MarketError::new(
                    ErrorKind::ConfigError,
                    format!(
                        "duplicate client_order_id '{}' in batch create at indexes {} and {}",
                        client_order_id, previous, index
                    ),
                ));
            }
        }
    }
    Ok(())
}

async fn validate_amend_orders(context: &LiveContext, request: &AmendOrdersRequest) -> Result<()> {
    if request.orders.is_empty() {
        return Err(MarketError::new(
            ErrorKind::ConfigError,
            "amend_orders requires at least one order",
        ));
    }
    for order in &request.orders {
        validate_amend_order(context, order).await?;
    }
    Ok(())
}

fn validate_cancel_orders(request: &CancelOrdersRequest) -> Result<()> {
    if request.orders.is_empty() {
        return Err(MarketError::new(
            ErrorKind::ConfigError,
            "cancel_orders requires at least one target",
        ));
    }
    for target in &request.orders {
        match (&target.order_id, &target.client_order_id) {
            (Some(_), _) | (None, Some(_)) => {}
            (None, None) => {
                return Err(MarketError::new(
                    ErrorKind::ConfigError,
                    "cancel_orders requires order_id or client_order_id on every target",
                ));
            }
        }
    }
    Ok(())
}

fn collect_batch_acks(acks: Vec<Option<CommandAck>>, operation: &str) -> Result<Vec<CommandAck>> {
    acks.into_iter()
        .enumerate()
        .map(|(index, ack)| {
            ack.ok_or_else(|| {
                MarketError::new(
                    ErrorKind::DecodeError,
                    format!("missing ack {index} while finalizing {operation}"),
                )
            })
        })
        .collect()
}

async fn cache_and_emit_create_receipts(
    context: &LiveContext,
    receipts: &[CommandReceipt],
    requests: &[CreateOrderRequest],
    transport: CommandTransport,
) -> Vec<CommandAck> {
    let identities = requests
        .iter()
        .map(|order| BatchIdentity {
            instrument_id: Some(order.instrument_id.clone()),
            order_id: None,
            client_order_id: order.client_order_id.clone(),
            #[cfg(feature = "bybit")]
            request_id: order.request_id.clone(),
        })
        .collect::<Vec<_>>();
    cache_and_emit_batch_receipts(
        context,
        receipts,
        &identities,
        CommandOperation::CreateOrder,
        transport,
    )
    .await
}

async fn cache_and_emit_amend_receipts(
    context: &LiveContext,
    receipts: &[CommandReceipt],
    requests: &[AmendOrderRequest],
    transport: CommandTransport,
) -> Vec<CommandAck> {
    let identities = requests
        .iter()
        .map(|order| BatchIdentity {
            instrument_id: Some(order.instrument_id.clone()),
            order_id: order.order_id.clone(),
            client_order_id: order.client_order_id.clone(),
            #[cfg(feature = "bybit")]
            request_id: order.request_id.clone(),
        })
        .collect::<Vec<_>>();
    cache_and_emit_batch_receipts(
        context,
        receipts,
        &identities,
        CommandOperation::AmendOrder,
        transport,
    )
    .await
}

async fn cache_and_emit_cancel_receipts(
    context: &LiveContext,
    receipts: &[CommandReceipt],
    targets: &[OrderTarget],
    transport: CommandTransport,
) -> Vec<CommandAck> {
    let identities = targets
        .iter()
        .map(|target| BatchIdentity {
            instrument_id: Some(target.instrument_id.clone()),
            order_id: target.order_id.clone(),
            client_order_id: target.client_order_id.clone(),
            #[cfg(feature = "bybit")]
            request_id: None,
        })
        .collect::<Vec<_>>();
    cache_and_emit_batch_receipts(
        context,
        receipts,
        &identities,
        CommandOperation::CancelOrder,
        transport,
    )
    .await
}

async fn cache_and_emit_batch_receipts(
    context: &LiveContext,
    receipts: &[CommandReceipt],
    identities: &[BatchIdentity],
    operation: CommandOperation,
    transport: CommandTransport,
) -> Vec<CommandAck> {
    let mut acks = Vec::with_capacity(receipts.len());
    for (receipt, identity) in receipts.iter().zip(identities) {
        if receipt.status == CommandStatus::UnknownExecution
            && let Some(instrument_id) = &identity.instrument_id
        {
            context
                .runtime_state
                .cache_pending_unknown(PendingUnknownCommand {
                    operation,
                    instrument_id: instrument_id.clone(),
                    order_id: identity.order_id.clone(),
                    client_order_id: identity.client_order_id.clone(),
                    recorded_at: timestamp_now_ms(),
                })
                .await;
        }
        acks.push(apply_command_receipt(context, receipt.clone(), transport).await);
    }
    acks
}

fn unknown_command_receipt(
    context: &LiveContext,
    operation: CommandOperation,
    instrument_id: Option<InstrumentId>,
    order_id: Option<OrderId>,
    client_order_id: Option<ClientOrderId>,
    request_id: Option<RequestId>,
) -> CommandReceipt {
    CommandReceipt {
        operation,
        status: CommandStatus::UnknownExecution,
        venue: context.config.venue,
        product: context.config.product,
        instrument_id,
        order_id,
        client_order_id,
        request_id,
        message: Some("command outcome requires reconcile".into()),
        native_code: None,
        retriable: true,
    }
}

#[cfg(feature = "binance")]
fn build_binance_batch_create_object(
    context: &LiveContext,
    request: &CreateOrderRequest,
) -> Result<Value> {
    let spec = require_spec(context, &request.instrument_id)?;
    let mut order = serde_json::Map::new();
    order.insert(
        "symbol".to_owned(),
        Value::String(spec.native_symbol.to_string()),
    );
    order.insert(
        "side".to_owned(),
        Value::String(binance_side(request.side).to_owned()),
    );
    order.insert(
        "type".to_owned(),
        Value::String(binance_order_type(request.order_type).to_owned()),
    );
    order.insert(
        "quantity".to_owned(),
        Value::String(format_quantity(request.quantity)),
    );
    if let Some(client_order_id) = &request.client_order_id {
        order.insert(
            "newClientOrderId".to_owned(),
            Value::String(client_order_id.to_string()),
        );
    }
    if let Some(price) = request.price {
        order.insert("price".to_owned(), Value::String(format_price(price)));
    }
    if let Some(trigger_price) = request.trigger_price {
        order.insert(
            "stopPrice".to_owned(),
            Value::String(format_price(trigger_price)),
        );
    }
    if let Some(trigger_type) = request.trigger_type {
        order.insert(
            "workingType".to_owned(),
            Value::String(binance_trigger_type(trigger_type).to_owned()),
        );
    }
    if let Some(time_in_force) = request.time_in_force {
        order.insert(
            "timeInForce".to_owned(),
            Value::String(binance_time_in_force(time_in_force, request.post_only).to_owned()),
        );
    }
    if request.reduce_only {
        order.insert("reduceOnly".to_owned(), Value::String("true".to_owned()));
    }
    Ok(Value::Object(order))
}

#[cfg(feature = "bybit")]
fn build_bybit_batch_create_object(
    context: &LiveContext,
    request: &CreateOrderRequest,
) -> Result<Value> {
    let spec = require_spec(context, &request.instrument_id)?;
    Ok(json!({
        "symbol": spec.native_symbol,
        "side": bybit_side(request.side),
        "orderType": bybit_order_type(request.order_type),
        "qty": format_quantity(request.quantity),
        "price": request.price.map(format_price),
        "triggerPrice": request.trigger_price.map(format_price),
        "triggerBy": request.trigger_type.map(bybit_trigger_type),
        "timeInForce": request.time_in_force.map(|value| bybit_time_in_force(value, request.post_only)),
        "orderLinkId": request.client_order_id.as_ref().map(ToString::to_string),
        "reduceOnly": request.reduce_only,
    }))
}

#[cfg(feature = "binance")]
fn build_binance_batch_amend_object(
    context: &LiveContext,
    request: &AmendOrderRequest,
) -> Result<Value> {
    let spec = require_spec(context, &request.instrument_id)?;
    let current_order = resolve_cached_order_for_amend(context, request)?;
    if current_order.order_type != OrderType::Limit {
        return Err(MarketError::new(
            ErrorKind::Unsupported,
            "binance batch amend currently supports only cached LIMIT orders",
        ));
    }
    if request.trigger_price.is_some() {
        return Err(MarketError::new(
            ErrorKind::Unsupported,
            "binance batch amend does not support modifying trigger_price on linear futures",
        ));
    }
    let price = request.price.or(current_order.price).ok_or_else(|| {
        MarketError::new(
            ErrorKind::ConfigError,
            "binance batch amend requires a limit price or a cached order price",
        )
    })?;
    let quantity = request.quantity.unwrap_or(current_order.quantity);

    let mut order = serde_json::Map::new();
    order.insert(
        "symbol".to_owned(),
        Value::String(spec.native_symbol.to_string()),
    );
    order.insert(
        "side".to_owned(),
        Value::String(binance_side(current_order.side).to_owned()),
    );
    order.insert(
        "quantity".to_owned(),
        Value::String(format_quantity(quantity)),
    );
    order.insert("price".to_owned(), Value::String(format_price(price)));
    append_binance_order_identity_json(
        &mut order,
        request.order_id.as_ref(),
        request.client_order_id.as_ref(),
    )?;
    Ok(Value::Object(order))
}

#[cfg(feature = "bybit")]
fn build_bybit_batch_amend_object(
    context: &LiveContext,
    request: &AmendOrderRequest,
) -> Result<Value> {
    let spec = require_spec(context, &request.instrument_id)?;
    let mut order = serde_json::Map::new();
    order.insert(
        "symbol".to_owned(),
        Value::String(spec.native_symbol.to_string()),
    );
    append_bybit_order_identity_json(
        &mut order,
        request.order_id.as_ref(),
        request.client_order_id.as_ref(),
    )?;
    if let Some(quantity) = request.quantity {
        order.insert("qty".to_owned(), Value::String(format_quantity(quantity)));
    }
    if let Some(price) = request.price {
        order.insert("price".to_owned(), Value::String(format_price(price)));
    }
    if let Some(trigger_price) = request.trigger_price {
        order.insert(
            "triggerPrice".to_owned(),
            Value::String(format_price(trigger_price)),
        );
    }
    Ok(Value::Object(order))
}

#[cfg(feature = "bybit")]
fn build_bybit_batch_cancel_object(context: &LiveContext, request: &OrderTarget) -> Result<Value> {
    let spec = require_spec(context, &request.instrument_id)?;
    let mut order = serde_json::Map::new();
    order.insert(
        "symbol".to_owned(),
        Value::String(spec.native_symbol.to_string()),
    );
    append_bybit_order_identity_json(
        &mut order,
        request.order_id.as_ref(),
        request.client_order_id.as_ref(),
    )?;
    Ok(Value::Object(order))
}

#[cfg(feature = "binance")]
fn resolve_cached_order_for_amend(
    context: &LiveContext,
    request: &AmendOrderRequest,
) -> Result<Order> {
    context
        .shared
        .read(|state| {
            state.orders().into_iter().find(|order| {
                order.instrument_id == request.instrument_id
                    && match (&request.order_id, &request.client_order_id) {
                        (Some(order_id), _) => &order.order_id == order_id,
                        (None, Some(client_order_id)) => {
                            order.client_order_id.as_ref() == Some(client_order_id)
                        }
                        (None, None) => false,
                    }
            })
        })
        .ok_or_else(|| {
            MarketError::new(
                ErrorKind::ConfigError,
                format!(
                    "binance batch amend requires cached order state for {}",
                    request.instrument_id
                ),
            )
        })
}

#[cfg(feature = "binance")]
fn classify_binance_batch_payload(
    adapter: &bat_markets_binance::BinanceLinearFuturesAdapter,
    operation: CommandOperation,
    payload: &str,
    request_ids: &[Option<RequestId>],
) -> Result<Vec<CommandReceipt>> {
    let items = serde_json::from_str::<Vec<Value>>(payload).map_err(|error| {
        MarketError::new(
            ErrorKind::DecodeError,
            format!("failed to parse binance batch payload: {error}"),
        )
    })?;
    if items.len() != request_ids.len() {
        return Err(MarketError::new(
            ErrorKind::DecodeError,
            format!(
                "binance batch payload item count {} does not match request count {}",
                items.len(),
                request_ids.len()
            ),
        ));
    }
    items
        .into_iter()
        .zip(request_ids.iter().cloned())
        .map(|(item, request_id)| {
            let item_payload = serde_json::to_string(&item).map_err(|error| {
                MarketError::new(
                    ErrorKind::DecodeError,
                    format!("failed to reserialize binance batch item: {error}"),
                )
            })?;
            adapter.classify_command(operation, Some(&item_payload), request_id)
        })
        .collect()
}

#[cfg(feature = "bybit")]
fn classify_bybit_batch_payload(
    context: &LiveContext,
    operation: CommandOperation,
    payload: &str,
    identities: &[BatchIdentity],
) -> Result<Vec<CommandReceipt>> {
    let value = serde_json::from_str::<Value>(payload).map_err(|error| {
        MarketError::new(
            ErrorKind::DecodeError,
            format!("failed to parse bybit batch payload: {error}"),
        )
    })?;
    let ret_code = value
        .get("retCode")
        .and_then(value_as_i64)
        .unwrap_or_default();
    if ret_code != 0 {
        let message = value
            .get("retMsg")
            .and_then(Value::as_str)
            .unwrap_or("bybit batch request rejected");
        return Err(MarketError::new(ErrorKind::ExchangeReject, message));
    }
    let results = value
        .get("result")
        .and_then(|result| result.get("list"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let ext = value
        .get("retExtInfo")
        .and_then(|result| result.get("list"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    if !results.is_empty() && results.len() != identities.len() {
        return Err(MarketError::new(
            ErrorKind::DecodeError,
            format!(
                "bybit batch result count {} does not match request count {}",
                results.len(),
                identities.len()
            ),
        ));
    }

    let mut receipts = Vec::with_capacity(identities.len());
    for (index, identity) in identities.iter().enumerate() {
        let result = results.get(index);
        let ext_item = ext.get(index);
        let native_code = ext_item
            .and_then(|item| item.get("code"))
            .and_then(value_as_i64)
            .unwrap_or_default();
        let message = ext_item
            .and_then(|item| item.get("msg"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| {
                value
                    .get("retMsg")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| "accepted".to_owned());
        let instrument_id = result
            .and_then(|item| item.get("symbol"))
            .and_then(Value::as_str)
            .and_then(|symbol| context.adapter.as_adapter().resolve_native_symbol(symbol))
            .map(|spec| spec.instrument_id.clone())
            .or_else(|| identity.instrument_id.clone());
        let order_id = result
            .and_then(|item| item.get("orderId"))
            .and_then(Value::as_str)
            .map(OrderId::from)
            .or_else(|| identity.order_id.clone());
        let client_order_id = result
            .and_then(|item| item.get("orderLinkId"))
            .and_then(Value::as_str)
            .map(ClientOrderId::from)
            .or_else(|| identity.client_order_id.clone());
        receipts.push(CommandReceipt {
            operation,
            status: if native_code == 0 {
                CommandStatus::Accepted
            } else {
                CommandStatus::Rejected
            },
            venue: context.config.venue,
            product: context.config.product,
            instrument_id,
            order_id,
            client_order_id,
            request_id: identity.request_id.clone(),
            message: Some(message.into_boxed_str()),
            native_code: (native_code != 0).then(|| native_code.to_string().into_boxed_str()),
            retriable: false,
        });
    }
    Ok(receipts)
}

#[cfg(feature = "bybit")]
fn value_as_i64(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number.as_i64(),
        Value::String(raw) => raw.parse::<i64>().ok(),
        _ => None,
    }
}

#[cfg(feature = "binance")]
async fn execution_specs(
    context: &LiveContext,
    request: Option<&ListExecutionsRequest>,
) -> Result<Vec<InstrumentSpec>> {
    if let Some(instrument_id) = request.and_then(|request| request.instrument_id.as_ref()) {
        return Ok(vec![require_spec(context, instrument_id)?]);
    }

    let mut specs = BTreeMap::<InstrumentId, InstrumentSpec>::new();
    for instrument_id in context.shared.read(|state| {
        let mut instrument_ids = BTreeMap::new();
        for position in state.positions() {
            instrument_ids.insert(position.instrument_id.clone(), ());
        }
        for order in state.orders() {
            instrument_ids.insert(order.instrument_id.clone(), ());
        }
        instrument_ids.into_keys().collect::<Vec<_>>()
    }) {
        if let Some(spec) = context
            .adapter
            .as_adapter()
            .resolve_instrument(&instrument_id)
        {
            specs.insert(instrument_id, spec);
        }
    }
    for pending in context.runtime_state.pending_unknown_commands().await {
        if let Some(spec) = context
            .adapter
            .as_adapter()
            .resolve_instrument(&pending.instrument_id)
        {
            specs.insert(pending.instrument_id, spec);
        }
    }
    Ok(specs.into_values().collect())
}

async fn resolve_pending_unknown_commands(context: &LiveContext) -> Result<usize> {
    let pending = resolve_pending_from_local_state(
        context,
        context.runtime_state.pending_unknown_commands().await,
    )
    .await;
    if pending.is_empty() {
        return Ok(0);
    }

    let mut unresolved = Vec::new();
    for command in pending {
        let request = GetOrderRequest {
            request_id: None,
            instrument_id: command.instrument_id.clone(),
            order_id: command.order_id.clone(),
            client_order_id: command.client_order_id.clone(),
        };

        let mut resolved = false;
        if request.order_id.is_some() || request.client_order_id.is_some() {
            match get_order(context, &request).await {
                Ok(_) => resolved = true,
                Err(error) if is_missing_order_error(&error) => {}
                Err(error) => return Err(error),
            }
        }

        if !resolved {
            unresolved.push(command);
        }
    }

    unresolved = resolve_pending_from_local_state(context, unresolved).await;
    if unresolved.is_empty() {
        context
            .runtime_state
            .replace_pending_unknown_commands(Vec::new())
            .await;
        return Ok(0);
    }

    unresolved = resolve_pending_from_recent_order_history(context, unresolved).await?;
    if unresolved.is_empty() {
        context
            .runtime_state
            .replace_pending_unknown_commands(Vec::new())
            .await;
        return Ok(0);
    }
    unresolved = resolve_pending_from_recent_execution_history(context, unresolved).await?;

    let unresolved_len = unresolved.len();
    context
        .runtime_state
        .replace_pending_unknown_commands(unresolved)
        .await;
    Ok(unresolved_len)
}

async fn refresh_execution_repair_evidence(context: &LiveContext) -> Result<()> {
    let mut evidence = context.shared.read(|state| {
        let mut instrument_ids = BTreeMap::<InstrumentId, Vec<PendingUnknownCommand>>::new();
        for position in state.positions() {
            instrument_ids
                .entry(position.instrument_id.clone())
                .or_default();
        }
        for order in state.open_orders() {
            instrument_ids
                .entry(order.instrument_id.clone())
                .or_default();
        }
        for execution in state.executions() {
            instrument_ids
                .entry(execution.instrument_id.clone())
                .or_default();
        }
        instrument_ids
    });

    for (instrument_id, commands) in
        pending_by_instrument(context.runtime_state.pending_unknown_commands().await)
    {
        evidence.entry(instrument_id).or_default().extend(commands);
    }

    for (instrument_id, pending) in evidence {
        if context
            .adapter
            .as_adapter()
            .resolve_instrument(&instrument_id)
            .is_none()
        {
            continue;
        }
        let executions =
            refresh_recent_execution_history(context, &instrument_id, &pending).await?;
        context.shared.write(|state| {
            if !executions.is_empty() {
                state.merge_executions(executions.clone());
            }
            state.mark_rest_success(None);
        });
    }

    Ok(())
}

async fn resolve_pending_from_local_state(
    context: &LiveContext,
    pending: Vec<PendingUnknownCommand>,
) -> Vec<PendingUnknownCommand> {
    if pending.is_empty() {
        return pending;
    }

    let (orders, executions) = context
        .shared
        .read(|state| (state.orders(), state.executions()));
    unresolved_pending_after_state(pending, &orders, &executions)
}

async fn resolve_pending_from_recent_order_history(
    context: &LiveContext,
    pending: Vec<PendingUnknownCommand>,
) -> Result<Vec<PendingUnknownCommand>> {
    if pending.is_empty() {
        return Ok(pending);
    }

    let mut unresolved = Vec::new();
    for (instrument_id, commands) in pending_by_instrument(pending) {
        let orders = refresh_recent_order_history(context, &instrument_id, &commands).await?;
        context.shared.write(|state| {
            if !orders.is_empty() {
                state.merge_order_history(orders.clone());
            }
            state.mark_rest_success(None);
        });
        unresolved.extend(unresolved_pending_after_orders(commands, &orders));
    }

    Ok(unresolved)
}

async fn resolve_pending_from_recent_execution_history(
    context: &LiveContext,
    pending: Vec<PendingUnknownCommand>,
) -> Result<Vec<PendingUnknownCommand>> {
    if pending.is_empty() {
        return Ok(pending);
    }

    let mut unresolved = Vec::new();
    for (instrument_id, commands) in pending_by_instrument(pending) {
        let executions =
            refresh_recent_execution_history(context, &instrument_id, &commands).await?;
        context.shared.write(|state| {
            if !executions.is_empty() {
                state.merge_executions(executions.clone());
            }
            state.mark_rest_success(None);
        });
        unresolved.extend(unresolved_pending_after_executions(commands, &executions));
    }

    Ok(unresolved)
}

fn pending_by_instrument(
    pending: Vec<PendingUnknownCommand>,
) -> BTreeMap<InstrumentId, Vec<PendingUnknownCommand>> {
    let mut grouped = BTreeMap::<InstrumentId, Vec<PendingUnknownCommand>>::new();
    for command in pending {
        grouped
            .entry(command.instrument_id.clone())
            .or_default()
            .push(command);
    }
    grouped
}

fn unresolved_pending_after_orders(
    pending: Vec<PendingUnknownCommand>,
    orders: &[Order],
) -> Vec<PendingUnknownCommand> {
    pending
        .into_iter()
        .filter(|command| {
            !orders
                .iter()
                .any(|order| order_matches_pending(order, command))
        })
        .collect()
}

fn unresolved_pending_after_executions(
    pending: Vec<PendingUnknownCommand>,
    executions: &[Execution],
) -> Vec<PendingUnknownCommand> {
    pending
        .into_iter()
        .filter(|command| {
            !executions
                .iter()
                .any(|execution| execution_matches_pending(execution, command))
        })
        .collect()
}

fn unresolved_pending_after_state(
    pending: Vec<PendingUnknownCommand>,
    orders: &[Order],
    executions: &[Execution],
) -> Vec<PendingUnknownCommand> {
    let unresolved = unresolved_pending_after_orders(pending, orders);
    unresolved_pending_after_executions(unresolved, executions)
}

async fn refresh_recent_order_history(
    context: &LiveContext,
    instrument_id: &InstrumentId,
    pending: &[PendingUnknownCommand],
) -> Result<Vec<Order>> {
    let spec = require_spec(context, instrument_id)?;
    let history_window = repair_time_window(context, instrument_id, pending);
    match &context.adapter {
        #[cfg(feature = "binance")]
        AdapterHandle::Binance(adapter) => {
            let mut query = vec![
                ("symbol".to_owned(), spec.native_symbol.to_string()),
                ("limit".to_owned(), "50".to_owned()),
            ];
            if let Some((start_time, end_time)) = history_window {
                query.push(("startTime".to_owned(), start_time.to_string()));
                query.push(("endTime".to_owned(), end_time.to_string()));
            }
            let pairs = query
                .iter()
                .map(|(key, value)| (key.as_str(), value.as_str()))
                .collect::<Vec<_>>();
            let payload = binance_signed_request_text(
                context,
                Method::GET,
                "/fapi/v1/allOrders",
                &pairs,
                "binance.order_history",
            )
            .await?;
            adapter.parse_order_history_snapshot(&payload, timestamp_now_ms())
        }
        #[cfg(feature = "bybit")]
        AdapterHandle::Bybit(adapter) => {
            let mut query = vec![
                ("category".to_owned(), "linear".to_owned()),
                ("symbol".to_owned(), spec.native_symbol.to_string()),
                ("limit".to_owned(), "50".to_owned()),
            ];
            if let Some((start_time, end_time)) = history_window {
                query.push(("startTime".to_owned(), start_time.to_string()));
                query.push(("endTime".to_owned(), end_time.to_string()));
            }
            let pairs = query
                .iter()
                .map(|(key, value)| (key.as_str(), value.as_str()))
                .collect::<Vec<_>>();
            let payload =
                bybit_signed_get_text(context, "/v5/order/history", &pairs, "bybit.order_history")
                    .await?;
            adapter.parse_order_history_snapshot(&payload, timestamp_now_ms())
        }
    }
}

async fn refresh_recent_execution_history(
    context: &LiveContext,
    instrument_id: &InstrumentId,
    pending: &[PendingUnknownCommand],
) -> Result<Vec<Execution>> {
    let history_window = repair_time_window(context, instrument_id, pending);
    match &context.adapter {
        #[cfg(feature = "binance")]
        AdapterHandle::Binance(adapter) => {
            let spec = require_spec(context, instrument_id)?;
            let mut query = vec![
                ("symbol".to_owned(), spec.native_symbol.to_string()),
                ("limit".to_owned(), "100".to_owned()),
            ];
            if let Some((start_time, end_time)) = history_window {
                query.push(("startTime".to_owned(), start_time.to_string()));
                query.push(("endTime".to_owned(), end_time.to_string()));
            }
            let pairs = query
                .iter()
                .map(|(key, value)| (key.as_str(), value.as_str()))
                .collect::<Vec<_>>();
            let payload = binance_signed_request_text(
                context,
                Method::GET,
                "/fapi/v1/userTrades",
                &pairs,
                "binance.execution_history",
            )
            .await?;
            adapter.parse_executions_snapshot(&payload)
        }
        #[cfg(feature = "bybit")]
        AdapterHandle::Bybit(adapter) => {
            let spec = require_spec(context, instrument_id)?;
            let mut query = vec![
                ("category".to_owned(), "linear".to_owned()),
                ("symbol".to_owned(), spec.native_symbol.to_string()),
                ("limit".to_owned(), "100".to_owned()),
            ];
            if let Some((start_time, end_time)) = history_window {
                query.push(("startTime".to_owned(), start_time.to_string()));
                query.push(("endTime".to_owned(), end_time.to_string()));
            }
            let pairs = query
                .iter()
                .map(|(key, value)| (key.as_str(), value.as_str()))
                .collect::<Vec<_>>();
            let payload = bybit_signed_get_text(
                context,
                "/v5/execution/list",
                &pairs,
                "bybit.execution_history",
            )
            .await?;
            adapter.parse_executions_snapshot(&payload)
        }
    }
}

fn repair_time_window(
    context: &LiveContext,
    instrument_id: &InstrumentId,
    pending: &[PendingUnknownCommand],
) -> Option<(i64, i64)> {
    let now_ms = timestamp_now_ms().value();
    let (latest_order_ms, latest_execution_ms) = context.shared.read(|state| {
        (
            state
                .latest_order_update_at(instrument_id)
                .map(|timestamp| timestamp.value()),
            state
                .latest_execution_at(instrument_id)
                .map(|timestamp| timestamp.value()),
        )
    });
    let fallback_last_private_ms = context
        .shared
        .health_snapshot()
        .last_private_msg_at
        .map(|timestamp| timestamp.value());
    let oldest_pending_ms = pending
        .iter()
        .map(|command| command.recorded_at.value())
        .min();
    repair_window_start_ms(
        now_ms,
        latest_order_ms,
        latest_execution_ms,
        fallback_last_private_ms,
        oldest_pending_ms,
    )
    .map(|start_ms| (start_ms, now_ms))
}

fn repair_window_start_ms(
    now_ms: i64,
    latest_order_ms: Option<i64>,
    latest_execution_ms: Option<i64>,
    fallback_last_private_ms: Option<i64>,
    oldest_pending_ms: Option<i64>,
) -> Option<i64> {
    let anchor = [latest_order_ms, latest_execution_ms, oldest_pending_ms]
        .into_iter()
        .flatten()
        .min()
        .or(fallback_last_private_ms)?;
    let lower_bound = now_ms.saturating_sub(HISTORY_REPAIR_MAX_LOOKBACK_MS);
    Some(
        anchor
            .saturating_sub(HISTORY_REPAIR_REWIND_MS)
            .max(lower_bound)
            .min(now_ms),
    )
}

fn order_matches_pending(order: &Order, pending: &PendingUnknownCommand) -> bool {
    order.instrument_id == pending.instrument_id
        && (pending
            .order_id
            .as_ref()
            .is_some_and(|order_id| &order.order_id == order_id)
            || pending
                .client_order_id
                .as_ref()
                .is_some_and(|client_order_id| {
                    order.client_order_id.as_ref() == Some(client_order_id)
                }))
}

fn execution_matches_pending(execution: &Execution, pending: &PendingUnknownCommand) -> bool {
    execution.instrument_id == pending.instrument_id
        && (pending
            .order_id
            .as_ref()
            .is_some_and(|order_id| &execution.order_id == order_id)
            || pending
                .client_order_id
                .as_ref()
                .is_some_and(|client_order_id| {
                    execution.client_order_id.as_ref() == Some(client_order_id)
                }))
}

fn is_missing_order_error(error: &MarketError) -> bool {
    matches!(
        error.kind,
        ErrorKind::ExchangeReject | ErrorKind::Unsupported
    )
}

fn sequence_gap_error(venue: Venue, operation: &str, at: Option<i64>) -> MarketError {
    let message = match at {
        Some(at) => format!("sequence gap detected near watermark {at}"),
        None => "sequence gap detected from transport idle or reset".to_owned(),
    };
    MarketError::new(ErrorKind::TransportError, message)
        .with_venue(venue, Product::LinearUsdt)
        .with_operation(operation)
        .with_retriable(true)
}

fn topic_key(value: impl Into<Box<str>>) -> TopicKey {
    TopicKey(value.into())
}

#[cfg(all(feature = "binance", feature = "bybit"))]
fn binance_adapter(context: &LiveContext) -> Option<&BinanceLinearFuturesAdapter> {
    match &context.adapter {
        AdapterHandle::Binance(adapter) => Some(adapter),
        AdapterHandle::Bybit(_) => None,
    }
}

#[cfg(all(feature = "binance", not(feature = "bybit")))]
fn binance_adapter(context: &LiveContext) -> Option<&BinanceLinearFuturesAdapter> {
    let AdapterHandle::Binance(adapter) = &context.adapter;
    Some(adapter)
}

#[cfg(all(feature = "bybit", feature = "binance"))]
fn bybit_adapter(context: &LiveContext) -> Option<&BybitLinearFuturesAdapter> {
    match &context.adapter {
        AdapterHandle::Bybit(adapter) => Some(adapter),
        AdapterHandle::Binance(_) => None,
    }
}

#[cfg(all(feature = "bybit", not(feature = "binance")))]
fn bybit_adapter(context: &LiveContext) -> Option<&BybitLinearFuturesAdapter> {
    let AdapterHandle::Bybit(adapter) = &context.adapter;
    Some(adapter)
}

#[cfg(feature = "binance")]
fn binance_public_sequence_observations(
    context: &LiveContext,
    payload: &str,
) -> Result<Vec<SequenceObservation>> {
    let Some(adapter) = binance_adapter(context) else {
        return Ok(Vec::new());
    };
    if serde_json::from_str::<binance_native::OpenInterestSnapshot>(payload).is_ok() {
        return Ok(Vec::new());
    }
    match adapter.parse_native_public(payload)? {
        binance_native::PublicMessage::AggTrade(event) => Ok(vec![SequenceObservation {
            topic: topic_key(format!("binance.public.aggTrade.{}", event.symbol)),
            value: event.agg_trade_id,
            strict_gap: true,
            reset: false,
        }]),
        binance_native::PublicMessage::Depth(event) => Ok(vec![SequenceObservation {
            topic: topic_key(format!("binance.public.depth.{}", event.symbol)),
            value: event.final_update_id,
            strict_gap: false,
            reset: false,
        }]),
        _ => Ok(Vec::new()),
    }
}

#[cfg(feature = "binance")]
fn binance_private_sequence_observations(
    context: &LiveContext,
    payload: &str,
) -> Result<Vec<SequenceObservation>> {
    let Some(adapter) = binance_adapter(context) else {
        return Ok(Vec::new());
    };
    match adapter.parse_native_private(payload)? {
        binance_native::PrivateMessage::OrderTradeUpdate(event) => Ok(event
            .order
            .trade_id
            .map(|trade_id| SequenceObservation {
                topic: topic_key(format!("binance.private.trade.{}", event.order.symbol)),
                value: trade_id,
                strict_gap: false,
                reset: false,
            })
            .into_iter()
            .collect()),
        binance_native::PrivateMessage::TradeLite(event) => Ok(vec![SequenceObservation {
            topic: topic_key(format!("binance.private.trade.{}", event.symbol)),
            value: event.trade_id,
            strict_gap: false,
            reset: false,
        }]),
        binance_native::PrivateMessage::AlgoUpdate(_) => Ok(Vec::new()),
        binance_native::PrivateMessage::AccountUpdate(_) => Ok(Vec::new()),
    }
}

#[cfg(feature = "bybit")]
fn bybit_public_sequence_observations(
    context: &LiveContext,
    payload: &str,
) -> Result<Vec<SequenceObservation>> {
    let Some(adapter) = bybit_adapter(context) else {
        return Ok(Vec::new());
    };
    let envelope = adapter.parse_native_public(payload)?;
    if !envelope.topic.starts_with("orderbook.") {
        return Ok(Vec::new());
    }

    let data: bybit_native::OrderBookData =
        serde_json::from_value(envelope.data).map_err(|error| {
            MarketError::new(
                ErrorKind::DecodeError,
                format!("failed to parse bybit orderbook sequence payload: {error}"),
            )
            .with_venue(Venue::Bybit, Product::LinearUsdt)
        })?;
    let Some(update_id) = data.update_id else {
        return Ok(Vec::new());
    };
    Ok(vec![SequenceObservation {
        topic: topic_key(format!("bybit.public.orderbook.{}", data.symbol)),
        value: update_id,
        strict_gap: true,
        reset: envelope.message_type.as_deref() == Some("snapshot"),
    }])
}

#[cfg(feature = "bybit")]
fn bybit_private_sequence_observations(
    context: &LiveContext,
    payload: &str,
) -> Result<Vec<SequenceObservation>> {
    let Some(adapter) = bybit_adapter(context) else {
        return Ok(Vec::new());
    };
    let envelope = adapter.parse_native_private(payload)?;
    match envelope.topic.as_str() {
        "position" => {
            let positions: Vec<bybit_native::PositionData> = serde_json::from_value(envelope.data)
                .map_err(|error| {
                    MarketError::new(
                        ErrorKind::DecodeError,
                        format!("failed to parse bybit position sequence payload: {error}"),
                    )
                    .with_venue(Venue::Bybit, Product::LinearUsdt)
                })?;
            Ok(positions
                .into_iter()
                .filter_map(|position| {
                    position.seq.map(|seq| SequenceObservation {
                        topic: topic_key(format!("bybit.private.position.{}", position.symbol)),
                        value: seq,
                        strict_gap: false,
                        reset: false,
                    })
                })
                .collect())
        }
        "execution" => {
            let executions: Vec<bybit_native::ExecutionData> =
                serde_json::from_value(envelope.data).map_err(|error| {
                    MarketError::new(
                        ErrorKind::DecodeError,
                        format!("failed to parse bybit execution sequence payload: {error}"),
                    )
                    .with_venue(Venue::Bybit, Product::LinearUsdt)
                })?;
            Ok(executions
                .into_iter()
                .filter_map(|execution| {
                    execution.seq.map(|seq| SequenceObservation {
                        topic: topic_key(format!("bybit.private.execution.{}", execution.symbol)),
                        value: seq,
                        strict_gap: false,
                        reset: false,
                    })
                })
                .collect())
        }
        "order" => {
            let orders: Vec<bybit_native::OrderData> = serde_json::from_value(envelope.data)
                .map_err(|error| {
                    MarketError::new(
                        ErrorKind::DecodeError,
                        format!("failed to parse bybit order sequence payload: {error}"),
                    )
                    .with_venue(Venue::Bybit, Product::LinearUsdt)
                })?;
            Ok(orders
                .into_iter()
                .map(|order| SequenceObservation {
                    topic: topic_key(format!(
                        "bybit.private.order.{}.{}",
                        order.symbol, order.order_id
                    )),
                    value: order.updated_time,
                    strict_gap: false,
                    reset: false,
                })
                .collect())
        }
        _ => Ok(Vec::new()),
    }
}

async fn response_text(
    venue: Venue,
    response: reqwest::Response,
    operation: &str,
) -> Result<String> {
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|error| classify_reqwest_error(venue, operation, error))?;
    if status.is_success() {
        Ok(text)
    } else {
        Err(classify_http_error(venue, operation, status, &text))
    }
}

fn classify_http_error(
    venue: Venue,
    operation: &str,
    status: StatusCode,
    body: &str,
) -> MarketError {
    let mut error = match status {
        StatusCode::TOO_MANY_REQUESTS => MarketError::new(ErrorKind::RateLimited, body),
        StatusCode::UNAUTHORIZED => MarketError::new(ErrorKind::AuthError, body),
        StatusCode::FORBIDDEN => MarketError::new(ErrorKind::PermissionDenied, body),
        _ if status.is_server_error() => MarketError::new(ErrorKind::TemporaryUnavailable, body),
        _ => MarketError::new(ErrorKind::ExchangeReject, body),
    };

    error = error
        .with_venue(venue, Product::LinearUsdt)
        .with_operation(operation)
        .with_retriable(matches!(
            status,
            StatusCode::TOO_MANY_REQUESTS
                | StatusCode::BAD_GATEWAY
                | StatusCode::SERVICE_UNAVAILABLE
                | StatusCode::GATEWAY_TIMEOUT
        ));

    #[cfg(feature = "binance")]
    if venue == Venue::Binance
        && let Ok(payload) = serde_json::from_str::<binance_native::ErrorResponse>(body)
    {
        error.message = payload.message.into();
        error.context.native_code = Some(payload.code.to_string().into());
    }

    error
}

fn classify_reqwest_error(venue: Venue, operation: &str, error: reqwest::Error) -> MarketError {
    let kind = if error.is_timeout() {
        ErrorKind::Timeout
    } else {
        ErrorKind::TransportError
    };
    let mut market_error = MarketError::new(kind, error.to_string());
    market_error = market_error
        .with_venue(venue, Product::LinearUsdt)
        .with_operation(operation)
        .with_retriable(true);
    market_error
}

fn transport_error(venue: Venue, operation: &str, error: impl std::fmt::Display) -> MarketError {
    MarketError::new(ErrorKind::TransportError, error.to_string())
        .with_venue(venue, Product::LinearUsdt)
        .with_operation(operation)
        .with_retriable(true)
}

fn is_uncertain_command_error(error: &MarketError) -> bool {
    matches!(
        error.kind,
        ErrorKind::TransportError | ErrorKind::Timeout | ErrorKind::TemporaryUnavailable
    )
}

#[cfg(feature = "binance")]
fn is_binance_ack(payload: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(payload) else {
        return false;
    };
    value.get("result").is_some() && value.get("id").is_some()
}

#[cfg(feature = "bybit")]
fn is_bybit_control_message(payload: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(payload) else {
        return false;
    };
    value.get("op").is_some() && value.get("topic").is_none()
}

fn retain_public_events_for_subscription(
    events: &mut Vec<PublicLaneEvent>,
    subscription: &PublicSubscription,
) {
    events.retain(|event| match event {
        PublicLaneEvent::Ticker(_) => subscription.ticker,
        PublicLaneEvent::Trade(_) => subscription.trades,
        PublicLaneEvent::BookTop(_) => subscription.book_top,
        PublicLaneEvent::OrderBookDelta(_) => subscription.order_book,
        PublicLaneEvent::MarkPrice(_) => subscription.mark_price,
        PublicLaneEvent::FundingRate(_) => subscription.funding_rate,
        PublicLaneEvent::OpenInterest(_) => subscription.open_interest,
        PublicLaneEvent::Liquidation(_) => subscription.liquidations,
        PublicLaneEvent::Kline(_) => !subscription.kline_intervals.is_empty(),
        PublicLaneEvent::Divergence(_) => true,
    });
}

fn ws_frame_debug_enabled() -> bool {
    std::env::var_os("BAT_MARKETS_DEBUG_WS_FRAMES").is_some()
}

fn timestamp_now_ms() -> TimestampMs {
    let since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0));
    TimestampMs::new(since_epoch.as_millis() as i64)
}

async fn with_rest_retries<F, Fut>(
    context: &LiveContext,
    operation: &str,
    mut f: F,
) -> Result<String>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<String>>,
{
    let mut attempt = 0_u32;
    loop {
        match f().await {
            Ok(value) => return Ok(value),
            Err(error)
                if attempt < context.config.retry.rest_retries
                    && matches!(
                        error.kind,
                        ErrorKind::TransportError
                            | ErrorKind::Timeout
                            | ErrorKind::RateLimited
                            | ErrorKind::TemporaryUnavailable
                    ) =>
            {
                attempt += 1;
                let backoff = rest_backoff(&context.config, attempt);
                sleep(backoff).await;
            }
            Err(error) => {
                return Err(error.with_operation(operation));
            }
        }
    }
}

fn rest_backoff(config: &bat_markets_core::BatMarketsConfig, attempt: u32) -> Duration {
    let initial = config.retry.rest_initial_backoff_ms.max(1);
    let max = config.retry.rest_max_backoff_ms.max(initial);
    let delay = initial.saturating_mul(2_u64.saturating_pow(attempt.min(8)));
    Duration::from_millis(delay.min(max))
}

fn validate_create_order(context: &LiveContext, request: &CreateOrderRequest) -> Result<()> {
    let spec = require_spec(context, &request.instrument_id)?;
    if request.quantity.value() < spec.min_qty.value() {
        return Err(MarketError::new(
            ErrorKind::ConfigError,
            format!(
                "order quantity {} is below min_qty {} for {}",
                request.quantity, spec.min_qty, spec.instrument_id
            ),
        ));
    }

    if let Some(price) = request.price {
        let notional = price.value() * request.quantity.value();
        if notional < spec.min_notional.value() {
            return Err(MarketError::new(
                ErrorKind::ConfigError,
                format!(
                    "order notional {notional} is below min_notional {} for {}",
                    spec.min_notional, spec.instrument_id
                ),
            ));
        }
    }

    match request.order_type {
        bat_markets_core::OrderType::Market => {
            if request.price.is_some() {
                return Err(MarketError::new(
                    ErrorKind::ConfigError,
                    "market orders must not include a limit price",
                ));
            }
        }
        bat_markets_core::OrderType::Limit => {
            if request.price.is_none() {
                return Err(MarketError::new(
                    ErrorKind::ConfigError,
                    "limit orders require price",
                ));
            }
        }
        bat_markets_core::OrderType::StopMarket => {
            if request.trigger_price.is_none() {
                return Err(MarketError::new(
                    ErrorKind::ConfigError,
                    "stop_market orders require trigger_price",
                ));
            }
            if request.price.is_some() {
                return Err(MarketError::new(
                    ErrorKind::ConfigError,
                    "stop_market orders must not include a limit price",
                ));
            }
        }
        bat_markets_core::OrderType::StopLimit => {
            if request.price.is_none() || request.trigger_price.is_none() {
                return Err(MarketError::new(
                    ErrorKind::ConfigError,
                    "stop_limit orders require both price and trigger_price",
                ));
            }
        }
        bat_markets_core::OrderType::TakeProfitMarket => {
            if request.trigger_price.is_none() {
                return Err(MarketError::new(
                    ErrorKind::ConfigError,
                    "take_profit_market orders require trigger_price",
                ));
            }
            if request.price.is_some() {
                return Err(MarketError::new(
                    ErrorKind::ConfigError,
                    "take_profit_market orders must not include a limit price",
                ));
            }
        }
        bat_markets_core::OrderType::TakeProfitLimit => {
            if request.price.is_none() || request.trigger_price.is_none() {
                return Err(MarketError::new(
                    ErrorKind::ConfigError,
                    "take_profit_limit orders require both price and trigger_price",
                ));
            }
        }
    }

    Ok(())
}

#[cfg(feature = "binance")]
fn order_identity_query(
    native_symbol: &str,
    order_id: Option<&bat_markets_core::OrderId>,
    client_order_id: Option<&bat_markets_core::ClientOrderId>,
) -> Result<Vec<(String, String)>> {
    let mut query = vec![("symbol".to_owned(), native_symbol.to_owned())];
    append_order_identity(&mut query, order_id, client_order_id)?;
    Ok(query)
}

#[cfg(feature = "binance")]
fn binance_algo_identity_query(
    order_id: Option<&bat_markets_core::OrderId>,
    client_order_id: Option<&bat_markets_core::ClientOrderId>,
) -> Result<Vec<(String, String)>> {
    let mut query = Vec::new();
    append_binance_algo_identity(&mut query, order_id, client_order_id)?;
    Ok(query)
}

fn append_order_identity(
    query: &mut Vec<(String, String)>,
    order_id: Option<&bat_markets_core::OrderId>,
    client_order_id: Option<&bat_markets_core::ClientOrderId>,
) -> Result<()> {
    match (order_id, client_order_id) {
        (Some(order_id), _) => query.push(("orderId".to_owned(), order_id.to_string())),
        (None, Some(client_order_id)) => {
            query.push(("origClientOrderId".to_owned(), client_order_id.to_string()));
        }
        (None, None) => {
            return Err(MarketError::new(
                ErrorKind::ConfigError,
                "either order_id or client_order_id is required",
            ));
        }
    }
    Ok(())
}

#[cfg(feature = "binance")]
fn append_binance_algo_identity(
    query: &mut Vec<(String, String)>,
    order_id: Option<&bat_markets_core::OrderId>,
    client_order_id: Option<&bat_markets_core::ClientOrderId>,
) -> Result<()> {
    match (order_id, client_order_id) {
        (Some(order_id), _) => {
            let algo_id = binance_algo_order_id_value(order_id).ok_or_else(|| {
                MarketError::new(
                    ErrorKind::ConfigError,
                    format!("expected binance algo order id, got '{order_id}'"),
                )
            })?;
            query.push(("algoId".to_owned(), algo_id.to_owned()));
        }
        (None, Some(client_order_id)) => {
            query.push(("clientAlgoId".to_owned(), client_order_id.to_string()));
        }
        (None, None) => {
            return Err(MarketError::new(
                ErrorKind::ConfigError,
                "either order_id or client_order_id is required",
            ));
        }
    }
    Ok(())
}

#[cfg(feature = "binance")]
fn is_binance_algo_order_id(order_id: Option<&bat_markets_core::OrderId>) -> bool {
    order_id.and_then(binance_algo_order_id_value).is_some()
}

#[cfg(feature = "binance")]
fn binance_algo_order_id_value(order_id: &bat_markets_core::OrderId) -> Option<&str> {
    order_id.as_str().strip_prefix("binance-algo:")
}

#[cfg(feature = "binance")]
fn append_binance_order_identity_json(
    query: &mut serde_json::Map<String, Value>,
    order_id: Option<&bat_markets_core::OrderId>,
    client_order_id: Option<&bat_markets_core::ClientOrderId>,
) -> Result<()> {
    match (order_id, client_order_id) {
        (Some(order_id), _) => {
            query.insert("orderId".to_owned(), Value::String(order_id.to_string()));
        }
        (None, Some(client_order_id)) => {
            query.insert(
                "origClientOrderId".to_owned(),
                Value::String(client_order_id.to_string()),
            );
        }
        (None, None) => {
            return Err(MarketError::new(
                ErrorKind::ConfigError,
                "either order_id or client_order_id is required",
            ));
        }
    }
    Ok(())
}

#[cfg(feature = "bybit")]
fn append_bybit_order_identity(
    query: &mut Vec<(String, String)>,
    order_id: Option<&bat_markets_core::OrderId>,
    client_order_id: Option<&bat_markets_core::ClientOrderId>,
) -> Result<()> {
    match (order_id, client_order_id) {
        (Some(order_id), _) => query.push(("orderId".to_owned(), order_id.to_string())),
        (None, Some(client_order_id)) => {
            query.push(("orderLinkId".to_owned(), client_order_id.to_string()));
        }
        (None, None) => {
            return Err(MarketError::new(
                ErrorKind::ConfigError,
                "either order_id or client_order_id is required",
            ));
        }
    }
    Ok(())
}

#[cfg(feature = "bybit")]
fn append_bybit_order_identity_json(
    query: &mut serde_json::Map<String, Value>,
    order_id: Option<&bat_markets_core::OrderId>,
    client_order_id: Option<&bat_markets_core::ClientOrderId>,
) -> Result<()> {
    match (order_id, client_order_id) {
        (Some(order_id), _) => {
            query.insert("orderId".to_owned(), Value::String(order_id.to_string()));
        }
        (None, Some(client_order_id)) => {
            query.insert(
                "orderLinkId".to_owned(),
                Value::String(client_order_id.to_string()),
            );
        }
        (None, None) => {
            return Err(MarketError::new(
                ErrorKind::ConfigError,
                "either order_id or client_order_id is required",
            ));
        }
    }
    Ok(())
}

#[cfg(feature = "bybit")]
fn body_to_object(pairs: Vec<(String, String)>) -> serde_json::Map<String, Value> {
    pairs
        .into_iter()
        .map(|(key, value)| (key, Value::String(value)))
        .collect()
}

fn format_price(price: Price) -> String {
    price.value().normalize().to_string()
}

fn format_quantity(quantity: Quantity) -> String {
    quantity.value().normalize().to_string()
}

#[cfg(feature = "binance")]
fn binance_trigger_type(trigger_type: bat_markets_core::TriggerType) -> &'static str {
    match trigger_type {
        bat_markets_core::TriggerType::LastPrice => "CONTRACT_PRICE",
        bat_markets_core::TriggerType::MarkPrice => "MARK_PRICE",
        bat_markets_core::TriggerType::IndexPrice => "MARK_PRICE",
    }
}

#[cfg(feature = "binance")]
fn binance_side(side: bat_markets_core::Side) -> &'static str {
    match side {
        bat_markets_core::Side::Buy => "BUY",
        bat_markets_core::Side::Sell => "SELL",
    }
}

#[cfg(feature = "bybit")]
fn bybit_side(side: bat_markets_core::Side) -> &'static str {
    match side {
        bat_markets_core::Side::Buy => "Buy",
        bat_markets_core::Side::Sell => "Sell",
    }
}

#[cfg(feature = "binance")]
fn binance_order_type(order_type: bat_markets_core::OrderType) -> &'static str {
    match order_type {
        bat_markets_core::OrderType::Market => "MARKET",
        bat_markets_core::OrderType::Limit => "LIMIT",
        bat_markets_core::OrderType::StopMarket => "STOP_MARKET",
        bat_markets_core::OrderType::StopLimit => "STOP",
        bat_markets_core::OrderType::TakeProfitMarket => "TAKE_PROFIT_MARKET",
        bat_markets_core::OrderType::TakeProfitLimit => "TAKE_PROFIT",
    }
}

#[cfg(feature = "bybit")]
fn bybit_order_type(order_type: bat_markets_core::OrderType) -> &'static str {
    match order_type {
        bat_markets_core::OrderType::Market => "Market",
        bat_markets_core::OrderType::Limit => "Limit",
        bat_markets_core::OrderType::StopMarket => "StopMarket",
        bat_markets_core::OrderType::StopLimit => "Stop",
        bat_markets_core::OrderType::TakeProfitMarket => "TakeProfitMarket",
        bat_markets_core::OrderType::TakeProfitLimit => "TakeProfit",
    }
}

#[cfg(feature = "bybit")]
fn bybit_trigger_type(trigger_type: bat_markets_core::TriggerType) -> &'static str {
    match trigger_type {
        bat_markets_core::TriggerType::LastPrice => "LastPrice",
        bat_markets_core::TriggerType::MarkPrice => "MarkPrice",
        bat_markets_core::TriggerType::IndexPrice => "IndexPrice",
    }
}

#[cfg(feature = "binance")]
fn binance_time_in_force(
    time_in_force: bat_markets_core::TimeInForce,
    post_only: bool,
) -> &'static str {
    if post_only {
        return "GTX";
    }
    match time_in_force {
        bat_markets_core::TimeInForce::Gtc => "GTC",
        bat_markets_core::TimeInForce::Ioc => "IOC",
        bat_markets_core::TimeInForce::Fok => "FOK",
        bat_markets_core::TimeInForce::PostOnly => "GTX",
    }
}

#[cfg(feature = "bybit")]
fn bybit_time_in_force(
    time_in_force: bat_markets_core::TimeInForce,
    post_only: bool,
) -> &'static str {
    if post_only {
        return "PostOnly";
    }
    match time_in_force {
        bat_markets_core::TimeInForce::Gtc => "GTC",
        bat_markets_core::TimeInForce::Ioc => "IOC",
        bat_markets_core::TimeInForce::Fok => "FOK",
        bat_markets_core::TimeInForce::PostOnly => "PostOnly",
    }
}

#[cfg(feature = "bybit")]
fn parse_decimal(raw: &str, venue: Venue) -> Result<Decimal> {
    raw.parse::<Decimal>().map_err(|error| {
        MarketError::new(
            ErrorKind::DecodeError,
            format!("invalid decimal '{raw}': {error}"),
        )
        .with_venue(venue, Product::LinearUsdt)
    })
}

#[cfg(test)]
mod tests {
    use super::{
        BatchIdentity, CommandTransport, ExecutionRepairScope, PendingUnknownCommand,
        SequenceTracker, bybit_private_sequence_observations, bybit_public_sequence_observations,
        classify_binance_batch_payload, classify_bybit_batch_payload, execution_repair_scope,
        needs_recent_history_repair, pending_by_instrument, repair_window_start_ms,
        retain_public_events_for_subscription, unresolved_pending_after_executions,
        unresolved_pending_after_orders, unresolved_pending_after_state, validate_create_orders,
    };
    use crate::client::{AdapterHandle, BatMarketsBuilder};
    use crate::stream::PublicSubscription;
    use bat_markets_core::{
        ClientOrderId, ClosePositionRequest, CommandLifecycleEvent, CommandOperation,
        CommandReceipt, CommandStatus, CreateOrderRequest, CreateOrdersRequest, DegradedReason,
        Execution, HealthReport, InstrumentId, MarginMode, Order, OrderId, Position,
        PositionDirection, PositionId, PositionMode, Price, Product, Quantity, ReconcileTrigger,
        RequestId, Side, TimestampMs, Venue,
    };
    use bat_markets_core::{OrderStatus, OrderType, PublicLaneEvent};
    use rust_decimal::Decimal;
    use tokio::time::{Duration, timeout};

    const BINANCE_COMMAND_BATCH_CREATE_OK: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/binance/command_batch_create_ok.json"
    ));
    const BYBIT_PUBLIC_ORDERBOOK: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/bybit/public_orderbook.json"
    ));
    const BYBIT_PUBLIC_ORDERBOOK_GAP: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/bybit/public_orderbook_gap.json"
    ));
    const BYBIT_PRIVATE_POSITION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/bybit/private_position.json"
    ));
    const BYBIT_PRIVATE_EXECUTION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/bybit/private_execution.json"
    ));

    #[cfg(feature = "bybit")]
    #[test]
    fn bybit_public_orderbook_gap_is_detected_from_watermark() {
        let client = BatMarketsBuilder::default()
            .venue(Venue::Bybit)
            .product(Product::LinearUsdt)
            .build()
            .expect("fixture bybit client should build");
        let context = client.live_context();
        let mut tracker = SequenceTracker::default();

        for observation in bybit_public_sequence_observations(&context, BYBIT_PUBLIC_ORDERBOOK)
            .expect("snapshot observation should parse")
        {
            tracker
                .observe(observation)
                .expect("snapshot watermark should initialize");
        }
        let mut observations =
            bybit_public_sequence_observations(&context, BYBIT_PUBLIC_ORDERBOOK_GAP)
                .expect("gap observation should parse");
        let gap = tracker
            .observe(
                observations
                    .pop()
                    .expect("gap fixture should emit one observation"),
            )
            .expect_err("gap fixture should trip sequence tracking");
        assert_eq!(gap, 11);
    }

    #[cfg(feature = "bybit")]
    #[test]
    fn bybit_private_sequence_payloads_surface_seq_values() {
        let client = BatMarketsBuilder::default()
            .venue(Venue::Bybit)
            .product(Product::LinearUsdt)
            .build()
            .expect("fixture bybit client should build");
        let context = client.live_context();

        let position = bybit_private_sequence_observations(&context, BYBIT_PRIVATE_POSITION)
            .expect("position observations should parse");
        let execution = bybit_private_sequence_observations(&context, BYBIT_PRIVATE_EXECUTION)
            .expect("execution observations should parse");

        assert_eq!(position.len(), 1);
        assert_eq!(position[0].value, 300);
        assert_eq!(execution.len(), 1);
        assert_eq!(execution[0].value, 301);
    }

    #[cfg(feature = "bybit")]
    #[test]
    fn public_subscription_filter_drops_unrequested_orderbook_delta_events() {
        let client = BatMarketsBuilder::default()
            .venue(Venue::Bybit)
            .product(Product::LinearUsdt)
            .build()
            .expect("fixture bybit client should build");
        let instrument_id = InstrumentId::from("BTC/USDT:USDT");
        let mut events = client
            .stream()
            .public()
            .ingest_json(BYBIT_PUBLIC_ORDERBOOK)
            .expect("fixture orderbook should parse");

        retain_public_events_for_subscription(
            &mut events,
            &PublicSubscription {
                instrument_ids: vec![instrument_id.clone()],
                ticker: false,
                trades: false,
                book_top: true,
                order_book: false,
                mark_price: false,
                funding_rate: false,
                open_interest: false,
                liquidations: false,
                kline_intervals: Vec::new(),
            },
        );

        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            PublicLaneEvent::BookTop(event) if event.instrument_id == instrument_id
        ));
    }

    #[test]
    fn pending_unknown_commands_group_by_instrument() {
        let btc = InstrumentId::from("BTC/USDT:USDT");
        let eth = InstrumentId::from("ETH/USDT:USDT");
        let grouped = pending_by_instrument(vec![
            pending_unknown(&btc, 1, "btc-1"),
            pending_unknown(&btc, 2, "btc-2"),
            pending_unknown(&eth, 3, "eth-1"),
        ]);

        assert_eq!(grouped.len(), 2);
        assert_eq!(grouped[&btc].len(), 2);
        assert_eq!(grouped[&eth].len(), 1);
    }

    #[test]
    fn recent_history_batch_can_resolve_multiple_pending_commands() {
        let instrument_id = InstrumentId::from("BTC/USDT:USDT");
        let first = pending_unknown(&instrument_id, 101, "first");
        let second = pending_unknown(&instrument_id, 102, "second");

        let after_orders = unresolved_pending_after_orders(
            vec![first.clone(), second.clone()],
            &[sample_order(101, "first", &instrument_id)],
        );
        assert_eq!(after_orders, vec![second.clone()]);

        let after_executions = unresolved_pending_after_executions(
            after_orders,
            &[sample_execution(102, &instrument_id)],
        );
        assert!(after_executions.is_empty());
    }

    #[test]
    fn batch_create_validation_requires_client_order_id_and_unique_identity() {
        let client = BatMarketsBuilder::default()
            .venue(Venue::Binance)
            .product(Product::LinearUsdt)
            .build()
            .expect("fixture binance client should build");
        let context = client.live_context();
        let instrument_id = InstrumentId::from("BTC/USDT:USDT");
        let quantity = Quantity::new(Decimal::new(1, 3));

        let missing_client_ids = CreateOrdersRequest {
            request_id: None,
            orders: vec![
                CreateOrderRequest {
                    request_id: None,
                    instrument_id: instrument_id.clone(),
                    client_order_id: None,
                    side: Side::Buy,
                    order_type: OrderType::Limit,
                    time_in_force: None,
                    quantity,
                    price: Some(Price::new(Decimal::new(70000, 0))),
                    trigger_price: None,
                    trigger_type: None,
                    reduce_only: false,
                    post_only: false,
                },
                CreateOrderRequest {
                    request_id: None,
                    instrument_id: instrument_id.clone(),
                    client_order_id: None,
                    side: Side::Sell,
                    order_type: OrderType::Limit,
                    time_in_force: None,
                    quantity,
                    price: Some(Price::new(Decimal::new(70010, 0))),
                    trigger_price: None,
                    trigger_type: None,
                    reduce_only: false,
                    post_only: false,
                },
            ],
        };
        let error = validate_create_orders(&context, &missing_client_ids)
            .expect_err("batch create without client ids should be rejected");
        assert!(error.to_string().contains("client_order_id"));

        let duplicate_client_ids = CreateOrdersRequest {
            request_id: None,
            orders: vec![
                CreateOrderRequest {
                    request_id: None,
                    instrument_id: instrument_id.clone(),
                    client_order_id: Some(ClientOrderId::from("dup-id")),
                    side: Side::Buy,
                    order_type: OrderType::Limit,
                    time_in_force: None,
                    quantity,
                    price: Some(Price::new(Decimal::new(70000, 0))),
                    trigger_price: None,
                    trigger_type: None,
                    reduce_only: false,
                    post_only: false,
                },
                CreateOrderRequest {
                    request_id: None,
                    instrument_id,
                    client_order_id: Some(ClientOrderId::from("dup-id")),
                    side: Side::Sell,
                    order_type: OrderType::Limit,
                    time_in_force: None,
                    quantity,
                    price: Some(Price::new(Decimal::new(70010, 0))),
                    trigger_price: None,
                    trigger_type: None,
                    reduce_only: false,
                    post_only: false,
                },
            ],
        };
        let error = validate_create_orders(&context, &duplicate_client_ids)
            .expect_err("batch create with duplicate client ids should be rejected");
        assert!(error.to_string().contains("duplicate client_order_id"));
    }

    #[cfg(feature = "binance")]
    #[test]
    fn binance_batch_classification_preserves_per_item_request_ids() {
        let client = BatMarketsBuilder::default()
            .venue(Venue::Binance)
            .product(Product::LinearUsdt)
            .build()
            .expect("fixture binance client should build");
        let context = client.live_context();
        let AdapterHandle::Binance(adapter) = &context.adapter else {
            panic!("binance adapter should be selected");
        };

        let receipts = classify_binance_batch_payload(
            adapter,
            CommandOperation::CreateOrder,
            BINANCE_COMMAND_BATCH_CREATE_OK,
            &[
                Some(RequestId::from("req-binance-1")),
                Some(RequestId::from("req-binance-2")),
            ],
        )
        .expect("binance batch payload should classify");

        assert_eq!(receipts.len(), 2);
        assert_eq!(
            receipts[0].instrument_id,
            Some(InstrumentId::from("BTC/USDT:USDT"))
        );
        assert_eq!(
            receipts[1].instrument_id,
            Some(InstrumentId::from("ETH/USDT:USDT"))
        );
        assert_eq!(
            receipts[0].request_id,
            Some(RequestId::from("req-binance-1"))
        );
        assert_eq!(
            receipts[1].request_id,
            Some(RequestId::from("req-binance-2"))
        );
        assert_eq!(
            receipts[1].client_order_id,
            Some(ClientOrderId::from("cli-binance-2"))
        );
    }

    #[cfg(feature = "bybit")]
    #[test]
    fn bybit_batch_classification_uses_identity_fallback_for_sparse_items() {
        let client = BatMarketsBuilder::default()
            .venue(Venue::Bybit)
            .product(Product::LinearUsdt)
            .build()
            .expect("fixture bybit client should build");
        let context = client.live_context();
        let payload = r#"{
            "retCode": 0,
            "retMsg": "OK",
            "result": {
                "list": [
                    {"symbol":"BTCUSDT","orderId":"bybit-order-1","orderLinkId":"cli-bybit-1"},
                    {"symbol":"ETHUSDT","orderId":"bybit-order-2"}
                ]
            },
            "retExtInfo": {
                "list": [
                    {"code":0,"msg":"OK"},
                    {"code":0,"msg":"OK"}
                ]
            }
        }"#;

        let receipts = classify_bybit_batch_payload(
            &context,
            CommandOperation::CreateOrder,
            payload,
            &[
                BatchIdentity {
                    instrument_id: Some(InstrumentId::from("BTC/USDT:USDT")),
                    order_id: None,
                    client_order_id: Some(ClientOrderId::from("cli-bybit-1")),
                    request_id: Some(RequestId::from("req-bybit-1")),
                },
                BatchIdentity {
                    instrument_id: Some(InstrumentId::from("ETH/USDT:USDT")),
                    order_id: None,
                    client_order_id: Some(ClientOrderId::from("cli-bybit-2")),
                    request_id: Some(RequestId::from("req-bybit-2")),
                },
            ],
        )
        .expect("bybit batch payload should classify");

        assert_eq!(receipts.len(), 2);
        assert_eq!(receipts[0].status, CommandStatus::Accepted);
        assert_eq!(receipts[1].status, CommandStatus::Accepted);
        assert_eq!(receipts[0].request_id, Some(RequestId::from("req-bybit-1")));
        assert_eq!(receipts[1].request_id, Some(RequestId::from("req-bybit-2")));
        assert_eq!(
            receipts[0].client_order_id,
            Some(ClientOrderId::from("cli-bybit-1"))
        );
        assert_eq!(
            receipts[1].client_order_id,
            Some(ClientOrderId::from("cli-bybit-2"))
        );
        assert_eq!(
            receipts[1].instrument_id,
            Some(InstrumentId::from("ETH/USDT:USDT"))
        );
    }

    #[test]
    fn local_state_resolution_prefers_existing_orders_and_executions() {
        let instrument_id = InstrumentId::from("BTC/USDT:USDT");
        let first = pending_unknown(&instrument_id, 101, "first");
        let second = pending_unknown(&instrument_id, 102, "second");
        let third = pending_unknown(&instrument_id, 103, "third");

        let unresolved = unresolved_pending_after_state(
            vec![first, second, third.clone()],
            &[sample_order(101, "first", &instrument_id)],
            &[sample_execution(102, &instrument_id)],
        );

        assert_eq!(unresolved, vec![third]);
    }

    #[test]
    fn periodic_reconcile_stays_snapshot_only_for_stale_snapshot_without_uncertainty() {
        let mut health = HealthReport::default();
        health.mark_snapshot_age(6_000, 5_000);

        assert!(!needs_recent_history_repair(&health, 0));
    }

    #[test]
    fn periodic_reconcile_uses_recent_history_when_command_is_uncertain() {
        let mut health = HealthReport::default();
        health.mark_command_uncertain();

        assert!(needs_recent_history_repair(&health, 0));
    }

    #[test]
    fn periodic_reconcile_uses_recent_history_when_private_gap_or_pending_unknown_exists() {
        let health = HealthReport {
            degraded_reason: Some(DegradedReason::PrivateStreamGap),
            ..HealthReport::default()
        };
        assert!(needs_recent_history_repair(&health, 0));

        let clean = HealthReport::default();
        assert!(needs_recent_history_repair(&clean, 1));
    }

    #[test]
    fn execution_repair_scope_prefers_local_evidence_only_when_gap_signals_exist() {
        let clean = HealthReport::default();
        assert_eq!(
            execution_repair_scope(ReconcileTrigger::UnknownExecution, &clean),
            ExecutionRepairScope::PendingUnknownOnly
        );
        assert_eq!(
            execution_repair_scope(ReconcileTrigger::Periodic, &clean),
            ExecutionRepairScope::PendingUnknownOnly
        );
        assert_eq!(
            execution_repair_scope(ReconcileTrigger::SequenceGap, &clean),
            ExecutionRepairScope::LocalEvidence
        );

        let gap = HealthReport {
            degraded_reason: Some(DegradedReason::PrivateStreamGap),
            ..HealthReport::default()
        };
        assert_eq!(
            execution_repair_scope(ReconcileTrigger::Periodic, &gap),
            ExecutionRepairScope::LocalEvidence
        );
    }

    #[test]
    fn repair_window_prefers_earliest_recent_anchor_and_applies_rewind() {
        let now_ms = 10_000_000;
        let start_ms = repair_window_start_ms(
            now_ms,
            Some(9_900_000),
            Some(9_950_000),
            Some(9_980_000),
            Some(9_970_000),
        )
        .expect("recent anchors should produce a bounded start window");

        assert_eq!(start_ms, 9_840_000);
    }

    #[test]
    fn repair_window_falls_back_to_last_private_message_and_caps_lookback() {
        let now_ms = 10 * 24 * 60 * 60 * 1_000;
        let start_ms = repair_window_start_ms(now_ms, None, None, Some(1_000), None)
            .expect("last private message should still anchor the window");

        assert_eq!(start_ms, now_ms - super::HISTORY_REPAIR_MAX_LOOKBACK_MS);
    }

    #[test]
    fn close_position_cache_actionable_requires_non_zero_and_enough_size() {
        let request = ClosePositionRequest {
            request_id: None,
            instrument_id: InstrumentId::from("BTC/USDT:USDT"),
            client_order_id: None,
            quantity: Some(Quantity::new(Decimal::new(2, 0))),
            price: None,
            time_in_force: None,
            post_only: false,
        };

        assert!(super::close_position_cache_is_actionable(
            &test_position("3", PositionDirection::Long),
            &request
        ));
        assert!(!super::close_position_cache_is_actionable(
            &test_position("1", PositionDirection::Long),
            &request
        ));
        assert!(!super::close_position_cache_is_actionable(
            &test_position("0", PositionDirection::Flat),
            &request
        ));
    }

    #[tokio::test]
    async fn unknown_execution_emits_lifecycle_ack_and_recovery_scheduled() {
        let client = BatMarketsBuilder::default()
            .venue(Venue::Binance)
            .product(Product::LinearUsdt)
            .build()
            .expect("fixture binance client should build");
        let context = client.live_context();
        let mut receiver = client.entry().subscribe();
        let receipt = CommandReceipt {
            operation: CommandOperation::CreateOrder,
            status: CommandStatus::UnknownExecution,
            venue: Venue::Binance,
            product: Product::LinearUsdt,
            instrument_id: Some(InstrumentId::from("BTC/USDT:USDT")),
            order_id: None,
            client_order_id: Some(ClientOrderId::from("cli-unknown")),
            request_id: None,
            message: Some("command outcome requires reconcile".into()),
            native_code: None,
            retriable: true,
        };

        super::apply_command_receipt(&context, receipt, CommandTransport::Rest).await;

        let first = timeout(Duration::from_secs(1), receiver.recv())
            .await
            .expect("ack event should arrive")
            .expect("ack event should parse");
        assert!(matches!(
            first,
            bat_markets_core::CommandLaneEvent::Lifecycle(CommandLifecycleEvent::Ack(_))
        ));

        let second = timeout(Duration::from_secs(1), receiver.recv())
            .await
            .expect("receipt event should arrive")
            .expect("receipt event should parse");
        assert!(matches!(
            second,
            bat_markets_core::CommandLaneEvent::Receipt(CommandReceipt {
                status: CommandStatus::UnknownExecution,
                ..
            })
        ));

        let third = timeout(Duration::from_secs(1), receiver.recv())
            .await
            .expect("recovery-scheduled event should arrive")
            .expect("recovery-scheduled event should parse");
        assert!(matches!(
            third,
            bat_markets_core::CommandLaneEvent::Lifecycle(
                CommandLifecycleEvent::RecoveryScheduled(_)
            )
        ));
    }

    #[test]
    fn bybit_order_identity_uses_order_link_id_for_client_order_id() {
        let mut query = Vec::new();
        super::append_bybit_order_identity(
            &mut query,
            None,
            Some(&ClientOrderId::from("cli-bybit")),
        )
        .expect("client_order_id should be accepted");

        assert_eq!(
            query,
            vec![("orderLinkId".to_owned(), "cli-bybit".to_owned())]
        );
    }

    fn test_position(size: &str, direction: PositionDirection) -> Position {
        Position {
            position_id: PositionId::from("test-position"),
            instrument_id: InstrumentId::from("BTC/USDT:USDT"),
            direction,
            size: Quantity::new(size.parse::<Decimal>().expect("valid decimal")),
            entry_price: None,
            mark_price: None,
            unrealized_pnl: None,
            leverage: None,
            margin_mode: MarginMode::Cross,
            position_mode: PositionMode::OneWay,
            updated_at: TimestampMs::new(1),
        }
    }

    fn pending_unknown(
        instrument_id: &InstrumentId,
        order_id: i64,
        client_order_id: &str,
    ) -> PendingUnknownCommand {
        PendingUnknownCommand {
            operation: CommandOperation::CreateOrder,
            instrument_id: instrument_id.clone(),
            order_id: Some(OrderId::from(order_id.to_string())),
            client_order_id: Some(ClientOrderId::from(client_order_id)),
            recorded_at: TimestampMs::new(1),
        }
    }

    fn sample_order(order_id: i64, client_order_id: &str, instrument_id: &InstrumentId) -> Order {
        Order {
            order_id: OrderId::from(order_id.to_string()),
            client_order_id: Some(ClientOrderId::from(client_order_id)),
            instrument_id: instrument_id.clone(),
            side: Side::Buy,
            order_type: OrderType::Limit,
            time_in_force: None,
            status: OrderStatus::Canceled,
            price: Some(Price::new(Decimal::new(60000, 0))),
            quantity: Quantity::new(Decimal::new(2, 3)),
            filled_quantity: Quantity::new(Decimal::ZERO),
            average_fill_price: None,
            reduce_only: false,
            post_only: true,
            created_at: TimestampMs::new(1),
            updated_at: TimestampMs::new(2),
            venue_status: None,
        }
    }

    fn sample_execution(order_id: i64, instrument_id: &InstrumentId) -> Execution {
        Execution {
            execution_id: bat_markets_core::TradeId::from(format!("fill-{order_id}")),
            order_id: OrderId::from(order_id.to_string()),
            client_order_id: None,
            instrument_id: instrument_id.clone(),
            side: Side::Buy,
            quantity: Quantity::new(Decimal::new(2, 3)),
            price: Price::new(Decimal::new(60000, 0)),
            fee: None,
            fee_asset: None,
            liquidity: None,
            executed_at: TimestampMs::new(3),
        }
    }
}
