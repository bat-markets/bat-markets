use std::collections::BTreeMap;

use bat_markets_core::{
    ErrorKind, FetchOhlcvRequest, FundingRate, InstrumentId, InstrumentSpec, Kline, KlineInterval,
    MarketError, OpenInterest, Result, Ticker, TimestampMs, TradeTick, Venue,
};

use crate::{client::BatMarkets, runtime};

/// Read-only ergonomic access to market snapshots maintained by the engine.
pub struct MarketClient<'a> {
    inner: &'a BatMarkets,
}

impl<'a> MarketClient<'a> {
    pub(crate) const fn new(inner: &'a BatMarkets) -> Self {
        Self { inner }
    }

    #[must_use]
    pub fn instrument_specs(&self) -> Vec<InstrumentSpec> {
        self.inner.instrument_specs()
    }

    #[must_use]
    pub fn ticker(&self, instrument_id: &InstrumentId) -> Option<Ticker> {
        self.inner
            .read_state(|state| state.ticker(instrument_id).cloned())
    }

    #[must_use]
    pub fn recent_trades(&self, instrument_id: &InstrumentId) -> Option<Vec<TradeTick>> {
        self.inner
            .read_state(|state| state.recent_trades(instrument_id))
    }

    #[must_use]
    pub fn book_top(&self, instrument_id: &InstrumentId) -> Option<bat_markets_core::BookTop> {
        self.inner
            .read_state(|state| state.book_top(instrument_id).cloned())
    }

    #[must_use]
    pub fn funding_rate(&self, instrument_id: &InstrumentId) -> Option<FundingRate> {
        self.inner
            .read_state(|state| state.funding_rate(instrument_id).cloned())
    }

    #[must_use]
    pub fn open_interest(&self, instrument_id: &InstrumentId) -> Option<OpenInterest> {
        self.inner
            .read_state(|state| state.open_interest(instrument_id).cloned())
    }

    pub fn require_instrument(&self, instrument_id: &InstrumentId) -> Result<InstrumentSpec> {
        self.instrument_specs()
            .into_iter()
            .find(|spec| &spec.instrument_id == instrument_id)
            .ok_or_else(|| {
                bat_markets_core::MarketError::new(
                    bat_markets_core::ErrorKind::Unsupported,
                    format!("unknown instrument {instrument_id}"),
                )
            })
    }

    /// Refresh live `InstrumentSpec` snapshots from the selected venue.
    ///
    /// ```no_run
    /// use bat_markets::{BatMarkets, errors::Result, types::{Product, Venue}};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<()> {
    /// let client = BatMarkets::builder()
    ///     .venue(Venue::Bybit)
    ///     .product(Product::LinearUsdt)
    ///     .build_live()
    ///     .await?;
    ///
    /// let refreshed = client.market().refresh_metadata().await?;
    /// println!("refreshed {} instruments", refreshed.len());
    /// # Ok(())
    /// # }
    /// ```
    pub async fn refresh_metadata(&self) -> Result<Vec<InstrumentSpec>> {
        runtime::refresh_metadata(&self.inner.live_context()).await
    }

    pub async fn refresh_open_interest(
        &self,
        instrument_id: &InstrumentId,
    ) -> Result<OpenInterest> {
        runtime::refresh_open_interest(&self.inner.live_context(), instrument_id).await
    }

    /// Fetch historical OHLCV / kline candles through the venue REST API.
    ///
    /// Intervals are accepted in unified ccxt-style notation such as `1m`, `5m`, `1h`,
    /// `1d`, `1w`, and `1M`. A single call can request between `1` and `30` instruments.
    /// The response is returned as a flat `Vec<Kline>`; each candle carries its own
    /// `instrument_id`, and multi-symbol responses preserve the request instrument order.
    ///
    /// ```no_run
    /// use bat_markets::{
    ///     BatMarkets,
    ///     errors::Result,
    ///     types::{FetchOhlcvRequest, InstrumentId, Product, Venue},
    /// };
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<()> {
    /// let client = BatMarkets::builder()
    ///     .venue(Venue::Binance)
    ///     .product(Product::LinearUsdt)
    ///     .build_live()
    ///     .await?;
    ///
    /// let candles = client
    ///     .market()
    ///     .fetch_ohlcv(&FetchOhlcvRequest::for_instruments(
    ///         vec![
    ///             InstrumentId::from("BTC/USDT:USDT"),
    ///             InstrumentId::from("ETH/USDT:USDT"),
    ///         ],
    ///         "5m",
    ///         None,
    ///         None,
    ///         Some(200),
    ///     ))
    ///     .await?;
    ///
    /// println!("loaded {} candles", candles.len());
    /// # Ok(())
    /// # }
    /// ```
    pub async fn fetch_ohlcv(&self, request: &FetchOhlcvRequest) -> Result<Vec<Kline>> {
        runtime::fetch_ohlcv(&self.inner.live_context(), request).await
    }

    /// Fetch and fully paginate a bounded OHLCV window for `1..=30` instruments.
    ///
    /// This method keeps calling `fetch_ohlcv()` until the requested `[start_time, end_time]`
    /// range is exhausted, then returns a flat `Vec<Kline>` grouped by request instrument order.
    ///
    /// ```no_run
    /// use bat_markets::{
    ///     BatMarkets,
    ///     errors::Result,
    ///     types::{FetchOhlcvRequest, InstrumentId, Product, TimestampMs, Venue},
    /// };
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<()> {
    /// let client = BatMarkets::builder()
    ///     .venue(Venue::Bybit)
    ///     .product(Product::LinearUsdt)
    ///     .build_live()
    ///     .await?;
    ///
    /// let candles = client
    ///     .market()
    ///     .fetch_ohlcv_window(&FetchOhlcvRequest::for_instruments(
    ///         vec![
    ///             InstrumentId::from("BTC/USDT:USDT"),
    ///             InstrumentId::from("ETH/USDT:USDT"),
    ///         ],
    ///         "1m",
    ///         Some(TimestampMs::new(1_710_000_000_000)),
    ///         Some(TimestampMs::new(1_710_086_399_999)),
    ///         Some(1_000),
    ///     ))
    ///     .await?;
    ///
    /// println!("loaded {} window candles", candles.len());
    /// # Ok(())
    /// # }
    /// ```
    pub async fn fetch_ohlcv_window(&self, request: &FetchOhlcvRequest) -> Result<Vec<Kline>> {
        let instrument_ids = request.instrument_ids()?.to_vec();
        let start_time = request.start_time.ok_or_else(|| {
            MarketError::new(
                ErrorKind::ConfigError,
                "fetch_ohlcv_window requires request.start_time",
            )
        })?;
        let end_time = request.end_time.ok_or_else(|| {
            MarketError::new(
                ErrorKind::ConfigError,
                "fetch_ohlcv_window requires request.end_time",
            )
        })?;
        if end_time.value() < start_time.value() {
            return Err(MarketError::new(
                ErrorKind::ConfigError,
                "fetch_ohlcv_window requires end_time >= start_time",
            ));
        }
        let interval = KlineInterval::parse(request.interval.as_ref()).ok_or_else(|| {
            MarketError::new(
                ErrorKind::Unsupported,
                format!("unsupported OHLCV interval '{}'", request.interval),
            )
        })?;

        let mut candles_by_instrument = instrument_ids
            .iter()
            .cloned()
            .map(|instrument_id| (instrument_id, Vec::<Kline>::new()))
            .collect::<BTreeMap<_, _>>();
        let mut next_start = start_time.value();
        let mut next_end = end_time.value();
        let venue = self.inner.venue();

        loop {
            let page = self
                .fetch_ohlcv(&FetchOhlcvRequest::for_instruments(
                    instrument_ids.clone(),
                    request.interval.clone(),
                    Some(TimestampMs::new(next_start)),
                    Some(TimestampMs::new(next_end)),
                    request.limit,
                ))
                .await?;
            if page.is_empty() {
                break;
            }

            let mut max_open_time = next_start;
            let mut min_open_time = i64::MAX;
            for candle in page {
                if candle.open_time.value() < start_time.value()
                    || candle.open_time.value() > end_time.value()
                {
                    continue;
                }

                let Some(candles) = candles_by_instrument.get_mut(&candle.instrument_id) else {
                    return Err(MarketError::new(
                        ErrorKind::TransportError,
                        format!(
                            "fetch_ohlcv_window returned unexpected instrument {}",
                            candle.instrument_id
                        ),
                    ));
                };

                max_open_time = max_open_time.max(candle.open_time.value());
                min_open_time = min_open_time.min(candle.open_time.value());
                candles.push(candle);
            }

            match venue {
                Venue::Binance => {
                    let Some(max_close_time) = interval.close_time_ms(max_open_time) else {
                        return Err(MarketError::new(
                            ErrorKind::DecodeError,
                            format!(
                                "failed to derive OHLCV close time for interval '{}'",
                                request.interval
                            ),
                        ));
                    };
                    if max_close_time >= end_time.value() {
                        break;
                    }
                    next_start = max_close_time + 1;
                }
                Venue::Bybit => {
                    if min_open_time <= start_time.value() || min_open_time == i64::MAX {
                        break;
                    }
                    next_end = min_open_time - 1;
                }
            }
        }

        let mut flattened = Vec::new();
        for instrument_id in instrument_ids {
            let mut candles = candles_by_instrument
                .remove(&instrument_id)
                .expect("window fetch should keep every requested instrument");
            candles.sort_by_key(|candle| candle.open_time.value());
            candles.dedup_by_key(|candle| candle.open_time);
            flattened.extend(candles);
        }
        Ok(flattened)
    }

    /// Alias for `fetch_ohlcv_window()` when the intent is “load the full requested range”.
    pub async fn fetch_ohlcv_all(&self, request: &FetchOhlcvRequest) -> Result<Vec<Kline>> {
        self.fetch_ohlcv_window(request).await
    }
}
