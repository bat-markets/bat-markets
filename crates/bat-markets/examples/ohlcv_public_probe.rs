use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use bat_markets::{
    BatMarketsBuilder, WatchOhlcvRequest,
    config::{AuthConfig, BatMarketsConfig, EndpointConfig},
    errors::Result,
    types::{
        FetchOhlcvRequest, InstrumentId, InstrumentStatus, Kline, Product, TimestampMs, Venue,
    },
};

const SYMBOL_TARGET: usize = 30;
const LOOKBACK_DAYS: usize = 3;
const ONE_MINUTE_MS: i64 = 60_000;
const PAGE_LIMIT: usize = 1_000;
const WATCH_TIMEOUT: Duration = Duration::from_secs(75);

const PREFERRED_SYMBOLS: &[&str] = &[
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
];

#[tokio::main]
async fn main() -> Result<()> {
    let venue = parse_venue()?;
    let plan = ProbePlan::default();

    println!(
        "probe start venue={venue:?} symbols={} days={} interval=1m limit={}",
        plan.symbol_target, plan.lookback_days, plan.page_limit
    );

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

    let symbols = preferred_symbols(&client, plan.symbol_target);
    println!(
        "metadata loaded venue={venue:?} active_symbols={} chosen_first={} chosen_last={}",
        client.market().instrument_specs().len(),
        symbols.first().expect("at least one chosen symbol"),
        symbols.last().expect("at least one chosen symbol"),
    );

    probe_fetch(&client, &symbols, plan).await?;
    probe_watch(&client, &symbols).await?;
    println!("probe done venue={venue:?}");
    Ok(())
}

#[derive(Clone, Copy, Debug)]
struct ProbePlan {
    symbol_target: usize,
    lookback_days: usize,
    page_limit: usize,
    end_open_time_ms: i64,
}

impl Default for ProbePlan {
    fn default() -> Self {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_millis() as i64;
        Self {
            symbol_target: SYMBOL_TARGET,
            lookback_days: LOOKBACK_DAYS,
            page_limit: PAGE_LIMIT,
            end_open_time_ms: (now_ms / ONE_MINUTE_MS) * ONE_MINUTE_MS - ONE_MINUTE_MS,
        }
    }
}

impl ProbePlan {
    const fn expected_candles(self) -> usize {
        self.lookback_days * 24 * 60
    }

    const fn end_open_time_ms(self) -> i64 {
        self.end_open_time_ms
    }

    fn start_open_time_ms(self) -> i64 {
        self.end_open_time_ms() - ((self.expected_candles() as i64 - 1) * ONE_MINUTE_MS)
    }

    fn end_close_time_ms(self) -> i64 {
        self.end_open_time_ms() + ONE_MINUTE_MS - 1
    }
}

async fn probe_fetch(
    client: &bat_markets::BatMarkets,
    symbols: &[InstrumentId],
    plan: ProbePlan,
) -> Result<()> {
    let fetch_before = client.diagnostics().snapshot().fetch_ohlcv.operations;
    let started_at = Instant::now();
    let page = client
        .market()
        .fetch_ohlcv(&FetchOhlcvRequest::for_instruments(
            symbols.to_vec(),
            "1m",
            Some(TimestampMs::new(plan.start_open_time_ms())),
            Some(TimestampMs::new(plan.end_close_time_ms())),
            Some(plan.page_limit),
        ))
        .await?;
    let total_fetch_calls = client.diagnostics().snapshot().fetch_ohlcv.operations - fetch_before;
    let batch_count = (total_fetch_calls as usize).div_ceil(symbols.len());

    let mut candles_by_instrument = symbols
        .iter()
        .cloned()
        .map(|instrument_id| (instrument_id, Vec::<Kline>::new()))
        .collect::<BTreeMap<_, _>>();
    for candle in page {
        if let Some(candles) = candles_by_instrument.get_mut(&candle.instrument_id) {
            candles.push(candle);
        }
    }
    println!(
        "fetch summary batches={} total_rest_calls={} estimated_binance_weight={} total_elapsed_ms={}",
        batch_count,
        total_fetch_calls,
        total_fetch_calls * 5,
        started_at.elapsed().as_millis(),
    );

    for symbol in symbols.iter().take(5) {
        let candles = normalized_candles(
            candles_by_instrument
                .get(symbol)
                .expect("chosen instrument should stay in the result map"),
        );
        println!(
            "fetch symbol={} candles={} first_open={} last_open={}",
            symbol,
            candles.len(),
            candles
                .first()
                .map(|c| c.open_time.value())
                .unwrap_or_default(),
            candles
                .last()
                .map(|c| c.open_time.value())
                .unwrap_or_default(),
        );
    }

    let incomplete = candles_by_instrument
        .iter()
        .filter_map(|(instrument_id, candles)| {
            let normalized = normalized_candles(candles);
            (normalized.len() != plan.expected_candles())
                .then(|| format!("{instrument_id}:{}", normalized.len()))
        })
        .collect::<Vec<_>>();
    if !incomplete.is_empty() {
        return Err(bat_markets::errors::MarketError::new(
            bat_markets::errors::ErrorKind::TransportError,
            format!("fetch probe incomplete windows: {}", incomplete.join(", ")),
        ));
    }

    Ok(())
}

fn normalized_candles(candles: &[Kline]) -> Vec<Kline> {
    let mut normalized = candles.to_vec();
    normalized.sort_by_key(|candle| candle.open_time.value());
    normalized.dedup_by_key(|candle| candle.open_time);
    normalized
}

async fn probe_watch(client: &bat_markets::BatMarkets, symbols: &[InstrumentId]) -> Result<()> {
    let mut watch = client
        .stream()
        .public()
        .watch_ohlcv(WatchOhlcvRequest::for_instruments(symbols.to_vec(), "1m"))
        .await?;
    let requested = symbols.iter().cloned().collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();
    let started_at = Instant::now();

    while seen.len() < requested.len() {
        let remaining = WATCH_TIMEOUT.saturating_sub(started_at.elapsed());
        if remaining.is_zero() {
            watch.abort();
            let missing = requested
                .difference(&seen)
                .map(ToString::to_string)
                .collect::<Vec<_>>();
            return Err(bat_markets::errors::MarketError::new(
                bat_markets::errors::ErrorKind::Timeout,
                format!(
                    "watch probe timed out, missing symbols: {}",
                    missing.join(", ")
                ),
            ));
        }

        let candle = tokio::time::timeout(remaining, watch.recv())
            .await
            .map_err(|_| {
                bat_markets::errors::MarketError::new(
                    bat_markets::errors::ErrorKind::Timeout,
                    "watch probe timed out waiting for the next candle",
                )
            })??;

        if seen.insert(candle.instrument_id.clone()) {
            println!(
                "watch new_symbol={} seen={}/{} interval={} closed={} open_time={} elapsed_ms={}",
                candle.instrument_id,
                seen.len(),
                requested.len(),
                candle.interval,
                candle.closed,
                candle.open_time.value(),
                started_at.elapsed().as_millis(),
            );
        }
    }

    watch.shutdown().await?;
    println!(
        "watch summary symbols_seen={} elapsed_ms={}",
        seen.len(),
        started_at.elapsed().as_millis(),
    );
    Ok(())
}

fn parse_venue() -> Result<Venue> {
    match env::var("BAT_MARKETS_PROBE_VENUE")
        .unwrap_or_else(|_| "binance".to_string())
        .to_ascii_lowercase()
        .as_str()
    {
        "binance" => Ok(Venue::Binance),
        "bybit" => Ok(Venue::Bybit),
        other => Err(bat_markets::errors::MarketError::new(
            bat_markets::errors::ErrorKind::ConfigError,
            format!("unsupported BAT_MARKETS_PROBE_VENUE '{other}'"),
        )),
    }
}

fn preferred_symbols(client: &bat_markets::BatMarkets, target: usize) -> Vec<InstrumentId> {
    let specs = client.market().instrument_specs();
    let mut selected = Vec::with_capacity(target);
    let mut used = BTreeSet::new();

    for preferred in PREFERRED_SYMBOLS {
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
