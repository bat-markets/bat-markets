use std::{env, time::Duration};

use rust_decimal::{Decimal, RoundingStrategy};
use tokio::time::sleep;

use bat_markets::{
    BatMarkets, BatMarketsBuilder, PublicSubscription,
    errors::Result,
    types::{
        CancelOrderRequest, ClientOrderId, CreateOrderRequest, GetOrderRequest, InstrumentId,
        OrderType, Price, Product, Quantity, Side, TimeInForce, Venue,
    },
};
use bat_markets_core::{InstrumentSpec, InstrumentStatus, VenueAdapter};
use bat_markets_testing::{has_binance_live_env, has_bybit_live_env};

const PREFERRED_SANDBOX_SYMBOLS: &[&str] = &["BTC/USDT:USDT", "ETH/USDT:USDT", "SOL/USDT:USDT"];

#[tokio::test]
async fn binance_sandbox_read_flows_are_env_gated() -> Result<()> {
    if !has_binance_live_env() {
        return Ok(());
    }

    let client = BatMarkets::builder()
        .venue(Venue::Binance)
        .product(Product::LinearUsdt)
        .build_live()
        .await?;

    assert!(client.native().binance()?.config().endpoints.sandbox);

    let first = preferred_sandbox_instrument(&client);

    let public = client
        .stream()
        .public()
        .spawn_live(PublicSubscription::all_for(vec![first.clone()]))
        .await?;
    let private = client.stream().private().spawn_live().await?;

    sleep(Duration::from_secs(2)).await;

    public.shutdown().await?;
    private.shutdown().await?;

    let _ = client.account().refresh().await?;
    let _ = client.position().refresh().await?;
    let _ = client.trade().refresh_open_orders(None).await?;
    let _ = client.trade().refresh_executions(None).await?;
    let _ = client.stream().private().reconcile().await?;

    assert!(client.market().ticker(&first).is_some() || client.market().book_top(&first).is_some());
    Ok(())
}

#[tokio::test]
async fn bybit_sandbox_read_flows_are_env_gated() -> Result<()> {
    if !has_bybit_live_env() {
        return Ok(());
    }

    let client = BatMarkets::builder()
        .venue(Venue::Bybit)
        .product(Product::LinearUsdt)
        .build_live()
        .await?;

    assert!(client.native().bybit()?.config().endpoints.sandbox);

    let first = preferred_sandbox_instrument(&client);

    let public = client
        .stream()
        .public()
        .spawn_live(PublicSubscription::all_for(vec![first.clone()]))
        .await?;
    let private = client.stream().private().spawn_live().await?;

    sleep(Duration::from_secs(2)).await;

    public.shutdown().await?;
    private.shutdown().await?;

    let _ = client.account().refresh().await?;
    let _ = client.position().refresh().await?;
    let _ = client.trade().refresh_open_orders(None).await?;
    let _ = client.trade().refresh_executions(None).await?;
    let _ = client.stream().private().reconcile().await?;

    assert!(client.market().open_interest(&first).is_some());
    Ok(())
}

#[tokio::test]
async fn binance_sandbox_create_cancel_is_manual_and_safe() -> Result<()> {
    if !has_binance_live_env() || env::var_os("BAT_MARKETS_ENABLE_SANDBOX_WRITES").is_none() {
        return Ok(());
    }

    let client = BatMarketsBuilder::default()
        .venue(Venue::Binance)
        .product(Product::LinearUsdt)
        .build_live()
        .await?;
    let Some((instrument_id, price, quantity)) = sandbox_order_parameters(&client).await? else {
        return Ok(());
    };

    assert!(client.native().binance()?.config().endpoints.sandbox);

    let private = client.stream().private().spawn_live().await?;
    let create = client
        .trade()
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
            reduce_only: false,
            post_only: true,
        })
        .await?;

    let order_id = create
        .order_id
        .clone()
        .expect("binance create order should surface order_id on accepted response");
    let client_order_id = create.client_order_id.clone();
    let fetched = client
        .trade()
        .get_order(&GetOrderRequest {
            request_id: None,
            instrument_id: instrument_id.clone(),
            order_id: Some(order_id),
            client_order_id: client_order_id.clone(),
        })
        .await?;
    assert_eq!(fetched.instrument_id, instrument_id);

    let _ = client
        .trade()
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
    let _ = client.trade().refresh_executions(None).await?;
    let _ = client.stream().private().reconcile().await?;

    private.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn bybit_sandbox_create_cancel_is_manual_and_safe() -> Result<()> {
    if !has_bybit_live_env() || env::var_os("BAT_MARKETS_ENABLE_SANDBOX_WRITES").is_none() {
        return Ok(());
    }

    let client = BatMarketsBuilder::default()
        .venue(Venue::Bybit)
        .product(Product::LinearUsdt)
        .build_live()
        .await?;
    let Some((instrument_id, price, quantity)) = sandbox_order_parameters(&client).await? else {
        return Ok(());
    };

    assert!(client.native().bybit()?.config().endpoints.sandbox);

    let private = client.stream().private().spawn_live().await?;
    let create = client
        .trade()
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
            reduce_only: false,
            post_only: true,
        })
        .await?;

    let order_id = create
        .order_id
        .clone()
        .expect("bybit create order should surface order_id on accepted response");
    let client_order_id = create.client_order_id.clone();
    let fetched = client
        .trade()
        .get_order(&GetOrderRequest {
            request_id: None,
            instrument_id: instrument_id.clone(),
            order_id: Some(order_id),
            client_order_id: client_order_id.clone(),
        })
        .await?;
    assert_eq!(fetched.instrument_id, instrument_id);

    let _ = client
        .trade()
        .cancel_order(&CancelOrderRequest {
            request_id: None,
            instrument_id,
            order_id: create.order_id,
            client_order_id,
        })
        .await?;
    let _ = client.trade().refresh_executions(None).await?;
    let _ = client.stream().private().reconcile().await?;

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

async fn autodiscover_sandbox_order_parameters(
    client: &BatMarkets,
) -> Result<Option<(InstrumentId, Price, Quantity)>> {
    let Some(account) = client.account().refresh().await? else {
        return Ok(None);
    };
    let available_balance = account.total_available_balance.value();
    if available_balance <= Decimal::ONE {
        return Ok(None);
    }

    let mut specs = client.market().instrument_specs();
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
            funding_rate: false,
            open_interest: false,
            kline_interval: None,
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

fn preferred_sandbox_instrument(client: &BatMarkets) -> InstrumentId {
    let specs = client.market().instrument_specs();
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
