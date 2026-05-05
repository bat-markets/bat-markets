use std::{env, time::Duration};

use tokio::time::{sleep, timeout};

use bat_markets::{
    BatMarketsBuilder,
    errors::Result,
    types::{InstrumentId, Venue},
};
use bat_markets_core::{ErrorKind, MarketError, VenueAdapter};
use bat_markets_testing::{
    LiveTestEndpointMode, has_binance_live_env, has_bybit_live_env, has_mexc_live_env,
    live_test_config,
};

#[tokio::test]
async fn binance_mainnet_read_flows_are_manual_and_read_only() -> Result<()> {
    if !has_binance_live_env() || env::var_os("BAT_MARKETS_ENABLE_MAINNET_READS").is_none() {
        return Ok(());
    }

    let config = live_test_config(Venue::Binance, LiveTestEndpointMode::Mainnet);
    let client = BatMarketsBuilder::default()
        .config(config)
        .build_live()
        .await?;

    assert!(
        !client
            .advanced()
            .native()
            .binance()?
            .config()
            .endpoints
            .sandbox
    );
    let instrument = preferred_mainnet_instrument(&client);
    let public = client.watch_ticker(instrument.clone()).await?;
    let private = client.watch_balance().await?;

    sleep(Duration::from_secs(3)).await;

    public.shutdown().await?;
    private.shutdown().await?;

    let ticker = client.fetch_ticker(&instrument).await?;
    let trades = client.fetch_trades(&instrument, Some(10)).await?;
    let order_book = client.fetch_order_book(&instrument, Some(5)).await?;
    let _ = client.fetch_open_interest(&instrument).await?;
    let _ = client.fetch_balance().await?;
    let _ = client.fetch_positions().await?;
    let _ = client.fetch_open_orders(None).await?;
    let _ = client.fetch_my_trades(None).await?;
    let _ = client.advanced().reconcile().await?;

    let mut ticker_watch = client.watch_ticker(instrument.clone()).await?;
    let mut trades_watch = client.watch_trades(instrument.clone()).await?;
    let mut order_book_watch = client
        .watch_order_book(instrument.clone(), Some(50))
        .await?;
    let _ = await_live_update("ticker", ticker_watch.recv()).await?;
    let _ = await_live_update("trade", trades_watch.recv()).await?;
    let _ = await_live_update("order_book", order_book_watch.recv()).await?;
    ticker_watch.shutdown().await?;
    trades_watch.shutdown().await?;
    order_book_watch.shutdown().await?;

    assert_eq!(ticker.instrument_id, instrument);
    assert!(!trades.is_empty());
    assert_eq!(order_book.instrument_id, instrument);
    assert!(
        client
            .advanced()
            .cached_open_interest(&instrument)
            .is_some()
    );
    Ok(())
}

#[tokio::test]
async fn bybit_mainnet_read_flows_are_manual_and_read_only() -> Result<()> {
    if !has_bybit_live_env() || env::var_os("BAT_MARKETS_ENABLE_MAINNET_READS").is_none() {
        return Ok(());
    }

    let config = live_test_config(Venue::Bybit, LiveTestEndpointMode::Mainnet);
    let client = BatMarketsBuilder::default()
        .config(config)
        .build_live()
        .await?;

    assert!(
        !client
            .advanced()
            .native()
            .bybit()?
            .config()
            .endpoints
            .sandbox
    );
    let instrument = preferred_mainnet_instrument(&client);
    let public = client.watch_ticker(instrument.clone()).await?;
    let private = client.watch_balance().await?;

    sleep(Duration::from_secs(3)).await;

    public.shutdown().await?;
    private.shutdown().await?;

    let ticker = client.fetch_ticker(&instrument).await?;
    let trades = client.fetch_trades(&instrument, Some(10)).await?;
    let order_book = client.fetch_order_book(&instrument, Some(5)).await?;
    let _ = client.fetch_open_interest(&instrument).await?;
    let _ = client.fetch_balance().await?;
    let _ = client.fetch_positions().await?;
    let _ = client.fetch_open_orders(None).await?;
    let _ = client.fetch_my_trades(None).await?;
    let _ = client.advanced().reconcile().await?;

    let mut ticker_watch = client.watch_ticker(instrument.clone()).await?;
    let mut trades_watch = client.watch_trades(instrument.clone()).await?;
    let mut order_book_watch = client
        .watch_order_book(instrument.clone(), Some(50))
        .await?;
    let _ = await_live_update("ticker", ticker_watch.recv()).await?;
    let _ = await_live_update("trade", trades_watch.recv()).await?;
    let _ = await_live_update("order_book", order_book_watch.recv()).await?;
    ticker_watch.shutdown().await?;
    trades_watch.shutdown().await?;
    order_book_watch.shutdown().await?;

    assert_eq!(ticker.instrument_id, instrument);
    assert!(!trades.is_empty());
    assert_eq!(order_book.instrument_id, instrument);
    assert!(
        client
            .advanced()
            .cached_open_interest(&instrument)
            .is_some()
    );
    Ok(())
}

#[tokio::test]
async fn mexc_mainnet_read_flows_are_manual_and_read_only() -> Result<()> {
    if !has_mexc_live_env() || env::var_os("BAT_MARKETS_ENABLE_MAINNET_READS").is_none() {
        return Ok(());
    }

    let config = live_test_config(Venue::Mexc, LiveTestEndpointMode::Mainnet);
    let client = BatMarketsBuilder::default()
        .config(config)
        .build_live()
        .await?;

    assert!(
        !client
            .advanced()
            .native()
            .mexc()?
            .config()
            .endpoints
            .sandbox
    );
    let instrument = preferred_mainnet_instrument(&client);
    let public = client.watch_ticker(instrument.clone()).await?;
    let private = client.watch_balance().await?;

    sleep(Duration::from_secs(3)).await;

    public.shutdown().await?;
    private.shutdown().await?;

    let ticker = client.fetch_ticker(&instrument).await?;
    let trades = client.fetch_trades(&instrument, Some(10)).await?;
    let order_book = client.fetch_order_book(&instrument, Some(5)).await?;
    let _ = client.fetch_open_interest(&instrument).await?;
    let _ = client.fetch_balance().await?;
    let _ = client.fetch_positions().await?;
    let _ = client.fetch_open_orders(None).await?;
    let _ = client.fetch_my_trades(None).await?;
    let _ = client.advanced().reconcile().await?;

    let mut order_book_watch = client
        .watch_order_book(instrument.clone(), Some(50))
        .await?;
    // MEXC ticker pushes are documented as once per second only when trades occur;
    // depth is the deterministic public-stream smoke path because it pushes every 200ms.
    let _ = await_live_update("order_book", order_book_watch.recv()).await?;
    order_book_watch.shutdown().await?;

    assert_eq!(ticker.instrument_id, instrument);
    assert!(!trades.is_empty());
    assert_eq!(order_book.instrument_id, instrument);
    assert!(
        client
            .advanced()
            .cached_open_interest(&instrument)
            .is_some()
    );
    Ok(())
}

fn preferred_mainnet_instrument(client: &bat_markets::BatMarkets) -> InstrumentId {
    const PREFERRED: &[&str] = &["BTC/USDT:USDT", "ETH/USDT:USDT", "SOL/USDT:USDT"];

    let specs = client.markets();
    for symbol in PREFERRED {
        if let Some(spec) = specs
            .iter()
            .find(|spec| spec.instrument_id.as_ref() == *symbol)
        {
            return spec.instrument_id.clone();
        }
    }

    specs
        .first()
        .expect("mainnet metadata should populate at least one instrument")
        .instrument_id
        .clone()
}

async fn await_live_update<T>(
    label: &str,
    future: impl std::future::Future<Output = Result<T>>,
) -> Result<T> {
    timeout(Duration::from_secs(5), future)
        .await
        .map_err(|error| {
            MarketError::new(
                ErrorKind::TransportError,
                format!("timed out waiting for live {label} update: {error}"),
            )
        })?
}
