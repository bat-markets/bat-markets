use std::collections::BTreeSet;

use tokio::{
    sync::{broadcast, oneshot},
    task::JoinHandle,
};

use bat_markets_core::{
    BookTop, CommandOperation, CommandReceipt, ErrorKind, InstrumentId, Kline, KlineInterval,
    PrivateLaneEvent, PublicLaneEvent, ReconcileReport, ReconcileTrigger, RequestId, Result,
    Ticker, TradeTick,
};

use crate::{client::BatMarkets, runtime};

/// Public market-data subscription plan for live websocket runners.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicSubscription {
    pub instrument_ids: Vec<InstrumentId>,
    pub ticker: bool,
    pub trades: bool,
    pub book_top: bool,
    pub funding_rate: bool,
    pub open_interest: bool,
    pub kline_interval: Option<Box<str>>,
}

impl PublicSubscription {
    #[must_use]
    pub fn all_for(instrument_ids: Vec<InstrumentId>) -> Self {
        Self {
            instrument_ids,
            ticker: true,
            trades: true,
            book_top: true,
            funding_rate: true,
            open_interest: true,
            kline_interval: Some("1m".into()),
        }
    }
}

/// Typed market-data watch request for one or many instruments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WatchInstrumentsRequest {
    pub instrument_ids: Vec<InstrumentId>,
}

impl WatchInstrumentsRequest {
    #[must_use]
    pub fn for_instrument(instrument_id: InstrumentId) -> Self {
        Self {
            instrument_ids: vec![instrument_id],
        }
    }

    #[must_use]
    pub fn for_instruments(instrument_ids: Vec<InstrumentId>) -> Self {
        Self { instrument_ids }
    }
}

/// Typed OHLCV watch request for one or many instruments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WatchOhlcvRequest {
    pub instrument_ids: Vec<InstrumentId>,
    pub interval: Box<str>,
}

impl WatchOhlcvRequest {
    #[must_use]
    pub fn for_instrument(instrument_id: InstrumentId, interval: impl Into<Box<str>>) -> Self {
        Self {
            instrument_ids: vec![instrument_id],
            interval: normalize_interval_box(interval.into()),
        }
    }

    #[must_use]
    pub fn for_instruments(
        instrument_ids: Vec<InstrumentId>,
        interval: impl Into<Box<str>>,
    ) -> Self {
        Self {
            instrument_ids,
            interval: normalize_interval_box(interval.into()),
        }
    }

    fn into_public_subscription(self) -> PublicSubscription {
        PublicSubscription {
            instrument_ids: self.instrument_ids,
            ticker: false,
            trades: false,
            book_top: false,
            funding_rate: false,
            open_interest: false,
            kline_interval: Some(self.interval),
        }
    }
}

/// Handle for a running live stream task.
pub struct LiveStreamHandle {
    pub(crate) shutdown: Option<oneshot::Sender<()>>,
    pub(crate) join: JoinHandle<Result<()>>,
}

impl LiveStreamHandle {
    pub async fn shutdown(mut self) -> Result<()> {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        self.join.await.map_err(|error| {
            bat_markets_core::MarketError::new(
                bat_markets_core::ErrorKind::TransportError,
                format!("stream task join failed: {error}"),
            )
        })?
    }

    pub fn abort(&self) {
        self.join.abort();
    }

    pub async fn wait(self) -> Result<()> {
        self.join.await.map_err(|error| {
            bat_markets_core::MarketError::new(
                bat_markets_core::ErrorKind::TransportError,
                format!("stream task join failed: {error}"),
            )
        })?
    }
}

/// Typed OHLCV subscription receiver over the shared public event bus.
pub struct OhlcvUpdates<'a> {
    inner: &'a BatMarkets,
    receiver: broadcast::Receiver<PublicLaneEvent>,
    instrument_ids: BTreeSet<InstrumentId>,
    interval: Box<str>,
}

impl<'a> OhlcvUpdates<'a> {
    fn new(
        inner: &'a BatMarkets,
        receiver: broadcast::Receiver<PublicLaneEvent>,
        request: WatchOhlcvRequest,
    ) -> Self {
        Self {
            inner,
            receiver,
            instrument_ids: request.instrument_ids.into_iter().collect(),
            interval: request.interval,
        }
    }

    pub async fn recv(&mut self) -> Result<Kline> {
        loop {
            let requested_interval = parse_watch_interval(self.interval.as_ref())?;
            let event = self.receiver.recv().await.map_err(|error| {
                bat_markets_core::MarketError::new(
                    ErrorKind::TransportError,
                    format!("ohlcv subscription receive failed: {error}"),
                )
            })?;

            let PublicLaneEvent::Kline(kline) = event else {
                continue;
            };
            let Some(incoming_interval) = KlineInterval::parse(kline.interval.as_ref()) else {
                continue;
            };
            if !self.instrument_ids.contains(&kline.instrument_id)
                || incoming_interval != requested_interval
            {
                continue;
            }

            let spec = self
                .inner
                .adapter
                .as_adapter()
                .resolve_instrument(&kline.instrument_id)
                .ok_or_else(|| {
                    bat_markets_core::MarketError::new(
                        ErrorKind::Unsupported,
                        format!(
                            "unknown instrument {} for kline update",
                            kline.instrument_id
                        ),
                    )
                })?;
            let mut unified = kline.to_unified(&spec);
            unified.interval = requested_interval.as_ccxt_str().into();
            return Ok(unified);
        }
    }
}

/// Live OHLCV watcher with typed updates and stream lifecycle control.
pub struct OhlcvWatch<'a> {
    updates: OhlcvUpdates<'a>,
    stream: LiveStreamHandle,
}

impl<'a> OhlcvWatch<'a> {
    pub async fn recv(&mut self) -> Result<Kline> {
        tokio::select! {
            update = self.updates.recv() => update,
            result = &mut self.stream.join => {
                match result {
                    Ok(Ok(())) => Err(bat_markets_core::MarketError::new(
                        ErrorKind::TransportError,
                        "OHLCV live stream finished before the next candle update",
                    )),
                    Ok(Err(error)) => Err(error),
                    Err(error) => Err(bat_markets_core::MarketError::new(
                        ErrorKind::TransportError,
                        format!("stream task join failed while waiting for OHLCV update: {error}"),
                    )),
                }
            }
        }
    }

    pub async fn shutdown(self) -> Result<()> {
        self.stream.shutdown().await
    }

    pub fn abort(&self) {
        self.stream.abort();
    }

    pub async fn wait(self) -> Result<()> {
        self.stream.wait().await
    }
}

/// Typed ticker subscription receiver over the shared public event bus.
pub struct TickerUpdates<'a> {
    inner: &'a BatMarkets,
    receiver: broadcast::Receiver<PublicLaneEvent>,
    instrument_ids: BTreeSet<InstrumentId>,
}

impl<'a> TickerUpdates<'a> {
    fn new(
        inner: &'a BatMarkets,
        receiver: broadcast::Receiver<PublicLaneEvent>,
        request: WatchInstrumentsRequest,
    ) -> Self {
        Self {
            inner,
            receiver,
            instrument_ids: request.instrument_ids.into_iter().collect(),
        }
    }

    pub async fn recv(&mut self) -> Result<Ticker> {
        loop {
            let event = self.receiver.recv().await.map_err(|error| {
                bat_markets_core::MarketError::new(
                    ErrorKind::TransportError,
                    format!("ticker subscription receive failed: {error}"),
                )
            })?;

            let PublicLaneEvent::Ticker(ticker) = event else {
                continue;
            };
            if !self.instrument_ids.contains(&ticker.instrument_id) {
                continue;
            }

            let spec = resolve_public_spec(self.inner, &ticker.instrument_id, "ticker")?;
            return Ok(ticker.to_unified(&spec));
        }
    }
}

/// Live ticker watcher with typed updates and stream lifecycle control.
pub struct TickerWatch<'a> {
    updates: TickerUpdates<'a>,
    stream: LiveStreamHandle,
}

impl<'a> TickerWatch<'a> {
    pub async fn recv(&mut self) -> Result<Ticker> {
        tokio::select! {
            update = self.updates.recv() => update,
            result = &mut self.stream.join => join_error(result, "ticker"),
        }
    }

    pub async fn shutdown(self) -> Result<()> {
        self.stream.shutdown().await
    }

    pub fn abort(&self) {
        self.stream.abort();
    }

    pub async fn wait(self) -> Result<()> {
        self.stream.wait().await
    }
}

/// Typed trade subscription receiver over the shared public event bus.
pub struct TradeUpdates<'a> {
    inner: &'a BatMarkets,
    receiver: broadcast::Receiver<PublicLaneEvent>,
    instrument_ids: BTreeSet<InstrumentId>,
}

impl<'a> TradeUpdates<'a> {
    fn new(
        inner: &'a BatMarkets,
        receiver: broadcast::Receiver<PublicLaneEvent>,
        request: WatchInstrumentsRequest,
    ) -> Self {
        Self {
            inner,
            receiver,
            instrument_ids: request.instrument_ids.into_iter().collect(),
        }
    }

    pub async fn recv(&mut self) -> Result<TradeTick> {
        loop {
            let event = self.receiver.recv().await.map_err(|error| {
                bat_markets_core::MarketError::new(
                    ErrorKind::TransportError,
                    format!("trade subscription receive failed: {error}"),
                )
            })?;

            let PublicLaneEvent::Trade(trade) = event else {
                continue;
            };
            if !self.instrument_ids.contains(&trade.instrument_id) {
                continue;
            }

            let spec = resolve_public_spec(self.inner, &trade.instrument_id, "trade")?;
            return Ok(trade.to_unified(&spec));
        }
    }
}

/// Live trades watcher with typed updates and stream lifecycle control.
pub struct TradesWatch<'a> {
    updates: TradeUpdates<'a>,
    stream: LiveStreamHandle,
}

impl<'a> TradesWatch<'a> {
    pub async fn recv(&mut self) -> Result<TradeTick> {
        tokio::select! {
            update = self.updates.recv() => update,
            result = &mut self.stream.join => join_error(result, "trade"),
        }
    }

    pub async fn shutdown(self) -> Result<()> {
        self.stream.shutdown().await
    }

    pub fn abort(&self) {
        self.stream.abort();
    }

    pub async fn wait(self) -> Result<()> {
        self.stream.wait().await
    }
}

/// Typed top-of-book subscription receiver over the shared public event bus.
pub struct BookTopUpdates<'a> {
    inner: &'a BatMarkets,
    receiver: broadcast::Receiver<PublicLaneEvent>,
    instrument_ids: BTreeSet<InstrumentId>,
}

impl<'a> BookTopUpdates<'a> {
    fn new(
        inner: &'a BatMarkets,
        receiver: broadcast::Receiver<PublicLaneEvent>,
        request: WatchInstrumentsRequest,
    ) -> Self {
        Self {
            inner,
            receiver,
            instrument_ids: request.instrument_ids.into_iter().collect(),
        }
    }

    pub async fn recv(&mut self) -> Result<BookTop> {
        loop {
            let event = self.receiver.recv().await.map_err(|error| {
                bat_markets_core::MarketError::new(
                    ErrorKind::TransportError,
                    format!("book_top subscription receive failed: {error}"),
                )
            })?;

            let PublicLaneEvent::BookTop(book_top) = event else {
                continue;
            };
            if !self.instrument_ids.contains(&book_top.instrument_id) {
                continue;
            }

            let spec = resolve_public_spec(self.inner, &book_top.instrument_id, "book_top")?;
            return Ok(book_top.to_unified(&spec));
        }
    }
}

/// Live top-of-book watcher with typed updates and stream lifecycle control.
pub struct BookTopWatch<'a> {
    updates: BookTopUpdates<'a>,
    stream: LiveStreamHandle,
}

impl<'a> BookTopWatch<'a> {
    pub async fn recv(&mut self) -> Result<BookTop> {
        tokio::select! {
            update = self.updates.recv() => update,
            result = &mut self.stream.join => join_error(result, "book_top"),
        }
    }

    pub async fn shutdown(self) -> Result<()> {
        self.stream.shutdown().await
    }

    pub fn abort(&self) {
        self.stream.abort();
    }

    pub async fn wait(self) -> Result<()> {
        self.stream.wait().await
    }
}

fn normalize_interval_box(interval: Box<str>) -> Box<str> {
    KlineInterval::parse(interval.as_ref())
        .map(Into::into)
        .unwrap_or(interval)
}

fn resolve_public_spec(
    inner: &BatMarkets,
    instrument_id: &InstrumentId,
    event_name: &str,
) -> Result<bat_markets_core::InstrumentSpec> {
    inner
        .adapter
        .as_adapter()
        .resolve_instrument(instrument_id)
        .ok_or_else(|| {
            bat_markets_core::MarketError::new(
                ErrorKind::Unsupported,
                format!("unknown instrument {instrument_id} for {event_name} update"),
            )
        })
}

fn join_error<T>(
    result: std::result::Result<Result<()>, tokio::task::JoinError>,
    event_name: &str,
) -> Result<T> {
    match result {
        Ok(Ok(())) => Err(bat_markets_core::MarketError::new(
            ErrorKind::TransportError,
            format!("{event_name} live stream finished before the next update"),
        )),
        Ok(Err(error)) => Err(error),
        Err(error) => Err(bat_markets_core::MarketError::new(
            ErrorKind::TransportError,
            format!("stream task join failed while waiting for {event_name} update: {error}"),
        )),
    }
}

fn parse_watch_interval(raw: &str) -> Result<KlineInterval> {
    KlineInterval::parse(raw).ok_or_else(|| {
        bat_markets_core::MarketError::new(
            ErrorKind::Unsupported,
            format!("unsupported OHLCV interval '{raw}'"),
        )
    })
}

/// Entry point for lane-specific ingestion.
pub struct StreamClient<'a> {
    inner: &'a BatMarkets,
}

impl<'a> StreamClient<'a> {
    pub(crate) const fn new(inner: &'a BatMarkets) -> Self {
        Self { inner }
    }

    #[must_use]
    pub fn public(&self) -> PublicLaneClient<'a> {
        PublicLaneClient { inner: self.inner }
    }

    #[must_use]
    pub fn private(&self) -> PrivateLaneClient<'a> {
        PrivateLaneClient { inner: self.inner }
    }

    #[must_use]
    pub fn command(&self) -> CommandLaneClient<'a> {
        CommandLaneClient { inner: self.inner }
    }
}

/// Public market-data lane ingestion.
pub struct PublicLaneClient<'a> {
    inner: &'a BatMarkets,
}

impl<'a> PublicLaneClient<'a> {
    pub fn ingest_json(&self, payload: &str) -> Result<Vec<PublicLaneEvent>> {
        let events = self.inner.adapter.as_adapter().parse_public(payload)?;
        self.inner.shared.apply_public_events(&events);
        Ok(events)
    }

    /// Subscribe to fast public-lane events emitted by fixture ingest or live runtime.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<PublicLaneEvent> {
        self.inner.shared.subscribe_public_events()
    }

    /// Subscribe to typed ticker updates already flowing through the public lane.
    #[must_use]
    pub fn subscribe_ticker(&self, request: WatchInstrumentsRequest) -> TickerUpdates<'a> {
        TickerUpdates::new(self.inner, self.subscribe(), request)
    }

    /// Subscribe to typed trade updates already flowing through the public lane.
    #[must_use]
    pub fn subscribe_trades(&self, request: WatchInstrumentsRequest) -> TradeUpdates<'a> {
        TradeUpdates::new(self.inner, self.subscribe(), request)
    }

    /// Subscribe to typed top-of-book updates already flowing through the public lane.
    #[must_use]
    pub fn subscribe_book_top(&self, request: WatchInstrumentsRequest) -> BookTopUpdates<'a> {
        BookTopUpdates::new(self.inner, self.subscribe(), request)
    }

    /// Subscribe to typed OHLCV updates already flowing through the public lane.
    #[must_use]
    pub fn subscribe_ohlcv(&self, request: WatchOhlcvRequest) -> OhlcvUpdates<'a> {
        OhlcvUpdates::new(self.inner, self.subscribe(), request)
    }

    /// Spawn a reconnecting live public-stream runner.
    pub async fn spawn_live(&self, subscription: PublicSubscription) -> Result<LiveStreamHandle> {
        runtime::spawn_public_stream(self.inner.live_context(), subscription).await
    }

    /// Spawn a reconnecting live ticker watcher for one or many instruments.
    pub async fn watch_ticker(&self, request: WatchInstrumentsRequest) -> Result<TickerWatch<'a>> {
        let updates = self.subscribe_ticker(request.clone());
        let stream = self
            .spawn_live(PublicSubscription {
                instrument_ids: request.instrument_ids,
                ticker: true,
                trades: false,
                book_top: false,
                funding_rate: false,
                open_interest: false,
                kline_interval: None,
            })
            .await?;
        Ok(TickerWatch { updates, stream })
    }

    /// Spawn a reconnecting live trade watcher for one or many instruments.
    pub async fn watch_trades(&self, request: WatchInstrumentsRequest) -> Result<TradesWatch<'a>> {
        let updates = self.subscribe_trades(request.clone());
        let stream = self
            .spawn_live(PublicSubscription {
                instrument_ids: request.instrument_ids,
                ticker: false,
                trades: true,
                book_top: false,
                funding_rate: false,
                open_interest: false,
                kline_interval: None,
            })
            .await?;
        Ok(TradesWatch { updates, stream })
    }

    /// Spawn a reconnecting live top-of-book watcher for one or many instruments.
    pub async fn watch_book_top(
        &self,
        request: WatchInstrumentsRequest,
    ) -> Result<BookTopWatch<'a>> {
        let updates = self.subscribe_book_top(request.clone());
        let stream = self
            .spawn_live(PublicSubscription {
                instrument_ids: request.instrument_ids,
                ticker: false,
                trades: false,
                book_top: true,
                funding_rate: false,
                open_interest: false,
                kline_interval: None,
            })
            .await?;
        Ok(BookTopWatch { updates, stream })
    }

    /// Spawn a reconnecting live OHLCV watcher for one or many instruments.
    ///
    /// Intervals are accepted in unified ccxt-style notation such as `1m`, `5m`, `1h`,
    /// `1d`, `1w`, and `1M`.
    ///
    /// ```no_run
    /// use bat_markets::{
    ///     BatMarkets, WatchOhlcvRequest,
    ///     errors::Result,
    ///     types::{InstrumentId, Product, Venue},
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
    /// let mut watch = client
    ///     .stream()
    ///     .public()
    ///     .watch_ohlcv(WatchOhlcvRequest::for_instruments(
    ///         vec![
    ///             InstrumentId::from("BTC/USDT:USDT"),
    ///             InstrumentId::from("ETH/USDT:USDT"),
    ///         ],
    ///         "1m",
    ///     ))
    ///     .await?;
    ///
    /// let candle = watch.recv().await?;
    /// println!("{} {}", candle.instrument_id, candle.close);
    /// watch.shutdown().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn watch_ohlcv(&self, request: WatchOhlcvRequest) -> Result<OhlcvWatch<'a>> {
        let updates = self.subscribe_ohlcv(request.clone());
        let stream = self.spawn_live(request.into_public_subscription()).await?;
        Ok(OhlcvWatch { updates, stream })
    }
}

/// Private state-lane ingestion.
pub struct PrivateLaneClient<'a> {
    inner: &'a BatMarkets,
}

impl<'a> PrivateLaneClient<'a> {
    pub fn ingest_json(&self, payload: &str) -> Result<Vec<PrivateLaneEvent>> {
        let events = self.inner.adapter.as_adapter().parse_private(payload)?;
        self.inner.write_state(|state| {
            for event in events.iter().cloned() {
                state.apply_private_event(event);
            }
        });
        Ok(events)
    }

    /// Spawn a reconnecting live private-stream runner.
    pub async fn spawn_live(&self) -> Result<LiveStreamHandle> {
        runtime::spawn_private_stream(self.inner.live_context()).await
    }

    /// Trigger a manual REST-backed repair cycle for the private-state lane.
    ///
    /// ```no_run
    /// use bat_markets::{BatMarkets, errors::Result, types::{Product, Venue}};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<()> {
    /// let client = BatMarkets::builder()
    ///     .venue(Venue::Binance)
    ///     .product(Product::LinearUsdt)
    ///     .build_live()
    ///     .await?;
    ///
    /// let report = client.stream().private().reconcile().await?;
    /// println!("reconcile outcome: {:?}", report.outcome);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn reconcile(&self) -> Result<ReconcileReport> {
        runtime::reconcile_private(&self.inner.live_context(), ReconcileTrigger::Manual).await
    }
}

/// Command-lane classification and state hint application.
pub struct CommandLaneClient<'a> {
    inner: &'a BatMarkets,
}

impl<'a> CommandLaneClient<'a> {
    pub fn classify_json(
        &self,
        operation: CommandOperation,
        payload: Option<&str>,
        request_id: Option<RequestId>,
    ) -> Result<CommandReceipt> {
        let receipt = self
            .inner
            .adapter
            .as_adapter()
            .classify_command(operation, payload, request_id)?;
        self.inner
            .write_state(|state| state.apply_command_receipt(&receipt));
        Ok(receipt)
    }
}

#[cfg(test)]
mod tests {
    use tokio::time::{Duration, timeout};

    use bat_markets_core::{InstrumentId, Product, PublicLaneEvent, Venue};

    use crate::{BatMarketsBuilder, WatchInstrumentsRequest, WatchOhlcvRequest};

    const BINANCE_PUBLIC_TRADE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/binance/public_trade.json"
    ));
    const BINANCE_PUBLIC_TICKER: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/binance/public_ticker.json"
    ));
    const BINANCE_PUBLIC_BOOK_TICKER: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/binance/public_book_ticker.json"
    ));
    const BINANCE_PUBLIC_KLINE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/binance/public_kline.json"
    ));

    #[tokio::test]
    async fn public_subscribe_receives_fixture_ingest_events() {
        let client = BatMarketsBuilder::default()
            .venue(Venue::Binance)
            .product(Product::LinearUsdt)
            .build()
            .expect("fixture client should build");
        let mut receiver = client.stream().public().subscribe();

        let events = client
            .stream()
            .public()
            .ingest_json(BINANCE_PUBLIC_TRADE)
            .expect("fixture payload should parse");
        assert!(!events.is_empty());

        let received = timeout(Duration::from_secs(1), receiver.recv())
            .await
            .expect("public event should arrive")
            .expect("receiver should stay open");

        assert!(matches!(received, PublicLaneEvent::Trade(_)));
    }

    #[tokio::test]
    async fn subscribe_ticker_receives_typed_updates() {
        let client = BatMarketsBuilder::default()
            .venue(Venue::Binance)
            .product(Product::LinearUsdt)
            .build()
            .expect("fixture client should build");
        let mut updates =
            client
                .stream()
                .public()
                .subscribe_ticker(WatchInstrumentsRequest::for_instrument(InstrumentId::from(
                    "BTC/USDT:USDT",
                )));

        client
            .stream()
            .public()
            .ingest_json(BINANCE_PUBLIC_TICKER)
            .expect("fixture ticker should parse");

        let received = timeout(Duration::from_secs(1), updates.recv())
            .await
            .expect("typed ticker should arrive")
            .expect("typed ticker should parse");

        assert_eq!(received.instrument_id, InstrumentId::from("BTC/USDT:USDT"));
        assert_eq!(received.last_price.to_string(), "70100.50");
    }

    #[tokio::test]
    async fn subscribe_trades_receives_typed_updates() {
        let client = BatMarketsBuilder::default()
            .venue(Venue::Binance)
            .product(Product::LinearUsdt)
            .build()
            .expect("fixture client should build");
        let mut updates =
            client
                .stream()
                .public()
                .subscribe_trades(WatchInstrumentsRequest::for_instrument(InstrumentId::from(
                    "BTC/USDT:USDT",
                )));

        client
            .stream()
            .public()
            .ingest_json(BINANCE_PUBLIC_TRADE)
            .expect("fixture trade should parse");

        let received = timeout(Duration::from_secs(1), updates.recv())
            .await
            .expect("typed trade should arrive")
            .expect("typed trade should parse");

        assert_eq!(received.instrument_id, InstrumentId::from("BTC/USDT:USDT"));
        assert!(!received.trade_id.as_ref().is_empty());
    }

    #[tokio::test]
    async fn subscribe_book_top_receives_typed_updates() {
        let client = BatMarketsBuilder::default()
            .venue(Venue::Binance)
            .product(Product::LinearUsdt)
            .build()
            .expect("fixture client should build");
        let mut updates =
            client
                .stream()
                .public()
                .subscribe_book_top(WatchInstrumentsRequest::for_instrument(InstrumentId::from(
                    "BTC/USDT:USDT",
                )));

        client
            .stream()
            .public()
            .ingest_json(BINANCE_PUBLIC_BOOK_TICKER)
            .expect("fixture book ticker should parse");

        let received = timeout(Duration::from_secs(1), updates.recv())
            .await
            .expect("typed book top should arrive")
            .expect("typed book top should parse");

        assert_eq!(received.instrument_id, InstrumentId::from("BTC/USDT:USDT"));
        assert_eq!(received.bid.price.to_string(), "70100.90");
        assert_eq!(received.ask.price.to_string(), "70101.10");
    }

    #[tokio::test]
    async fn subscribe_ohlcv_receives_typed_kline_updates() {
        let client = BatMarketsBuilder::default()
            .venue(Venue::Binance)
            .product(Product::LinearUsdt)
            .build()
            .expect("fixture client should build");
        let mut updates =
            client
                .stream()
                .public()
                .subscribe_ohlcv(WatchOhlcvRequest::for_instrument(
                    InstrumentId::from("BTC/USDT:USDT"),
                    "1m",
                ));

        client
            .stream()
            .public()
            .ingest_json(BINANCE_PUBLIC_KLINE)
            .expect("fixture kline should parse");

        let received = timeout(Duration::from_secs(1), updates.recv())
            .await
            .expect("typed kline should arrive")
            .expect("typed kline should parse");

        assert_eq!(received.instrument_id, InstrumentId::from("BTC/USDT:USDT"));
        assert_eq!(received.interval.as_ref(), "1m");
    }

    #[tokio::test]
    async fn subscribe_ohlcv_filters_symbols_before_yielding() {
        let client = BatMarketsBuilder::default()
            .venue(Venue::Binance)
            .product(Product::LinearUsdt)
            .build()
            .expect("fixture client should build");
        let mut updates =
            client
                .stream()
                .public()
                .subscribe_ohlcv(WatchOhlcvRequest::for_instrument(
                    InstrumentId::from("BTC/USDT:USDT"),
                    "1m",
                ));

        client
            .stream()
            .public()
            .ingest_json(
                r#"{
                    "e":"kline",
                    "E":1710000002000,
                    "s":"ETHUSDT",
                    "k":{
                        "i":"1m",
                        "t":1710000000000,
                        "T":1710000059999,
                        "o":"3200.00",
                        "h":"3210.00",
                        "l":"3195.00",
                        "c":"3205.00",
                        "v":"42.0",
                        "x":false
                    }
                }"#,
            )
            .expect("eth kline should parse");
        client
            .stream()
            .public()
            .ingest_json(BINANCE_PUBLIC_KLINE)
            .expect("btc kline should parse");

        let received = timeout(Duration::from_secs(1), updates.recv())
            .await
            .expect("filtered btc kline should arrive")
            .expect("filtered btc kline should parse");

        assert_eq!(received.instrument_id, InstrumentId::from("BTC/USDT:USDT"));
    }
}
