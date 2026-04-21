use std::sync::Arc;

use tokio::sync::broadcast;

use bat_markets_core::{
    AmendOrderRequest, AmendOrdersRequest, CancelAllOrdersRequest, CancelOrderRequest,
    CancelOrdersRequest, ClosePositionRequest, CommandAck, CommandLaneEvent, CommandLifecycleEvent,
    CommandReceipt, CommandStatus, CommandTransport, CreateOrderRequest, CreateOrdersRequest,
    ErrorKind, Result, SetLeverageRequest, SetMarginModeRequest, SetPositionModeRequest,
    ValidateOrderRequest,
};

use crate::{
    client::{BatMarkets, SharedState},
    runtime,
};

/// Low-latency command handle with lifecycle tracking over the shared command bus.
pub struct PendingCommandHandle {
    ack: CommandAck,
    shared: Arc<SharedState>,
    receiver: Option<broadcast::Receiver<CommandLaneEvent>>,
    initial_receipt_pending: bool,
}

impl PendingCommandHandle {
    pub(crate) fn from_ack(inner: &BatMarkets, ack: CommandAck) -> Self {
        Self {
            ack,
            shared: Arc::clone(&inner.shared),
            receiver: None,
            initial_receipt_pending: true,
        }
    }

    pub(crate) fn from_receipt(
        inner: &BatMarkets,
        receipt: CommandReceipt,
        transport: CommandTransport,
    ) -> Self {
        Self::from_ack(
            inner,
            CommandAck {
                receipt,
                transport,
                acknowledged_at: timestamp_now_ms(),
            },
        )
    }

    #[must_use]
    pub const fn ack(&self) -> &CommandAck {
        &self.ack
    }

    pub async fn receipt(&mut self) -> Result<CommandReceipt> {
        if self.initial_receipt_pending {
            self.initial_receipt_pending = false;
            return Ok(self.ack.receipt.clone());
        }

        loop {
            let event = self.receiver_mut().recv().await.map_err(|error| {
                bat_markets_core::MarketError::new(
                    ErrorKind::TransportError,
                    format!("command receipt receive failed: {error}"),
                )
            })?;

            let CommandLaneEvent::Receipt(receipt) = event else {
                continue;
            };
            if matches_receipt(&self.ack.receipt, &receipt) {
                return Ok(receipt);
            }
        }
    }

    pub async fn next_lifecycle(&mut self) -> Result<CommandLifecycleEvent> {
        loop {
            let event = self.receiver_mut().recv().await.map_err(|error| {
                bat_markets_core::MarketError::new(
                    ErrorKind::TransportError,
                    format!("command lifecycle receive failed: {error}"),
                )
            })?;

            let CommandLaneEvent::Lifecycle(lifecycle) = event else {
                continue;
            };
            if matches_lifecycle(&self.ack.receipt, &lifecycle) {
                return Ok(lifecycle);
            }
        }
    }

    pub async fn resolved(&mut self) -> Result<CommandReceipt> {
        if self.ack.receipt.status != CommandStatus::UnknownExecution {
            return Ok(self.ack.receipt.clone());
        }

        loop {
            let lifecycle = self.next_lifecycle().await?;
            if matches!(lifecycle, CommandLifecycleEvent::RecoveryCompleted { .. }) {
                return Ok(self.ack.receipt.clone());
            }
        }
    }

    fn receiver_mut(&mut self) -> &mut broadcast::Receiver<CommandLaneEvent> {
        self.receiver
            .get_or_insert_with(|| self.shared.subscribe_command_events())
    }
}

/// Low-latency order-entry surface separated from read-side trade queries.
pub struct EntryClient<'a> {
    inner: &'a BatMarkets,
}

impl<'a> EntryClient<'a> {
    pub(crate) const fn new(inner: &'a BatMarkets) -> Self {
        Self { inner }
    }

    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<CommandLaneEvent> {
        self.inner.shared.subscribe_command_events()
    }

    pub async fn create_order(&self, request: &CreateOrderRequest) -> Result<PendingCommandHandle> {
        let ack = runtime::create_order(&self.inner.live_context(), request).await?;
        Ok(PendingCommandHandle::from_ack(self.inner, ack))
    }

    pub async fn create_orders(
        &self,
        request: &CreateOrdersRequest,
    ) -> Result<Vec<PendingCommandHandle>> {
        let acks = runtime::create_orders(&self.inner.live_context(), request).await?;
        Ok(acks
            .into_iter()
            .map(|ack| PendingCommandHandle::from_ack(self.inner, ack))
            .collect())
    }

    pub async fn amend_order(&self, request: &AmendOrderRequest) -> Result<PendingCommandHandle> {
        let ack = runtime::amend_order(&self.inner.live_context(), request).await?;
        Ok(PendingCommandHandle::from_ack(self.inner, ack))
    }

    pub async fn amend_orders(
        &self,
        request: &AmendOrdersRequest,
    ) -> Result<Vec<PendingCommandHandle>> {
        let acks = runtime::amend_orders(&self.inner.live_context(), request).await?;
        Ok(acks
            .into_iter()
            .map(|ack| PendingCommandHandle::from_ack(self.inner, ack))
            .collect())
    }

    pub async fn cancel_order(&self, request: &CancelOrderRequest) -> Result<PendingCommandHandle> {
        let ack = runtime::cancel_order(&self.inner.live_context(), request).await?;
        Ok(PendingCommandHandle::from_ack(self.inner, ack))
    }

    pub async fn cancel_orders(
        &self,
        request: &CancelOrdersRequest,
    ) -> Result<Vec<PendingCommandHandle>> {
        let acks = runtime::cancel_orders(&self.inner.live_context(), request).await?;
        Ok(acks
            .into_iter()
            .map(|ack| PendingCommandHandle::from_ack(self.inner, ack))
            .collect())
    }

    pub async fn cancel_all_orders(
        &self,
        request: &CancelAllOrdersRequest,
    ) -> Result<PendingCommandHandle> {
        let receipt = runtime::cancel_all_orders(&self.inner.live_context(), request).await?;
        Ok(PendingCommandHandle::from_receipt(
            self.inner,
            receipt,
            CommandTransport::Rest,
        ))
    }

    pub async fn close_position(
        &self,
        request: &ClosePositionRequest,
    ) -> Result<PendingCommandHandle> {
        let ack = runtime::close_position(&self.inner.live_context(), request).await?;
        Ok(PendingCommandHandle::from_ack(self.inner, ack))
    }

    pub async fn validate_order(
        &self,
        request: &ValidateOrderRequest,
    ) -> Result<PendingCommandHandle> {
        let receipt = runtime::validate_order(&self.inner.live_context(), request).await?;
        Ok(PendingCommandHandle::from_receipt(
            self.inner,
            receipt,
            CommandTransport::Rest,
        ))
    }

    pub async fn set_leverage(&self, request: &SetLeverageRequest) -> Result<PendingCommandHandle> {
        let receipt = runtime::set_leverage(&self.inner.live_context(), request).await?;
        Ok(PendingCommandHandle::from_receipt(
            self.inner,
            receipt,
            CommandTransport::Rest,
        ))
    }

    pub async fn set_margin_mode(
        &self,
        request: &SetMarginModeRequest,
    ) -> Result<PendingCommandHandle> {
        let receipt = runtime::set_margin_mode(&self.inner.live_context(), request).await?;
        Ok(PendingCommandHandle::from_receipt(
            self.inner,
            receipt,
            CommandTransport::Rest,
        ))
    }

    pub async fn set_position_mode(
        &self,
        request: &SetPositionModeRequest,
    ) -> Result<PendingCommandHandle> {
        let receipt = runtime::set_position_mode(&self.inner.live_context(), request).await?;
        Ok(PendingCommandHandle::from_receipt(
            self.inner,
            receipt,
            CommandTransport::Rest,
        ))
    }
}

fn matches_lifecycle(receipt: &CommandReceipt, lifecycle: &CommandLifecycleEvent) -> bool {
    match lifecycle {
        CommandLifecycleEvent::Ack(ack)
        | CommandLifecycleEvent::RecoveryScheduled(ack)
        | CommandLifecycleEvent::RecoveryCompleted { ack, .. } => {
            matches_receipt(receipt, &ack.receipt)
        }
        CommandLifecycleEvent::Receipt(other) => matches_receipt(receipt, other),
    }
}

fn matches_receipt(left: &CommandReceipt, right: &CommandReceipt) -> bool {
    if left.operation != right.operation {
        return false;
    }

    if let (Some(left_id), Some(right_id)) = (&left.order_id, &right.order_id) {
        return left_id == right_id;
    }
    if let (Some(left_id), Some(right_id)) = (&left.client_order_id, &right.client_order_id) {
        return left_id == right_id;
    }
    if let (Some(left_id), Some(right_id)) = (&left.request_id, &right.request_id) {
        return left_id == right_id;
    }

    left.instrument_id == right.instrument_id
}

fn timestamp_now_ms() -> bat_markets_core::TimestampMs {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
        .min(i64::MAX as u128) as i64;
    bat_markets_core::TimestampMs::new(millis)
}
