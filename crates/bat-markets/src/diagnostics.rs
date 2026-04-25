use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

/// Snapshot of observed lock wait/hold costs for shared engine state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LockDiagnosticsSnapshot {
    /// Number of lock observations included in this snapshot.
    pub operations: u64,
    /// Total observed time waiting to acquire the lock.
    pub wait_total_ns: u64,
    /// Maximum observed time waiting to acquire the lock.
    pub wait_max_ns: u64,
    /// Total observed time holding the lock.
    pub hold_total_ns: u64,
    /// Maximum observed time holding the lock.
    pub hold_max_ns: u64,
}

impl LockDiagnosticsSnapshot {
    /// Return the average lock wait time in nanoseconds.
    #[must_use]
    pub const fn average_wait_ns(&self) -> u64 {
        if self.operations == 0 {
            0
        } else {
            self.wait_total_ns / self.operations
        }
    }

    /// Return the average lock hold time in nanoseconds.
    #[must_use]
    pub const fn average_hold_ns(&self) -> u64 {
        if self.operations == 0 {
            0
        } else {
            self.hold_total_ns / self.operations
        }
    }
}

/// Snapshot of accumulated runtime latency for a named operation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LatencyDiagnosticsSnapshot {
    /// Number of operation observations included in this snapshot.
    pub operations: u64,
    /// Total observed operation latency.
    pub total_ns: u64,
    /// Maximum observed operation latency.
    pub max_ns: u64,
}

impl LatencyDiagnosticsSnapshot {
    /// Return the average observed latency in nanoseconds.
    #[must_use]
    pub const fn average_ns(&self) -> u64 {
        if self.operations == 0 {
            0
        } else {
            self.total_ns / self.operations
        }
    }
}

/// Cheap runtime and lock diagnostics for live operator inspection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RuntimeDiagnosticsSnapshot {
    /// Shared-state read lock wait/hold costs.
    pub state_reads: LockDiagnosticsSnapshot,
    /// Shared-state write lock wait/hold costs.
    pub state_writes: LockDiagnosticsSnapshot,
    /// Live metadata refresh latency.
    pub refresh_metadata: LatencyDiagnosticsSnapshot,
    /// Public REST ticker fetch latency.
    pub fetch_ticker: LatencyDiagnosticsSnapshot,
    /// Public REST mark-price fetch latency.
    pub fetch_mark_price: LatencyDiagnosticsSnapshot,
    /// Public REST funding-rate fetch latency.
    pub fetch_funding_rate: LatencyDiagnosticsSnapshot,
    /// Public REST trade fetch latency.
    pub fetch_trades: LatencyDiagnosticsSnapshot,
    /// Public REST order-book fetch latency.
    pub fetch_order_book: LatencyDiagnosticsSnapshot,
    /// Public liquidation cache/fetch latency.
    pub fetch_liquidations: LatencyDiagnosticsSnapshot,
    /// Public REST OHLCV fetch latency.
    pub fetch_ohlcv: LatencyDiagnosticsSnapshot,
    /// Account refresh latency.
    pub refresh_account: LatencyDiagnosticsSnapshot,
    /// Position refresh latency.
    pub refresh_positions: LatencyDiagnosticsSnapshot,
    /// Open-order refresh latency.
    pub refresh_open_orders: LatencyDiagnosticsSnapshot,
    /// Execution-history refresh latency.
    pub refresh_executions: LatencyDiagnosticsSnapshot,
    /// Single-order REST lookup latency.
    pub get_order: LatencyDiagnosticsSnapshot,
    /// Create-order command latency.
    pub create_order: LatencyDiagnosticsSnapshot,
    /// Batch create-order command latency.
    pub create_orders: LatencyDiagnosticsSnapshot,
    /// Amend-order command latency.
    pub amend_order: LatencyDiagnosticsSnapshot,
    /// Batch amend-order command latency.
    pub amend_orders: LatencyDiagnosticsSnapshot,
    /// Cancel-order command latency.
    pub cancel_order: LatencyDiagnosticsSnapshot,
    /// Batch cancel-order command latency.
    pub cancel_orders: LatencyDiagnosticsSnapshot,
    /// Cancel-all-orders command latency.
    pub cancel_all_orders: LatencyDiagnosticsSnapshot,
    /// Close-position command latency.
    pub close_position: LatencyDiagnosticsSnapshot,
    /// Validate-order command latency.
    pub validate_order: LatencyDiagnosticsSnapshot,
    /// Set-leverage command latency.
    pub set_leverage: LatencyDiagnosticsSnapshot,
    /// Set-margin-mode command latency.
    pub set_margin_mode: LatencyDiagnosticsSnapshot,
    /// Set-position-mode command latency.
    pub set_position_mode: LatencyDiagnosticsSnapshot,
    /// Private reconcile cycle latency.
    pub reconcile_private: LatencyDiagnosticsSnapshot,
    /// Open-interest refresh latency.
    pub refresh_open_interest: LatencyDiagnosticsSnapshot,
}

#[derive(Debug, Default)]
pub(crate) struct SharedStateDiagnostics {
    reads: AtomicLockDiagnostics,
    writes: AtomicLockDiagnostics,
}

impl SharedStateDiagnostics {
    pub(crate) fn observe_read(&self, wait: Duration, hold: Duration) {
        self.reads.observe(wait, hold);
    }

    pub(crate) fn observe_write(&self, wait: Duration, hold: Duration) {
        self.writes.observe(wait, hold);
    }

    pub(crate) fn read_snapshot(&self) -> LockDiagnosticsSnapshot {
        self.reads.snapshot()
    }

    pub(crate) fn write_snapshot(&self) -> LockDiagnosticsSnapshot {
        self.writes.snapshot()
    }
}

#[derive(Debug, Default)]
pub(crate) struct RuntimeDiagnosticsState {
    refresh_metadata: AtomicLatencyDiagnostics,
    fetch_ticker: AtomicLatencyDiagnostics,
    fetch_mark_price: AtomicLatencyDiagnostics,
    fetch_funding_rate: AtomicLatencyDiagnostics,
    fetch_trades: AtomicLatencyDiagnostics,
    fetch_order_book: AtomicLatencyDiagnostics,
    fetch_liquidations: AtomicLatencyDiagnostics,
    fetch_ohlcv: AtomicLatencyDiagnostics,
    refresh_account: AtomicLatencyDiagnostics,
    refresh_positions: AtomicLatencyDiagnostics,
    refresh_open_orders: AtomicLatencyDiagnostics,
    refresh_executions: AtomicLatencyDiagnostics,
    get_order: AtomicLatencyDiagnostics,
    create_order: AtomicLatencyDiagnostics,
    create_orders: AtomicLatencyDiagnostics,
    amend_order: AtomicLatencyDiagnostics,
    amend_orders: AtomicLatencyDiagnostics,
    cancel_order: AtomicLatencyDiagnostics,
    cancel_orders: AtomicLatencyDiagnostics,
    cancel_all_orders: AtomicLatencyDiagnostics,
    close_position: AtomicLatencyDiagnostics,
    validate_order: AtomicLatencyDiagnostics,
    set_leverage: AtomicLatencyDiagnostics,
    set_margin_mode: AtomicLatencyDiagnostics,
    set_position_mode: AtomicLatencyDiagnostics,
    reconcile_private: AtomicLatencyDiagnostics,
    refresh_open_interest: AtomicLatencyDiagnostics,
}

impl RuntimeDiagnosticsState {
    pub(crate) fn observe(&self, operation: RuntimeOperation, elapsed: Duration) {
        let diagnostics = match operation {
            RuntimeOperation::RefreshMetadata => &self.refresh_metadata,
            RuntimeOperation::FetchTicker => &self.fetch_ticker,
            RuntimeOperation::FetchMarkPrice => &self.fetch_mark_price,
            RuntimeOperation::FetchFundingRate => &self.fetch_funding_rate,
            RuntimeOperation::FetchTrades => &self.fetch_trades,
            RuntimeOperation::FetchOrderBook => &self.fetch_order_book,
            RuntimeOperation::FetchLiquidations => &self.fetch_liquidations,
            RuntimeOperation::FetchOhlcv => &self.fetch_ohlcv,
            RuntimeOperation::RefreshAccount => &self.refresh_account,
            RuntimeOperation::RefreshPositions => &self.refresh_positions,
            RuntimeOperation::RefreshOpenOrders => &self.refresh_open_orders,
            RuntimeOperation::RefreshExecutions => &self.refresh_executions,
            RuntimeOperation::GetOrder => &self.get_order,
            RuntimeOperation::CreateOrder => &self.create_order,
            RuntimeOperation::CreateOrders => &self.create_orders,
            RuntimeOperation::AmendOrder => &self.amend_order,
            RuntimeOperation::AmendOrders => &self.amend_orders,
            RuntimeOperation::CancelOrder => &self.cancel_order,
            RuntimeOperation::CancelOrders => &self.cancel_orders,
            RuntimeOperation::CancelAllOrders => &self.cancel_all_orders,
            RuntimeOperation::ClosePosition => &self.close_position,
            RuntimeOperation::ValidateOrder => &self.validate_order,
            RuntimeOperation::SetLeverage => &self.set_leverage,
            RuntimeOperation::SetMarginMode => &self.set_margin_mode,
            RuntimeOperation::SetPositionMode => &self.set_position_mode,
            RuntimeOperation::ReconcilePrivate => &self.reconcile_private,
            RuntimeOperation::RefreshOpenInterest => &self.refresh_open_interest,
        };
        diagnostics.observe(elapsed);
    }

    pub(crate) fn snapshot(&self) -> RuntimeDiagnosticsSnapshot {
        RuntimeDiagnosticsSnapshot {
            state_reads: LockDiagnosticsSnapshot::default(),
            state_writes: LockDiagnosticsSnapshot::default(),
            refresh_metadata: self.refresh_metadata.snapshot(),
            fetch_ticker: self.fetch_ticker.snapshot(),
            fetch_mark_price: self.fetch_mark_price.snapshot(),
            fetch_funding_rate: self.fetch_funding_rate.snapshot(),
            fetch_trades: self.fetch_trades.snapshot(),
            fetch_order_book: self.fetch_order_book.snapshot(),
            fetch_liquidations: self.fetch_liquidations.snapshot(),
            fetch_ohlcv: self.fetch_ohlcv.snapshot(),
            refresh_account: self.refresh_account.snapshot(),
            refresh_positions: self.refresh_positions.snapshot(),
            refresh_open_orders: self.refresh_open_orders.snapshot(),
            refresh_executions: self.refresh_executions.snapshot(),
            get_order: self.get_order.snapshot(),
            create_order: self.create_order.snapshot(),
            create_orders: self.create_orders.snapshot(),
            amend_order: self.amend_order.snapshot(),
            amend_orders: self.amend_orders.snapshot(),
            cancel_order: self.cancel_order.snapshot(),
            cancel_orders: self.cancel_orders.snapshot(),
            cancel_all_orders: self.cancel_all_orders.snapshot(),
            close_position: self.close_position.snapshot(),
            validate_order: self.validate_order.snapshot(),
            set_leverage: self.set_leverage.snapshot(),
            set_margin_mode: self.set_margin_mode.snapshot(),
            set_position_mode: self.set_position_mode.snapshot(),
            reconcile_private: self.reconcile_private.snapshot(),
            refresh_open_interest: self.refresh_open_interest.snapshot(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeOperation {
    RefreshMetadata,
    FetchTicker,
    FetchMarkPrice,
    FetchFundingRate,
    FetchTrades,
    FetchOrderBook,
    FetchLiquidations,
    FetchOhlcv,
    RefreshAccount,
    RefreshPositions,
    RefreshOpenOrders,
    RefreshExecutions,
    GetOrder,
    CreateOrder,
    CreateOrders,
    AmendOrder,
    AmendOrders,
    CancelOrder,
    CancelOrders,
    CancelAllOrders,
    ClosePosition,
    ValidateOrder,
    SetLeverage,
    SetMarginMode,
    SetPositionMode,
    ReconcilePrivate,
    RefreshOpenInterest,
}

#[derive(Debug, Default)]
struct AtomicLockDiagnostics {
    operations: AtomicU64,
    wait_total_ns: AtomicU64,
    wait_max_ns: AtomicU64,
    hold_total_ns: AtomicU64,
    hold_max_ns: AtomicU64,
}

impl AtomicLockDiagnostics {
    fn observe(&self, wait: Duration, hold: Duration) {
        let wait_ns = duration_ns(wait);
        let hold_ns = duration_ns(hold);
        self.operations.fetch_add(1, Ordering::Relaxed);
        self.wait_total_ns.fetch_add(wait_ns, Ordering::Relaxed);
        self.hold_total_ns.fetch_add(hold_ns, Ordering::Relaxed);
        update_max(&self.wait_max_ns, wait_ns);
        update_max(&self.hold_max_ns, hold_ns);
    }

    fn snapshot(&self) -> LockDiagnosticsSnapshot {
        LockDiagnosticsSnapshot {
            operations: self.operations.load(Ordering::Relaxed),
            wait_total_ns: self.wait_total_ns.load(Ordering::Relaxed),
            wait_max_ns: self.wait_max_ns.load(Ordering::Relaxed),
            hold_total_ns: self.hold_total_ns.load(Ordering::Relaxed),
            hold_max_ns: self.hold_max_ns.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Default)]
struct AtomicLatencyDiagnostics {
    operations: AtomicU64,
    total_ns: AtomicU64,
    max_ns: AtomicU64,
}

impl AtomicLatencyDiagnostics {
    fn observe(&self, elapsed: Duration) {
        let elapsed_ns = duration_ns(elapsed);
        self.operations.fetch_add(1, Ordering::Relaxed);
        self.total_ns.fetch_add(elapsed_ns, Ordering::Relaxed);
        update_max(&self.max_ns, elapsed_ns);
    }

    fn snapshot(&self) -> LatencyDiagnosticsSnapshot {
        LatencyDiagnosticsSnapshot {
            operations: self.operations.load(Ordering::Relaxed),
            total_ns: self.total_ns.load(Ordering::Relaxed),
            max_ns: self.max_ns.load(Ordering::Relaxed),
        }
    }
}

fn duration_ns(duration: Duration) -> u64 {
    duration.as_nanos().min(u128::from(u64::MAX)) as u64
}

fn update_max(cell: &AtomicU64, sample: u64) {
    let mut current = cell.load(Ordering::Relaxed);
    while sample > current {
        match cell.compare_exchange_weak(current, sample, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return,
            Err(observed) => current = observed,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{RuntimeDiagnosticsState, RuntimeOperation};
    use crate::BatMarketsBuilder;
    use bat_markets_core::{Product, Venue};

    #[test]
    fn diagnostics_snapshot_tracks_state_lock_activity() {
        let client = BatMarketsBuilder::default()
            .venue(Venue::Binance)
            .product(Product::LinearUsdt)
            .build()
            .expect("fixture client should build");

        let before = client.advanced().diagnostics();
        assert_eq!(before.state_reads.operations, 0);
        assert_eq!(before.state_writes.operations, 0);

        let _ = client.markets();
        client.write_state(|state| state.mark_rest_success(None));

        let after = client.advanced().diagnostics();
        assert!(after.state_reads.operations >= 1);
        assert!(after.state_writes.operations >= 1);
    }

    #[test]
    fn runtime_latency_snapshot_accumulates_average_and_max() {
        let diagnostics = RuntimeDiagnosticsState::default();
        diagnostics.observe(
            RuntimeOperation::ReconcilePrivate,
            Duration::from_micros(100),
        );
        diagnostics.observe(
            RuntimeOperation::ReconcilePrivate,
            Duration::from_micros(300),
        );

        let snapshot = diagnostics.snapshot();
        assert_eq!(snapshot.reconcile_private.operations, 2);
        assert_eq!(snapshot.reconcile_private.average_ns(), 200_000);
        assert_eq!(snapshot.reconcile_private.max_ns, 300_000);
    }
}
