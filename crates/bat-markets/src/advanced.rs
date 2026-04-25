use tokio::sync::broadcast;

use bat_markets_core::{
    CommandLaneEvent, CommandOperation, CommandReceipt, PrivateLaneEvent, PublicLaneEvent,
    ReconcileReport, ReconcileTrigger, RequestId, Result,
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
