use std::{
    collections::BTreeSet,
    env,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use tokio::{
    task::JoinSet,
    time::{Instant, sleep, timeout},
};

use bat_markets::{
    BatMarkets, BatMarketsBuilder, WatchOhlcvRequest,
    config::{AuthConfig, BatMarketsConfig, EndpointConfig},
    errors::Result,
    types::{
        FetchOhlcvRequest, InstrumentId, InstrumentStatus, Kline, Product, TimestampMs, Venue,
    },
};
use bat_markets_core::{ErrorKind, MarketError};
use bat_markets_testing::{build_binance, build_bybit, has_binance_live_env, has_bybit_live_env};

const FRONTEND_SYMBOL_TARGET: usize = 30;
const FRONTEND_LOOKBACK_DAYS: i64 = 3;
const ONE_MINUTE_MS: i64 = 60_000;
const DEFAULT_PAGE_LIMIT: usize = 1_000;
const DEFAULT_FETCH_CONCURRENCY: usize = 4;
const DEFAULT_PAGE_PAUSE_MS: u64 = 50;
const DEFAULT_WATCH_TIMEOUT_SECS: u64 = 75;
const LOCAL_WATCH_ROUNDS: usize = 20;

const PREFERRED_OHLCV_SYMBOLS: &[&str] = &[
    "BTC/USDT:USDT",
    "ETH/USDT:USDT",
    "SOL/USDT:USDT",
    "BNB/USDT:USDT",
    "XRP/USDT:USDT",
    "ADA/USDT:USDT",
    "DOGE/USDT:USDT",
    "LINK/USDT:USDT",
    "LTC/USDT:USDT",
    "AVAX/USDT:USDT",
    "DOT/USDT:USDT",
    "BCH/USDT:USDT",
    "TRX/USDT:USDT",
    "SUI/USDT:USDT",
    "TON/USDT:USDT",
    "APT/USDT:USDT",
    "NEAR/USDT:USDT",
    "ETC/USDT:USDT",
    "FIL/USDT:USDT",
    "ATOM/USDT:USDT",
    "ARB/USDT:USDT",
    "OP/USDT:USDT",
    "INJ/USDT:USDT",
    "UNI/USDT:USDT",
    "AAVE/USDT:USDT",
    "MATIC/USDT:USDT",
    "SEI/USDT:USDT",
    "TIA/USDT:USDT",
    "WIF/USDT:USDT",
    "PEPE/USDT:USDT",
    "1000PEPE/USDT:USDT",
    "TAO/USDT:USDT",
    "FET/USDT:USDT",
    "WLD/USDT:USDT",
    "JUP/USDT:USDT",
    "ENA/USDT:USDT",
    "RUNE/USDT:USDT",
    "XLM/USDT:USDT",
    "HBAR/USDT:USDT",
    "ALGO/USDT:USDT",
];

#[derive(Clone, Copy, Debug)]
struct OhlcvStressPlan {
    symbol_target: usize,
    lookback_days: i64,
    page_limit: usize,
    fetch_concurrency: usize,
    page_pause: Duration,
    watch_timeout: Duration,
    end_open_time_ms: i64,
}

impl OhlcvStressPlan {
    fn from_env() -> Self {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_millis() as i64;
        let end_open_time_ms = (now_ms / ONE_MINUTE_MS) * ONE_MINUTE_MS - ONE_MINUTE_MS;

        Self {
            symbol_target: env_usize("BAT_MARKETS_OHLCV_STRESS_SYMBOLS", FRONTEND_SYMBOL_TARGET)
                .clamp(1, FRONTEND_SYMBOL_TARGET),
            lookback_days: env_i64("BAT_MARKETS_OHLCV_STRESS_DAYS", FRONTEND_LOOKBACK_DAYS).max(1),
            page_limit: env_usize("BAT_MARKETS_OHLCV_STRESS_PAGE_LIMIT", DEFAULT_PAGE_LIMIT)
                .clamp(1, DEFAULT_PAGE_LIMIT),
            fetch_concurrency: env_usize(
                "BAT_MARKETS_OHLCV_STRESS_CONCURRENCY",
                DEFAULT_FETCH_CONCURRENCY,
            )
            .max(1),
            page_pause: Duration::from_millis(env_u64(
                "BAT_MARKETS_OHLCV_STRESS_PAGE_PAUSE_MS",
                DEFAULT_PAGE_PAUSE_MS,
            )),
            watch_timeout: Duration::from_secs(env_u64(
                "BAT_MARKETS_OHLCV_STRESS_WATCH_TIMEOUT_SECS",
                DEFAULT_WATCH_TIMEOUT_SECS,
            )),
            end_open_time_ms,
        }
    }

    const fn expected_candles(self) -> usize {
        (self.lookback_days as usize) * 24 * 60
    }

    fn start_open_time_ms(self) -> i64 {
        self.end_open_time_ms - ((self.expected_candles() as i64 - 1) * ONE_MINUTE_MS)
    }

    fn end_close_time_ms(self) -> i64 {
        self.end_open_time_ms + ONE_MINUTE_MS - 1
    }
}

#[derive(Debug)]
struct FetchWindowReport {
    instrument_id: InstrumentId,
    candles: Vec<Kline>,
    requests: u64,
}

#[test]
fn stress_binance_subscribe_ohlcv_handles_30_symbols_locally() -> Result<()> {
    let client = build_binance();
    let symbols = available_ohlcv_symbols(&client, FRONTEND_SYMBOL_TARGET);

    let runtime =
        tokio::runtime::Runtime::new().expect("tokio runtime should build for local ohlcv stress");
    runtime.block_on(async move {
        let mut updates = client
            .stream()
            .public()
            .subscribe_ohlcv(WatchOhlcvRequest::for_instruments(symbols.clone(), "1m"));

        for round in 0..LOCAL_WATCH_ROUNDS {
            for (index, instrument_id) in symbols.iter().enumerate() {
                let spec = client.market().require_instrument(instrument_id)?;
                let open_time = 1_710_000_000_000_i64
                    + (round as i64 * symbols.len() as i64 + index as i64) * ONE_MINUTE_MS;
                client
                    .stream()
                    .public()
                    .ingest_json(&binance_kline_payload(
                        spec.native_symbol.as_ref(),
                        open_time,
                        round % 2 == 0,
                    ))?;

                let received = timeout(Duration::from_secs(1), updates.recv())
                    .await
                    .expect("typed binance ohlcv update should arrive")
                    .expect("typed binance ohlcv update should parse");
                assert_eq!(received.instrument_id, *instrument_id);
                assert_eq!(received.interval.as_ref(), "1m");
                assert_eq!(received.open_time.value(), open_time);
                assert_eq!(received.close_time.value(), open_time + ONE_MINUTE_MS - 1);
            }
        }

        Ok(())
    })
}

#[test]
fn stress_bybit_subscribe_ohlcv_handles_30_symbols_locally() -> Result<()> {
    let client = build_bybit();
    let symbols = available_ohlcv_symbols(&client, FRONTEND_SYMBOL_TARGET);

    let runtime =
        tokio::runtime::Runtime::new().expect("tokio runtime should build for local ohlcv stress");
    runtime.block_on(async move {
        let mut updates = client
            .stream()
            .public()
            .subscribe_ohlcv(WatchOhlcvRequest::for_instruments(symbols.clone(), "1m"));

        for round in 0..LOCAL_WATCH_ROUNDS {
            for (index, instrument_id) in symbols.iter().enumerate() {
                let spec = client.market().require_instrument(instrument_id)?;
                let open_time = 1_710_100_000_000_i64
                    + (round as i64 * symbols.len() as i64 + index as i64) * ONE_MINUTE_MS;
                client.stream().public().ingest_json(&bybit_kline_payload(
                    spec.native_symbol.as_ref(),
                    open_time,
                    round % 2 == 0,
                ))?;

                let received = timeout(Duration::from_secs(1), updates.recv())
                    .await
                    .expect("typed bybit ohlcv update should arrive")
                    .expect("typed bybit ohlcv update should parse");
                assert_eq!(received.instrument_id, *instrument_id);
                assert_eq!(received.interval.as_ref(), "1m");
                assert_eq!(received.open_time.value(), open_time);
                assert_eq!(received.close_time.value(), open_time + ONE_MINUTE_MS - 1);
            }
        }

        Ok(())
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn binance_mainnet_fetch_ohlcv_frontend_window_stays_within_budget() -> Result<()> {
    if !has_binance_live_env() || env::var_os("BAT_MARKETS_ENABLE_MAINNET_OHLCV_STRESS").is_none() {
        return Ok(());
    }

    exercise_fetch_ohlcv_frontend_window(Venue::Binance).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bybit_mainnet_fetch_ohlcv_frontend_window_stays_within_budget() -> Result<()> {
    if !has_bybit_live_env() || env::var_os("BAT_MARKETS_ENABLE_MAINNET_OHLCV_STRESS").is_none() {
        return Ok(());
    }

    exercise_fetch_ohlcv_frontend_window(Venue::Bybit).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn binance_mainnet_watch_ohlcv_multisymbol_streams_live_updates() -> Result<()> {
    if !has_binance_live_env() || env::var_os("BAT_MARKETS_ENABLE_MAINNET_OHLCV_STRESS").is_none() {
        return Ok(());
    }

    exercise_watch_ohlcv_live(Venue::Binance).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bybit_mainnet_watch_ohlcv_multisymbol_streams_live_updates() -> Result<()> {
    if !has_bybit_live_env() || env::var_os("BAT_MARKETS_ENABLE_MAINNET_OHLCV_STRESS").is_none() {
        return Ok(());
    }

    exercise_watch_ohlcv_live(Venue::Bybit).await
}

async fn exercise_fetch_ohlcv_frontend_window(venue: Venue) -> Result<()> {
    let plan = OhlcvStressPlan::from_env();
    let client = Arc::new(build_mainnet_client(venue).await?);
    let symbols = preferred_ohlcv_symbols(client.as_ref(), plan.symbol_target);

    let diagnostics_before = client.diagnostics().snapshot().fetch_ohlcv.operations;
    let started_at = Instant::now();
    let reports = fetch_frontend_window(Arc::clone(&client), symbols.clone(), plan).await?;
    let elapsed = started_at.elapsed();
    let diagnostics_after = client.diagnostics().snapshot().fetch_ohlcv.operations;
    let observed_requests = diagnostics_after.saturating_sub(diagnostics_before);
    let total_requests = reports.iter().map(|report| report.requests).sum::<u64>();

    assert_eq!(
        observed_requests, total_requests,
        "fetch diagnostics request count should match paged fetches"
    );

    for report in &reports {
        assert_dense_minute_window(report, plan);
    }

    let max_expected_requests =
        ((plan.expected_candles().div_ceil(plan.page_limit)) * plan.symbol_target) as u64;
    assert!(
        total_requests <= max_expected_requests,
        "expected at most {max_expected_requests} paged requests, observed {total_requests}"
    );

    match venue {
        Venue::Binance => {
            let estimated_weight = total_requests * 5;
            assert!(
                estimated_weight <= 2_400,
                "binance estimated kline request weight {estimated_weight} exceeds 2400/min budget"
            );
            println!(
                "venue={venue:?} symbols={} candles_per_symbol={} requests={} estimated_weight={} elapsed_ms={}",
                plan.symbol_target,
                plan.expected_candles(),
                total_requests,
                estimated_weight,
                elapsed.as_millis(),
            );
        }
        Venue::Bybit => {
            assert!(
                total_requests <= 600,
                "bybit HTTP requests {total_requests} exceed the default 600 requests / 5 seconds IP window"
            );
            println!(
                "venue={venue:?} symbols={} candles_per_symbol={} requests={} elapsed_ms={}",
                plan.symbol_target,
                plan.expected_candles(),
                total_requests,
                elapsed.as_millis(),
            );
        }
    }

    Ok(())
}

async fn exercise_watch_ohlcv_live(venue: Venue) -> Result<()> {
    let plan = OhlcvStressPlan::from_env();
    let client = build_mainnet_client(venue).await?;
    let symbols = preferred_ohlcv_symbols(&client, plan.symbol_target);
    let requested: BTreeSet<_> = symbols.iter().cloned().collect();
    let mut seen = BTreeSet::new();
    let mut updates = 0_u64;

    let mut watch = client
        .stream()
        .public()
        .watch_ohlcv(WatchOhlcvRequest::for_instruments(symbols.clone(), "1m"))
        .await?;

    let started_at = Instant::now();
    let deadline = started_at + plan.watch_timeout;

    while seen.len() < symbols.len() {
        let now = Instant::now();
        if now >= deadline {
            watch.abort();
            let missing = requested
                .difference(&seen)
                .map(ToString::to_string)
                .collect::<Vec<_>>();
            return Err(MarketError::new(
                ErrorKind::TransportError,
                format!(
                    "watch_ohlcv timed out after {:?}; received {} symbols, missing: {}",
                    plan.watch_timeout,
                    seen.len(),
                    missing.join(", ")
                ),
            ));
        }

        let remaining = deadline.saturating_duration_since(now);
        let kline = timeout(remaining, watch.recv()).await.map_err(|_| {
            MarketError::new(
                ErrorKind::TransportError,
                format!(
                    "watch_ohlcv timed out waiting for live update after {:?}",
                    plan.watch_timeout
                ),
            )
        })??;

        assert!(
            requested.contains(&kline.instrument_id),
            "watch_ohlcv returned unexpected instrument {}",
            kline.instrument_id
        );
        assert_eq!(kline.interval.as_ref(), "1m");
        assert!(kline.close_time.value() >= kline.open_time.value());

        seen.insert(kline.instrument_id.clone());
        updates += 1;
    }

    watch.shutdown().await?;

    println!(
        "venue={venue:?} requested_symbols={} unique_symbols_seen={} updates={} elapsed_ms={}",
        symbols.len(),
        seen.len(),
        updates,
        started_at.elapsed().as_millis(),
    );

    Ok(())
}

async fn fetch_frontend_window(
    client: Arc<BatMarkets>,
    symbols: Vec<InstrumentId>,
    plan: OhlcvStressPlan,
) -> Result<Vec<FetchWindowReport>> {
    let mut reports = Vec::with_capacity(symbols.len());

    for chunk in symbols.chunks(plan.fetch_concurrency) {
        let mut jobs = JoinSet::new();
        for instrument_id in chunk.iter().cloned() {
            let client = Arc::clone(&client);
            jobs.spawn(async move { fetch_symbol_window(client, instrument_id, plan).await });
        }

        while let Some(joined) = jobs.join_next().await {
            let report = joined.map_err(|error| {
                MarketError::new(
                    ErrorKind::TransportError,
                    format!("fetch_ohlcv stress task join failed: {error}"),
                )
            })??;
            reports.push(report);
        }
    }

    reports.sort_by(|left, right| left.instrument_id.cmp(&right.instrument_id));
    Ok(reports)
}

async fn fetch_symbol_window(
    client: Arc<BatMarkets>,
    instrument_id: InstrumentId,
    plan: OhlcvStressPlan,
) -> Result<FetchWindowReport> {
    let mut candles = Vec::with_capacity(plan.expected_candles());
    let mut next_start = plan.start_open_time_ms();
    let mut requests = 0_u64;

    while next_start <= plan.end_open_time_ms {
        let page = client
            .market()
            .fetch_ohlcv(&FetchOhlcvRequest {
                instrument_id: instrument_id.clone(),
                interval: "1m".into(),
                start_time: Some(TimestampMs::new(next_start)),
                end_time: Some(TimestampMs::new(plan.end_close_time_ms())),
                limit: Some(plan.page_limit),
            })
            .await?;
        requests += 1;

        if page.is_empty() {
            return Err(MarketError::new(
                ErrorKind::TransportError,
                format!("fetch_ohlcv returned an empty page for {instrument_id}"),
            ));
        }

        let mut page_last_open = next_start;
        for candle in page {
            if candle.open_time.value() < plan.start_open_time_ms()
                || candle.open_time.value() > plan.end_open_time_ms
            {
                continue;
            }

            if candles
                .last()
                .is_some_and(|existing: &Kline| existing.open_time == candle.open_time)
            {
                continue;
            }

            page_last_open = candle.open_time.value();
            candles.push(candle);
        }

        if page_last_open >= plan.end_open_time_ms || candles.len() >= plan.expected_candles() {
            break;
        }

        next_start = page_last_open + ONE_MINUTE_MS;
        sleep(plan.page_pause).await;
    }

    Ok(FetchWindowReport {
        instrument_id,
        candles,
        requests,
    })
}

fn assert_dense_minute_window(report: &FetchWindowReport, plan: OhlcvStressPlan) {
    assert_eq!(
        report.candles.len(),
        plan.expected_candles(),
        "{} should return exactly {} one-minute candles for the requested 3-day window",
        report.instrument_id,
        plan.expected_candles(),
    );

    let first = report
        .candles
        .first()
        .expect("dense ohlcv window should contain a first candle");
    let last = report
        .candles
        .last()
        .expect("dense ohlcv window should contain a last candle");

    assert_eq!(first.interval.as_ref(), "1m");
    assert_eq!(first.open_time.value(), plan.start_open_time_ms());
    assert_eq!(last.open_time.value(), plan.end_open_time_ms);
    assert_eq!(last.close_time.value(), plan.end_close_time_ms());

    for window in report.candles.windows(2) {
        let current = &window[0];
        let next = &window[1];
        assert_eq!(current.interval.as_ref(), "1m");
        assert_eq!(next.interval.as_ref(), "1m");
        assert_eq!(
            next.open_time.value() - current.open_time.value(),
            ONE_MINUTE_MS,
            "{} should have dense 1m spacing",
            report.instrument_id
        );
        assert_eq!(
            current.close_time.value() + 1,
            next.open_time.value(),
            "{} should have contiguous close/open boundaries",
            report.instrument_id
        );
    }
}

async fn build_mainnet_client(venue: Venue) -> Result<BatMarkets> {
    let auth = match venue {
        Venue::Binance => AuthConfig::Env {
            api_key_var: "BINANCE_API_KEY".into(),
            api_secret_var: "BINANCE_API_SECRET".into(),
        },
        Venue::Bybit => AuthConfig::Env {
            api_key_var: "BYBIT_API_KEY".into(),
            api_secret_var: "BYBIT_API_SECRET".into(),
        },
    };

    let config = BatMarketsConfig {
        venue,
        product: Product::LinearUsdt,
        auth,
        endpoints: EndpointConfig::mainnet_defaults(venue),
        ..BatMarketsConfig::new(venue, Product::LinearUsdt)
    };

    BatMarketsBuilder::default()
        .config(config)
        .build_live()
        .await
}

fn preferred_ohlcv_symbols(client: &BatMarkets, target: usize) -> Vec<InstrumentId> {
    let selected = collect_ohlcv_symbols(client, target);
    assert_eq!(
        selected.len(),
        target,
        "expected to resolve {target} active public instruments for OHLCV stress"
    );
    selected
}

fn available_ohlcv_symbols(client: &BatMarkets, target: usize) -> Vec<InstrumentId> {
    let selected = collect_ohlcv_symbols(client, target);
    assert!(
        !selected.is_empty(),
        "expected at least one active public instrument for OHLCV fixture stress"
    );
    selected
}

fn collect_ohlcv_symbols(client: &BatMarkets, target: usize) -> Vec<InstrumentId> {
    let specs = client.market().instrument_specs();
    let mut selected = Vec::with_capacity(target);
    let mut used = BTreeSet::new();

    for preferred in PREFERRED_OHLCV_SYMBOLS {
        if let Some(spec) = specs.iter().find(|spec| {
            spec.instrument_id.as_ref() == *preferred
                && spec.status == InstrumentStatus::Active
                && spec.support.public_streams
        }) && used.insert(spec.instrument_id.clone())
        {
            selected.push(spec.instrument_id.clone());
        }
        if selected.len() == target {
            return selected;
        }
    }

    for spec in specs {
        if spec.status != InstrumentStatus::Active || !spec.support.public_streams {
            continue;
        }
        if used.insert(spec.instrument_id.clone()) {
            selected.push(spec.instrument_id);
        }
        if selected.len() == target {
            break;
        }
    }

    selected
}

fn binance_kline_payload(symbol: &str, open_time: i64, closed: bool) -> String {
    format!(
        r#"{{
            "e":"kline",
            "E":{event_time},
            "s":"{symbol}",
            "k":{{
                "i":"1m",
                "t":{open_time},
                "T":{close_time},
                "o":"100.0",
                "h":"101.0",
                "l":"99.5",
                "c":"100.5",
                "v":"42.0",
                "x":{closed}
            }}
        }}"#,
        event_time = open_time + 15_000,
        symbol = symbol,
        open_time = open_time,
        close_time = open_time + ONE_MINUTE_MS - 1,
        closed = if closed { "true" } else { "false" },
    )
}

fn bybit_kline_payload(symbol: &str, open_time: i64, closed: bool) -> String {
    format!(
        r#"{{
            "topic":"kline.1.{symbol}",
            "ts":{event_time},
            "type":"snapshot",
            "data":[
                {{
                    "start":{open_time},
                    "end":{close_time},
                    "interval":"1",
                    "open":"100.0",
                    "close":"100.5",
                    "high":"101.0",
                    "low":"99.5",
                    "volume":"42.0",
                    "turnover":"4200.0",
                    "confirm":{closed},
                    "timestamp":{event_time},
                    "symbol":"{symbol}"
                }}
            ]
        }}"#,
        event_time = open_time + 15_000,
        symbol = symbol,
        open_time = open_time,
        close_time = open_time + ONE_MINUTE_MS - 1,
        closed = if closed { "true" } else { "false" },
    )
}

fn env_usize(key: &str, default: usize) -> usize {
    env::var(key)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(default)
}

fn env_i64(key: &str, default: i64) -> i64 {
    env::var(key)
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(default)
}

fn env_u64(key: &str, default: u64) -> u64 {
    env::var(key)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default)
}
