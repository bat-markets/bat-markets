use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    time::Duration,
};

use rust_decimal::{Decimal, RoundingStrategy};
use tokio::time::{Instant, sleep, timeout};

use bat_markets::{
    BatMarkets, OrdersWatch, RuntimeDiagnosticsSnapshot,
    errors::Result,
    stream::{AccountWatch, WatchInstrumentsRequest},
    types::{
        AccountSummary, Balance, CancelOrderRequest, CancelOrdersRequest, ClientOrderId,
        ClosePositionRequest, CommandStatus, CommandTransport, CreateOrderRequest,
        CreateOrdersRequest, Execution, GetOrderRequest, InstrumentId, InstrumentSpec,
        InstrumentStatus, ListExecutionsRequest, ListOpenOrdersRequest, MarginMode, Order,
        OrderStatus, OrderTarget, OrderType, Position, PositionDirection, PositionId, PositionMode,
        Price, Quantity, Side, TimeInForce, TimestampMs, TriggerType,
    },
};
use bat_markets_core::{ErrorKind, MarketError};

const PREFERRED_LIVE_SYMBOLS: &[&str] = &[
    "XRP/USDT:USDT",
    "DOGE/USDT:USDT",
    "ADA/USDT:USDT",
    "TRX/USDT:USDT",
    "SOL/USDT:USDT",
    "ETH/USDT:USDT",
    "BTC/USDT:USDT",
];
const ORDER_EVENT_TIMEOUT: Duration = Duration::from_secs(25);
const EXECUTION_EVENT_TIMEOUT: Duration = Duration::from_secs(25);
const POSITION_EVENT_TIMEOUT: Duration = Duration::from_secs(25);
const BALANCE_EVENT_TIMEOUT: Duration = Duration::from_secs(25);
const ACCOUNT_EVENT_TIMEOUT: Duration = Duration::from_secs(25);
const OPEN_ORDER_TIMEOUT: Duration = Duration::from_secs(15);
const PRIVATE_STREAM_WARMUP: Duration = Duration::from_secs(2);
const OPTIONAL_PRIVATE_STREAM_OBSERVE_TIMEOUT: Duration = Duration::from_secs(5);
const MAKER_TICKS_AWAY: i64 = 8;
const EXTENDED_STRESS_DEFAULT_ROUNDS: usize = 3;
const EXTENDED_STRESS_DEFAULT_BURST_SIZE: usize = 5;
const PROTECTIVE_TRIGGER_BPS: i64 = 35;
const PROTECTIVE_MIN_TICKS: i64 = 40;
const BURST_DISTANCE_BPS: i64 = 40;
const BURST_MIN_TICKS: i64 = 50;

#[derive(Clone, Debug)]
pub struct LiveTradeCyclePlan {
    pub instrument_id: InstrumentId,
    pub market_quantity: Quantity,
    pub maker_buy_price: Price,
    pub maker_sell_price: Price,
    pub reference_bid: Price,
    pub reference_ask: Price,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LiveTradeEventCounts {
    pub orders: usize,
    pub executions: usize,
    pub positions: usize,
    pub balances: usize,
    pub account: usize,
}

#[derive(Clone, Debug)]
pub struct LiveTradeCycleReport {
    pub instrument_id: InstrumentId,
    pub market_quantity: Quantity,
    pub maker_buy_price: Price,
    pub maker_sell_price: Price,
    pub open_order: Order,
    pub open_execution: Execution,
    pub close_order: Order,
    pub close_execution: Execution,
    pub maker_buy_order: Order,
    pub maker_buy_cancel: Order,
    pub maker_sell_order: Order,
    pub maker_sell_cancel: Order,
    pub open_transport: CommandTransport,
    pub close_transport: CommandTransport,
    pub maker_buy_transport: CommandTransport,
    pub maker_buy_cancel_transport: CommandTransport,
    pub maker_sell_transport: CommandTransport,
    pub maker_sell_cancel_transport: CommandTransport,
    pub balance_update: Balance,
    pub account_update: AccountSummary,
    pub final_position: Position,
    pub final_open_orders: Vec<Order>,
    pub refreshed_executions: Vec<Execution>,
    pub event_counts: LiveTradeEventCounts,
    pub diagnostics: RuntimeDiagnosticsSnapshot,
}

#[derive(Clone, Debug, Default)]
pub struct LatencySummary {
    pub samples: usize,
    pub min_ns: u64,
    pub avg_ns: u64,
    pub p95_ns: u64,
    pub max_ns: u64,
}

#[derive(Clone, Debug)]
pub struct TimedOrderUpdate {
    pub order: Order,
    pub latency_ns: u64,
}

#[derive(Clone, Debug)]
pub struct BurstRoundReport {
    pub round: usize,
    pub create_ack_ns: u64,
    pub create_stream: LatencySummary,
    pub create_stream_latencies_ns: Vec<u64>,
    pub cancel_ack_ns: u64,
    pub cancel_stream: LatencySummary,
    pub cancel_stream_latencies_ns: Vec<u64>,
    pub create_transports: Vec<CommandTransport>,
    pub cancel_transports: Vec<CommandTransport>,
}

#[derive(Clone, Debug)]
pub struct ProtectiveOrderReport {
    pub stop_order: Order,
    pub take_profit_order: Order,
    pub stop_transport: CommandTransport,
    pub take_profit_transport: CommandTransport,
    pub stop_cancel: Order,
    pub take_profit_cancel: Order,
    pub stop_cancel_transport: CommandTransport,
    pub take_profit_cancel_transport: CommandTransport,
}

#[derive(Clone, Debug)]
pub struct StreamLatencyObservation {
    pub latency_ns: u64,
    pub streamed: bool,
}

#[derive(Clone, Debug)]
pub struct BinanceExtendedStressReport {
    pub instrument_id: InstrumentId,
    pub market_quantity: Quantity,
    pub burst_quantity: Quantity,
    pub open_order: Order,
    pub open_execution: Execution,
    pub open_execution_latency: StreamLatencyObservation,
    pub open_transport: CommandTransport,
    pub protective: ProtectiveOrderReport,
    pub close_order: Order,
    pub close_execution: Execution,
    pub close_execution_latency: StreamLatencyObservation,
    pub close_transport: CommandTransport,
    pub position_open_latency: StreamLatencyObservation,
    pub position_flat_latency: StreamLatencyObservation,
    pub balance_latency: Option<StreamLatencyObservation>,
    pub account_latency: Option<StreamLatencyObservation>,
    pub balance_update: Balance,
    pub account_update: AccountSummary,
    pub burst_rounds: Vec<BurstRoundReport>,
    pub hft_create_ack: LatencySummary,
    pub hft_create_stream: LatencySummary,
    pub hft_cancel_ack: LatencySummary,
    pub hft_cancel_stream: LatencySummary,
    pub final_position: Position,
    pub final_open_orders: Vec<Order>,
    pub refreshed_executions: Vec<Execution>,
    pub event_counts: LiveTradeEventCounts,
    pub diagnostics: RuntimeDiagnosticsSnapshot,
}

pub fn binance_mainnet_trade_cycle_enabled() -> bool {
    env::var_os("BAT_MARKETS_ENABLE_BINANCE_MAINNET_TRADE_CYCLE").is_some()
}

pub fn binance_mainnet_extended_stress_enabled() -> bool {
    env::var_os("BAT_MARKETS_ENABLE_BINANCE_MAINNET_EXTENDED_STRESS").is_some()
}

pub async fn prepare_binance_trade_cycle(
    client: &BatMarkets,
) -> Result<Option<LiveTradeCyclePlan>> {
    let Some(account) = client.fetch_balance().await?.summary else {
        return Ok(None);
    };
    let positions = client.fetch_positions().await?;
    let open_orders = client.fetch_open_orders(None).await?;
    let occupied = occupied_instruments(&positions, &open_orders);
    let preferred_symbol = env::var("BAT_MARKETS_LIVE_TRADE_SYMBOL")
        .ok()
        .map(|value| InstrumentId::from(value.trim()));
    let max_notional =
        env_decimal("BAT_MARKETS_LIVE_TRADE_MAX_NOTIONAL")?.unwrap_or_else(default_max_notional);
    let available_balance = account.total_available_balance.value();
    if available_balance <= Decimal::ZERO {
        return Ok(None);
    }
    let max_budget = (available_balance * Decimal::new(35, 2)).min(max_notional);
    if max_budget <= Decimal::ZERO {
        return Ok(None);
    }

    if let Some(instrument_id) = preferred_symbol {
        let spec = client.market().require_instrument(&instrument_id)?;
        return build_live_trade_plan(client, &spec, max_budget, &occupied).await;
    }

    let mut specs = client.markets();
    specs.sort_by(|left, right| {
        preferred_live_rank(left)
            .cmp(&preferred_live_rank(right))
            .then_with(|| left.min_notional.value().cmp(&right.min_notional.value()))
            .then_with(|| left.native_symbol.cmp(&right.native_symbol))
    });

    for spec in specs {
        if let Some(plan) = build_live_trade_plan(client, &spec, max_budget, &occupied).await? {
            return Ok(Some(plan));
        }
    }

    Ok(None)
}

pub async fn run_binance_trade_cycle(client: &BatMarkets) -> Result<Option<LiveTradeCycleReport>> {
    let Some(plan) = prepare_binance_trade_cycle(client).await? else {
        return Ok(None);
    };

    let mut public = client
        .stream()
        .public()
        .watch_book_top(WatchInstrumentsRequest::for_instrument(
            plan.instrument_id.clone(),
        ))
        .await?;
    let mut orders = client.watch_orders().await?;
    let mut executions = client.watch_my_trades().await?;
    let mut positions = client.watch_positions().await?;
    let mut balances = client.watch_balance().await?;
    let mut account = client.stream().private().watch_account().await?;

    let _ = public.recv().await?;
    sleep(PRIVATE_STREAM_WARMUP).await;

    let _ = client.fetch_balance().await?;
    let _ = client.fetch_positions().await?;
    let _ = client
        .fetch_open_orders(Some(&ListOpenOrdersRequest {
            instrument_id: Some(plan.instrument_id.clone()),
        }))
        .await?;
    let _ = client
        .fetch_my_trades(Some(&ListExecutionsRequest {
            instrument_id: Some(plan.instrument_id.clone()),
            limit: Some(25),
        }))
        .await?;

    let mut event_counts = LiveTradeEventCounts::default();
    let open_client_order_id = live_client_order_id("bx-open");
    let close_client_order_id = live_client_order_id("bx-close");
    let maker_buy_client_order_id = live_client_order_id("bx-mbuy");
    let maker_sell_client_order_id = live_client_order_id("bx-msell");

    validate_live_order(
        client,
        &CreateOrderRequest {
            request_id: None,
            instrument_id: plan.instrument_id.clone(),
            client_order_id: Some(open_client_order_id.clone()),
            side: Side::Buy,
            order_type: OrderType::Market,
            time_in_force: None,
            quantity: plan.market_quantity,
            price: None,
            trigger_price: None,
            trigger_type: None,
            reduce_only: false,
            post_only: false,
        },
    )
    .await?;

    validate_live_order(
        client,
        &CreateOrderRequest {
            request_id: None,
            instrument_id: plan.instrument_id.clone(),
            client_order_id: Some(maker_buy_client_order_id.clone()),
            side: Side::Buy,
            order_type: OrderType::Limit,
            time_in_force: Some(TimeInForce::Gtc),
            quantity: plan.market_quantity,
            price: Some(plan.maker_buy_price),
            trigger_price: None,
            trigger_type: None,
            reduce_only: false,
            post_only: true,
        },
    )
    .await?;

    validate_live_order(
        client,
        &CreateOrderRequest {
            request_id: None,
            instrument_id: plan.instrument_id.clone(),
            client_order_id: Some(maker_sell_client_order_id.clone()),
            side: Side::Sell,
            order_type: OrderType::Limit,
            time_in_force: Some(TimeInForce::Gtc),
            quantity: plan.market_quantity,
            price: Some(plan.maker_sell_price),
            trigger_price: None,
            trigger_type: None,
            reduce_only: false,
            post_only: true,
        },
    )
    .await?;

    let mut open_handle = client
        .create_order(&CreateOrderRequest {
            request_id: None,
            instrument_id: plan.instrument_id.clone(),
            client_order_id: Some(open_client_order_id.clone()),
            side: Side::Buy,
            order_type: OrderType::Market,
            time_in_force: None,
            quantity: plan.market_quantity,
            price: None,
            trigger_price: None,
            trigger_type: None,
            reduce_only: false,
            post_only: false,
        })
        .await?;
    let open_transport = open_handle.ack().transport;
    ensure_command_not_rejected(&mut open_handle, "open market order").await?;

    let (open_execution, open_execution_latency) = observe_or_refresh_execution_latency(
        client,
        &mut executions,
        &plan.instrument_id,
        &open_client_order_id,
        open_handle.ack().receipt.order_id.as_ref(),
    )
    .await?;
    if open_execution_latency.streamed {
        event_counts.executions += 1;
    }

    let (_, open_position_streamed) =
        observe_or_refresh_position_state(client, &mut positions, &plan.instrument_id, false)
            .await?;
    if open_position_streamed {
        event_counts.positions += 1;
    }

    let (balance_update, balance_streamed) =
        observe_or_refresh_balance(client, &mut balances).await?;
    if balance_streamed {
        event_counts.balances += 1;
    }
    let (account_update, account_streamed) =
        observe_or_refresh_account(client, &mut account).await?;
    if account_streamed {
        event_counts.account += 1;
    }

    let open_order = await_rest_order_state(
        client,
        &plan.instrument_id,
        open_handle.ack().receipt.order_id.clone(),
        Some(open_client_order_id.clone()),
        &[OrderStatus::Filled, OrderStatus::PartiallyFilled],
    )
    .await?;

    let mut close_handle = client
        .close_position(&ClosePositionRequest {
            request_id: None,
            instrument_id: plan.instrument_id.clone(),
            quantity: None,
            client_order_id: Some(close_client_order_id.clone()),
            price: None,
            time_in_force: None,
            post_only: false,
        })
        .await?;
    let close_transport = close_handle.ack().transport;
    ensure_command_not_rejected(&mut close_handle, "close position").await?;

    let (close_execution, close_execution_latency) = observe_or_refresh_execution_latency(
        client,
        &mut executions,
        &plan.instrument_id,
        &close_client_order_id,
        close_handle.ack().receipt.order_id.as_ref(),
    )
    .await?;
    if close_execution_latency.streamed {
        event_counts.executions += 1;
    }

    let (final_position, final_position_streamed) =
        observe_or_refresh_position_state(client, &mut positions, &plan.instrument_id, true)
            .await?;
    if final_position_streamed {
        event_counts.positions += 1;
    }

    let close_order = await_rest_order_state(
        client,
        &plan.instrument_id,
        close_handle.ack().receipt.order_id.clone(),
        Some(close_client_order_id.clone()),
        &[OrderStatus::Filled, OrderStatus::PartiallyFilled],
    )
    .await?;

    let mut maker_buy_handle = client
        .create_order(&CreateOrderRequest {
            request_id: None,
            instrument_id: plan.instrument_id.clone(),
            client_order_id: Some(maker_buy_client_order_id.clone()),
            side: Side::Buy,
            order_type: OrderType::Limit,
            time_in_force: Some(TimeInForce::Gtc),
            quantity: plan.market_quantity,
            price: Some(plan.maker_buy_price),
            trigger_price: None,
            trigger_type: None,
            reduce_only: false,
            post_only: true,
        })
        .await?;
    let maker_buy_transport = maker_buy_handle.ack().transport;
    ensure_command_not_rejected(&mut maker_buy_handle, "maker buy order").await?;

    let maker_buy_order = await_order_update(
        &mut orders,
        &plan.instrument_id,
        &maker_buy_client_order_id,
        &[OrderStatus::New, OrderStatus::PartiallyFilled],
    )
    .await?;
    event_counts.orders += 1;

    await_open_order_presence(
        client,
        &plan.instrument_id,
        &maker_buy_client_order_id,
        true,
    )
    .await?;

    let mut maker_buy_cancel_handle = client
        .cancel_order(&CancelOrderRequest {
            request_id: None,
            instrument_id: plan.instrument_id.clone(),
            order_id: maker_buy_handle.ack().receipt.order_id.clone(),
            client_order_id: Some(maker_buy_client_order_id.clone()),
        })
        .await?;
    let maker_buy_cancel_transport = maker_buy_cancel_handle.ack().transport;
    ensure_command_not_rejected(&mut maker_buy_cancel_handle, "cancel maker buy").await?;

    let maker_buy_cancel = await_order_update(
        &mut orders,
        &plan.instrument_id,
        &maker_buy_client_order_id,
        &[
            OrderStatus::Canceled,
            OrderStatus::Expired,
            OrderStatus::PendingCancel,
        ],
    )
    .await?;
    event_counts.orders += 1;

    await_open_order_presence(
        client,
        &plan.instrument_id,
        &maker_buy_client_order_id,
        false,
    )
    .await?;

    let mut maker_sell_handle = client
        .create_order(&CreateOrderRequest {
            request_id: None,
            instrument_id: plan.instrument_id.clone(),
            client_order_id: Some(maker_sell_client_order_id.clone()),
            side: Side::Sell,
            order_type: OrderType::Limit,
            time_in_force: Some(TimeInForce::Gtc),
            quantity: plan.market_quantity,
            price: Some(plan.maker_sell_price),
            trigger_price: None,
            trigger_type: None,
            reduce_only: false,
            post_only: true,
        })
        .await?;
    let maker_sell_transport = maker_sell_handle.ack().transport;
    ensure_command_not_rejected(&mut maker_sell_handle, "maker sell order").await?;

    let maker_sell_order = await_order_update(
        &mut orders,
        &plan.instrument_id,
        &maker_sell_client_order_id,
        &[OrderStatus::New, OrderStatus::PartiallyFilled],
    )
    .await?;
    event_counts.orders += 1;

    await_open_order_presence(
        client,
        &plan.instrument_id,
        &maker_sell_client_order_id,
        true,
    )
    .await?;

    let mut maker_sell_cancel_handle = client
        .cancel_order(&CancelOrderRequest {
            request_id: None,
            instrument_id: plan.instrument_id.clone(),
            order_id: maker_sell_handle.ack().receipt.order_id.clone(),
            client_order_id: Some(maker_sell_client_order_id.clone()),
        })
        .await?;
    let maker_sell_cancel_transport = maker_sell_cancel_handle.ack().transport;
    ensure_command_not_rejected(&mut maker_sell_cancel_handle, "cancel maker sell").await?;

    let maker_sell_cancel = await_order_update(
        &mut orders,
        &plan.instrument_id,
        &maker_sell_client_order_id,
        &[
            OrderStatus::Canceled,
            OrderStatus::Expired,
            OrderStatus::PendingCancel,
        ],
    )
    .await?;
    event_counts.orders += 1;

    await_open_order_presence(
        client,
        &plan.instrument_id,
        &maker_sell_client_order_id,
        false,
    )
    .await?;

    let final_open_orders = client
        .fetch_open_orders(Some(&ListOpenOrdersRequest {
            instrument_id: Some(plan.instrument_id.clone()),
        }))
        .await?;
    let refreshed_executions = client
        .fetch_my_trades(Some(&ListExecutionsRequest {
            instrument_id: Some(plan.instrument_id.clone()),
            limit: Some(50),
        }))
        .await?;
    let final_positions = client.fetch_positions().await?;
    let final_position = final_positions
        .into_iter()
        .find(|position| position.instrument_id == plan.instrument_id)
        .unwrap_or(final_position);
    let diagnostics = client.advanced().diagnostics();

    public.shutdown().await?;
    orders.shutdown().await?;
    executions.shutdown().await?;
    positions.shutdown().await?;
    balances.shutdown().await?;
    account.shutdown().await?;

    Ok(Some(LiveTradeCycleReport {
        instrument_id: plan.instrument_id,
        market_quantity: plan.market_quantity,
        maker_buy_price: plan.maker_buy_price,
        maker_sell_price: plan.maker_sell_price,
        open_order,
        open_execution,
        close_order,
        close_execution,
        maker_buy_order,
        maker_buy_cancel,
        maker_sell_order,
        maker_sell_cancel,
        open_transport,
        close_transport,
        maker_buy_transport,
        maker_buy_cancel_transport,
        maker_sell_transport,
        maker_sell_cancel_transport,
        balance_update,
        account_update,
        final_position,
        final_open_orders,
        refreshed_executions,
        event_counts,
        diagnostics,
    }))
}

pub async fn run_binance_extended_stress(
    client: &BatMarkets,
) -> Result<Option<BinanceExtendedStressReport>> {
    let Some(plan) = prepare_binance_trade_cycle(client).await? else {
        return Ok(None);
    };

    let spec = client.market().require_instrument(&plan.instrument_id)?;
    let burst_quantity = burst_order_quantity(&spec, plan.reference_ask)?;

    let mut public = client
        .stream()
        .public()
        .watch_book_top(WatchInstrumentsRequest::for_instrument(
            plan.instrument_id.clone(),
        ))
        .await?;
    let mut orders = client.watch_orders().await?;
    let mut executions = client.watch_my_trades().await?;
    let mut positions = client.watch_positions().await?;
    let mut balances = client.watch_balance().await?;
    let mut account = client.stream().private().watch_account().await?;

    let _ = public.recv().await?;
    sleep(PRIVATE_STREAM_WARMUP).await;

    let _ = client.fetch_balance().await?;
    let _ = client.fetch_positions().await?;
    let _ = client
        .fetch_open_orders(Some(&ListOpenOrdersRequest {
            instrument_id: Some(plan.instrument_id.clone()),
        }))
        .await?;
    let _ = client
        .fetch_my_trades(Some(&ListExecutionsRequest {
            instrument_id: Some(plan.instrument_id.clone()),
            limit: Some(50),
        }))
        .await?;

    let mut event_counts = LiveTradeEventCounts::default();
    let open_client_order_id = live_client_order_id("bxs-open");
    let close_client_order_id = live_client_order_id("bxs-close");
    let stop_client_order_id = live_client_order_id("bxs-sl");
    let take_profit_client_order_id = live_client_order_id("bxs-tp");

    let open_request = CreateOrderRequest {
        request_id: None,
        instrument_id: plan.instrument_id.clone(),
        client_order_id: Some(open_client_order_id.clone()),
        side: Side::Buy,
        order_type: OrderType::Market,
        time_in_force: None,
        quantity: plan.market_quantity,
        price: None,
        trigger_price: None,
        trigger_type: None,
        reduce_only: false,
        post_only: false,
    };
    validate_live_order(client, &open_request).await?;

    let mut open_handle = client.create_order(&open_request).await?;
    let open_transport = open_handle.ack().transport;
    ensure_command_not_rejected(&mut open_handle, "extended stress open market order").await?;

    let (open_execution, open_execution_latency) = observe_or_refresh_execution_latency(
        client,
        &mut executions,
        &plan.instrument_id,
        &open_client_order_id,
        open_handle.ack().receipt.order_id.as_ref(),
    )
    .await?;
    if open_execution_latency.streamed {
        event_counts.executions += 1;
    }
    let open_quantity = open_execution.quantity;
    let protective_prices = protective_trigger_prices(&spec, open_execution.price);

    let (_open_position, position_open_latency) =
        observe_or_refresh_position_latency(client, &mut positions, &plan.instrument_id, false)
            .await?;
    if position_open_latency.streamed {
        event_counts.positions += 1;
    }
    let (balance_update, balance_latency) =
        observe_or_refresh_balance_latency(client, &mut balances).await?;
    if balance_latency
        .as_ref()
        .is_some_and(|latency| latency.streamed)
    {
        event_counts.balances += 1;
    }
    let (account_update, account_latency) =
        observe_or_refresh_account_latency(client, &mut account).await?;
    if account_latency
        .as_ref()
        .is_some_and(|latency| latency.streamed)
    {
        event_counts.account += 1;
    }

    let open_order = await_rest_order_state(
        client,
        &plan.instrument_id,
        open_handle.ack().receipt.order_id.clone(),
        Some(open_client_order_id.clone()),
        &[OrderStatus::Filled, OrderStatus::PartiallyFilled],
    )
    .await?;

    let stop_request = CreateOrderRequest {
        request_id: None,
        instrument_id: plan.instrument_id.clone(),
        client_order_id: Some(stop_client_order_id.clone()),
        side: Side::Sell,
        order_type: OrderType::StopMarket,
        time_in_force: None,
        quantity: open_quantity,
        price: None,
        trigger_price: Some(protective_prices.stop_price),
        trigger_type: Some(TriggerType::MarkPrice),
        reduce_only: true,
        post_only: false,
    };
    let take_profit_request = CreateOrderRequest {
        request_id: None,
        instrument_id: plan.instrument_id.clone(),
        client_order_id: Some(take_profit_client_order_id.clone()),
        side: Side::Sell,
        order_type: OrderType::TakeProfitMarket,
        time_in_force: None,
        quantity: open_quantity,
        price: None,
        trigger_price: Some(protective_prices.take_profit_price),
        trigger_type: Some(TriggerType::MarkPrice),
        reduce_only: true,
        post_only: false,
    };
    validate_live_order(client, &stop_request).await?;
    validate_live_order(client, &take_profit_request).await?;

    let protective_create_started = Instant::now();
    let protective_handles = client
        .create_orders(&CreateOrdersRequest {
            request_id: None,
            orders: vec![stop_request.clone(), take_profit_request.clone()],
        })
        .await?;
    let _protective_create_ack_ns = saturating_duration_ns(protective_create_started.elapsed());
    if protective_handles.len() != 2 {
        return Err(MarketError::new(
            ErrorKind::TransportError,
            format!(
                "expected 2 protective handles, got {}",
                protective_handles.len()
            ),
        ));
    }
    let mut protective_handles = protective_handles;
    let stop_transport = protective_handles[0].ack().transport;
    let take_profit_transport = protective_handles[1].ack().transport;
    ensure_command_not_rejected(&mut protective_handles[0], "protective stop market").await?;
    ensure_command_not_rejected(&mut protective_handles[1], "protective take profit market")
        .await?;

    let protective_created = await_order_updates_for_ids(
        &mut orders,
        &plan.instrument_id,
        &[
            stop_client_order_id.clone(),
            take_profit_client_order_id.clone(),
        ],
        &[OrderStatus::New, OrderStatus::PartiallyFilled],
        protective_create_started,
    )
    .await?;
    event_counts.orders += protective_created.len();
    let stop_order = protective_created
        .get(&stop_client_order_id)
        .map(|update| update.order.clone())
        .ok_or_else(|| {
            MarketError::new(
                ErrorKind::TransportError,
                "missing stop order update in protective create batch",
            )
        })?;
    let take_profit_order = protective_created
        .get(&take_profit_client_order_id)
        .map(|update| update.order.clone())
        .ok_or_else(|| {
            MarketError::new(
                ErrorKind::TransportError,
                "missing take-profit order update in protective create batch",
            )
        })?;

    await_open_order_presence(client, &plan.instrument_id, &stop_client_order_id, true).await?;
    await_open_order_presence(
        client,
        &plan.instrument_id,
        &take_profit_client_order_id,
        true,
    )
    .await?;

    let protective_cancel_started = Instant::now();
    let protective_cancel_handles = client
        .cancel_orders(&CancelOrdersRequest {
            request_id: None,
            orders: vec![
                OrderTarget {
                    instrument_id: plan.instrument_id.clone(),
                    order_id: Some(stop_order.order_id.clone()),
                    client_order_id: Some(stop_client_order_id.clone()),
                },
                OrderTarget {
                    instrument_id: plan.instrument_id.clone(),
                    order_id: Some(take_profit_order.order_id.clone()),
                    client_order_id: Some(take_profit_client_order_id.clone()),
                },
            ],
        })
        .await?;
    if protective_cancel_handles.len() != 2 {
        return Err(MarketError::new(
            ErrorKind::TransportError,
            format!(
                "expected 2 protective cancel handles, got {}",
                protective_cancel_handles.len()
            ),
        ));
    }
    let mut protective_cancel_handles = protective_cancel_handles;
    let stop_cancel_transport = protective_cancel_handles[0].ack().transport;
    let take_profit_cancel_transport = protective_cancel_handles[1].ack().transport;
    ensure_command_not_rejected(&mut protective_cancel_handles[0], "cancel protective stop")
        .await?;
    ensure_command_not_rejected(
        &mut protective_cancel_handles[1],
        "cancel protective take profit",
    )
    .await?;

    let protective_canceled = await_order_updates_for_ids(
        &mut orders,
        &plan.instrument_id,
        &[
            stop_client_order_id.clone(),
            take_profit_client_order_id.clone(),
        ],
        &[
            OrderStatus::Canceled,
            OrderStatus::Expired,
            OrderStatus::PendingCancel,
        ],
        protective_cancel_started,
    )
    .await?;
    event_counts.orders += protective_canceled.len();
    let stop_cancel = protective_canceled
        .get(&stop_client_order_id)
        .map(|update| update.order.clone())
        .ok_or_else(|| {
            MarketError::new(
                ErrorKind::TransportError,
                "missing stop cancel update in protective batch",
            )
        })?;
    let take_profit_cancel = protective_canceled
        .get(&take_profit_client_order_id)
        .map(|update| update.order.clone())
        .ok_or_else(|| {
            MarketError::new(
                ErrorKind::TransportError,
                "missing take-profit cancel update in protective batch",
            )
        })?;

    await_open_order_presence(client, &plan.instrument_id, &stop_client_order_id, false).await?;
    await_open_order_presence(
        client,
        &plan.instrument_id,
        &take_profit_client_order_id,
        false,
    )
    .await?;

    let close_request = ClosePositionRequest {
        request_id: None,
        instrument_id: plan.instrument_id.clone(),
        quantity: None,
        client_order_id: Some(close_client_order_id.clone()),
        price: None,
        time_in_force: None,
        post_only: false,
    };
    let mut close_handle = client.close_position(&close_request).await?;
    let close_transport = close_handle.ack().transport;
    ensure_command_not_rejected(&mut close_handle, "extended stress close position").await?;

    let (close_execution, close_execution_latency) = observe_or_refresh_execution_latency(
        client,
        &mut executions,
        &plan.instrument_id,
        &close_client_order_id,
        close_handle.ack().receipt.order_id.as_ref(),
    )
    .await?;
    if close_execution_latency.streamed {
        event_counts.executions += 1;
    }
    let (final_position, position_flat_latency) =
        observe_or_refresh_position_latency(client, &mut positions, &plan.instrument_id, true)
            .await?;
    if position_flat_latency.streamed {
        event_counts.positions += 1;
    }
    let close_order = await_rest_order_state(
        client,
        &plan.instrument_id,
        close_handle.ack().receipt.order_id.clone(),
        Some(close_client_order_id.clone()),
        &[OrderStatus::Filled, OrderStatus::PartiallyFilled],
    )
    .await?;

    let rounds = env_usize("BAT_MARKETS_BINANCE_EXTENDED_STRESS_ROUNDS")?
        .unwrap_or(EXTENDED_STRESS_DEFAULT_ROUNDS)
        .max(1);
    let burst_size = env_usize("BAT_MARKETS_BINANCE_EXTENDED_STRESS_BURST_SIZE")?
        .unwrap_or(EXTENDED_STRESS_DEFAULT_BURST_SIZE)
        .max(1);
    let mut burst_rounds = Vec::with_capacity(rounds);
    let mut create_ack_latencies = Vec::with_capacity(rounds);
    let mut cancel_ack_latencies = Vec::with_capacity(rounds);
    let mut create_stream_latencies = Vec::with_capacity(rounds * burst_size);
    let mut cancel_stream_latencies = Vec::with_capacity(rounds * burst_size);

    for round in 0..rounds {
        let round_report = run_binance_burst_round(
            client,
            &plan,
            &spec,
            burst_quantity,
            round,
            burst_size,
            &mut orders,
        )
        .await?;
        event_counts.orders += burst_size * 2;
        create_ack_latencies.push(round_report.create_ack_ns);
        cancel_ack_latencies.push(round_report.cancel_ack_ns);
        create_stream_latencies.extend(round_report.create_stream_latencies_ns.iter().copied());
        cancel_stream_latencies.extend(round_report.cancel_stream_latencies_ns.iter().copied());
        burst_rounds.push(round_report);
    }

    let final_open_orders = client
        .fetch_open_orders(Some(&ListOpenOrdersRequest {
            instrument_id: Some(plan.instrument_id.clone()),
        }))
        .await?;
    let refreshed_executions = client
        .fetch_my_trades(Some(&ListExecutionsRequest {
            instrument_id: Some(plan.instrument_id.clone()),
            limit: Some(100),
        }))
        .await?;
    let diagnostics = client.advanced().diagnostics();

    public.shutdown().await?;
    orders.shutdown().await?;
    executions.shutdown().await?;
    positions.shutdown().await?;
    balances.shutdown().await?;
    account.shutdown().await?;

    Ok(Some(BinanceExtendedStressReport {
        instrument_id: plan.instrument_id,
        market_quantity: plan.market_quantity,
        burst_quantity,
        open_order,
        open_execution,
        open_execution_latency,
        open_transport,
        protective: ProtectiveOrderReport {
            stop_order,
            take_profit_order,
            stop_transport,
            take_profit_transport,
            stop_cancel,
            take_profit_cancel,
            stop_cancel_transport,
            take_profit_cancel_transport,
        },
        close_order,
        close_execution,
        close_execution_latency,
        close_transport,
        position_open_latency,
        position_flat_latency,
        balance_latency,
        account_latency,
        balance_update,
        account_update,
        burst_rounds,
        hft_create_ack: summarize_latencies(&create_ack_latencies),
        hft_create_stream: summarize_latencies(&create_stream_latencies),
        hft_cancel_ack: summarize_latencies(&cancel_ack_latencies),
        hft_cancel_stream: summarize_latencies(&cancel_stream_latencies),
        final_position,
        final_open_orders,
        refreshed_executions,
        event_counts,
        diagnostics,
    }))
}

async fn run_binance_burst_round(
    client: &BatMarkets,
    plan: &LiveTradeCyclePlan,
    spec: &InstrumentSpec,
    quantity: Quantity,
    round: usize,
    burst_size: usize,
    orders: &mut OrdersWatch<'_>,
) -> Result<BurstRoundReport> {
    let create_request = build_binance_burst_request(plan, spec, quantity, round, burst_size);
    let client_order_ids = create_request
        .orders
        .iter()
        .filter_map(|order| order.client_order_id.clone())
        .collect::<Vec<_>>();

    for order in &create_request.orders {
        validate_live_order(client, order).await?;
    }

    let create_started = Instant::now();
    let create_handles = client.create_orders(&create_request).await?;
    let create_ack_ns = saturating_duration_ns(create_started.elapsed());
    if create_handles.len() != burst_size {
        return Err(MarketError::new(
            ErrorKind::TransportError,
            format!(
                "expected {burst_size} create handles in round {round}, got {}",
                create_handles.len()
            ),
        ));
    }
    let mut create_handles = create_handles;
    let create_transports = create_handles
        .iter()
        .map(|handle| handle.ack().transport)
        .collect::<Vec<_>>();
    for handle in &mut create_handles {
        ensure_command_not_rejected(handle, "binance burst create order").await?;
    }

    let created = await_order_updates_for_ids(
        orders,
        &plan.instrument_id,
        &client_order_ids,
        &[OrderStatus::New, OrderStatus::PartiallyFilled],
        create_started,
    )
    .await?;
    let create_stream_latencies_ns = created
        .values()
        .map(|update| update.latency_ns)
        .collect::<Vec<_>>();
    let create_stream = summarize_latencies(&create_stream_latencies_ns);
    for client_order_id in &client_order_ids {
        await_open_order_presence(client, &plan.instrument_id, client_order_id, true).await?;
    }

    let cancel_started = Instant::now();
    let cancel_handles = client
        .cancel_orders(&CancelOrdersRequest {
            request_id: None,
            orders: client_order_ids
                .iter()
                .map(|client_order_id| OrderTarget {
                    instrument_id: plan.instrument_id.clone(),
                    order_id: created
                        .get(client_order_id)
                        .map(|update| update.order.order_id.clone()),
                    client_order_id: Some(client_order_id.clone()),
                })
                .collect(),
        })
        .await?;
    let cancel_ack_ns = saturating_duration_ns(cancel_started.elapsed());
    if cancel_handles.len() != burst_size {
        return Err(MarketError::new(
            ErrorKind::TransportError,
            format!(
                "expected {burst_size} cancel handles in round {round}, got {}",
                cancel_handles.len()
            ),
        ));
    }
    let mut cancel_handles = cancel_handles;
    let cancel_transports = cancel_handles
        .iter()
        .map(|handle| handle.ack().transport)
        .collect::<Vec<_>>();
    for handle in &mut cancel_handles {
        ensure_command_not_rejected(handle, "binance burst cancel order").await?;
    }

    let canceled = await_order_updates_for_ids(
        orders,
        &plan.instrument_id,
        &client_order_ids,
        &[
            OrderStatus::Canceled,
            OrderStatus::Expired,
            OrderStatus::PendingCancel,
        ],
        cancel_started,
    )
    .await?;
    let cancel_stream_latencies_ns = canceled
        .values()
        .map(|update| update.latency_ns)
        .collect::<Vec<_>>();
    let cancel_stream = summarize_latencies(&cancel_stream_latencies_ns);
    for client_order_id in &client_order_ids {
        await_open_order_presence(client, &plan.instrument_id, client_order_id, false).await?;
    }

    Ok(BurstRoundReport {
        round,
        create_ack_ns,
        create_stream,
        create_stream_latencies_ns,
        cancel_ack_ns,
        cancel_stream,
        cancel_stream_latencies_ns,
        create_transports,
        cancel_transports,
    })
}

fn build_binance_burst_request(
    plan: &LiveTradeCyclePlan,
    spec: &InstrumentSpec,
    quantity: Quantity,
    round: usize,
    burst_size: usize,
) -> CreateOrdersRequest {
    let distance_from_book = maker_burst_distance(spec, plan.reference_ask);
    let orders = (0..burst_size)
        .map(|index| {
            let distance = distance_from_book
                + spec.tick_size.value() * Decimal::new((round * burst_size + index + 1) as i64, 0);
            let side = if index % 2 == 0 {
                Side::Buy
            } else {
                Side::Sell
            };
            let price = match side {
                Side::Buy => Price::new(quantize_down(
                    (plan.reference_bid.value() - distance).max(spec.tick_size.value()),
                    spec.tick_size.value(),
                    spec.price_scale,
                )),
                Side::Sell => Price::new(quantize_up(
                    plan.reference_ask.value() + distance,
                    spec.tick_size.value(),
                    spec.price_scale,
                )),
            };
            CreateOrderRequest {
                request_id: None,
                instrument_id: plan.instrument_id.clone(),
                client_order_id: Some(live_client_order_id(&format!("bxhft-r{round}-o{index}"))),
                side,
                order_type: OrderType::Limit,
                time_in_force: Some(TimeInForce::Gtc),
                quantity,
                price: Some(price),
                trigger_price: None,
                trigger_type: None,
                reduce_only: false,
                post_only: true,
            }
        })
        .collect();

    CreateOrdersRequest {
        request_id: None,
        orders,
    }
}

fn burst_order_quantity(spec: &InstrumentSpec, ask: Price) -> Result<Quantity> {
    let step = spec.step_size.value();
    let quantity = quantize_up(
        (spec.min_notional.value() * notional_buffer_multiplier()) / ask.value(),
        step,
        spec.qty_scale,
    );
    if quantity <= Decimal::ZERO {
        return Err(MarketError::new(
            ErrorKind::ConfigError,
            format!("invalid burst quantity for {}", spec.instrument_id),
        ));
    }
    Ok(Quantity::new(quantity))
}

struct ProtectiveTriggerPrices {
    stop_price: Price,
    take_profit_price: Price,
}

fn protective_trigger_prices(spec: &InstrumentSpec, entry_price: Price) -> ProtectiveTriggerPrices {
    let tick = spec.tick_size.value();
    let ratio = entry_price.value() * Decimal::new(PROTECTIVE_TRIGGER_BPS, 4);
    let absolute = tick * Decimal::new(PROTECTIVE_MIN_TICKS, 0);
    let offset = ratio.max(absolute);
    let stop_price = Price::new(quantize_down(
        (entry_price.value() - offset).max(tick),
        tick,
        spec.price_scale,
    ));
    let take_profit_price = Price::new(quantize_up(
        entry_price.value() + offset,
        tick,
        spec.price_scale,
    ));
    ProtectiveTriggerPrices {
        stop_price,
        take_profit_price,
    }
}

fn maker_burst_distance(spec: &InstrumentSpec, reference_ask: Price) -> Decimal {
    let tick_distance = spec.tick_size.value() * Decimal::new(BURST_MIN_TICKS, 0);
    let ratio_distance = reference_ask.value() * Decimal::new(BURST_DISTANCE_BPS, 4);
    tick_distance.max(ratio_distance)
}

async fn build_live_trade_plan(
    client: &BatMarkets,
    spec: &InstrumentSpec,
    max_budget: Decimal,
    occupied: &BTreeSet<InstrumentId>,
) -> Result<Option<LiveTradeCyclePlan>> {
    if spec.status != InstrumentStatus::Active
        || !spec.support.private_trading
        || !spec.support.public_streams
        || occupied.contains(&spec.instrument_id)
    {
        return Ok(None);
    }

    let book_top = match client.market().fetch_book_top(&spec.instrument_id).await {
        Ok(book_top) => book_top,
        Err(_) => return Ok(None),
    };
    let bid = book_top.bid.price;
    let ask = book_top.ask.price;
    let tick = spec.tick_size.value();
    let step = spec.step_size.value();
    if bid.value() <= Decimal::ZERO
        || ask.value() <= Decimal::ZERO
        || tick <= Decimal::ZERO
        || step <= Decimal::ZERO
    {
        return Ok(None);
    }

    let min_qty = quantize_up(spec.min_qty.value(), step, spec.qty_scale);
    let required_for_notional = quantize_up(
        (spec.min_notional.value() * notional_buffer_multiplier()) / ask.value(),
        step,
        spec.qty_scale,
    );
    let quantity_value = min_qty.max(required_for_notional);
    if quantity_value <= Decimal::ZERO || quantity_value * ask.value() > max_budget {
        return Ok(None);
    }

    let maker_buy = quantize_down(
        (bid.value() - tick * Decimal::new(MAKER_TICKS_AWAY, 0)).max(tick),
        tick,
        spec.price_scale,
    );
    let maker_sell = quantize_up(
        ask.value() + tick * Decimal::new(MAKER_TICKS_AWAY, 0),
        tick,
        spec.price_scale,
    );
    if maker_buy <= Decimal::ZERO || maker_sell <= maker_buy {
        return Ok(None);
    }

    Ok(Some(LiveTradeCyclePlan {
        instrument_id: spec.instrument_id.clone(),
        market_quantity: Quantity::new(quantity_value),
        maker_buy_price: Price::new(maker_buy),
        maker_sell_price: Price::new(maker_sell),
        reference_bid: bid,
        reference_ask: ask,
    }))
}

async fn validate_live_order(client: &BatMarkets, order: &CreateOrderRequest) -> Result<()> {
    let handle = match client
        .validate_order(&bat_markets::types::ValidateOrderRequest {
            request_id: None,
            order: order.clone(),
        })
        .await
    {
        Ok(handle) => handle,
        Err(error)
            if is_binance_algo_validation_gap(order, error.context.native_code.as_deref()) =>
        {
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    if handle.ack().receipt.status == CommandStatus::Rejected {
        if is_binance_algo_validation_gap(order, handle.ack().receipt.native_code.as_deref()) {
            return Ok(());
        }
        return Err(MarketError::new(
            ErrorKind::Unsupported,
            format!(
                "live validate_order rejected {} {}",
                order.instrument_id,
                handle
                    .ack()
                    .receipt
                    .message
                    .as_deref()
                    .unwrap_or("without message")
            ),
        ));
    }
    Ok(())
}

fn is_binance_algo_validation_gap(order: &CreateOrderRequest, native_code: Option<&str>) -> bool {
    matches!(
        order.order_type,
        OrderType::StopMarket
            | OrderType::StopLimit
            | OrderType::TakeProfitMarket
            | OrderType::TakeProfitLimit
    ) && native_code == Some("-4120")
}

async fn ensure_command_not_rejected(
    handle: &mut bat_markets::PendingCommandHandle,
    label: &str,
) -> Result<()> {
    match handle.ack().receipt.status {
        CommandStatus::Accepted => Ok(()),
        CommandStatus::Rejected => Err(MarketError::new(
            ErrorKind::Unsupported,
            format!(
                "{label} was rejected: {}",
                handle
                    .ack()
                    .receipt
                    .message
                    .as_deref()
                    .unwrap_or("without message")
            ),
        )),
        CommandStatus::UnknownExecution => {
            let _ = handle.resolved().await?;
            Ok(())
        }
    }
}

async fn await_order_update(
    watch: &mut OrdersWatch<'_>,
    instrument_id: &InstrumentId,
    client_order_id: &ClientOrderId,
    acceptable_statuses: &[OrderStatus],
) -> Result<Order> {
    timeout(ORDER_EVENT_TIMEOUT, async {
        loop {
            let order = watch.recv().await?;
            if order.instrument_id != *instrument_id
                || order.client_order_id.as_ref() != Some(client_order_id)
            {
                continue;
            }
            if acceptable_statuses.contains(&order.status) {
                return Ok(order);
            }
        }
    })
    .await
    .map_err(|_| {
        MarketError::new(
            ErrorKind::TransportError,
            format!("timed out waiting for order update {client_order_id}"),
        )
    })?
}

async fn await_execution_update(
    watch: &mut bat_markets::ExecutionsWatch<'_>,
    instrument_id: &InstrumentId,
    client_order_id: &ClientOrderId,
) -> Result<Execution> {
    timeout(EXECUTION_EVENT_TIMEOUT, async {
        loop {
            let execution = watch.recv().await?;
            if execution.instrument_id == *instrument_id
                && execution.client_order_id.as_ref() == Some(client_order_id)
            {
                return Ok(execution);
            }
        }
    })
    .await
    .map_err(|_| {
        MarketError::new(
            ErrorKind::TransportError,
            format!("timed out waiting for execution update {client_order_id}"),
        )
    })?
}

async fn await_position_state(
    watch: &mut bat_markets::PositionsWatch<'_>,
    instrument_id: &InstrumentId,
    expect_flat: bool,
) -> Result<Position> {
    timeout(POSITION_EVENT_TIMEOUT, async {
        loop {
            let position = watch.recv().await?;
            if position.instrument_id != *instrument_id {
                continue;
            }
            let is_flat = position.direction == PositionDirection::Flat
                || position.size.value() == Decimal::ZERO;
            if is_flat == expect_flat {
                return Ok(position);
            }
        }
    })
    .await
    .map_err(|_| {
        MarketError::new(
            ErrorKind::TransportError,
            format!(
                "timed out waiting for {} position update on {instrument_id}",
                if expect_flat { "flat" } else { "non-flat" }
            ),
        )
    })?
}

async fn await_balance_update(watch: &mut bat_markets::BalancesWatch<'_>) -> Result<Balance> {
    timeout(BALANCE_EVENT_TIMEOUT, async {
        loop {
            let balance = watch.recv().await?;
            if balance.asset.as_ref() == "USDT" {
                return Ok(balance);
            }
        }
    })
    .await
    .map_err(|_| {
        MarketError::new(
            ErrorKind::TransportError,
            "timed out waiting for balance update",
        )
    })?
}

async fn await_account_update(watch: &mut AccountWatch<'_>) -> Result<AccountSummary> {
    timeout(ACCOUNT_EVENT_TIMEOUT, watch.recv())
        .await
        .map_err(|_| {
            MarketError::new(
                ErrorKind::TransportError,
                "timed out waiting for account update",
            )
        })?
}

async fn await_open_order_presence(
    client: &BatMarkets,
    instrument_id: &InstrumentId,
    client_order_id: &ClientOrderId,
    expected_present: bool,
) -> Result<()> {
    let deadline = tokio::time::Instant::now() + OPEN_ORDER_TIMEOUT;
    loop {
        let orders = client
            .fetch_open_orders(Some(&ListOpenOrdersRequest {
                instrument_id: Some(instrument_id.clone()),
            }))
            .await?;
        let present = orders.iter().any(|order| {
            order.instrument_id == *instrument_id
                && order.client_order_id.as_ref() == Some(client_order_id)
        });
        if present == expected_present {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(MarketError::new(
                ErrorKind::TransportError,
                format!(
                    "timed out waiting for open-order presence={} on {} ({client_order_id})",
                    expected_present, instrument_id
                ),
            ));
        }
        sleep(Duration::from_millis(250)).await;
    }
}

async fn observe_or_refresh_position_state(
    client: &BatMarkets,
    watch: &mut bat_markets::PositionsWatch<'_>,
    instrument_id: &InstrumentId,
    expect_flat: bool,
) -> Result<(Position, bool)> {
    match timeout(
        OPTIONAL_PRIVATE_STREAM_OBSERVE_TIMEOUT,
        await_position_state(watch, instrument_id, expect_flat),
    )
    .await
    {
        Ok(Ok(position)) => Ok((position, true)),
        Ok(Err(_)) | Err(_) => await_position_snapshot(client, instrument_id, expect_flat)
            .await
            .map(|position| (position, false)),
    }
}

async fn observe_or_refresh_position_latency(
    client: &BatMarkets,
    watch: &mut bat_markets::PositionsWatch<'_>,
    instrument_id: &InstrumentId,
    expect_flat: bool,
) -> Result<(Position, StreamLatencyObservation)> {
    let started = Instant::now();
    match timeout(
        OPTIONAL_PRIVATE_STREAM_OBSERVE_TIMEOUT,
        await_position_state(watch, instrument_id, expect_flat),
    )
    .await
    {
        Ok(Ok(position)) => Ok((
            position,
            StreamLatencyObservation {
                latency_ns: saturating_duration_ns(started.elapsed()),
                streamed: true,
            },
        )),
        Ok(Err(_)) | Err(_) => await_position_snapshot(client, instrument_id, expect_flat)
            .await
            .map(|position| {
                (
                    position,
                    StreamLatencyObservation {
                        latency_ns: saturating_duration_ns(started.elapsed()),
                        streamed: false,
                    },
                )
            }),
    }
}

async fn observe_or_refresh_execution_latency(
    client: &BatMarkets,
    watch: &mut bat_markets::ExecutionsWatch<'_>,
    instrument_id: &InstrumentId,
    client_order_id: &ClientOrderId,
    order_id: Option<&bat_markets::types::OrderId>,
) -> Result<(Execution, StreamLatencyObservation)> {
    let started = Instant::now();
    match timeout(
        OPTIONAL_PRIVATE_STREAM_OBSERVE_TIMEOUT,
        await_execution_update(watch, instrument_id, client_order_id),
    )
    .await
    {
        Ok(Ok(execution)) => Ok((
            execution,
            StreamLatencyObservation {
                latency_ns: saturating_duration_ns(started.elapsed()),
                streamed: true,
            },
        )),
        Ok(Err(_)) | Err(_) => {
            await_execution_snapshot(client, instrument_id, client_order_id, order_id)
                .await
                .map(|execution| {
                    (
                        execution,
                        StreamLatencyObservation {
                            latency_ns: saturating_duration_ns(started.elapsed()),
                            streamed: false,
                        },
                    )
                })
        }
    }
}

async fn observe_or_refresh_balance(
    client: &BatMarkets,
    watch: &mut bat_markets::BalancesWatch<'_>,
) -> Result<(Balance, bool)> {
    match timeout(
        OPTIONAL_PRIVATE_STREAM_OBSERVE_TIMEOUT,
        await_balance_update(watch),
    )
    .await
    {
        Ok(Ok(balance)) => Ok((balance, true)),
        Ok(Err(_)) | Err(_) => {
            let _ = client.fetch_balance().await?;
            client
                .account()
                .balances()
                .into_iter()
                .find(|balance| balance.asset.as_ref() == "USDT")
                .map(|balance| (balance, false))
                .ok_or_else(|| {
                    MarketError::new(
                        ErrorKind::TransportError,
                        "missing refreshed USDT balance snapshot",
                    )
                })
        }
    }
}

async fn observe_or_refresh_balance_latency(
    client: &BatMarkets,
    watch: &mut bat_markets::BalancesWatch<'_>,
) -> Result<(Balance, Option<StreamLatencyObservation>)> {
    let started = Instant::now();
    match timeout(
        OPTIONAL_PRIVATE_STREAM_OBSERVE_TIMEOUT,
        await_balance_update(watch),
    )
    .await
    {
        Ok(Ok(balance)) => Ok((
            balance,
            Some(StreamLatencyObservation {
                latency_ns: saturating_duration_ns(started.elapsed()),
                streamed: true,
            }),
        )),
        Ok(Err(_)) | Err(_) => {
            let _ = client.fetch_balance().await?;
            client
                .account()
                .balances()
                .into_iter()
                .find(|balance| balance.asset.as_ref() == "USDT")
                .map(|balance| {
                    (
                        balance,
                        Some(StreamLatencyObservation {
                            latency_ns: saturating_duration_ns(started.elapsed()),
                            streamed: false,
                        }),
                    )
                })
                .ok_or_else(|| {
                    MarketError::new(
                        ErrorKind::TransportError,
                        "missing refreshed USDT balance snapshot",
                    )
                })
        }
    }
}

async fn observe_or_refresh_account(
    client: &BatMarkets,
    watch: &mut AccountWatch<'_>,
) -> Result<(AccountSummary, bool)> {
    match timeout(
        OPTIONAL_PRIVATE_STREAM_OBSERVE_TIMEOUT,
        await_account_update(watch),
    )
    .await
    {
        Ok(Ok(summary)) => Ok((summary, true)),
        Ok(Err(_)) | Err(_) => refresh_account_summary(client)
            .await
            .map(|summary| (summary, false)),
    }
}

async fn observe_or_refresh_account_latency(
    client: &BatMarkets,
    watch: &mut AccountWatch<'_>,
) -> Result<(AccountSummary, Option<StreamLatencyObservation>)> {
    let started = Instant::now();
    match timeout(
        OPTIONAL_PRIVATE_STREAM_OBSERVE_TIMEOUT,
        await_account_update(watch),
    )
    .await
    {
        Ok(Ok(summary)) => Ok((
            summary,
            Some(StreamLatencyObservation {
                latency_ns: saturating_duration_ns(started.elapsed()),
                streamed: true,
            }),
        )),
        Ok(Err(_)) | Err(_) => refresh_account_summary(client).await.map(|summary| {
            (
                summary,
                Some(StreamLatencyObservation {
                    latency_ns: saturating_duration_ns(started.elapsed()),
                    streamed: false,
                }),
            )
        }),
    }
}

async fn refresh_account_summary(client: &BatMarkets) -> Result<AccountSummary> {
    client.fetch_balance().await?.summary.ok_or_else(|| {
        MarketError::new(
            ErrorKind::TransportError,
            "missing refreshed account summary snapshot",
        )
    })
}

async fn await_position_snapshot(
    client: &BatMarkets,
    instrument_id: &InstrumentId,
    expect_flat: bool,
) -> Result<Position> {
    let deadline = tokio::time::Instant::now() + POSITION_EVENT_TIMEOUT;
    loop {
        let positions = client.fetch_positions().await?;
        if let Some(position) = positions
            .into_iter()
            .find(|position| position.instrument_id == *instrument_id)
        {
            let is_flat = position.direction == PositionDirection::Flat
                || position.size.value() == Decimal::ZERO;
            if is_flat == expect_flat {
                return Ok(position);
            }
        } else if expect_flat {
            return Ok(synthetic_flat_position(instrument_id));
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(MarketError::new(
                ErrorKind::TransportError,
                format!("timed out waiting for refreshed position state on {instrument_id}"),
            ));
        }
        sleep(Duration::from_millis(250)).await;
    }
}

async fn await_rest_order_state(
    client: &BatMarkets,
    instrument_id: &InstrumentId,
    order_id: Option<bat_markets::types::OrderId>,
    client_order_id: Option<ClientOrderId>,
    acceptable_statuses: &[OrderStatus],
) -> Result<Order> {
    let deadline = tokio::time::Instant::now() + ORDER_EVENT_TIMEOUT;
    loop {
        let result = client
            .fetch_order(&GetOrderRequest {
                request_id: None,
                instrument_id: instrument_id.clone(),
                order_id: order_id.clone(),
                client_order_id: client_order_id.clone(),
            })
            .await;
        if let Ok(order) = result
            && acceptable_statuses.contains(&order.status)
        {
            return Ok(order);
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(MarketError::new(
                ErrorKind::TransportError,
                format!("timed out waiting for REST order state on {instrument_id}"),
            ));
        }
        sleep(Duration::from_millis(250)).await;
    }
}

async fn await_execution_snapshot(
    client: &BatMarkets,
    instrument_id: &InstrumentId,
    client_order_id: &ClientOrderId,
    order_id: Option<&bat_markets::types::OrderId>,
) -> Result<Execution> {
    let deadline = tokio::time::Instant::now() + EXECUTION_EVENT_TIMEOUT;
    loop {
        let executions = client
            .fetch_my_trades(Some(&ListExecutionsRequest {
                instrument_id: Some(instrument_id.clone()),
                limit: Some(50),
            }))
            .await?;
        if let Some(execution) = executions.into_iter().find(|execution| {
            execution.instrument_id == *instrument_id
                && (execution.client_order_id.as_ref() == Some(client_order_id)
                    || order_id.is_some_and(|order_id| &execution.order_id == order_id))
        }) {
            return Ok(execution);
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(MarketError::new(
                ErrorKind::TransportError,
                format!(
                    "timed out waiting for refreshed execution state on {} ({client_order_id})",
                    instrument_id
                ),
            ));
        }
        sleep(Duration::from_millis(250)).await;
    }
}

async fn await_order_updates_for_ids(
    watch: &mut OrdersWatch<'_>,
    instrument_id: &InstrumentId,
    client_order_ids: &[ClientOrderId],
    acceptable_statuses: &[OrderStatus],
    started_at: Instant,
) -> Result<BTreeMap<ClientOrderId, TimedOrderUpdate>> {
    let mut pending = client_order_ids.iter().cloned().collect::<BTreeSet<_>>();
    timeout(ORDER_EVENT_TIMEOUT, async {
        let mut updates = BTreeMap::new();
        while !pending.is_empty() {
            let order = watch.recv().await?;
            if order.instrument_id != *instrument_id {
                continue;
            }
            let Some(client_order_id) = order.client_order_id.clone() else {
                continue;
            };
            if !pending.contains(&client_order_id) || !acceptable_statuses.contains(&order.status) {
                continue;
            }
            updates.insert(
                client_order_id.clone(),
                TimedOrderUpdate {
                    order,
                    latency_ns: saturating_duration_ns(started_at.elapsed()),
                },
            );
            pending.remove(&client_order_id);
        }
        Ok(updates)
    })
    .await
    .map_err(|_| {
        MarketError::new(
            ErrorKind::TransportError,
            format!(
                "timed out waiting for {} order updates on {}",
                client_order_ids.len(),
                instrument_id
            ),
        )
    })?
}

fn summarize_latencies(samples: &[u64]) -> LatencySummary {
    if samples.is_empty() {
        return LatencySummary::default();
    }

    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let min_ns = sorted[0];
    let max_ns = *sorted.last().unwrap_or(&min_ns);
    let avg_ns = (sorted.iter().copied().map(u128::from).sum::<u128>() / sorted.len() as u128)
        .min(u64::MAX as u128) as u64;
    let p95_index = ((sorted.len() - 1) * 95) / 100;
    let p95_ns = sorted[p95_index];

    LatencySummary {
        samples: sorted.len(),
        min_ns,
        avg_ns,
        p95_ns,
        max_ns,
    }
}

fn saturating_duration_ns(duration: Duration) -> u64 {
    duration.as_nanos().min(u64::MAX as u128) as u64
}

fn occupied_instruments(positions: &[Position], open_orders: &[Order]) -> BTreeSet<InstrumentId> {
    let mut occupied = BTreeSet::new();
    for position in positions {
        if position.size.value() > Decimal::ZERO && position.direction != PositionDirection::Flat {
            occupied.insert(position.instrument_id.clone());
        }
    }
    for order in open_orders {
        occupied.insert(order.instrument_id.clone());
    }
    occupied
}

fn preferred_live_rank(spec: &InstrumentSpec) -> usize {
    PREFERRED_LIVE_SYMBOLS
        .iter()
        .position(|symbol| spec.instrument_id.as_ref() == *symbol)
        .unwrap_or(PREFERRED_LIVE_SYMBOLS.len())
}

fn quantize_down(value: Decimal, step: Decimal, scale: u32) -> Decimal {
    let steps = (value / step).floor();
    (steps * step).round_dp_with_strategy(scale, RoundingStrategy::ToZero)
}

fn quantize_up(value: Decimal, step: Decimal, scale: u32) -> Decimal {
    let steps = (value / step).ceil();
    (steps * step).round_dp_with_strategy(scale, RoundingStrategy::AwayFromZero)
}

fn live_client_order_id(prefix: &str) -> ClientOrderId {
    ClientOrderId::from(format!("{prefix}-{}", chrono_suffix()))
}

fn chrono_suffix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis() as i64)
        .unwrap_or(0)
}

fn env_decimal(name: &str) -> Result<Option<Decimal>> {
    env::var(name)
        .ok()
        .map(|value| {
            value.trim().parse::<Decimal>().map_err(|error| {
                MarketError::new(ErrorKind::ConfigError, format!("invalid {name}: {error}"))
            })
        })
        .transpose()
}

fn env_usize(name: &str) -> Result<Option<usize>> {
    env::var(name)
        .ok()
        .map(|value| {
            value.trim().parse::<usize>().map_err(|error| {
                MarketError::new(ErrorKind::ConfigError, format!("invalid {name}: {error}"))
            })
        })
        .transpose()
}

fn synthetic_flat_position(instrument_id: &InstrumentId) -> Position {
    Position {
        position_id: PositionId::from(format!("synthetic-flat:{instrument_id}")),
        instrument_id: instrument_id.clone(),
        direction: PositionDirection::Flat,
        size: Quantity::new(Decimal::ZERO),
        entry_price: None,
        mark_price: None,
        unrealized_pnl: None,
        leverage: None,
        margin_mode: MarginMode::Cross,
        position_mode: PositionMode::OneWay,
        updated_at: timestamp_now_ms(),
    }
}

fn timestamp_now_ms() -> TimestampMs {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
        .min(i64::MAX as u128) as i64;
    TimestampMs::new(millis)
}

fn default_max_notional() -> Decimal {
    Decimal::new(12, 0)
}

fn notional_buffer_multiplier() -> Decimal {
    Decimal::new(105, 2)
}
