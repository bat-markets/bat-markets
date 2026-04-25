use tokio::sync::broadcast;

use bat_markets_core::{
    AccountSummary, Balance, BookTop, CommandLaneEvent, CommandOperation, CommandReceipt,
    ErrorKind, Execution, FundingRate, HealthNotification, InstrumentId, InstrumentSpec,
    Liquidation, MarkPrice, OpenInterest, Order, Position, PrivateLaneEvent, PublicLaneEvent,
    ReconcileReport, ReconcileTrigger, RequestId, Result, Ticker, TradeTick,
};

use crate::{
    client::BatMarkets, diagnostics::RuntimeDiagnosticsSnapshot, native::NativeClient, runtime,
};

/// Low-level escape hatch for replay tools, custom transports, and diagnostics.
///
/// The primary API is exposed directly on [`BatMarkets`]. Use `advanced()` only
/// when an integration needs raw lane events, fixture ingestion, command
/// response classification, manual reconciliation, diagnostics, or
/// venue-specific native adapter access.
pub struct AdvancedClient<'a> {
    inner: &'a BatMarkets,
}

impl<'a> AdvancedClient<'a> {
    pub(crate) const fn new(inner: &'a BatMarkets) -> Self {
        Self { inner }
    }

    /// Decode a venue public websocket payload and merge its events into state.
    pub fn ingest_public_json(&self, payload: &str) -> Result<Vec<PublicLaneEvent>> {
        let events = self.inner.adapter.as_adapter().parse_public(payload)?;
        self.inner.shared.apply_public_events(&events);
        Ok(events)
    }

    /// Decode a venue private websocket payload and merge its events into state.
    pub fn ingest_private_json(&self, payload: &str) -> Result<Vec<PrivateLaneEvent>> {
        let events = self.inner.adapter.as_adapter().parse_private(payload)?;
        self.inner.shared.apply_private_events(&events);
        Ok(events)
    }

    /// Subscribe to raw public-lane events emitted by ingest or live runtime.
    #[must_use]
    pub fn subscribe_public_events(&self) -> broadcast::Receiver<PublicLaneEvent> {
        self.inner.shared.subscribe_public_events()
    }

    /// Subscribe to raw private-lane events emitted by ingest or live runtime.
    #[must_use]
    pub fn subscribe_private_events(&self) -> broadcast::Receiver<PrivateLaneEvent> {
        self.inner.shared.subscribe_private_events()
    }

    /// Subscribe to raw command-lane lifecycle and receipt events.
    #[must_use]
    pub fn subscribe_command_events(&self) -> broadcast::Receiver<CommandLaneEvent> {
        self.inner.shared.subscribe_command_events()
    }

    /// Subscribe to transition-style health notifications.
    ///
    /// Notifications are emitted for structural runtime changes, not for every
    /// market-data tick. Most applications should prefer [`BatMarkets::watch_status`].
    #[must_use]
    pub fn subscribe_health_notifications(&self) -> broadcast::Receiver<HealthNotification> {
        self.inner.shared.subscribe_health_notifications()
    }

    /// Return metadata for a known instrument or an `Unsupported` error.
    pub fn require_instrument(&self, instrument_id: &InstrumentId) -> Result<InstrumentSpec> {
        self.inner
            .instrument_specs()
            .into_iter()
            .find(|spec| &spec.instrument_id == instrument_id)
            .ok_or_else(|| {
                bat_markets_core::MarketError::new(
                    ErrorKind::Unsupported,
                    format!("unknown instrument {instrument_id}"),
                )
            })
    }

    /// Return the latest cached ticker for an instrument.
    #[must_use]
    pub fn cached_ticker(&self, instrument_id: &InstrumentId) -> Option<Ticker> {
        self.inner
            .read_state(|state| state.ticker(instrument_id).cloned())
    }

    /// Return cached recent public trades for an instrument.
    #[must_use]
    pub fn cached_recent_trades(&self, instrument_id: &InstrumentId) -> Option<Vec<TradeTick>> {
        self.inner
            .read_state(|state| state.recent_trades(instrument_id))
    }

    /// Return the latest cached top-of-book snapshot for an instrument.
    #[must_use]
    pub fn cached_book_top(&self, instrument_id: &InstrumentId) -> Option<BookTop> {
        self.inner
            .read_state(|state| state.book_top(instrument_id).cloned())
    }

    /// Return the latest cached funding-rate snapshot for an instrument.
    #[must_use]
    pub fn cached_funding_rate(&self, instrument_id: &InstrumentId) -> Option<FundingRate> {
        self.inner
            .read_state(|state| state.funding_rate(instrument_id).cloned())
    }

    /// Return the latest cached mark-price snapshot for an instrument.
    #[must_use]
    pub fn cached_mark_price(&self, instrument_id: &InstrumentId) -> Option<MarkPrice> {
        self.inner
            .read_state(|state| state.mark_price(instrument_id).cloned())
    }

    /// Return the latest cached open-interest snapshot for an instrument.
    #[must_use]
    pub fn cached_open_interest(&self, instrument_id: &InstrumentId) -> Option<OpenInterest> {
        self.inner
            .read_state(|state| state.open_interest(instrument_id).cloned())
    }

    /// Return cached liquidation events for an instrument.
    #[must_use]
    pub fn cached_liquidations(&self, instrument_id: &InstrumentId) -> Option<Vec<Liquidation>> {
        self.inner
            .read_state(|state| state.liquidations(instrument_id))
    }

    /// Return cached balances from the private state projection.
    #[must_use]
    pub fn cached_balances(&self) -> Vec<Balance> {
        self.inner
            .read_state(bat_markets_core::EngineState::balances)
    }

    /// Return the cached account summary, if one has been observed or fetched.
    #[must_use]
    pub fn cached_account_summary(&self) -> Option<AccountSummary> {
        self.inner
            .read_state(bat_markets_core::EngineState::account_summary)
    }

    /// Return cached positions from the private state projection.
    #[must_use]
    pub fn cached_positions(&self) -> Vec<Position> {
        self.inner
            .read_state(bat_markets_core::EngineState::positions)
    }

    /// Return all cached orders known to the engine state.
    #[must_use]
    pub fn cached_orders(&self) -> Vec<Order> {
        self.inner.read_state(bat_markets_core::EngineState::orders)
    }

    /// Return cached orders that are currently open.
    #[must_use]
    pub fn cached_open_orders(&self) -> Vec<Order> {
        self.inner
            .read_state(bat_markets_core::EngineState::open_orders)
    }

    /// Return cached private execution fills known to the engine state.
    #[must_use]
    pub fn cached_executions(&self) -> Vec<Execution> {
        self.inner
            .read_state(bat_markets_core::EngineState::executions)
    }

    /// Classify a raw command response payload and apply the resulting state hint.
    ///
    /// This hook is intended for custom transports. Normal write flows should
    /// use root command methods such as [`BatMarkets::create_order`].
    pub fn classify_command_json(
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

    /// Run a manual REST-backed private-state reconciliation pass.
    pub async fn reconcile(&self) -> Result<ReconcileReport> {
        runtime::reconcile_private(&self.inner.live_context(), ReconcileTrigger::Manual).await
    }

    /// Return local runtime and state-lock diagnostics.
    #[must_use]
    pub fn diagnostics(&self) -> RuntimeDiagnosticsSnapshot {
        let mut snapshot = self.inner.runtime_state.diagnostics.snapshot();
        snapshot.state_reads = self.inner.shared.read_diagnostics();
        snapshot.state_writes = self.inner.shared.write_diagnostics();
        snapshot
    }

    /// Access venue-specific adapter functionality.
    #[must_use]
    pub fn native(&self) -> NativeClient<'_> {
        NativeClient::new(&self.inner.adapter)
    }
}
