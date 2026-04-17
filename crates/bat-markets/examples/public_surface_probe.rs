use std::{
    env,
    time::{Duration, Instant},
};

use bat_markets::{
    BatMarketsBuilder, WatchInstrumentsRequest, WatchOhlcvRequest,
    config::{AuthConfig, BatMarketsConfig, EndpointConfig},
    errors::{ErrorKind, MarketError, Result},
    types::{
        FetchOhlcvRequest, FetchTradesRequest, InstrumentId, InstrumentStatus, Kline, Product,
        TimestampMs, Venue,
    },
};

const WATCH_TIMEOUT: Duration = Duration::from_secs(60);
const OHLCV_LIMIT: usize = 10;
const TRADE_LIMIT: usize = 10;

const PREFERRED_SYMBOLS: &[&str] = &["BTC/USDT:USDT", "ETH/USDT:USDT", "SOL/USDT:USDT"];

#[tokio::main]
async fn main() -> Result<()> {
    let venue = parse_venue()?;
    println!("public probe start venue={venue:?}");

    let client = BatMarketsBuilder::default()
        .config(BatMarketsConfig {
            venue,
            product: Product::LinearUsdt,
            auth: AuthConfig::Env {
                api_key_var: "__BAT_MARKETS_UNUSED_PUBLIC_KEY__".into(),
                api_secret_var: "__BAT_MARKETS_UNUSED_PUBLIC_SECRET__".into(),
            },
            endpoints: EndpointConfig::mainnet_defaults(venue),
            ..BatMarketsConfig::new(venue, Product::LinearUsdt)
        })
        .build_live()
        .await?;

    let instruments = preferred_symbols(&client);
    let primary = instruments
        .first()
        .cloned()
        .ok_or_else(|| MarketError::new(ErrorKind::Unsupported, "no active instruments found"))?;
    let secondary = instruments
        .get(1)
        .cloned()
        .unwrap_or_else(|| primary.clone());

    println!(
        "metadata venue={venue:?} instruments={} primary={} secondary={}",
        client.market().instrument_specs().len(),
        primary,
        secondary,
    );

    probe_fetches(&client, &primary).await?;
    probe_watches(&client, &[primary, secondary]).await?;

    println!("public probe done venue={venue:?}");
    Ok(())
}

async fn probe_fetches(client: &bat_markets::BatMarkets, instrument: &InstrumentId) -> Result<()> {
    let ticker = client.market().fetch_ticker(instrument).await?;
    println!(
        "fetch_ticker instrument={} last={} mark={} index={} volume_24h={} turnover_24h={} event_time={}",
        ticker.instrument_id,
        ticker.last_price,
        ticker
            .mark_price
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| "-".into()),
        ticker
            .index_price
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| "-".into()),
        ticker
            .volume_24h
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| "-".into()),
        ticker
            .turnover_24h
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| "-".into()),
        ticker.event_time.value(),
    );

    let trades = client
        .market()
        .fetch_trades(&FetchTradesRequest::new(
            instrument.clone(),
            Some(TRADE_LIMIT),
        ))
        .await?;
    let first_trade = trades.first().ok_or_else(|| {
        MarketError::new(ErrorKind::TransportError, "fetch_trades returned no trades")
    })?;
    let last_trade = trades.last().ok_or_else(|| {
        MarketError::new(ErrorKind::TransportError, "fetch_trades returned no trades")
    })?;
    println!(
        "fetch_trades instrument={} count={} first=(id:{} side:{:?} px:{} qty:{} t:{}) last=(id:{} side:{:?} px:{} qty:{} t:{})",
        instrument,
        trades.len(),
        first_trade.trade_id,
        first_trade.aggressor_side,
        first_trade.price,
        first_trade.quantity,
        first_trade.event_time.value(),
        last_trade.trade_id,
        last_trade.aggressor_side,
        last_trade.price,
        last_trade.quantity,
        last_trade.event_time.value(),
    );

    let book_top = client.market().fetch_book_top(instrument).await?;
    println!(
        "fetch_book_top instrument={} bid=({},{}) ask=({},{}) event_time={}",
        book_top.instrument_id,
        book_top.bid.price,
        book_top.bid.quantity,
        book_top.ask.price,
        book_top.ask.quantity,
        book_top.event_time.value(),
    );

    let end_ms = timestamp_floor_minute_ms() - 1;
    let start_ms = end_ms - ((OHLCV_LIMIT as i64 - 1) * 60_000);
    let ohlcv = client
        .market()
        .fetch_ohlcv(&FetchOhlcvRequest::for_instrument(
            instrument.clone(),
            "1m",
            Some(TimestampMs::new(start_ms)),
            Some(TimestampMs::new(end_ms)),
            Some(OHLCV_LIMIT),
        ))
        .await?;
    let first = normalized_candles(&ohlcv).first().cloned().ok_or_else(|| {
        MarketError::new(ErrorKind::TransportError, "fetch_ohlcv returned no candles")
    })?;
    let last = normalized_candles(&ohlcv).last().cloned().ok_or_else(|| {
        MarketError::new(ErrorKind::TransportError, "fetch_ohlcv returned no candles")
    })?;
    println!(
        "fetch_ohlcv instrument={} count={} first=(open:{} close:{} t:{}..{} closed:{}) last=(open:{} close:{} t:{}..{} closed:{})",
        instrument,
        ohlcv.len(),
        first.open,
        first.close,
        first.open_time.value(),
        first.close_time.value(),
        first.closed,
        last.open,
        last.close,
        last.open_time.value(),
        last.close_time.value(),
        last.closed,
    );

    Ok(())
}

async fn probe_watches(
    client: &bat_markets::BatMarkets,
    instruments: &[InstrumentId],
) -> Result<()> {
    let mut ticker_watch = client
        .stream()
        .public()
        .watch_ticker(WatchInstrumentsRequest::for_instruments(
            instruments.to_vec(),
        ))
        .await?;
    let mut trades_watch = client
        .stream()
        .public()
        .watch_trades(WatchInstrumentsRequest::for_instruments(
            instruments.to_vec(),
        ))
        .await?;
    let mut book_top_watch = client
        .stream()
        .public()
        .watch_book_top(WatchInstrumentsRequest::for_instruments(
            instruments.to_vec(),
        ))
        .await?;
    let mut ohlcv_watch = client
        .stream()
        .public()
        .watch_ohlcv(WatchOhlcvRequest::for_instruments(
            instruments.to_vec(),
            "1m",
        ))
        .await?;

    let started_at = Instant::now();
    let ticker = await_update("ticker", ticker_watch.recv()).await?;
    println!(
        "watch_ticker instrument={} last={} event_time={} elapsed_ms={}",
        ticker.instrument_id,
        ticker.last_price,
        ticker.event_time.value(),
        started_at.elapsed().as_millis(),
    );
    let trade = await_update("trade", trades_watch.recv()).await?;
    println!(
        "watch_trades instrument={} trade_id={} side={:?} px={} qty={} event_time={} elapsed_ms={}",
        trade.instrument_id,
        trade.trade_id,
        trade.aggressor_side,
        trade.price,
        trade.quantity,
        trade.event_time.value(),
        started_at.elapsed().as_millis(),
    );
    let book_top = await_update("book_top", book_top_watch.recv()).await?;
    println!(
        "watch_book_top instrument={} bid=({},{}) ask=({},{}) event_time={} elapsed_ms={}",
        book_top.instrument_id,
        book_top.bid.price,
        book_top.bid.quantity,
        book_top.ask.price,
        book_top.ask.quantity,
        book_top.event_time.value(),
        started_at.elapsed().as_millis(),
    );
    let candle = await_update("ohlcv", ohlcv_watch.recv()).await?;
    println!(
        "watch_ohlcv instrument={} interval={} open={} close={} t:{}..{} closed={} elapsed_ms={}",
        candle.instrument_id,
        candle.interval,
        candle.open,
        candle.close,
        candle.open_time.value(),
        candle.close_time.value(),
        candle.closed,
        started_at.elapsed().as_millis(),
    );

    ticker_watch.shutdown().await?;
    trades_watch.shutdown().await?;
    book_top_watch.shutdown().await?;
    ohlcv_watch.shutdown().await?;
    Ok(())
}

async fn await_update<T>(
    label: &str,
    future: impl std::future::Future<Output = Result<T>>,
) -> Result<T> {
    tokio::time::timeout(WATCH_TIMEOUT, future)
        .await
        .map_err(|error| {
            MarketError::new(
                ErrorKind::Timeout,
                format!("timed out waiting for {label} update: {error}"),
            )
        })?
}

fn preferred_symbols(client: &bat_markets::BatMarkets) -> Vec<InstrumentId> {
    let specs = client.market().instrument_specs();
    let mut chosen = Vec::new();

    for symbol in PREFERRED_SYMBOLS {
        if let Some(spec) = specs.iter().find(|spec| {
            spec.instrument_id.as_ref() == *symbol && spec.status == InstrumentStatus::Active
        }) {
            chosen.push(spec.instrument_id.clone());
        }
    }

    if chosen.is_empty()
        && let Some(spec) = specs
            .iter()
            .find(|spec| spec.status == InstrumentStatus::Active)
    {
        chosen.push(spec.instrument_id.clone());
    }

    chosen
}

fn normalized_candles(candles: &[Kline]) -> Vec<Kline> {
    let mut normalized = candles.to_vec();
    normalized.sort_by_key(|candle| candle.open_time.value());
    normalized.dedup_by_key(|candle| candle.open_time);
    normalized
}

fn parse_venue() -> Result<Venue> {
    match env::var("BAT_MARKETS_PROBE_VENUE")
        .unwrap_or_else(|_| "binance".into())
        .to_ascii_lowercase()
        .as_str()
    {
        "binance" => Ok(Venue::Binance),
        "bybit" => Ok(Venue::Bybit),
        other => Err(MarketError::new(
            ErrorKind::ConfigError,
            format!("unsupported BAT_MARKETS_PROBE_VENUE '{other}'"),
        )),
    }
}

fn timestamp_floor_minute_ms() -> i64 {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time should be after epoch")
        .as_millis() as i64;
    (now_ms / 60_000) * 60_000
}
