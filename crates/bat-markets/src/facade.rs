use tokio::sync::broadcast;

use bat_markets_core::{
    AccountSnapshot, CancelAllOrdersRequest, CancelOrderRequest, CancelOrdersRequest,
    ClosePositionRequest, CommandAck, CommandLaneEvent, CommandReceipt, CommandTransport,
    CreateOrderRequest, CreateOrdersRequest, EditOrderRequest, EditOrdersRequest, Execution,
    FetchOhlcvRequest, FetchOrderBookRequest, FetchTickersRequest, FetchTradesRequest, FundingRate,
    GetOrderRequest, HealthReport, InstrumentId, InstrumentSpec, Kline, Liquidation,
    ListExecutionsRequest, ListOpenOrdersRequest, MarkPrice, OpenInterest, Order,
    OrderBookSnapshot, Position, Result, SetLeverageRequest, SetMarginModeRequest,
    SetPositionModeRequest, Ticker, TradeTick, ValidateOrderRequest, WatchOrderBookRequest,
};

use crate::{
    advanced::AdvancedClient,
    client::BatMarkets,
    entry::PendingCommandHandle,
    health::StatusWatch,
    runtime::{self, CommandTransportMode},
    stream::{
        BalancesWatch, ExecutionsWatch, FundingRateWatch, LiquidationWatch, MarkPriceWatch,
        OhlcvWatch, OpenInterestWatch, OrderBookWatch, OrdersWatch, PositionsWatch, TickerWatch,
        TradesWatch, WatchInstrumentsRequest, WatchOhlcvRequest,
    },
};

impl BatMarkets {
    /// Return the cached market metadata currently known to the engine.
    ///
    /// This is the CCXT-style local metadata accessor. It never performs
    /// network I/O. Use [`Self::load_markets`] when live venue metadata should
    /// be refreshed first.
    #[must_use]
    pub fn markets(&self) -> Vec<InstrumentSpec> {
        self.instrument_specs()
    }

    /// Refresh venue metadata through REST and return the loaded instruments.
    pub async fn load_markets(&self) -> Result<Vec<InstrumentSpec>> {
        runtime::refresh_metadata(&self.live_context()).await
    }

    /// Fetch the latest ticker snapshot through REST.
    pub async fn fetch_ticker(&self, instrument_id: &InstrumentId) -> Result<Ticker> {
        runtime::fetch_ticker(&self.live_context(), instrument_id).await
    }

    /// Fetch ticker snapshots for one or many instruments through REST.
    pub async fn fetch_tickers(&self, instrument_ids: Vec<InstrumentId>) -> Result<Vec<Ticker>> {
        runtime::fetch_tickers(
            &self.live_context(),
            &FetchTickersRequest::for_instruments(instrument_ids),
        )
        .await
    }

    /// Fetch a focused order-book snapshot through REST.
    pub async fn fetch_order_book(
        &self,
        instrument_id: &InstrumentId,
        limit: Option<usize>,
    ) -> Result<OrderBookSnapshot> {
        runtime::fetch_order_book(
            &self.live_context(),
            &FetchOrderBookRequest::new(instrument_id.clone(), limit),
        )
        .await
    }

    /// Fetch historical OHLCV candles through REST.
    ///
    /// `FetchOhlcvRequest` keeps the time-window and multi-symbol parameters
    /// explicit while preserving the CCXT `fetch_ohlcv` method family.
    pub async fn fetch_ohlcv(&self, request: &FetchOhlcvRequest) -> Result<Vec<Kline>> {
        self.market().fetch_ohlcv(request).await
    }

    /// Fetch recent public trades through REST.
    pub async fn fetch_trades(
        &self,
        instrument_id: &InstrumentId,
        limit: Option<usize>,
    ) -> Result<Vec<TradeTick>> {
        runtime::fetch_trades(
            &self.live_context(),
            &FetchTradesRequest::new(instrument_id.clone(), limit),
        )
        .await
    }

    /// Fetch the latest mark-price snapshot through REST.
    pub async fn fetch_mark_price(&self, instrument_id: &InstrumentId) -> Result<MarkPrice> {
        runtime::fetch_mark_price(&self.live_context(), instrument_id).await
    }

    /// Fetch the latest funding-rate snapshot through REST.
    pub async fn fetch_funding_rate(&self, instrument_id: &InstrumentId) -> Result<FundingRate> {
        runtime::fetch_funding_rate(&self.live_context(), instrument_id).await
    }

    /// Fetch and cache the latest open-interest snapshot through REST.
    pub async fn fetch_open_interest(&self, instrument_id: &InstrumentId) -> Result<OpenInterest> {
        runtime::refresh_open_interest(&self.live_context(), instrument_id).await
    }

    /// Fetch recent liquidation events from the public liquidation cache.
    pub async fn fetch_liquidations(
        &self,
        instrument_id: &InstrumentId,
        limit: Option<usize>,
    ) -> Result<Vec<Liquidation>> {
        runtime::fetch_liquidations(&self.live_context(), instrument_id, limit).await
    }

    /// Fetch account balances and summary through REST.
    pub async fn fetch_balance(&self) -> Result<AccountSnapshot> {
        runtime::fetch_balance(&self.live_context()).await
    }

    /// Fetch current positions through REST.
    pub async fn fetch_positions(&self) -> Result<Vec<Position>> {
        runtime::refresh_positions(&self.live_context()).await
    }

    /// Fetch open orders through REST.
    pub async fn fetch_open_orders(
        &self,
        request: Option<&ListOpenOrdersRequest>,
    ) -> Result<Vec<Order>> {
        runtime::refresh_open_orders(&self.live_context(), request).await
    }

    /// Fetch one order through REST.
    pub async fn fetch_order(&self, request: &GetOrderRequest) -> Result<Order> {
        runtime::get_order(&self.live_context(), request).await
    }

    /// Fetch recent private execution history through REST.
    pub async fn fetch_my_trades(
        &self,
        request: Option<&ListExecutionsRequest>,
    ) -> Result<Vec<Execution>> {
        runtime::refresh_executions(&self.live_context(), request).await
    }

    /// Watch live ticker updates for one instrument over the shared public WS hub.
    pub async fn watch_ticker(&self, instrument_id: InstrumentId) -> Result<TickerWatch<'_>> {
        self.stream()
            .public()
            .watch_ticker(WatchInstrumentsRequest::for_instrument(instrument_id))
            .await
    }

    /// Watch live ticker updates for one or many instruments over the shared public WS hub.
    pub async fn watch_tickers(
        &self,
        instrument_ids: Vec<InstrumentId>,
    ) -> Result<TickerWatch<'_>> {
        self.stream()
            .public()
            .watch_tickers(WatchInstrumentsRequest::for_instruments(instrument_ids))
            .await
    }

    /// Watch live public trades for one instrument over the shared public WS hub.
    pub async fn watch_trades(&self, instrument_id: InstrumentId) -> Result<TradesWatch<'_>> {
        self.stream()
            .public()
            .watch_trades(WatchInstrumentsRequest::for_instrument(instrument_id))
            .await
    }

    /// Watch live public trades for one or many instruments over the shared public WS hub.
    pub async fn watch_trades_for_symbols(
        &self,
        instrument_ids: Vec<InstrumentId>,
    ) -> Result<TradesWatch<'_>> {
        self.stream()
            .public()
            .watch_trades(WatchInstrumentsRequest::for_instruments(instrument_ids))
            .await
    }

    /// Watch live order-book deltas for one instrument over the shared public WS hub.
    pub async fn watch_order_book(
        &self,
        instrument_id: InstrumentId,
        limit: Option<usize>,
    ) -> Result<OrderBookWatch<'_>> {
        self.stream()
            .public()
            .watch_order_book(WatchOrderBookRequest::new(instrument_id, limit))
            .await
    }

    /// Watch live OHLCV candles for one instrument over the shared public WS hub.
    pub async fn watch_ohlcv(
        &self,
        instrument_id: InstrumentId,
        interval: impl Into<Box<str>>,
    ) -> Result<OhlcvWatch<'_>> {
        self.stream()
            .public()
            .watch_ohlcv(WatchOhlcvRequest::for_instrument(instrument_id, interval))
            .await
    }

    /// Watch live OHLCV candles for one or many instruments over the shared public WS hub.
    pub async fn watch_ohlcv_for_symbols(
        &self,
        instrument_ids: Vec<InstrumentId>,
        interval: impl Into<Box<str>>,
    ) -> Result<OhlcvWatch<'_>> {
        self.stream()
            .public()
            .watch_ohlcv(WatchOhlcvRequest::for_instruments(instrument_ids, interval))
            .await
    }

    /// Watch live mark-price updates for one instrument over the shared public WS hub.
    pub async fn watch_mark_price(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<MarkPriceWatch<'_>> {
        self.stream()
            .public()
            .watch_mark_prices(WatchInstrumentsRequest::for_instrument(instrument_id))
            .await
    }

    /// Watch live funding-rate updates for one instrument over the shared public WS hub.
    pub async fn watch_funding_rate(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<FundingRateWatch<'_>> {
        self.stream()
            .public()
            .watch_funding_rates(WatchInstrumentsRequest::for_instrument(instrument_id))
            .await
    }

    /// Watch live open-interest updates for one instrument over the shared public WS hub.
    pub async fn watch_open_interest(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<OpenInterestWatch<'_>> {
        self.stream()
            .public()
            .watch_open_interest(WatchInstrumentsRequest::for_instrument(instrument_id))
            .await
    }

    /// Watch live liquidation events for one instrument over the shared public WS hub.
    pub async fn watch_liquidations(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<LiquidationWatch<'_>> {
        self.stream()
            .public()
            .watch_liquidations(WatchInstrumentsRequest::for_instrument(instrument_id))
            .await
    }

    /// Return the current runtime status snapshot.
    #[must_use]
    pub fn status(&self) -> HealthReport {
        self.shared.health_snapshot()
    }

    /// Watch runtime status changes.
    #[must_use]
    pub fn watch_status(&self) -> StatusWatch {
        StatusWatch::new(self.shared.subscribe_health())
    }

    /// Watch private balance updates over the shared private WS hub.
    pub async fn watch_balance(&self) -> Result<BalancesWatch<'_>> {
        self.stream().private().watch_balances().await
    }

    /// Watch private order updates over the shared private WS hub.
    pub async fn watch_orders(&self) -> Result<OrdersWatch<'_>> {
        self.stream().private().watch_orders().await
    }

    /// Watch private execution updates over the shared private WS hub.
    pub async fn watch_my_trades(&self) -> Result<ExecutionsWatch<'_>> {
        self.stream().private().watch_executions().await
    }

    /// Watch private position updates over the shared private WS hub.
    pub async fn watch_positions(&self) -> Result<PositionsWatch<'_>> {
        self.stream().private().watch_positions().await
    }

    /// Submit a create-order command and return a lifecycle-aware handle.
    pub async fn create_order(&self, request: &CreateOrderRequest) -> Result<PendingCommandHandle> {
        let receiver = self.shared.subscribe_command_events();
        command_handle_from_ack(
            runtime::create_order(&self.live_context(), request).await?,
            receiver,
        )
    }

    /// Submit a batch create-order command.
    pub async fn create_orders(
        &self,
        request: &CreateOrdersRequest,
    ) -> Result<Vec<PendingCommandHandle>> {
        let receivers = pre_subscribe_command_receivers(self, request.orders.len());
        command_handles_from_acks(
            runtime::create_orders(&self.live_context(), request).await?,
            receivers,
        )
    }

    /// Submit an edit-order command.
    pub async fn edit_order(&self, request: &EditOrderRequest) -> Result<PendingCommandHandle> {
        let receiver = self.shared.subscribe_command_events();
        command_handle_from_ack(
            runtime::amend_order(&self.live_context(), request).await?,
            receiver,
        )
    }

    /// Submit a batch edit-order command.
    pub async fn edit_orders(
        &self,
        request: &EditOrdersRequest,
    ) -> Result<Vec<PendingCommandHandle>> {
        let receivers = pre_subscribe_command_receivers(self, request.orders.len());
        command_handles_from_acks(
            runtime::amend_orders(&self.live_context(), request).await?,
            receivers,
        )
    }

    /// Submit a cancel-order command.
    pub async fn cancel_order(&self, request: &CancelOrderRequest) -> Result<PendingCommandHandle> {
        let receiver = self.shared.subscribe_command_events();
        command_handle_from_ack(
            runtime::cancel_order(&self.live_context(), request).await?,
            receiver,
        )
    }

    /// Submit a batch cancel-order command.
    pub async fn cancel_orders(
        &self,
        request: &CancelOrdersRequest,
    ) -> Result<Vec<PendingCommandHandle>> {
        let receivers = pre_subscribe_command_receivers(self, request.orders.len());
        command_handles_from_acks(
            runtime::cancel_orders(&self.live_context(), request).await?,
            receivers,
        )
    }

    /// Cancel all open orders matching the request scope.
    pub async fn cancel_all_orders(
        &self,
        request: &CancelAllOrdersRequest,
    ) -> Result<PendingCommandHandle> {
        let receiver = self.shared.subscribe_command_events();
        command_handle_from_receipt(
            runtime::cancel_all_orders(&self.live_context(), request).await?,
            CommandTransport::Rest,
            receiver,
        )
    }

    /// Submit a close-position command.
    pub async fn close_position(
        &self,
        request: &ClosePositionRequest,
    ) -> Result<PendingCommandHandle> {
        let receiver = self.shared.subscribe_command_events();
        command_handle_from_ack(
            runtime::close_position(&self.live_context(), request).await?,
            receiver,
        )
    }

    /// Validate an order request without intentionally opening a position.
    pub async fn validate_order(
        &self,
        request: &ValidateOrderRequest,
    ) -> Result<PendingCommandHandle> {
        let receiver = self.shared.subscribe_command_events();
        command_handle_from_receipt(
            runtime::validate_order(&self.live_context(), request).await?,
            CommandTransport::Rest,
            receiver,
        )
    }

    /// Set leverage for the requested instrument.
    pub async fn set_leverage(&self, request: &SetLeverageRequest) -> Result<PendingCommandHandle> {
        let receiver = self.shared.subscribe_command_events();
        command_handle_from_receipt(
            runtime::set_leverage(&self.live_context(), request).await?,
            CommandTransport::Rest,
            receiver,
        )
    }

    /// Set margin mode for the requested instrument or account scope.
    pub async fn set_margin_mode(
        &self,
        request: &SetMarginModeRequest,
    ) -> Result<PendingCommandHandle> {
        let receiver = self.shared.subscribe_command_events();
        command_handle_from_receipt(
            runtime::set_margin_mode(&self.live_context(), request).await?,
            CommandTransport::Rest,
            receiver,
        )
    }

    /// Set one-way or hedge position mode where the venue supports it.
    pub async fn set_position_mode(
        &self,
        request: &SetPositionModeRequest,
    ) -> Result<PendingCommandHandle> {
        let receiver = self.shared.subscribe_command_events();
        command_handle_from_receipt(
            runtime::set_position_mode(&self.live_context(), request).await?,
            CommandTransport::Rest,
            receiver,
        )
    }

    /// Submit a create-order command and require websocket command transport.
    pub async fn create_order_ws(
        &self,
        request: &CreateOrderRequest,
    ) -> Result<PendingCommandHandle> {
        let context = self.live_context_with_command_transport(CommandTransportMode::WebSocketOnly);
        let receiver = self.shared.subscribe_command_events();
        command_handle_from_ack(runtime::create_order(&context, request).await?, receiver)
    }

    /// Submit a websocket-only batch create command.
    pub async fn create_orders_ws(
        &self,
        request: &CreateOrdersRequest,
    ) -> Result<Vec<PendingCommandHandle>> {
        let context = self.live_context_with_command_transport(CommandTransportMode::WebSocketOnly);
        let receivers = pre_subscribe_command_receivers(self, request.orders.len());
        command_handles_from_acks(runtime::create_orders(&context, request).await?, receivers)
    }

    /// Submit a websocket-only edit-order command.
    pub async fn edit_order_ws(&self, request: &EditOrderRequest) -> Result<PendingCommandHandle> {
        let context = self.live_context_with_command_transport(CommandTransportMode::WebSocketOnly);
        let receiver = self.shared.subscribe_command_events();
        command_handle_from_ack(runtime::amend_order(&context, request).await?, receiver)
    }

    /// Submit a websocket-only cancel-order command.
    pub async fn cancel_order_ws(
        &self,
        request: &CancelOrderRequest,
    ) -> Result<PendingCommandHandle> {
        let context = self.live_context_with_command_transport(CommandTransportMode::WebSocketOnly);
        let receiver = self.shared.subscribe_command_events();
        command_handle_from_ack(runtime::cancel_order(&context, request).await?, receiver)
    }

    /// Submit a websocket-only batch cancel command.
    pub async fn cancel_orders_ws(
        &self,
        request: &CancelOrdersRequest,
    ) -> Result<Vec<PendingCommandHandle>> {
        let context = self.live_context_with_command_transport(CommandTransportMode::WebSocketOnly);
        let receivers = pre_subscribe_command_receivers(self, request.orders.len());
        command_handles_from_acks(runtime::cancel_orders(&context, request).await?, receivers)
    }

    /// Submit a websocket-only cancel-all command.
    pub async fn cancel_all_orders_ws(
        &self,
        request: &CancelAllOrdersRequest,
    ) -> Result<PendingCommandHandle> {
        let context = self.live_context_with_command_transport(CommandTransportMode::WebSocketOnly);
        let receiver = self.shared.subscribe_command_events();
        command_handle_from_receipt(
            runtime::cancel_all_orders(&context, request).await?,
            CommandTransport::WebSocket,
            receiver,
        )
    }

    /// Submit a websocket-only batch edit command.
    pub async fn edit_orders_ws(
        &self,
        request: &EditOrdersRequest,
    ) -> Result<Vec<PendingCommandHandle>> {
        let context = self.live_context_with_command_transport(CommandTransportMode::WebSocketOnly);
        let receivers = pre_subscribe_command_receivers(self, request.orders.len());
        command_handles_from_acks(runtime::amend_orders(&context, request).await?, receivers)
    }

    /// Access low-level ingestion, raw subscriptions, reconciliation, diagnostics, and native APIs.
    #[must_use]
    pub fn advanced(&self) -> AdvancedClient<'_> {
        AdvancedClient::new(self)
    }
}

fn command_handle_from_ack(
    ack: CommandAck,
    receiver: broadcast::Receiver<CommandLaneEvent>,
) -> Result<PendingCommandHandle> {
    Ok(PendingCommandHandle::from_ack(ack, receiver))
}

fn command_handle_from_receipt(
    receipt: CommandReceipt,
    transport: CommandTransport,
    receiver: broadcast::Receiver<CommandLaneEvent>,
) -> Result<PendingCommandHandle> {
    Ok(PendingCommandHandle::from_receipt(
        receipt, transport, receiver,
    ))
}

fn command_handles_from_acks(
    acks: Vec<CommandAck>,
    receivers: Vec<broadcast::Receiver<CommandLaneEvent>>,
) -> Result<Vec<PendingCommandHandle>> {
    Ok(acks
        .into_iter()
        .zip(receivers)
        .map(|(ack, receiver)| PendingCommandHandle::from_ack(ack, receiver))
        .collect())
}

fn pre_subscribe_command_receivers(
    inner: &BatMarkets,
    count: usize,
) -> Vec<broadcast::Receiver<CommandLaneEvent>> {
    (0..count)
        .map(|_| inner.shared.subscribe_command_events())
        .collect()
}

#[cfg(test)]
mod tests {
    use bat_markets_core::{HealthStatus, Product, Venue};

    use crate::BatMarkets;

    #[test]
    fn root_metadata_and_status_methods_are_available() {
        let client = BatMarkets::builder()
            .venue(Venue::Binance)
            .product(Product::LinearUsdt)
            .build()
            .expect("static client");

        assert!(!client.markets().is_empty());
        assert_eq!(client.status().status, HealthStatus::Disconnected);
        assert_eq!(
            client.watch_status().current().status,
            HealthStatus::Disconnected
        );
    }

    #[cfg(feature = "binance")]
    #[test]
    fn advanced_native_is_the_low_level_escape_hatch() {
        let client = BatMarkets::builder()
            .venue(Venue::Binance)
            .product(Product::LinearUsdt)
            .build()
            .expect("static client");

        assert!(client.advanced().native().binance().is_ok());
    }
}
