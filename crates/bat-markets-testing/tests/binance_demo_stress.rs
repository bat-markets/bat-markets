use std::{env, time::Duration};

use rust_decimal::{Decimal, RoundingStrategy};
use tokio::time::{Instant, sleep};

use bat_markets::{
    BatMarkets, BatMarketsBuilder, PublicSubscription,
    errors::Result,
    types::{
        CancelOrderRequest, ClientOrderId, CreateOrderRequest, GetOrderRequest, OrderType, Price,
        Product, Quantity, Side, TimeInForce, Venue,
    },
};
use bat_markets_core::{InstrumentSpec, InstrumentStatus, MarketError, VenueAdapter};
use bat_markets_testing::has_binance_live_env;

const PREFERRED_SYMBOLS: &[&str] = &["BTC/USDT:USDT", "ETH/USDT:USDT", "SOL/USDT:USDT"];

#[tokio::test]
async fn binance_demo_create_cancel_stress_is_stable() -> Result<()> {
    if !has_binance_live_env() || env::var_os("BAT_MARKETS_ENABLE_SANDBOX_STRESS").is_none() {
        return Ok(());
    }

    let iterations = env::var("BAT_MARKETS_STRESS_ITERS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(8)
        .max(1);

    let client = BatMarketsBuilder::default()
        .venue(Venue::Binance)
        .product(Product::LinearUsdt)
        .build_live()
        .await?;

    assert!(client.native().binance()?.config().endpoints.sandbox);

    let spec = preferred_stress_spec(&client)?;
    let public = client
        .stream()
        .public()
        .spawn_live(PublicSubscription {
            instrument_ids: vec![spec.instrument_id.clone()],
            ticker: true,
            trades: false,
            book_top: true,
            funding_rate: false,
            open_interest: false,
            kline_interval: None,
        })
        .await?;
    let private = client.stream().private().spawn_live().await?;

    wait_for_live_public_price(&client, &spec).await?;

    let mut create_latencies = Vec::with_capacity(iterations);
    let mut get_latencies = Vec::with_capacity(iterations);
    let mut cancel_latencies = Vec::with_capacity(iterations);
    let mut reconcile_latencies = Vec::with_capacity(iterations);
    let mut failures = Vec::new();

    for iteration in 0..iterations {
        let (price, quantity) = stress_order_parameters(&client, &spec).await?;
        let client_order_id =
            ClientOrderId::from(format!("codex-stress-{iteration}-{}", chrono_suffix()));

        let create_started = Instant::now();
        let create = client
            .trade()
            .create_order(&CreateOrderRequest {
                request_id: None,
                instrument_id: spec.instrument_id.clone(),
                client_order_id: Some(client_order_id.clone()),
                side: Side::Buy,
                order_type: OrderType::Limit,
                time_in_force: Some(TimeInForce::Gtc),
                quantity,
                price: Some(price),
                reduce_only: false,
                post_only: true,
            })
            .await;
        create_latencies.push(create_started.elapsed());

        let create = match create {
            Ok(create) => create,
            Err(error) => {
                failures.push(format!("iteration {iteration}: create failed: {error}"));
                continue;
            }
        };

        let Some(order_id) = create.order_id.clone() else {
            failures.push(format!(
                "iteration {iteration}: create response missing order_id for {}",
                client_order_id
            ));
            continue;
        };

        let get_started = Instant::now();
        let order = client
            .trade()
            .get_order(&GetOrderRequest {
                request_id: None,
                instrument_id: spec.instrument_id.clone(),
                order_id: Some(order_id.clone()),
                client_order_id: Some(client_order_id.clone()),
            })
            .await;
        get_latencies.push(get_started.elapsed());

        if let Err(error) = order {
            failures.push(format!("iteration {iteration}: get_order failed: {error}"));
            continue;
        }

        let cancel_started = Instant::now();
        let cancel = client
            .trade()
            .cancel_order(&CancelOrderRequest {
                request_id: None,
                instrument_id: spec.instrument_id.clone(),
                order_id: Some(order_id.clone()),
                client_order_id: Some(client_order_id.clone()),
            })
            .await;
        cancel_latencies.push(cancel_started.elapsed());

        if let Err(error) = cancel {
            failures.push(format!("iteration {iteration}: cancel failed: {error}"));
            continue;
        }

        let reconcile_started = Instant::now();
        let _ = client.trade().refresh_open_orders(None).await?;
        let _ = client.trade().refresh_executions(None).await?;
        let report = client.stream().private().reconcile().await?;
        reconcile_latencies.push(reconcile_started.elapsed());

        let terminal = client
            .trade()
            .get_order(&GetOrderRequest {
                request_id: None,
                instrument_id: spec.instrument_id.clone(),
                order_id: Some(order_id),
                client_order_id: Some(client_order_id.clone()),
            })
            .await?;

        println!(
            "iter={iteration} create_ms={} get_ms={} cancel_ms={} reconcile_ms={} order_status={:?} reconcile_outcome={:?}",
            create_latencies
                .last()
                .expect("create latency present")
                .as_millis(),
            get_latencies
                .last()
                .expect("get latency present")
                .as_millis(),
            cancel_latencies
                .last()
                .expect("cancel latency present")
                .as_millis(),
            reconcile_latencies
                .last()
                .expect("reconcile latency present")
                .as_millis(),
            terminal.status,
            report.outcome,
        );
    }

    let _ = client.trade().refresh_open_orders(None).await?;
    let _ = client.trade().refresh_executions(None).await?;
    let _ = client.stream().private().reconcile().await?;
    let final_open_orders = client.trade().open_orders();

    public.shutdown().await?;
    private.shutdown().await?;

    println!(
        "summary instrument={} iterations={} final_open_orders={} create_p95_ms={} cancel_p95_ms={} reconcile_p95_ms={}",
        spec.instrument_id,
        iterations,
        final_open_orders.len(),
        p95_millis(&create_latencies),
        p95_millis(&cancel_latencies),
        p95_millis(&reconcile_latencies),
    );

    assert!(
        failures.is_empty(),
        "stress failures encountered:\n{}",
        failures.join("\n")
    );
    assert!(
        final_open_orders.is_empty(),
        "stress run left open orders behind: {:?}",
        final_open_orders
    );

    Ok(())
}

fn preferred_stress_spec(client: &BatMarkets) -> Result<InstrumentSpec> {
    let specs = client.market().instrument_specs();
    for symbol in PREFERRED_SYMBOLS {
        if let Some(spec) = specs
            .iter()
            .find(|spec| spec.instrument_id.as_ref() == *symbol)
        {
            return Ok(spec.clone());
        }
    }

    specs.into_iter().next().ok_or_else(|| {
        MarketError::new(
            bat_markets_core::ErrorKind::Unsupported,
            "sandbox metadata bootstrap returned no instruments",
        )
    })
}

async fn stress_order_parameters(
    client: &BatMarkets,
    spec: &InstrumentSpec,
) -> Result<(Price, Quantity)> {
    if spec.status != InstrumentStatus::Active || !spec.support.private_trading {
        return Err(MarketError::new(
            bat_markets_core::ErrorKind::Unsupported,
            format!(
                "instrument {} is not tradable in stress harness",
                spec.instrument_id
            ),
        ));
    }

    let _ = client.account().refresh().await?;
    let reference_price = client
        .market()
        .book_top(&spec.instrument_id)
        .map(|book| book.bid.price)
        .or_else(|| {
            client
                .market()
                .ticker(&spec.instrument_id)
                .map(|ticker| ticker.last_price)
        })
        .ok_or_else(|| {
            MarketError::new(
                bat_markets_core::ErrorKind::TransportError,
                format!("missing live public price for {}", spec.instrument_id),
            )
        })?;

    let tick = spec.tick_size.value();
    let step = spec.step_size.value();
    let price_value = quantize_down(
        (reference_price.value() - tick * Decimal::new(10, 0)).max(tick),
        tick,
        spec.price_scale,
    );
    let min_qty = quantize_up(spec.min_qty.value(), step, spec.qty_scale);
    let required_for_notional = quantize_up(
        spec.min_notional.value() / price_value,
        step,
        spec.qty_scale,
    );
    let quantity_value = min_qty.max(required_for_notional);

    Ok((Price::new(price_value), Quantity::new(quantity_value)))
}

async fn wait_for_live_public_price(client: &BatMarkets, spec: &InstrumentSpec) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if client.market().book_top(&spec.instrument_id).is_some()
            || client.market().ticker(&spec.instrument_id).is_some()
        {
            return Ok(());
        }

        if Instant::now() >= deadline {
            return Err(MarketError::new(
                bat_markets_core::ErrorKind::TransportError,
                format!(
                    "timed out waiting for live public price for {}",
                    spec.instrument_id
                ),
            ));
        }

        sleep(Duration::from_millis(250)).await;
    }
}

fn quantize_down(value: Decimal, step: Decimal, scale: u32) -> Decimal {
    let steps = (value / step).floor();
    (steps * step).round_dp_with_strategy(scale, RoundingStrategy::ToZero)
}

fn quantize_up(value: Decimal, step: Decimal, scale: u32) -> Decimal {
    let steps = (value / step).ceil();
    (steps * step).round_dp_with_strategy(scale, RoundingStrategy::AwayFromZero)
}

fn p95_millis(samples: &[Duration]) -> u128 {
    if samples.is_empty() {
        return 0;
    }

    let mut values = samples.iter().map(Duration::as_millis).collect::<Vec<_>>();
    values.sort_unstable();
    let index = ((values.len() * 95).saturating_sub(1) / 100).min(values.len() - 1);
    values[index]
}

fn chrono_suffix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis() as i64)
        .unwrap_or(0)
}
