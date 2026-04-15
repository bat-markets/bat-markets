use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

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

use bat_markets_core::{
    CancelOrderRequest, ClientOrderId, CommandOperation, CommandReceipt, CommandStatus,
    CreateOrderRequest, ErrorKind, Execution, GetOrderRequest, InstrumentId, InstrumentSpec,
    ListExecutionsRequest, ListOpenOrdersRequest, MarginMode, MarketError, OpenInterest, Order,
    OrderId, Price, PrivateLaneEvent, Product, PublicLaneEvent, Quantity, ReconcileOutcome,
    ReconcileReport, ReconcileTrigger, Result, SequenceNumber, SetLeverageRequest,
    SetMarginModeRequest, TimestampMs, Venue, VenueAdapter,
};

#[cfg(feature = "binance")]
use bat_markets_binance::native as binance_native;
#[cfg(feature = "bybit")]
use bat_markets_bybit::native as bybit_native;

use crate::{
    client::{AdapterHandle, LiveContext},
    stream::{LiveStreamHandle, PublicSubscription},
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

#[derive(Debug)]
pub(crate) struct LiveRuntimeState {
    pending_unknown_commands: Mutex<Vec<PendingUnknownCommand>>,
    #[cfg(feature = "bybit")]
    bybit_account_context: Mutex<Option<bat_markets_bybit::BybitAccountContext>>,
}

impl Default for LiveRuntimeState {
    fn default() -> Self {
        Self {
            pending_unknown_commands: Mutex::new(Vec::new()),
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
    let specs = match &context.adapter {
        #[cfg(feature = "binance")]
        AdapterHandle::Binance(adapter) => {
            let payload =
                public_get_with_retry(context, "/fapi/v1/exchangeInfo", &[], "binance.metadata")
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

pub(crate) async fn refresh_account(
    context: &LiveContext,
) -> Result<Option<bat_markets_core::AccountSummary>> {
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

    Ok(snapshot.summary)
}

pub(crate) async fn refresh_positions(
    context: &LiveContext,
) -> Result<Vec<bat_markets_core::Position>> {
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

pub(crate) async fn refresh_open_orders(
    context: &LiveContext,
    request: Option<&ListOpenOrdersRequest>,
) -> Result<Vec<Order>> {
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
            adapter.parse_open_orders_snapshot(&payload, timestamp_now_ms())?
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
            let payload =
                bybit_signed_get_text(context, "/v5/order/realtime", &pairs, "bybit.open_orders")
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

pub(crate) async fn refresh_executions(
    context: &LiveContext,
    request: Option<&ListExecutionsRequest>,
) -> Result<Vec<Execution>> {
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
            let payload =
                bybit_signed_get_text(context, "/v5/execution/list", &pairs, "bybit.executions")
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

pub(crate) async fn get_order(context: &LiveContext, request: &GetOrderRequest) -> Result<Order> {
    let spec = require_spec(context, &request.instrument_id)?;
    let order = match &context.adapter {
        #[cfg(feature = "binance")]
        AdapterHandle::Binance(adapter) => {
            let query = order_identity_query(
                &spec.native_symbol,
                request.order_id.as_ref(),
                request.client_order_id.as_ref(),
            )?;
            let pairs = query
                .iter()
                .map(|(key, value)| (key.as_str(), value.as_str()))
                .collect::<Vec<_>>();
            let payload = binance_signed_request_text(
                context,
                Method::GET,
                "/fapi/v1/order",
                &pairs,
                "binance.get_order",
            )
            .await?;
            adapter.parse_order_snapshot(&payload, timestamp_now_ms())?
        }
        #[cfg(feature = "bybit")]
        AdapterHandle::Bybit(adapter) => {
            let mut query = vec![
                ("category".to_owned(), "linear".to_owned()),
                ("symbol".to_owned(), spec.native_symbol.to_string()),
            ];
            append_order_identity(
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

    let order_for_state = order.clone();
    context.shared.write(|state| {
        state.apply_private_event(PrivateLaneEvent::Order(order_for_state));
        state.mark_rest_success(None);
    });
    Ok(order)
}

pub(crate) async fn create_order(
    context: &LiveContext,
    request: &CreateOrderRequest,
) -> Result<CommandReceipt> {
    validate_create_order(context, request)?;
    context.command_limiter.acquire().await;
    let receipt = match &context.adapter {
        #[cfg(feature = "binance")]
        AdapterHandle::Binance(adapter) => {
            let spec = require_spec(context, &request.instrument_id)?;
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
            if let Some(time_in_force) = request.time_in_force {
                owned.push((
                    "timeInForce".to_owned(),
                    binance_time_in_force(time_in_force, request.post_only).to_owned(),
                ));
            }
            if request.reduce_only {
                owned.push(("reduceOnly".to_owned(), "true".to_owned()));
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
                Ok(payload) => adapter.classify_command(
                    CommandOperation::CreateOrder,
                    Some(&payload),
                    request.request_id.clone(),
                )?,
                Err(error) if is_uncertain_command_error(&error) => adapter.classify_command(
                    CommandOperation::CreateOrder,
                    None,
                    request.request_id.clone(),
                )?,
                Err(error) => return Err(error),
            }
        }
        #[cfg(feature = "bybit")]
        AdapterHandle::Bybit(adapter) => {
            let spec = require_spec(context, &request.instrument_id)?;
            let body = json!({
                "category": "linear",
                "symbol": spec.native_symbol,
                "side": bybit_side(request.side),
                "orderType": bybit_order_type(request.order_type),
                "qty": format_quantity(request.quantity),
                "price": request.price.map(format_price),
                "timeInForce": request.time_in_force.map(|value| bybit_time_in_force(value, request.post_only)),
                "orderLinkId": request.client_order_id.as_ref().map(ToString::to_string),
                "reduceOnly": request.reduce_only,
            });
            let body = serde_json::to_string(&body).map_err(|error| {
                MarketError::new(
                    ErrorKind::ConfigError,
                    format!("failed to serialize bybit create order body: {error}"),
                )
            })?;
            match bybit_signed_post_text(context, "/v5/order/create", &body, "bybit.create_order")
                .await
            {
                Ok(payload) => adapter.classify_command(
                    CommandOperation::CreateOrder,
                    Some(&payload),
                    request.request_id.clone(),
                )?,
                Err(error) if is_uncertain_command_error(&error) => adapter.classify_command(
                    CommandOperation::CreateOrder,
                    None,
                    request.request_id.clone(),
                )?,
                Err(error) => return Err(error),
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
    apply_command_receipt(context, receipt.clone()).await;
    Ok(receipt)
}

pub(crate) async fn cancel_order(
    context: &LiveContext,
    request: &CancelOrderRequest,
) -> Result<CommandReceipt> {
    let spec = require_spec(context, &request.instrument_id)?;
    context.command_limiter.acquire().await;
    let receipt = match &context.adapter {
        #[cfg(feature = "binance")]
        AdapterHandle::Binance(adapter) => {
            let query = order_identity_query(
                &spec.native_symbol,
                request.order_id.as_ref(),
                request.client_order_id.as_ref(),
            )?;
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
                Ok(payload) => adapter.classify_command(
                    CommandOperation::CancelOrder,
                    Some(&payload),
                    request.request_id.clone(),
                )?,
                Err(error) if is_uncertain_command_error(&error) => adapter.classify_command(
                    CommandOperation::CancelOrder,
                    None,
                    request.request_id.clone(),
                )?,
                Err(error) => return Err(error),
            }
        }
        #[cfg(feature = "bybit")]
        AdapterHandle::Bybit(adapter) => {
            let mut body = vec![
                ("category".to_owned(), "linear".to_owned()),
                ("symbol".to_owned(), spec.native_symbol.to_string()),
            ];
            append_order_identity(
                &mut body,
                request.order_id.as_ref(),
                request.client_order_id.as_ref(),
            )?;
            let body = serde_json::to_string(&body_to_object(body)).map_err(|error| {
                MarketError::new(
                    ErrorKind::ConfigError,
                    format!("failed to serialize bybit cancel order body: {error}"),
                )
            })?;
            match bybit_signed_post_text(context, "/v5/order/cancel", &body, "bybit.cancel_order")
                .await
            {
                Ok(payload) => adapter.classify_command(
                    CommandOperation::CancelOrder,
                    Some(&payload),
                    request.request_id.clone(),
                )?,
                Err(error) if is_uncertain_command_error(&error) => adapter.classify_command(
                    CommandOperation::CancelOrder,
                    None,
                    request.request_id.clone(),
                )?,
                Err(error) => return Err(error),
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
    apply_command_receipt(context, receipt.clone()).await;
    Ok(receipt)
}

pub(crate) async fn set_leverage(
    context: &LiveContext,
    request: &SetLeverageRequest,
) -> Result<CommandReceipt> {
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
    apply_command_receipt(context, receipt.clone()).await;
    Ok(receipt)
}

pub(crate) async fn set_margin_mode(
    context: &LiveContext,
    request: &SetMarginModeRequest,
) -> Result<CommandReceipt> {
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
    apply_command_receipt(context, receipt.clone()).await;
    Ok(receipt)
}

pub(crate) async fn refresh_open_interest(
    context: &LiveContext,
    instrument_id: &InstrumentId,
) -> Result<OpenInterest> {
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
            let response = serde_json::from_str::<bybit_native::MarketTickersResponse>(&payload)
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

    let event = open_interest.clone();
    context.shared.write(|state| {
        let _ = state.apply_public_event(bat_markets_core::PublicLaneEvent::OpenInterest(event));
        state.mark_rest_success(None);
    });
    Ok(open_interest)
}

pub(crate) async fn reconcile_private(
    context: &LiveContext,
    trigger: ReconcileTrigger,
) -> Result<ReconcileReport> {
    let repaired_at = timestamp_now_ms();
    let result = async {
        let _ = refresh_account(context).await?;
        let _ = refresh_positions(context).await?;
        let _ = refresh_open_orders(context, None).await?;
        let _ = refresh_executions(context, None).await?;
        let unresolved = resolve_pending_unknown_commands(context).await?;
        Ok::<usize, MarketError>(unresolved)
    }
    .await;

    match result {
        Ok(0) => {
            let report = ReconcileReport {
                trigger,
                outcome: ReconcileOutcome::Synchronized,
                repaired_at,
                note: Some("snapshot reconcile completed".into()),
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

async fn apply_command_receipt(context: &LiveContext, receipt: CommandReceipt) {
    let needs_reconcile = matches!(receipt.status, CommandStatus::UnknownExecution);
    context
        .shared
        .write(|state| state.apply_command_receipt(&receipt));
    if needs_reconcile {
        let _ = reconcile_private(context, ReconcileTrigger::UnknownExecution).await;
    }
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
        if subscription.funding_rate {
            streams.push(format!("{symbol}@markPrice@1s"));
        }
        if let Some(interval_value) = &subscription.kline_interval {
            streams.push(format!("{symbol}@kline_{interval_value}"));
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
                    context.shared.write(|state| {
                        let _ = state.apply_public_event(PublicLaneEvent::Divergence(
                            bat_markets_core::DivergenceEvent::SequenceGap { at: None },
                        ));
                    });
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
                            context.shared.write(|state| {
                                let _ = state.apply_public_event(PublicLaneEvent::Divergence(
                                    bat_markets_core::DivergenceEvent::SequenceGap {
                                        at: Some(SequenceNumber::new(at.max(0) as u64)),
                                    },
                                ));
                            });
                            return Err(sequence_gap_error(context.config.venue, "binance.public_ws.sequence", Some(at)));
                        }
                    }
                    let events = context.adapter.as_adapter().parse_public(&payload)?;
                    context.shared.write(|state| {
                        for event in events {
                            let _ = state.apply_public_event(event);
                        }
                    });
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
                    context.shared.write(|state| {
                        state.apply_private_event(PrivateLaneEvent::Divergence(
                            bat_markets_core::DivergenceEvent::SequenceGap { at: None },
                        ));
                    });
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
                    for observation in binance_private_sequence_observations(context, &payload)? {
                        if let Err(at) = sequence.observe(observation) {
                            context.shared.write(|state| {
                                state.apply_private_event(PrivateLaneEvent::Divergence(
                                    bat_markets_core::DivergenceEvent::SequenceGap {
                                        at: Some(SequenceNumber::new(at.max(0) as u64)),
                                    },
                                ));
                            });
                            return Err(sequence_gap_error(context.config.venue, "binance.private_ws.sequence", Some(at)));
                        }
                    }
                    let events = context.adapter.as_adapter().parse_private(&payload)?;
                    context.shared.write(|state| {
                        for event in events {
                            state.apply_private_event(event);
                        }
                    });
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
        if subscription.ticker || subscription.funding_rate || subscription.open_interest {
            args.push(format!("tickers.{}", spec.native_symbol));
        }
        if subscription.trades {
            args.push(format!("publicTrade.{}", spec.native_symbol));
        }
        if subscription.book_top {
            args.push(format!("orderbook.1.{}", spec.native_symbol));
        }
        if let Some(interval_value) = &subscription.kline_interval {
            args.push(format!("kline.{interval_value}.{}", spec.native_symbol));
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
                    context.shared.write(|state| {
                        let _ = state.apply_public_event(PublicLaneEvent::Divergence(
                            bat_markets_core::DivergenceEvent::SequenceGap { at: None },
                        ));
                    });
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
                if let Message::Text(payload) = message {
                    if is_bybit_control_message(&payload) {
                        continue;
                    }
                    for observation in bybit_public_sequence_observations(context, &payload)? {
                        if let Err(at) = sequence.observe(observation) {
                            context.shared.write(|state| {
                                let _ = state.apply_public_event(PublicLaneEvent::Divergence(
                                    bat_markets_core::DivergenceEvent::SequenceGap {
                                        at: Some(SequenceNumber::new(at.max(0) as u64)),
                                    },
                                ));
                            });
                            return Err(sequence_gap_error(context.config.venue, "bybit.public_ws.sequence", Some(at)));
                        }
                    }
                    let events = context.adapter.as_adapter().parse_public(&payload)?;
                    context.shared.write(|state| {
                        for event in events {
                            let _ = state.apply_public_event(event);
                        }
                    });
                    last_frame_at = Instant::now();
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
                    context.shared.write(|state| {
                        state.apply_private_event(PrivateLaneEvent::Divergence(
                            bat_markets_core::DivergenceEvent::SequenceGap { at: None },
                        ));
                    });
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
                if let Message::Text(payload) = message {
                    if is_bybit_control_message(&payload) {
                        continue;
                    }
                    for observation in bybit_private_sequence_observations(context, &payload)? {
                        if let Err(at) = sequence.observe(observation) {
                            context.shared.write(|state| {
                                state.apply_private_event(PrivateLaneEvent::Divergence(
                                    bat_markets_core::DivergenceEvent::SequenceGap {
                                        at: Some(SequenceNumber::new(at.max(0) as u64)),
                                    },
                                ));
                            });
                            return Err(sequence_gap_error(context.config.venue, "bybit.private_ws.sequence", Some(at)));
                        }
                    }
                    let events = context.adapter.as_adapter().parse_private(&payload)?;
                    context.shared.write(|state| {
                        for event in events {
                            state.apply_private_event(event);
                        }
                    });
                    last_frame_at = Instant::now();
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
    let adapter = match &context.adapter {
        AdapterHandle::Bybit(adapter) => adapter,
        #[cfg(feature = "binance")]
        AdapterHandle::Binance(_) => {
            return Err(MarketError::new(
                ErrorKind::Unsupported,
                "bybit account context requested for non-bybit adapter",
            ));
        }
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
    let pending = context.runtime_state.pending_unknown_commands().await;
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
            resolved = refresh_order_history_for_pending(context, &command).await?;
        }
        if !resolved {
            resolved = refresh_execution_history_for_pending(context, &command).await?;
        }

        if !resolved {
            unresolved.push(command);
        }
    }

    let unresolved_len = unresolved.len();
    context
        .runtime_state
        .replace_pending_unknown_commands(unresolved)
        .await;
    Ok(unresolved_len)
}

async fn refresh_order_history_for_pending(
    context: &LiveContext,
    pending: &PendingUnknownCommand,
) -> Result<bool> {
    let spec = require_spec(context, &pending.instrument_id)?;
    let orders = match &context.adapter {
        #[cfg(feature = "binance")]
        AdapterHandle::Binance(adapter) => {
            let mut query = vec![("symbol".to_owned(), spec.native_symbol.to_string())];
            if let Some(order_id) = &pending.order_id {
                query.push(("orderId".to_owned(), order_id.to_string()));
            } else {
                return Ok(false);
            }
            query.push(("limit".to_owned(), "10".to_owned()));
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
            adapter.parse_order_history_snapshot(&payload, timestamp_now_ms())?
        }
        #[cfg(feature = "bybit")]
        AdapterHandle::Bybit(adapter) => {
            let mut query = vec![
                ("category".to_owned(), "linear".to_owned()),
                ("symbol".to_owned(), spec.native_symbol.to_string()),
                ("limit".to_owned(), "20".to_owned()),
            ];
            if let Some(order_id) = &pending.order_id {
                query.push(("orderId".to_owned(), order_id.to_string()));
            }
            if let Some(client_order_id) = &pending.client_order_id {
                query.push(("orderLinkId".to_owned(), client_order_id.to_string()));
            }
            let pairs = query
                .iter()
                .map(|(key, value)| (key.as_str(), value.as_str()))
                .collect::<Vec<_>>();
            let payload =
                bybit_signed_get_text(context, "/v5/order/history", &pairs, "bybit.order_history")
                    .await?;
            adapter.parse_order_history_snapshot(&payload, timestamp_now_ms())?
        }
    };

    let found = orders
        .iter()
        .any(|order| order_matches_pending(order, pending));
    if found {
        context.shared.write(|state| {
            state.merge_order_history(orders);
            state.mark_rest_success(None);
        });
    }
    Ok(found)
}

async fn refresh_execution_history_for_pending(
    context: &LiveContext,
    pending: &PendingUnknownCommand,
) -> Result<bool> {
    let executions = match &context.adapter {
        #[cfg(feature = "binance")]
        AdapterHandle::Binance(adapter) => {
            let spec = require_spec(context, &pending.instrument_id)?;
            let mut query = vec![
                ("symbol".to_owned(), spec.native_symbol.to_string()),
                ("limit".to_owned(), "50".to_owned()),
            ];
            if let Some(order_id) = &pending.order_id {
                query.push(("orderId".to_owned(), order_id.to_string()));
            } else {
                return Ok(false);
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
            adapter.parse_executions_snapshot(&payload)?
        }
        #[cfg(feature = "bybit")]
        AdapterHandle::Bybit(adapter) => {
            let mut query = vec![("category".to_owned(), "linear".to_owned())];
            if let Some(order_id) = &pending.order_id {
                query.push(("orderId".to_owned(), order_id.to_string()));
            }
            if let Some(client_order_id) = &pending.client_order_id {
                query.push(("orderLinkId".to_owned(), client_order_id.to_string()));
            }
            if pending.order_id.is_none() && pending.client_order_id.is_none() {
                return Ok(false);
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
            adapter.parse_executions_snapshot(&payload)?
        }
    };

    let found = executions
        .iter()
        .any(|execution| execution_matches_pending(execution, pending));
    if found {
        context.shared.write(|state| {
            state.merge_executions(executions);
            state.mark_rest_success(None);
        });
    }
    Ok(found)
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

#[cfg(feature = "binance")]
fn binance_public_sequence_observations(
    context: &LiveContext,
    payload: &str,
) -> Result<Vec<SequenceObservation>> {
    let adapter = match &context.adapter {
        AdapterHandle::Binance(adapter) => adapter,
        #[cfg(feature = "bybit")]
        AdapterHandle::Bybit(_) => return Ok(Vec::new()),
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
        _ => Ok(Vec::new()),
    }
}

#[cfg(feature = "binance")]
fn binance_private_sequence_observations(
    context: &LiveContext,
    payload: &str,
) -> Result<Vec<SequenceObservation>> {
    let adapter = match &context.adapter {
        AdapterHandle::Binance(adapter) => adapter,
        #[cfg(feature = "bybit")]
        AdapterHandle::Bybit(_) => return Ok(Vec::new()),
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
        binance_native::PrivateMessage::AccountUpdate(_) => Ok(Vec::new()),
    }
}

#[cfg(feature = "bybit")]
fn bybit_public_sequence_observations(
    context: &LiveContext,
    payload: &str,
) -> Result<Vec<SequenceObservation>> {
    let adapter = match &context.adapter {
        AdapterHandle::Bybit(adapter) => adapter,
        #[cfg(feature = "binance")]
        AdapterHandle::Binance(_) => return Ok(Vec::new()),
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
    let adapter = match &context.adapter {
        AdapterHandle::Bybit(adapter) => adapter,
        #[cfg(feature = "binance")]
        AdapterHandle::Binance(_) => return Ok(Vec::new()),
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
    }
}

#[cfg(feature = "bybit")]
fn bybit_order_type(order_type: bat_markets_core::OrderType) -> &'static str {
    match order_type {
        bat_markets_core::OrderType::Market => "Market",
        bat_markets_core::OrderType::Limit => "Limit",
        bat_markets_core::OrderType::StopMarket => "StopMarket",
        bat_markets_core::OrderType::StopLimit => "Stop",
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
        SequenceTracker, bybit_private_sequence_observations, bybit_public_sequence_observations,
    };
    use crate::client::BatMarketsBuilder;
    use bat_markets_core::{Product, Venue};

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
}
