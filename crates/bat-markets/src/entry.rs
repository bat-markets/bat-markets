use tokio::sync::broadcast;

use bat_markets_core::{
    CommandAck, CommandLaneEvent, CommandLifecycleEvent, CommandReceipt, CommandStatus,
    CommandTransport, ErrorKind, ReconcileOutcome, ReconcileReport, Result,
};

/// Low-latency command handle with lifecycle tracking over the shared command bus.
pub struct PendingCommandHandle {
    ack: CommandAck,
    receiver: broadcast::Receiver<CommandLaneEvent>,
    initial_receipt_pending: bool,
}

impl PendingCommandHandle {
    pub(crate) fn from_ack(
        ack: CommandAck,
        receiver: broadcast::Receiver<CommandLaneEvent>,
    ) -> Self {
        Self {
            ack,
            receiver,
            initial_receipt_pending: true,
        }
    }

    pub(crate) fn from_receipt(
        receipt: CommandReceipt,
        transport: CommandTransport,
        receiver: broadcast::Receiver<CommandLaneEvent>,
    ) -> Self {
        Self::from_ack(
            CommandAck {
                receipt,
                transport,
                acknowledged_at: timestamp_now_ms(),
            },
            receiver,
        )
    }

    /// Return the immediate acknowledgement captured when the command was sent.
    #[must_use]
    pub const fn ack(&self) -> &CommandAck {
        &self.ack
    }

    /// Return the initial receipt, then subsequent matching receipts from the command bus.
    ///
    /// The first call is always the receipt embedded in [`Self::ack`]. Later
    /// calls wait for matching command-lane receipt events.
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

    /// Wait for the next lifecycle event matching this command.
    ///
    /// This observes acknowledgement, recovery scheduling, recovery completion,
    /// and follow-up receipt events emitted by the command lane.
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

    /// Return a receipt that is resolved as far as local evidence allows.
    ///
    /// Non-uncertain receipts are returned immediately. `UnknownExecution`
    /// receipts wait for a matching recovery completion and then map the
    /// reconcile report into the best known final status.
    pub async fn resolved(&mut self) -> Result<CommandReceipt> {
        if self.ack.receipt.status != CommandStatus::UnknownExecution {
            return Ok(self.ack.receipt.clone());
        }

        loop {
            let lifecycle = self.next_lifecycle().await?;
            if let CommandLifecycleEvent::RecoveryCompleted { report, .. } = lifecycle {
                return Ok(receipt_after_recovery(&self.ack.receipt, &report));
            }
        }
    }

    fn receiver_mut(&mut self) -> &mut broadcast::Receiver<CommandLaneEvent> {
        &mut self.receiver
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

fn receipt_after_recovery(receipt: &CommandReceipt, report: &ReconcileReport) -> CommandReceipt {
    let mut resolved = receipt.clone();
    match report.outcome {
        ReconcileOutcome::Synchronized => {
            resolved.status = CommandStatus::Accepted;
            resolved.retriable = false;
            resolved.message = Some(
                report
                    .note
                    .clone()
                    .unwrap_or_else(|| "command outcome resolved by reconcile".into()),
            );
        }
        ReconcileOutcome::StillUncertain | ReconcileOutcome::Diverged => {
            resolved.status = CommandStatus::UnknownExecution;
            resolved.retriable = true;
            resolved.message =
                Some(report.note.clone().unwrap_or_else(|| {
                    "command outcome remains unresolved after reconcile".into()
                }));
        }
    }
    resolved
}

fn timestamp_now_ms() -> bat_markets_core::TimestampMs {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
        .min(i64::MAX as u128) as i64;
    bat_markets_core::TimestampMs::new(millis)
}

#[cfg(test)]
mod tests {
    use bat_markets_core::{
        CommandOperation, CommandReceipt, CommandStatus, Product, ReconcileOutcome,
        ReconcileReport, ReconcileTrigger, TimestampMs, Venue,
    };

    #[test]
    fn recovery_synchronized_returns_accepted_non_retriable_receipt() {
        let receipt = unknown_receipt();
        let report = report(
            ReconcileOutcome::Synchronized,
            "recent history resolved command",
        );

        let resolved = super::receipt_after_recovery(&receipt, &report);

        assert_eq!(resolved.status, CommandStatus::Accepted);
        assert!(!resolved.retriable);
        assert_eq!(
            resolved.message.as_deref(),
            Some("recent history resolved command")
        );
    }

    #[test]
    fn recovery_still_uncertain_keeps_receipt_explicitly_unknown() {
        let receipt = unknown_receipt();
        let report = report(
            ReconcileOutcome::StillUncertain,
            "1 pending command outcomes still unresolved",
        );

        let resolved = super::receipt_after_recovery(&receipt, &report);

        assert_eq!(resolved.status, CommandStatus::UnknownExecution);
        assert!(resolved.retriable);
        assert_eq!(
            resolved.message.as_deref(),
            Some("1 pending command outcomes still unresolved")
        );
    }

    fn unknown_receipt() -> CommandReceipt {
        CommandReceipt {
            operation: CommandOperation::CreateOrder,
            status: CommandStatus::UnknownExecution,
            venue: Venue::Binance,
            product: Product::LinearUsdt,
            instrument_id: None,
            order_id: None,
            client_order_id: None,
            request_id: None,
            message: Some("command outcome requires reconcile".into()),
            native_code: None,
            retriable: true,
        }
    }

    fn report(outcome: ReconcileOutcome, note: &str) -> ReconcileReport {
        ReconcileReport {
            trigger: ReconcileTrigger::UnknownExecution,
            outcome,
            repaired_at: TimestampMs::new(1),
            note: Some(note.into()),
        }
    }
}
