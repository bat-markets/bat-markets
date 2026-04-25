use std::collections::BTreeSet;

use tokio::{
    sync::{broadcast, oneshot},
    task::JoinHandle,
};

use bat_markets_core::{
    AccountSummary, Balance, BookTop, CommandLaneEvent, CommandLifecycleEvent, CommandOperation,
    CommandReceipt, ErrorKind, Execution, FundingRate, InstrumentId, Kline, KlineInterval,
    Liquidation, MarkPrice, OpenInterest, Order, OrderBookDelta, Position, PrivateLaneEvent,
    PublicLaneEvent, ReconcileReport, ReconcileTrigger, RequestId, Result, Ticker, TradeTick,
    WatchFastFeedRequest, WatchOrderBookRequest,
};

use crate::{
    client::BatMarkets,
    runtime,
    subscriptions::{PrivateSubscriptionLease, PublicSubscriptionLease},
};

/// Public market-data subscription plan for live websocket runners.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicSubscription {
    pub instrument_ids: Vec<InstrumentId>,
    pub ticker: bool,
    pub trades: bool,
    pub book_top: bool,
    pub order_book: bool,
    pub mark_price: bool,
    pub funding_rate: bool,
    pub open_interest: bool,
    pub liquidations: bool,
    pub kline_intervals: Vec<Box<str>>,
}

impl PublicSubscription {
    #[must_use]
    pub fn all_for(instrument_ids: Vec<InstrumentId>) -> Self {
        Self {
            instrument_ids,
            ticker: true,
            trades: true,
            book_top: true,
            order_book: false,
            mark_price: true,
            funding_rate: true,
            open_interest: true,
            liquidations: false,
            kline_intervals: Vec::new(),
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
            order_book: false,
            mark_price: false,
            funding_rate: false,
            open_interest: false,
            liquidations: false,
            kline_intervals: vec![self.interval],
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

async fn recv_public_event(
    receiver: &mut broadcast::Receiver<PublicLaneEvent>,
    label: &str,
) -> Result<PublicLaneEvent> {
    loop {
        match receiver.recv().await {
            Ok(event) => return Ok(event),
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(error) => {
                return Err(bat_markets_core::MarketError::new(
                    ErrorKind::TransportError,
                    format!("{label} receive failed: {error}"),
                ));
            }
        }
    }
}

async fn recv_private_event(
    receiver: &mut broadcast::Receiver<PrivateLaneEvent>,
    label: &str,
) -> Result<PrivateLaneEvent> {
    loop {
        match receiver.recv().await {
            Ok(event) => return Ok(event),
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(error) => {
                return Err(bat_markets_core::MarketError::new(
                    ErrorKind::TransportError,
                    format!("{label} receive failed: {error}"),
                ));
            }
        }
    }
}

async fn recv_command_event(
    receiver: &mut broadcast::Receiver<CommandLaneEvent>,
    label: &str,
) -> Result<CommandLaneEvent> {
    loop {
        match receiver.recv().await {
            Ok(event) => return Ok(event),
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(error) => {
                return Err(bat_markets_core::MarketError::new(
                    ErrorKind::TransportError,
                    format!("{label} receive failed: {error}"),
                ));
            }
        }
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
            let event = recv_public_event(&mut self.receiver, "ohlcv subscription").await?;

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
    _lease: PublicSubscriptionLease,
}

impl<'a> OhlcvWatch<'a> {
    pub async fn recv(&mut self) -> Result<Kline> {
        self.updates.recv().await
    }

    pub async fn shutdown(self) -> Result<()> {
        Ok(())
    }

    pub fn abort(&self) {}

    pub async fn wait(self) -> Result<()> {
        Ok(())
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
            let event = recv_public_event(&mut self.receiver, "ticker subscription").await?;

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
    _lease: PublicSubscriptionLease,
}

impl<'a> TickerWatch<'a> {
    pub async fn recv(&mut self) -> Result<Ticker> {
        self.updates.recv().await
    }

    pub async fn shutdown(self) -> Result<()> {
        Ok(())
    }

    pub fn abort(&self) {}

    pub async fn wait(self) -> Result<()> {
        Ok(())
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
            let event = recv_public_event(&mut self.receiver, "trade subscription").await?;

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
    _lease: PublicSubscriptionLease,
}

impl<'a> TradesWatch<'a> {
    pub async fn recv(&mut self) -> Result<TradeTick> {
        self.updates.recv().await
    }

    pub async fn shutdown(self) -> Result<()> {
        Ok(())
    }

    pub fn abort(&self) {}

    pub async fn wait(self) -> Result<()> {
        Ok(())
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
            let event = recv_public_event(&mut self.receiver, "book_top subscription").await?;

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
    _lease: PublicSubscriptionLease,
}

impl<'a> BookTopWatch<'a> {
    pub async fn recv(&mut self) -> Result<BookTop> {
        self.updates.recv().await
    }

    pub async fn shutdown(self) -> Result<()> {
        Ok(())
    }

    pub fn abort(&self) {}

    pub async fn wait(self) -> Result<()> {
        Ok(())
    }
}

/// Typed mark-price subscription receiver over the shared public event bus.
pub struct MarkPriceUpdates<'a> {
    inner: &'a BatMarkets,
    receiver: broadcast::Receiver<PublicLaneEvent>,
    instrument_ids: BTreeSet<InstrumentId>,
}

impl<'a> MarkPriceUpdates<'a> {
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

    pub async fn recv(&mut self) -> Result<MarkPrice> {
        loop {
            let event = recv_public_event(&mut self.receiver, "mark_price subscription").await?;

            let PublicLaneEvent::MarkPrice(mark_price) = event else {
                continue;
            };
            if !self.instrument_ids.contains(&mark_price.instrument_id) {
                continue;
            }

            let spec = resolve_public_spec(self.inner, &mark_price.instrument_id, "mark_price")?;
            return Ok(mark_price.to_unified(&spec));
        }
    }
}

pub struct MarkPriceWatch<'a> {
    updates: MarkPriceUpdates<'a>,
    _lease: PublicSubscriptionLease,
}

impl<'a> MarkPriceWatch<'a> {
    pub async fn recv(&mut self) -> Result<MarkPrice> {
        self.updates.recv().await
    }

    pub async fn shutdown(self) -> Result<()> {
        Ok(())
    }
}

/// Typed funding-rate subscription receiver over the shared public event bus.
pub struct FundingRateUpdates {
    receiver: broadcast::Receiver<PublicLaneEvent>,
    instrument_ids: BTreeSet<InstrumentId>,
}

impl FundingRateUpdates {
    fn new(
        receiver: broadcast::Receiver<PublicLaneEvent>,
        request: WatchInstrumentsRequest,
    ) -> Self {
        Self {
            receiver,
            instrument_ids: request.instrument_ids.into_iter().collect(),
        }
    }

    pub async fn recv(&mut self) -> Result<FundingRate> {
        loop {
            let event = recv_public_event(&mut self.receiver, "funding_rate subscription").await?;

            let PublicLaneEvent::FundingRate(funding_rate) = event else {
                continue;
            };
            if !self.instrument_ids.contains(&funding_rate.instrument_id) {
                continue;
            }
            return Ok(funding_rate);
        }
    }
}

pub struct FundingRateWatch<'a> {
    updates: FundingRateUpdates,
    _lease: PublicSubscriptionLease,
    _marker: std::marker::PhantomData<&'a BatMarkets>,
}

impl<'a> FundingRateWatch<'a> {
    pub async fn recv(&mut self) -> Result<FundingRate> {
        self.updates.recv().await
    }

    pub async fn shutdown(self) -> Result<()> {
        Ok(())
    }
}

/// Typed open-interest subscription receiver over the shared public event bus.
pub struct OpenInterestUpdates {
    receiver: broadcast::Receiver<PublicLaneEvent>,
    instrument_ids: BTreeSet<InstrumentId>,
}

impl OpenInterestUpdates {
    fn new(
        receiver: broadcast::Receiver<PublicLaneEvent>,
        request: WatchInstrumentsRequest,
    ) -> Self {
        Self {
            receiver,
            instrument_ids: request.instrument_ids.into_iter().collect(),
        }
    }

    pub async fn recv(&mut self) -> Result<OpenInterest> {
        loop {
            let event = recv_public_event(&mut self.receiver, "open_interest subscription").await?;

            let PublicLaneEvent::OpenInterest(open_interest) = event else {
                continue;
            };
            if !self.instrument_ids.contains(&open_interest.instrument_id) {
                continue;
            }
            return Ok(open_interest);
        }
    }
}

pub struct OpenInterestWatch<'a> {
    updates: OpenInterestUpdates,
    _lease: PublicSubscriptionLease,
    _marker: std::marker::PhantomData<&'a BatMarkets>,
}

impl<'a> OpenInterestWatch<'a> {
    pub async fn recv(&mut self) -> Result<OpenInterest> {
        self.updates.recv().await
    }

    pub async fn shutdown(self) -> Result<()> {
        Ok(())
    }
}

/// Typed liquidation subscription receiver over the shared public event bus.
pub struct LiquidationUpdates<'a> {
    inner: &'a BatMarkets,
    receiver: broadcast::Receiver<PublicLaneEvent>,
    instrument_ids: BTreeSet<InstrumentId>,
}

impl<'a> LiquidationUpdates<'a> {
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

    pub async fn recv(&mut self) -> Result<Liquidation> {
        loop {
            let event = recv_public_event(&mut self.receiver, "liquidation subscription").await?;

            let PublicLaneEvent::Liquidation(liquidation) = event else {
                continue;
            };
            if !self.instrument_ids.contains(&liquidation.instrument_id) {
                continue;
            }

            let spec = resolve_public_spec(self.inner, &liquidation.instrument_id, "liquidation")?;
            return Ok(liquidation.to_unified(&spec));
        }
    }
}

pub struct LiquidationWatch<'a> {
    updates: LiquidationUpdates<'a>,
    _lease: PublicSubscriptionLease,
}

impl<'a> LiquidationWatch<'a> {
    pub async fn recv(&mut self) -> Result<Liquidation> {
        self.updates.recv().await
    }

    pub async fn shutdown(self) -> Result<()> {
        Ok(())
    }
}

/// Typed order-book delta subscription receiver over the shared public event bus.
pub struct OrderBookUpdates<'a> {
    inner: &'a BatMarkets,
    receiver: broadcast::Receiver<PublicLaneEvent>,
    instrument_id: InstrumentId,
}

impl<'a> OrderBookUpdates<'a> {
    fn new(
        inner: &'a BatMarkets,
        receiver: broadcast::Receiver<PublicLaneEvent>,
        request: WatchOrderBookRequest,
    ) -> Self {
        Self {
            inner,
            receiver,
            instrument_id: request.instrument_id,
        }
    }

    pub async fn recv(&mut self) -> Result<OrderBookDelta> {
        loop {
            let event = recv_public_event(&mut self.receiver, "order_book subscription").await?;

            let PublicLaneEvent::OrderBookDelta(delta) = event else {
                continue;
            };
            if delta.instrument_id != self.instrument_id {
                continue;
            }

            let spec = resolve_public_spec(self.inner, &delta.instrument_id, "order_book")?;
            return Ok(delta.to_unified(&spec));
        }
    }
}

pub struct OrderBookWatch<'a> {
    updates: OrderBookUpdates<'a>,
    _lease: PublicSubscriptionLease,
}

impl<'a> OrderBookWatch<'a> {
    pub async fn recv(&mut self) -> Result<OrderBookDelta> {
        self.updates.recv().await
    }

    pub async fn shutdown(self) -> Result<()> {
        Ok(())
    }
}

/// Compact fast-feed receiver for frontend fanout.
pub struct FastFeedUpdates {
    receiver: broadcast::Receiver<PublicLaneEvent>,
    request: WatchFastFeedRequest,
    instrument_ids: BTreeSet<InstrumentId>,
}

impl FastFeedUpdates {
    fn new(receiver: broadcast::Receiver<PublicLaneEvent>, request: WatchFastFeedRequest) -> Self {
        Self {
            receiver,
            instrument_ids: request.instrument_ids.iter().cloned().collect(),
            request,
        }
    }

    pub async fn recv(&mut self) -> Result<PublicLaneEvent> {
        loop {
            let event = recv_public_event(&mut self.receiver, "fast feed").await?;

            let instrument_id = match &event {
                PublicLaneEvent::Ticker(event) => {
                    if !self.request.ticker {
                        continue;
                    }
                    &event.instrument_id
                }
                PublicLaneEvent::Trade(event) => {
                    if !self.request.trades {
                        continue;
                    }
                    &event.instrument_id
                }
                PublicLaneEvent::BookTop(event) => {
                    if !self.request.book_top {
                        continue;
                    }
                    &event.instrument_id
                }
                PublicLaneEvent::MarkPrice(event) => {
                    if !self.request.mark_price {
                        continue;
                    }
                    &event.instrument_id
                }
                PublicLaneEvent::FundingRate(event) => {
                    if !self.request.funding_rate {
                        continue;
                    }
                    &event.instrument_id
                }
                PublicLaneEvent::OpenInterest(event) => {
                    if !self.request.open_interest {
                        continue;
                    }
                    &event.instrument_id
                }
                PublicLaneEvent::Liquidation(event) => {
                    if !self.request.liquidations {
                        continue;
                    }
                    &event.instrument_id
                }
                _ => continue,
            };

            if self.instrument_ids.contains(instrument_id) {
                return Ok(event);
            }
        }
    }
}

pub struct FastFeedWatch<'a> {
    updates: FastFeedUpdates,
    _lease: PublicSubscriptionLease,
    _marker: std::marker::PhantomData<&'a BatMarkets>,
}

impl<'a> FastFeedWatch<'a> {
    pub async fn recv(&mut self) -> Result<PublicLaneEvent> {
        self.updates.recv().await
    }

    pub async fn shutdown(self) -> Result<()> {
        Ok(())
    }
}

/// Typed order subscription receiver over the shared private event bus.
pub struct OrderUpdates {
    receiver: broadcast::Receiver<PrivateLaneEvent>,
}

impl OrderUpdates {
    fn new(receiver: broadcast::Receiver<PrivateLaneEvent>) -> Self {
        Self { receiver }
    }

    pub async fn recv(&mut self) -> Result<Order> {
        loop {
            let event = recv_private_event(&mut self.receiver, "order subscription").await?;
            if let PrivateLaneEvent::Order(order) = event {
                return Ok(order);
            }
        }
    }
}

pub struct OrdersWatch<'a> {
    updates: OrderUpdates,
    _lease: PrivateSubscriptionLease,
    _marker: std::marker::PhantomData<&'a BatMarkets>,
}

impl<'a> OrdersWatch<'a> {
    pub async fn recv(&mut self) -> Result<Order> {
        self.updates.recv().await
    }

    pub async fn shutdown(self) -> Result<()> {
        Ok(())
    }
}

pub struct ExecutionUpdates {
    receiver: broadcast::Receiver<PrivateLaneEvent>,
}

impl ExecutionUpdates {
    fn new(receiver: broadcast::Receiver<PrivateLaneEvent>) -> Self {
        Self { receiver }
    }

    pub async fn recv(&mut self) -> Result<Execution> {
        loop {
            let event = recv_private_event(&mut self.receiver, "execution subscription").await?;
            if let PrivateLaneEvent::Execution(execution) = event {
                return Ok(execution);
            }
        }
    }
}

pub struct ExecutionsWatch<'a> {
    updates: ExecutionUpdates,
    _lease: PrivateSubscriptionLease,
    _marker: std::marker::PhantomData<&'a BatMarkets>,
}

impl<'a> ExecutionsWatch<'a> {
    pub async fn recv(&mut self) -> Result<Execution> {
        self.updates.recv().await
    }

    pub async fn shutdown(self) -> Result<()> {
        Ok(())
    }
}

pub struct PositionUpdates {
    receiver: broadcast::Receiver<PrivateLaneEvent>,
}

impl PositionUpdates {
    fn new(receiver: broadcast::Receiver<PrivateLaneEvent>) -> Self {
        Self { receiver }
    }

    pub async fn recv(&mut self) -> Result<Position> {
        loop {
            let event = recv_private_event(&mut self.receiver, "position subscription").await?;
            if let PrivateLaneEvent::Position(position) = event {
                return Ok(position);
            }
        }
    }
}

pub struct PositionsWatch<'a> {
    updates: PositionUpdates,
    _lease: PrivateSubscriptionLease,
    _marker: std::marker::PhantomData<&'a BatMarkets>,
}

impl<'a> PositionsWatch<'a> {
    pub async fn recv(&mut self) -> Result<Position> {
        self.updates.recv().await
    }

    pub async fn shutdown(self) -> Result<()> {
        Ok(())
    }
}

pub struct BalanceUpdates {
    receiver: broadcast::Receiver<PrivateLaneEvent>,
}

impl BalanceUpdates {
    fn new(receiver: broadcast::Receiver<PrivateLaneEvent>) -> Self {
        Self { receiver }
    }

    pub async fn recv(&mut self) -> Result<Balance> {
        loop {
            let event = recv_private_event(&mut self.receiver, "balance subscription").await?;
            if let PrivateLaneEvent::Balance(balance) = event {
                return Ok(balance);
            }
        }
    }
}

pub struct BalancesWatch<'a> {
    updates: BalanceUpdates,
    _lease: PrivateSubscriptionLease,
    _marker: std::marker::PhantomData<&'a BatMarkets>,
}

impl<'a> BalancesWatch<'a> {
    pub async fn recv(&mut self) -> Result<Balance> {
        self.updates.recv().await
    }

    pub async fn shutdown(self) -> Result<()> {
        Ok(())
    }
}

pub struct AccountUpdates<'a> {
    inner: &'a BatMarkets,
    receiver: broadcast::Receiver<PrivateLaneEvent>,
}

impl<'a> AccountUpdates<'a> {
    fn new(inner: &'a BatMarkets, receiver: broadcast::Receiver<PrivateLaneEvent>) -> Self {
        Self { inner, receiver }
    }

    pub async fn recv(&mut self) -> Result<AccountSummary> {
        loop {
            let event = recv_private_event(&mut self.receiver, "account subscription").await?;
            if !matches!(event, PrivateLaneEvent::Balance(_)) {
                continue;
            }
            if let Some(summary) = self.inner.account().summary() {
                return Ok(summary);
            }
        }
    }
}

pub struct AccountWatch<'a> {
    updates: AccountUpdates<'a>,
    _lease: PrivateSubscriptionLease,
}

impl<'a> AccountWatch<'a> {
    pub async fn recv(&mut self) -> Result<AccountSummary> {
        self.updates.recv().await
    }

    pub async fn shutdown(self) -> Result<()> {
        Ok(())
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

    /// Subscribe to typed mark-price updates already flowing through the public lane.
    #[must_use]
    pub fn subscribe_mark_prices(&self, request: WatchInstrumentsRequest) -> MarkPriceUpdates<'a> {
        MarkPriceUpdates::new(self.inner, self.subscribe(), request)
    }

    /// Subscribe to typed funding-rate updates already flowing through the public lane.
    #[must_use]
    pub fn subscribe_funding_rates(&self, request: WatchInstrumentsRequest) -> FundingRateUpdates {
        FundingRateUpdates::new(self.subscribe(), request)
    }

    /// Subscribe to typed open-interest updates already flowing through the public lane.
    #[must_use]
    pub fn subscribe_open_interest(&self, request: WatchInstrumentsRequest) -> OpenInterestUpdates {
        OpenInterestUpdates::new(self.subscribe(), request)
    }

    /// Subscribe to typed order-book deltas already flowing through the public lane.
    #[must_use]
    pub fn subscribe_order_book(&self, request: WatchOrderBookRequest) -> OrderBookUpdates<'a> {
        OrderBookUpdates::new(self.inner, self.subscribe(), request)
    }

    /// Subscribe to typed liquidation updates already flowing through the public lane.
    #[must_use]
    pub fn subscribe_liquidations(
        &self,
        request: WatchInstrumentsRequest,
    ) -> LiquidationUpdates<'a> {
        LiquidationUpdates::new(self.inner, self.subscribe(), request)
    }

    /// Subscribe to typed OHLCV updates already flowing through the public lane.
    #[must_use]
    pub fn subscribe_ohlcv(&self, request: WatchOhlcvRequest) -> OhlcvUpdates<'a> {
        OhlcvUpdates::new(self.inner, self.subscribe(), request)
    }

    /// Subscribe to a compact multi-topic fast feed already flowing through the public lane.
    #[must_use]
    pub fn subscribe_fast(&self, request: WatchFastFeedRequest) -> FastFeedUpdates {
        FastFeedUpdates::new(self.subscribe(), request)
    }

    /// Spawn a reconnecting live public-stream runner.
    ///
    /// Prefer `watch_*` / `subscribe_fast(...)` in applications so the subscription hub can
    /// preserve shared subscriptions and avoid accidental duplicate sockets.
    pub async fn spawn_live(&self, subscription: PublicSubscription) -> Result<LiveStreamHandle> {
        runtime::spawn_public_stream(self.inner.live_context(), subscription).await
    }

    /// Spawn a reconnecting live ticker watcher for one or many instruments.
    pub async fn watch_ticker(&self, request: WatchInstrumentsRequest) -> Result<TickerWatch<'a>> {
        let updates = self.subscribe_ticker(request.clone());
        let lease = self
            .inner
            .subscription_hubs
            .public
            .acquire(PublicSubscription {
                instrument_ids: request.instrument_ids,
                ticker: true,
                trades: false,
                book_top: false,
                order_book: false,
                mark_price: false,
                funding_rate: false,
                open_interest: false,
                liquidations: false,
                kline_intervals: Vec::new(),
            })
            .await?;
        Ok(TickerWatch {
            updates,
            _lease: lease,
        })
    }

    /// Spawn a reconnecting live trade watcher for one or many instruments.
    pub async fn watch_trades(&self, request: WatchInstrumentsRequest) -> Result<TradesWatch<'a>> {
        let updates = self.subscribe_trades(request.clone());
        let lease = self
            .inner
            .subscription_hubs
            .public
            .acquire(PublicSubscription {
                instrument_ids: request.instrument_ids,
                ticker: false,
                trades: true,
                book_top: false,
                order_book: false,
                mark_price: false,
                funding_rate: false,
                open_interest: false,
                liquidations: false,
                kline_intervals: Vec::new(),
            })
            .await?;
        Ok(TradesWatch {
            updates,
            _lease: lease,
        })
    }

    /// Spawn a reconnecting live top-of-book watcher for one or many instruments.
    pub async fn watch_book_top(
        &self,
        request: WatchInstrumentsRequest,
    ) -> Result<BookTopWatch<'a>> {
        let updates = self.subscribe_book_top(request.clone());
        let lease = self
            .inner
            .subscription_hubs
            .public
            .acquire(PublicSubscription {
                instrument_ids: request.instrument_ids,
                ticker: false,
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
        Ok(BookTopWatch {
            updates,
            _lease: lease,
        })
    }

    /// Spawn a reconnecting live mark-price watcher for one or many instruments.
    pub async fn watch_mark_prices(
        &self,
        request: WatchInstrumentsRequest,
    ) -> Result<MarkPriceWatch<'a>> {
        let updates = self.subscribe_mark_prices(request.clone());
        let lease = self
            .inner
            .subscription_hubs
            .public
            .acquire(PublicSubscription {
                instrument_ids: request.instrument_ids,
                ticker: false,
                trades: false,
                book_top: false,
                order_book: false,
                mark_price: true,
                funding_rate: false,
                open_interest: false,
                liquidations: false,
                kline_intervals: Vec::new(),
            })
            .await?;
        Ok(MarkPriceWatch {
            updates,
            _lease: lease,
        })
    }

    /// Spawn a reconnecting live funding-rate watcher for one or many instruments.
    pub async fn watch_funding_rates(
        &self,
        request: WatchInstrumentsRequest,
    ) -> Result<FundingRateWatch<'a>> {
        let updates = self.subscribe_funding_rates(request.clone());
        let lease = self
            .inner
            .subscription_hubs
            .public
            .acquire(PublicSubscription {
                instrument_ids: request.instrument_ids,
                ticker: false,
                trades: false,
                book_top: false,
                order_book: false,
                mark_price: false,
                funding_rate: true,
                open_interest: false,
                liquidations: false,
                kline_intervals: Vec::new(),
            })
            .await?;
        Ok(FundingRateWatch {
            updates,
            _lease: lease,
            _marker: std::marker::PhantomData,
        })
    }

    /// Spawn a reconnecting live open-interest watcher for one or many instruments.
    pub async fn watch_open_interest(
        &self,
        request: WatchInstrumentsRequest,
    ) -> Result<OpenInterestWatch<'a>> {
        let updates = self.subscribe_open_interest(request.clone());
        let lease = self
            .inner
            .subscription_hubs
            .public
            .acquire(PublicSubscription {
                instrument_ids: request.instrument_ids,
                ticker: false,
                trades: false,
                book_top: false,
                order_book: false,
                mark_price: false,
                funding_rate: false,
                open_interest: true,
                liquidations: false,
                kline_intervals: Vec::new(),
            })
            .await?;
        Ok(OpenInterestWatch {
            updates,
            _lease: lease,
            _marker: std::marker::PhantomData,
        })
    }

    /// Spawn a reconnecting focused order-book watcher.
    pub async fn watch_order_book(
        &self,
        request: WatchOrderBookRequest,
    ) -> Result<OrderBookWatch<'a>> {
        let updates = self.subscribe_order_book(request.clone());
        let lease = self
            .inner
            .subscription_hubs
            .public
            .acquire(PublicSubscription {
                instrument_ids: vec![request.instrument_id],
                ticker: false,
                trades: false,
                book_top: false,
                order_book: true,
                mark_price: false,
                funding_rate: false,
                open_interest: false,
                liquidations: false,
                kline_intervals: Vec::new(),
            })
            .await?;
        Ok(OrderBookWatch {
            updates,
            _lease: lease,
        })
    }

    /// Spawn a reconnecting liquidation watcher for one or many instruments.
    pub async fn watch_liquidations(
        &self,
        request: WatchInstrumentsRequest,
    ) -> Result<LiquidationWatch<'a>> {
        let updates = self.subscribe_liquidations(request.clone());
        let lease = self
            .inner
            .subscription_hubs
            .public
            .acquire(PublicSubscription {
                instrument_ids: request.instrument_ids,
                ticker: false,
                trades: false,
                book_top: false,
                order_book: false,
                mark_price: false,
                funding_rate: false,
                open_interest: false,
                liquidations: true,
                kline_intervals: Vec::new(),
            })
            .await?;
        Ok(LiquidationWatch {
            updates,
            _lease: lease,
        })
    }

    /// Spawn a reconnecting compact fast feed watcher.
    pub async fn watch_fast(&self, request: WatchFastFeedRequest) -> Result<FastFeedWatch<'a>> {
        let updates = self.subscribe_fast(request.clone());
        let lease = self
            .inner
            .subscription_hubs
            .public
            .acquire(PublicSubscription {
                instrument_ids: request.instrument_ids,
                ticker: request.ticker,
                trades: request.trades,
                book_top: request.book_top,
                order_book: false,
                mark_price: request.mark_price,
                funding_rate: request.funding_rate,
                open_interest: request.open_interest,
                liquidations: request.liquidations,
                kline_intervals: Vec::new(),
            })
            .await?;
        Ok(FastFeedWatch {
            updates,
            _lease: lease,
            _marker: std::marker::PhantomData,
        })
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
        let lease = self
            .inner
            .subscription_hubs
            .public
            .acquire(request.into_public_subscription())
            .await?;
        Ok(OhlcvWatch {
            updates,
            _lease: lease,
        })
    }

    /// Alias with plural naming for consistency with multi-symbol streams.
    pub async fn watch_tickers(&self, request: WatchInstrumentsRequest) -> Result<TickerWatch<'a>> {
        self.watch_ticker(request).await
    }
}

/// Private state-lane ingestion.
pub struct PrivateLaneClient<'a> {
    inner: &'a BatMarkets,
}

impl<'a> PrivateLaneClient<'a> {
    pub fn ingest_json(&self, payload: &str) -> Result<Vec<PrivateLaneEvent>> {
        let events = self.inner.adapter.as_adapter().parse_private(payload)?;
        self.inner.shared.apply_private_events(&events);
        Ok(events)
    }

    /// Subscribe to fast private-lane events emitted by fixture ingest or live runtime.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<PrivateLaneEvent> {
        self.inner.shared.subscribe_private_events()
    }

    #[must_use]
    pub fn subscribe_orders(&self) -> OrderUpdates {
        OrderUpdates::new(self.subscribe())
    }

    #[must_use]
    pub fn subscribe_executions(&self) -> ExecutionUpdates {
        ExecutionUpdates::new(self.subscribe())
    }

    #[must_use]
    pub fn subscribe_positions(&self) -> PositionUpdates {
        PositionUpdates::new(self.subscribe())
    }

    #[must_use]
    pub fn subscribe_balances(&self) -> BalanceUpdates {
        BalanceUpdates::new(self.subscribe())
    }

    #[must_use]
    pub fn subscribe_account(&self) -> AccountUpdates<'a> {
        AccountUpdates::new(self.inner, self.subscribe())
    }

    /// Spawn a reconnecting live private-stream runner.
    ///
    /// Prefer `watch_*` in applications so the private subscription hub preserves one shared
    /// account stream.
    pub async fn spawn_live(&self) -> Result<LiveStreamHandle> {
        runtime::spawn_private_stream(self.inner.live_context()).await
    }

    pub async fn watch_orders(&self) -> Result<OrdersWatch<'a>> {
        let updates = self.subscribe_orders();
        let lease = self.inner.subscription_hubs.private.acquire().await?;
        Ok(OrdersWatch {
            updates,
            _lease: lease,
            _marker: std::marker::PhantomData,
        })
    }

    pub async fn watch_executions(&self) -> Result<ExecutionsWatch<'a>> {
        let updates = self.subscribe_executions();
        let lease = self.inner.subscription_hubs.private.acquire().await?;
        Ok(ExecutionsWatch {
            updates,
            _lease: lease,
            _marker: std::marker::PhantomData,
        })
    }

    pub async fn watch_positions(&self) -> Result<PositionsWatch<'a>> {
        let updates = self.subscribe_positions();
        let lease = self.inner.subscription_hubs.private.acquire().await?;
        Ok(PositionsWatch {
            updates,
            _lease: lease,
            _marker: std::marker::PhantomData,
        })
    }

    pub async fn watch_balances(&self) -> Result<BalancesWatch<'a>> {
        let updates = self.subscribe_balances();
        let lease = self.inner.subscription_hubs.private.acquire().await?;
        Ok(BalancesWatch {
            updates,
            _lease: lease,
            _marker: std::marker::PhantomData,
        })
    }

    pub async fn watch_account(&self) -> Result<AccountWatch<'a>> {
        let updates = self.subscribe_account();
        let lease = self.inner.subscription_hubs.private.acquire().await?;
        Ok(AccountWatch {
            updates,
            _lease: lease,
        })
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
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<CommandLaneEvent> {
        self.inner.shared.subscribe_command_events()
    }

    pub async fn next_lifecycle(&self) -> Result<CommandLifecycleEvent> {
        let mut receiver = self.subscribe();
        loop {
            let event = recv_command_event(&mut receiver, "command subscription").await?;
            if let CommandLaneEvent::Lifecycle(lifecycle) = event {
                return Ok(lifecycle);
            }
        }
    }

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
        self.inner
            .shared
            .emit_command_event(CommandLaneEvent::Receipt(receipt.clone()));
        Ok(receipt)
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::broadcast;
    use tokio::time::{Duration, timeout};

    use bat_markets_core::{InstrumentId, Product, PublicLaneEvent, Venue};

    use crate::{BatMarketsBuilder, WatchInstrumentsRequest, WatchOhlcvRequest};
    use bat_markets_core::{WatchFastFeedRequest, WatchOrderBookRequest};

    use super::recv_public_event;

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
    const BINANCE_PUBLIC_MARK_PRICE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/binance/public_mark_price.json"
    ));
    const BINANCE_PUBLIC_LIQUIDATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/binance/public_liquidation.json"
    ));
    const BYBIT_PUBLIC_ORDERBOOK: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/bybit/public_orderbook.json"
    ));
    const BYBIT_PRIVATE_ORDER: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/bybit/private_order.json"
    ));
    const BYBIT_PRIVATE_EXECUTION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/bybit/private_execution.json"
    ));
    const BYBIT_PRIVATE_WALLET: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/bybit/private_wallet.json"
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
    async fn recv_public_event_skips_lagged_updates() {
        let client = BatMarketsBuilder::default()
            .venue(Venue::Binance)
            .product(Product::LinearUsdt)
            .build()
            .expect("fixture client should build");
        let sample_event = client
            .stream()
            .public()
            .ingest_json(BINANCE_PUBLIC_TICKER)
            .expect("fixture ticker should parse")
            .into_iter()
            .next()
            .expect("ticker fixture should emit one event");

        let (tx, mut receiver) = broadcast::channel(2);
        tx.send(sample_event.clone())
            .expect("first event should enter channel");
        tx.send(sample_event.clone())
            .expect("second event should enter channel");
        tx.send(sample_event)
            .expect("third event should enter channel and force lag");

        let received = timeout(
            Duration::from_secs(1),
            recv_public_event(&mut receiver, "public test subscription"),
        )
        .await
        .expect("lagged public receive should still complete")
        .expect("lagged public receive should retry and succeed");

        assert!(matches!(received, PublicLaneEvent::Ticker(_)));
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

    #[tokio::test]
    async fn subscribe_mark_prices_receives_typed_updates() {
        let client = BatMarketsBuilder::default()
            .venue(Venue::Binance)
            .product(Product::LinearUsdt)
            .build()
            .expect("fixture client should build");
        let mut updates = client.stream().public().subscribe_mark_prices(
            WatchInstrumentsRequest::for_instrument(InstrumentId::from("BTC/USDT:USDT")),
        );

        client
            .stream()
            .public()
            .ingest_json(BINANCE_PUBLIC_MARK_PRICE)
            .expect("fixture mark price should parse");

        let received = timeout(Duration::from_secs(1), updates.recv())
            .await
            .expect("typed mark price should arrive")
            .expect("typed mark price should parse");

        assert_eq!(received.instrument_id, InstrumentId::from("BTC/USDT:USDT"));
    }

    #[tokio::test]
    async fn subscribe_fast_filters_multi_topic_updates() {
        let client = BatMarketsBuilder::default()
            .venue(Venue::Binance)
            .product(Product::LinearUsdt)
            .build()
            .expect("fixture client should build");
        let mut updates = client
            .stream()
            .public()
            .subscribe_fast(WatchFastFeedRequest {
                instrument_ids: vec![InstrumentId::from("BTC/USDT:USDT")],
                ticker: false,
                trades: false,
                book_top: false,
                mark_price: true,
                funding_rate: false,
                open_interest: false,
                liquidations: false,
            });

        client
            .stream()
            .public()
            .ingest_json(BINANCE_PUBLIC_TICKER)
            .expect("fixture ticker should parse");
        client
            .stream()
            .public()
            .ingest_json(BINANCE_PUBLIC_MARK_PRICE)
            .expect("fixture mark price should parse");

        let received = timeout(Duration::from_secs(1), updates.recv())
            .await
            .expect("fast feed mark price should arrive")
            .expect("fast feed event should parse");

        assert!(matches!(received, PublicLaneEvent::MarkPrice(_)));
    }

    #[tokio::test]
    async fn subscribe_order_book_receives_typed_updates() {
        let client = BatMarketsBuilder::default()
            .venue(Venue::Bybit)
            .product(Product::LinearUsdt)
            .build()
            .expect("fixture client should build");
        let mut updates =
            client
                .stream()
                .public()
                .subscribe_order_book(WatchOrderBookRequest::new(
                    InstrumentId::from("BTC/USDT:USDT"),
                    Some(50),
                ));

        client
            .stream()
            .public()
            .ingest_json(BYBIT_PUBLIC_ORDERBOOK)
            .expect("fixture orderbook should parse");

        let received = timeout(Duration::from_secs(1), updates.recv())
            .await
            .expect("typed order book should arrive")
            .expect("typed order book should parse");

        assert_eq!(received.instrument_id, InstrumentId::from("BTC/USDT:USDT"));
        assert!(!received.bids.is_empty());
        assert!(!received.asks.is_empty());
    }

    #[tokio::test]
    async fn subscribe_liquidations_receives_typed_updates() {
        let client = BatMarketsBuilder::default()
            .venue(Venue::Binance)
            .product(Product::LinearUsdt)
            .build()
            .expect("fixture client should build");
        let mut updates = client.stream().public().subscribe_liquidations(
            WatchInstrumentsRequest::for_instrument(InstrumentId::from("BTC/USDT:USDT")),
        );

        client
            .stream()
            .public()
            .ingest_json(BINANCE_PUBLIC_LIQUIDATION)
            .expect("fixture liquidation should parse");

        let received = timeout(Duration::from_secs(1), updates.recv())
            .await
            .expect("typed liquidation should arrive")
            .expect("typed liquidation should parse");

        assert_eq!(received.instrument_id, InstrumentId::from("BTC/USDT:USDT"));
        assert_eq!(received.side, bat_markets_core::Side::Sell);
        assert_eq!(received.quantity.to_string(), "0.014");
    }

    #[tokio::test]
    async fn private_subscribe_order_execution_and_balance_receive_fixture_updates() {
        let client = BatMarketsBuilder::default()
            .venue(Venue::Bybit)
            .product(Product::LinearUsdt)
            .build()
            .expect("fixture client should build");
        let mut orders = client.stream().private().subscribe_orders();
        let mut executions = client.stream().private().subscribe_executions();
        let mut balances = client.stream().private().subscribe_balances();

        client
            .stream()
            .private()
            .ingest_json(BYBIT_PRIVATE_ORDER)
            .expect("fixture private order should parse");
        client
            .stream()
            .private()
            .ingest_json(BYBIT_PRIVATE_EXECUTION)
            .expect("fixture private execution should parse");
        client
            .stream()
            .private()
            .ingest_json(BYBIT_PRIVATE_WALLET)
            .expect("fixture private wallet should parse");

        let order = timeout(Duration::from_secs(1), orders.recv())
            .await
            .expect("order update should arrive")
            .expect("order update should parse");
        let execution = timeout(Duration::from_secs(1), executions.recv())
            .await
            .expect("execution update should arrive")
            .expect("execution update should parse");
        let balance = timeout(Duration::from_secs(1), balances.recv())
            .await
            .expect("balance update should arrive")
            .expect("balance update should parse");

        assert_eq!(order.instrument_id, InstrumentId::from("BTC/USDT:USDT"));
        assert_eq!(execution.instrument_id, InstrumentId::from("BTC/USDT:USDT"));
        assert_eq!(balance.asset.to_string(), "USDT");
    }
}
