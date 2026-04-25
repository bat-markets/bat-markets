use std::{env, time::Duration};

use rust_decimal::{Decimal, RoundingStrategy};
use tokio::time::{Instant, sleep};

use bat_markets::{
    BatMarkets, BatMarketsBuilder, PublicSubscription,
    errors::Result,
    types::{
        CancelOrderRequest, CancelOrdersRequest, ClientOrderId, CommandStatus, CreateOrderRequest,
        CreateOrdersRequest, GetOrderRequest, InstrumentId, ListOpenOrdersRequest, OrderTarget,
        OrderType, Price, Quantity, Side, TimeInForce, ValidateOrderRequest, Venue,
    },
};
use bat_markets_core::{CommandTransport, InstrumentSpec, InstrumentStatus, VenueAdapter};
use bat_markets_testing::{
    LiveTestEndpointMode, has_binance_live_env, has_bybit_live_env, live_test_config,
    live_test_uses_sandbox,
};

const PREFERRED_SANDBOX_SYMBOLS: &[&str] = &["BTC/USDT:USDT", "ETH/USDT:USDT", "SOL/USDT:USDT"];

fn live_writes_enabled() -> bool {
    env::var_os("BAT_MARKETS_ENABLE_LIVE_WRITES").is_some()
        || env::var_os("BAT_MARKETS_ENABLE_SANDBOX_WRITES").is_some()
}

#[tokio::test]
async fn binance_sandbox_read_flows_are_env_gated() -> Result<()> {
    if !has_binance_live_env() {
        return Ok(());
    }

    let client = BatMarketsBuilder::default()
        .config(live_test_config(
            Venue::Binance,
            LiveTestEndpointMode::Sandbox,
        ))
        .build_live()
        .await?;

    assert_eq!(
        client
            .advanced()
            .native()
            .binance()?
            .config()
            .endpoints
            .sandbox,
        live_test_uses_sandbox(Venue::Binance, LiveTestEndpointMode::Sandbox)
    );

    let first = preferred_sandbox_instrument(&client);

    let public = client
        .stream()
        .public()
        .spawn_live(PublicSubscription::all_for(vec![first.clone()]))
        .await?;
    let private = client.stream().private().spawn_live().await?;

    wait_for_public_market_signal(
        &client,
        &first,
        Duration::from_secs(8),
        |client, instrument_id| {
            client.market().ticker(instrument_id).is_some()
                || client.market().book_top(instrument_id).is_some()
                || client.market().recent_trades(instrument_id).is_some()
        },
    )
    .await?;

    public.shutdown().await?;
    private.shutdown().await?;

    let _ = client.fetch_balance().await?;
    let _ = client.fetch_positions().await?;
    let _ = client.fetch_open_orders(None).await?;
    let _ = client.fetch_my_trades(None).await?;
    let _ = client.advanced().reconcile().await?;

    assert!(
        client.market().ticker(&first).is_some()
            || client.market().book_top(&first).is_some()
            || client.market().recent_trades(&first).is_some()
    );
    Ok(())
}

#[tokio::test]
async fn bybit_sandbox_read_flows_are_env_gated() -> Result<()> {
    if !has_bybit_live_env() {
        return Ok(());
    }

    let client = BatMarketsBuilder::default()
        .config(live_test_config(
            Venue::Bybit,
            LiveTestEndpointMode::Sandbox,
        ))
        .build_live()
        .await?;

    assert_eq!(
        client
            .advanced()
            .native()
            .bybit()?
            .config()
            .endpoints
            .sandbox,
        live_test_uses_sandbox(Venue::Bybit, LiveTestEndpointMode::Sandbox)
    );

    let first = preferred_sandbox_instrument(&client);

    let public = client
        .stream()
        .public()
        .spawn_live(PublicSubscription::all_for(vec![first.clone()]))
        .await?;
    let private = client.stream().private().spawn_live().await?;

    wait_for_public_market_signal(
        &client,
        &first,
        Duration::from_secs(8),
        |client, instrument_id| {
            client.market().ticker(instrument_id).is_some()
                || client.market().book_top(instrument_id).is_some()
                || client.market().open_interest(instrument_id).is_some()
        },
    )
    .await?;

    public.shutdown().await?;
    private.shutdown().await?;

    let _ = client.fetch_balance().await?;
    let _ = client.fetch_positions().await?;
    let _ = client.fetch_open_orders(None).await?;
    let _ = client.fetch_my_trades(None).await?;
    let _ = client.advanced().reconcile().await?;

    assert!(client.market().open_interest(&first).is_some());
    Ok(())
}

#[tokio::test]
async fn binance_sandbox_create_cancel_is_manual_and_safe() -> Result<()> {
    if !has_binance_live_env() || !live_writes_enabled() {
        return Ok(());
    }

    let client = BatMarketsBuilder::default()
        .config(live_test_config(
            Venue::Binance,
            LiveTestEndpointMode::Sandbox,
        ))
        .build_live()
        .await?;
    let Some((instrument_id, price, quantity)) = sandbox_order_parameters(&client).await? else {
        return Ok(());
    };

    assert_eq!(
        client
            .advanced()
            .native()
            .binance()?
            .config()
            .endpoints
            .sandbox,
        live_test_uses_sandbox(Venue::Binance, LiveTestEndpointMode::Sandbox)
    );

    let private = client.stream().private().spawn_live().await?;
    let mut create_handle = client
        .create_order(&CreateOrderRequest {
            request_id: None,
            instrument_id: instrument_id.clone(),
            client_order_id: Some(ClientOrderId::from(format!(
                "codex-binance-{}",
                chrono_suffix()
            ))),
            side: Side::Buy,
            order_type: OrderType::Limit,
            time_in_force: Some(TimeInForce::Gtc),
            quantity,
            price: Some(price),
            trigger_price: None,
            trigger_type: None,
            reduce_only: false,
            post_only: true,
        })
        .await?;
    let create = create_handle.receipt().await?;

    let order_id = create
        .order_id
        .clone()
        .expect("binance create order should surface order_id on accepted response");
    let client_order_id = create.client_order_id.clone();
    let fetched = client
        .fetch_order(&GetOrderRequest {
            request_id: None,
            instrument_id: instrument_id.clone(),
            order_id: Some(order_id),
            client_order_id: client_order_id.clone(),
        })
        .await?;
    assert_eq!(fetched.instrument_id, instrument_id);

    let mut cancel_handle = client
        .cancel_order(&CancelOrderRequest {
            request_id: None,
            instrument_id,
            order_id: Some(
                create
                    .order_id
                    .expect("cancel flow requires order_id from create response"),
            ),
            client_order_id,
        })
        .await?;
    let _ = cancel_handle.receipt().await?;
    let _ = client.fetch_my_trades(None).await?;
    let _ = client.advanced().reconcile().await?;

    private.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn bybit_sandbox_create_cancel_is_manual_and_safe() -> Result<()> {
    if !has_bybit_live_env() || !live_writes_enabled() {
        return Ok(());
    }

    let client = BatMarketsBuilder::default()
        .config(live_test_config(
            Venue::Bybit,
            LiveTestEndpointMode::Sandbox,
        ))
        .build_live()
        .await?;
    let Some((instrument_id, price, quantity)) = sandbox_order_parameters(&client).await? else {
        return Ok(());
    };

    assert_eq!(
        client
            .advanced()
            .native()
            .bybit()?
            .config()
            .endpoints
            .sandbox,
        live_test_uses_sandbox(Venue::Bybit, LiveTestEndpointMode::Sandbox)
    );

    let private = client.stream().private().spawn_live().await?;
    let mut create_handle = client
        .create_order(&CreateOrderRequest {
            request_id: None,
            instrument_id: instrument_id.clone(),
            client_order_id: Some(ClientOrderId::from(format!(
                "codex-bybit-{}",
                chrono_suffix()
            ))),
            side: Side::Buy,
            order_type: OrderType::Limit,
            time_in_force: Some(TimeInForce::Gtc),
            quantity,
            price: Some(price),
            trigger_price: None,
            trigger_type: None,
            reduce_only: false,
            post_only: true,
        })
        .await?;
    let create = create_handle.receipt().await?;

    let order_id = create
        .order_id
        .clone()
        .expect("bybit create order should surface order_id on accepted response");
    let client_order_id = create.client_order_id.clone();
    let fetched = client
        .fetch_order(&GetOrderRequest {
            request_id: None,
            instrument_id: instrument_id.clone(),
            order_id: Some(order_id),
            client_order_id: client_order_id.clone(),
        })
        .await?;
    assert_eq!(fetched.instrument_id, instrument_id);

    let mut cancel_handle = client
        .cancel_order(&CancelOrderRequest {
            request_id: None,
            instrument_id,
            order_id: create.order_id,
            client_order_id,
        })
        .await?;
    let _ = cancel_handle.receipt().await?;
    let _ = client.fetch_my_trades(None).await?;
    let _ = client.advanced().reconcile().await?;

    private.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn binance_sandbox_batch_validate_create_cancel_is_manual_and_safe() -> Result<()> {
    if !has_binance_live_env() || !live_writes_enabled() {
        return Ok(());
    }

    let client = BatMarketsBuilder::default()
        .config(live_test_config(
            Venue::Binance,
            LiveTestEndpointMode::Sandbox,
        ))
        .build_live()
        .await?;
    let Some((instrument_id, prices, quantity)) = sandbox_batch_order_parameters(&client).await?
    else {
        return Ok(());
    };

    assert_eq!(
        client
            .advanced()
            .native()
            .binance()?
            .config()
            .endpoints
            .sandbox,
        live_test_uses_sandbox(Venue::Binance, LiveTestEndpointMode::Sandbox)
    );

    let create_request = sandbox_batch_request(Venue::Binance, &instrument_id, prices, quantity);
    let cancel_request = sandbox_batch_cancel_request(&create_request);
    let client_order_ids = create_request
        .orders
        .iter()
        .filter_map(|order| order.client_order_id.clone())
        .collect::<Vec<_>>();

    let private = client.stream().private().spawn_live().await?;
    validate_batch_orders(&client, &create_request).await?;

    let create = client.create_orders(&create_request).await?;
    assert_eq!(create.len(), 2);
    assert!(
        create
            .iter()
            .all(|handle| handle.ack().receipt.status == CommandStatus::Accepted)
    );
    assert!(
        create
            .iter()
            .all(|handle| handle.ack().transport == CommandTransport::WebSocket)
    );

    await_open_order_state(&client, &instrument_id, &client_order_ids, true).await?;

    let cancel = client.cancel_orders(&cancel_request).await?;
    assert_eq!(cancel.len(), 2);
    assert!(
        cancel
            .iter()
            .all(|handle| handle.ack().receipt.status == CommandStatus::Accepted)
    );
    assert!(
        cancel
            .iter()
            .all(|handle| handle.ack().transport == CommandTransport::WebSocket)
    );

    await_open_order_state(&client, &instrument_id, &client_order_ids, false).await?;
    let _ = client.fetch_my_trades(None).await?;
    let _ = client.advanced().reconcile().await?;

    private.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn bybit_sandbox_batch_validate_create_cancel_is_manual_and_safe() -> Result<()> {
    if !has_bybit_live_env() || !live_writes_enabled() {
        return Ok(());
    }

    let client = BatMarketsBuilder::default()
        .config(live_test_config(
            Venue::Bybit,
            LiveTestEndpointMode::Sandbox,
        ))
        .build_live()
        .await?;
    let Some((instrument_id, prices, quantity)) = sandbox_batch_order_parameters(&client).await?
    else {
        return Ok(());
    };

    assert_eq!(
        client
            .advanced()
            .native()
            .bybit()?
            .config()
            .endpoints
            .sandbox,
        live_test_uses_sandbox(Venue::Bybit, LiveTestEndpointMode::Sandbox)
    );

    let create_request = sandbox_batch_request(Venue::Bybit, &instrument_id, prices, quantity);
    let cancel_request = sandbox_batch_cancel_request(&create_request);
    let client_order_ids = create_request
        .orders
        .iter()
        .filter_map(|order| order.client_order_id.clone())
        .collect::<Vec<_>>();

    let private = client.stream().private().spawn_live().await?;
    validate_batch_orders(&client, &create_request).await?;

    let create = client.create_orders(&create_request).await?;
    assert_eq!(create.len(), 2);
    assert!(
        create
            .iter()
            .all(|handle| handle.ack().receipt.status == CommandStatus::Accepted)
    );
    assert!(
        create
            .iter()
            .all(|handle| handle.ack().transport == CommandTransport::WebSocket)
    );

    await_open_order_state(&client, &instrument_id, &client_order_ids, true).await?;

    let cancel = client.cancel_orders(&cancel_request).await?;
    assert_eq!(cancel.len(), 2);
    assert!(
        cancel
            .iter()
            .all(|handle| handle.ack().receipt.status == CommandStatus::Accepted)
    );
    assert!(
        cancel
            .iter()
            .all(|handle| handle.ack().transport == CommandTransport::WebSocket)
    );

    await_open_order_state(&client, &instrument_id, &client_order_ids, false).await?;
    let _ = client.fetch_my_trades(None).await?;
    let _ = client.advanced().reconcile().await?;

    private.shutdown().await?;
    Ok(())
}

async fn sandbox_order_parameters(
    client: &BatMarkets,
) -> Result<Option<(InstrumentId, Price, Quantity)>> {
    let instrument_id = match env::var("BAT_MARKETS_SANDBOX_SYMBOL") {
        Ok(value) => InstrumentId::from(value),
        Err(_) => return autodiscover_sandbox_order_parameters(client).await,
    };
    let price = match env::var("BAT_MARKETS_SANDBOX_LIMIT_PRICE") {
        Ok(value) => value.parse::<Decimal>().map_err(|error| {
            bat_markets_core::MarketError::new(
                bat_markets_core::ErrorKind::ConfigError,
                format!("invalid BAT_MARKETS_SANDBOX_LIMIT_PRICE: {error}"),
            )
        })?,
        Err(_) => return Ok(None),
    };
    let quantity = match env::var("BAT_MARKETS_SANDBOX_QTY") {
        Ok(value) => value.parse::<Decimal>().map_err(|error| {
            bat_markets_core::MarketError::new(
                bat_markets_core::ErrorKind::ConfigError,
                format!("invalid BAT_MARKETS_SANDBOX_QTY: {error}"),
            )
        })?,
        Err(_) => return Ok(None),
    };

    Ok(Some((
        instrument_id,
        Price::new(price),
        Quantity::new(quantity),
    )))
}

async fn sandbox_batch_order_parameters(
    client: &BatMarkets,
) -> Result<Option<(InstrumentId, [Price; 2], Quantity)>> {
    let Some((instrument_id, first_price, quantity)) = sandbox_order_parameters(client).await?
    else {
        return Ok(None);
    };
    let spec = client
        .markets()
        .into_iter()
        .find(|spec| spec.instrument_id == instrument_id)
        .ok_or_else(|| {
            bat_markets_core::MarketError::new(
                bat_markets_core::ErrorKind::ConfigError,
                format!("missing spec for sandbox batch instrument {instrument_id}"),
            )
        })?;

    let tick = spec.tick_size.value();
    let fallback = quantize_down(
        (first_price.value() - tick * Decimal::new(2, 0)).max(tick),
        tick,
        spec.price_scale,
    );
    let second_price = if fallback <= Decimal::ZERO || fallback == first_price.value() {
        first_price
    } else {
        Price::new(fallback)
    };

    Ok(Some((instrument_id, [first_price, second_price], quantity)))
}

async fn autodiscover_sandbox_order_parameters(
    client: &BatMarkets,
) -> Result<Option<(InstrumentId, Price, Quantity)>> {
    let Some(account) = client.fetch_balance().await?.summary else {
        return Ok(None);
    };
    let available_balance = account.total_available_balance.value();
    if available_balance <= Decimal::ONE {
        return Ok(None);
    }

    let mut specs = client.markets();
    specs.sort_by(|left, right| {
        preferred_rank(left)
            .cmp(&preferred_rank(right))
            .then_with(|| left.min_notional.value().cmp(&right.min_notional.value()))
            .then_with(|| left.native_symbol.cmp(&right.native_symbol))
    });

    let max_budget = (available_balance * Decimal::new(35, 2)).min(Decimal::new(9, 0));
    for spec in specs {
        if spec.status != InstrumentStatus::Active || !spec.support.private_trading {
            continue;
        }
        if spec.min_notional.value() > max_budget {
            continue;
        }

        if let Some(parameters) = discover_spec_order_parameters(client, &spec, max_budget).await? {
            return Ok(Some(parameters));
        }
    }

    Ok(None)
}

async fn wait_for_public_market_signal(
    client: &BatMarkets,
    instrument_id: &InstrumentId,
    timeout: Duration,
    ready: impl Fn(&BatMarkets, &InstrumentId) -> bool,
) -> Result<()> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if ready(client, instrument_id) {
            return Ok(());
        }
        sleep(Duration::from_millis(200)).await;
    }

    Err(bat_markets_core::MarketError::new(
        bat_markets_core::ErrorKind::Timeout,
        format!("timed out waiting for public market signal for {instrument_id}"),
    ))
}

async fn discover_spec_order_parameters(
    client: &BatMarkets,
    spec: &InstrumentSpec,
    max_budget: Decimal,
) -> Result<Option<(InstrumentId, Price, Quantity)>> {
    let public = client
        .stream()
        .public()
        .spawn_live(PublicSubscription {
            instrument_ids: vec![spec.instrument_id.clone()],
            ticker: true,
            trades: false,
            book_top: true,
            order_book: false,
            mark_price: false,
            funding_rate: false,
            open_interest: false,
            liquidations: false,
            kline_intervals: Vec::new(),
        })
        .await?;
    sleep(Duration::from_secs(2)).await;
    public.shutdown().await?;

    let reference_price = client
        .market()
        .book_top(&spec.instrument_id)
        .map(|book| book.bid.price)
        .or_else(|| {
            client
                .market()
                .ticker(&spec.instrument_id)
                .map(|ticker| ticker.last_price)
        });
    let Some(reference_price) = reference_price else {
        return Ok(None);
    };

    let tick = spec.tick_size.value();
    let step = spec.step_size.value();
    if tick <= Decimal::ZERO || step <= Decimal::ZERO {
        return Ok(None);
    }

    let price_value = quantize_down(
        (reference_price.value() - tick * Decimal::new(5, 0)).max(tick),
        tick,
        spec.price_scale,
    );
    if price_value <= Decimal::ZERO {
        return Ok(None);
    }

    let min_qty = quantize_up(spec.min_qty.value(), step, spec.qty_scale);
    let required_for_notional = quantize_up(
        spec.min_notional.value() / price_value,
        step,
        spec.qty_scale,
    );
    let quantity_value = min_qty.max(required_for_notional);
    if quantity_value <= Decimal::ZERO || price_value * quantity_value > max_budget {
        return Ok(None);
    }

    Ok(Some((
        spec.instrument_id.clone(),
        Price::new(price_value),
        Quantity::new(quantity_value),
    )))
}

fn quantize_down(value: Decimal, step: Decimal, scale: u32) -> Decimal {
    let steps = (value / step).floor();
    (steps * step).round_dp_with_strategy(scale, RoundingStrategy::ToZero)
}

fn quantize_up(value: Decimal, step: Decimal, scale: u32) -> Decimal {
    let steps = (value / step).ceil();
    (steps * step).round_dp_with_strategy(scale, RoundingStrategy::AwayFromZero)
}

fn sandbox_batch_request(
    venue: Venue,
    instrument_id: &InstrumentId,
    prices: [Price; 2],
    quantity: Quantity,
) -> CreateOrdersRequest {
    let suffix = chrono_suffix();
    let prefix = match venue {
        Venue::Binance => "codex-binance-batch",
        Venue::Bybit => "codex-bybit-batch",
    };

    CreateOrdersRequest {
        request_id: None,
        orders: vec![
            CreateOrderRequest {
                request_id: None,
                instrument_id: instrument_id.clone(),
                client_order_id: Some(ClientOrderId::from(format!("{prefix}-{suffix}-1"))),
                side: Side::Buy,
                order_type: OrderType::Limit,
                time_in_force: Some(TimeInForce::Gtc),
                quantity,
                price: Some(prices[0]),
                trigger_price: None,
                trigger_type: None,
                reduce_only: false,
                post_only: true,
            },
            CreateOrderRequest {
                request_id: None,
                instrument_id: instrument_id.clone(),
                client_order_id: Some(ClientOrderId::from(format!("{prefix}-{suffix}-2"))),
                side: Side::Buy,
                order_type: OrderType::Limit,
                time_in_force: Some(TimeInForce::Gtc),
                quantity,
                price: Some(prices[1]),
                trigger_price: None,
                trigger_type: None,
                reduce_only: false,
                post_only: true,
            },
        ],
    }
}

fn sandbox_batch_cancel_request(request: &CreateOrdersRequest) -> CancelOrdersRequest {
    CancelOrdersRequest {
        request_id: None,
        orders: request
            .orders
            .iter()
            .map(|order| OrderTarget {
                instrument_id: order.instrument_id.clone(),
                order_id: None,
                client_order_id: order.client_order_id.clone(),
            })
            .collect(),
    }
}

async fn validate_batch_orders(client: &BatMarkets, request: &CreateOrdersRequest) -> Result<()> {
    for order in &request.orders {
        let handle = client
            .validate_order(&ValidateOrderRequest {
                request_id: None,
                order: order.clone(),
            })
            .await?;
        assert_eq!(handle.ack().receipt.status, CommandStatus::Accepted);
    }
    Ok(())
}

async fn await_open_order_state(
    client: &BatMarkets,
    instrument_id: &InstrumentId,
    client_order_ids: &[ClientOrderId],
    expected_present: bool,
) -> Result<()> {
    for _ in 0..20 {
        let orders = client
            .fetch_open_orders(Some(&ListOpenOrdersRequest {
                instrument_id: Some(instrument_id.clone()),
            }))
            .await?;
        let all_present = client_order_ids.iter().all(|client_order_id| {
            orders.iter().any(|order| {
                order.client_order_id.as_ref() == Some(client_order_id)
                    && order.instrument_id == *instrument_id
            })
        });
        let any_present = client_order_ids.iter().any(|client_order_id| {
            orders.iter().any(|order| {
                order.client_order_id.as_ref() == Some(client_order_id)
                    && order.instrument_id == *instrument_id
            })
        });
        if (expected_present && all_present) || (!expected_present && !any_present) {
            return Ok(());
        }
        sleep(Duration::from_millis(250)).await;
    }

    Err(bat_markets_core::MarketError::new(
        bat_markets_core::ErrorKind::TransportError,
        format!(
            "timed out waiting for sandbox open-order state expected_present={expected_present}"
        ),
    ))
}

fn preferred_sandbox_instrument(client: &BatMarkets) -> InstrumentId {
    let specs = client.markets();
    for symbol in PREFERRED_SANDBOX_SYMBOLS {
        if let Some(spec) = specs
            .iter()
            .find(|spec| spec.instrument_id.as_ref() == *symbol)
        {
            return spec.instrument_id.clone();
        }
    }

    specs
        .first()
        .expect("sandbox metadata bootstrap should populate instruments")
        .instrument_id
        .clone()
}

fn preferred_rank(spec: &InstrumentSpec) -> usize {
    PREFERRED_SANDBOX_SYMBOLS
        .iter()
        .position(|symbol| spec.instrument_id.as_ref() == *symbol)
        .unwrap_or(PREFERRED_SANDBOX_SYMBOLS.len())
}

fn chrono_suffix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis() as i64)
        .unwrap_or(0)
}
