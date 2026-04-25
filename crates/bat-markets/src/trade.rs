use bat_markets_core::{
    CancelOrderRequest, CommandReceipt, CreateOrderRequest, Execution, GetOrderRequest,
    ListExecutionsRequest, ListOpenOrdersRequest, Order, Result,
};

use crate::{client::BatMarkets, runtime};

/// Ergonomic access to order and execution state.
pub struct TradeClient<'a> {
    inner: &'a BatMarkets,
}

impl<'a> TradeClient<'a> {
    pub(crate) const fn new(inner: &'a BatMarkets) -> Self {
        Self { inner }
    }

    /// Return all cached orders known to the engine state.
    #[must_use]
    pub fn orders(&self) -> Vec<Order> {
        self.inner.read_state(bat_markets_core::EngineState::orders)
    }

    /// Return cached orders that are currently open.
    #[must_use]
    pub fn open_orders(&self) -> Vec<Order> {
        self.inner
            .read_state(bat_markets_core::EngineState::open_orders)
    }

    /// Return cached execution fills known to the engine state.
    #[must_use]
    pub fn executions(&self) -> Vec<Execution> {
        self.inner
            .read_state(bat_markets_core::EngineState::executions)
    }

    /// Refresh open orders through live REST and merge them into engine state.
    pub async fn refresh_open_orders(
        &self,
        request: Option<&ListOpenOrdersRequest>,
    ) -> Result<Vec<Order>> {
        runtime::refresh_open_orders(&self.inner.live_context(), request).await
    }

    /// Fetch one order through live REST.
    pub async fn get_order(&self, request: &GetOrderRequest) -> Result<Order> {
        runtime::get_order(&self.inner.live_context(), request).await
    }

    /// Refresh recent execution history from venue REST snapshots and merge it into engine state.
    pub async fn refresh_executions(
        &self,
        request: Option<&ListExecutionsRequest>,
    ) -> Result<Vec<Execution>> {
        runtime::refresh_executions(&self.inner.live_context(), request).await
    }

    /// Submit a live create-order command.
    ///
    /// Transport failures are classified into explicit `UnknownExecution` receipts rather than
    /// being hidden as generic timeouts.
    #[deprecated(
        since = "0.2.0",
        note = "use entry().create_order(...) for low-latency command handles and lifecycle tracking"
    )]
    pub async fn create_order(&self, request: &CreateOrderRequest) -> Result<CommandReceipt> {
        Ok(runtime::create_order(&self.inner.live_context(), request)
            .await?
            .receipt)
    }

    /// Submit a live cancel-order command and return only the immediate receipt.
    ///
    /// Prefer [`EntryClient::cancel_order`](crate::EntryClient::cancel_order)
    /// for lifecycle-aware command tracking.
    #[deprecated(
        since = "0.2.0",
        note = "use entry().cancel_order(...) for low-latency command handles and lifecycle tracking"
    )]
    pub async fn cancel_order(&self, request: &CancelOrderRequest) -> Result<CommandReceipt> {
        Ok(runtime::cancel_order(&self.inner.live_context(), request)
            .await?
            .receipt)
    }
}
