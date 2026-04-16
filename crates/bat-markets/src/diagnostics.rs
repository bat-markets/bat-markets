use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use crate::client::BatMarkets;

/// Snapshot of observed lock wait/hold costs for shared engine state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LockDiagnosticsSnapshot {
    pub operations: u64,
    pub wait_total_ns: u64,
    pub wait_max_ns: u64,
    pub hold_total_ns: u64,
    pub hold_max_ns: u64,
}

impl LockDiagnosticsSnapshot {
    #[must_use]
    pub const fn average_wait_ns(&self) -> u64 {
        if self.operations == 0 {
            0
        } else {
            self.wait_total_ns / self.operations
        }
    }

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
    pub operations: u64,
    pub total_ns: u64,
    pub max_ns: u64,
}

impl LatencyDiagnosticsSnapshot {
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
    pub state_reads: LockDiagnosticsSnapshot,
    pub state_writes: LockDiagnosticsSnapshot,
    pub refresh_metadata: LatencyDiagnosticsSnapshot,
    pub fetch_ohlcv: LatencyDiagnosticsSnapshot,
    pub refresh_account: LatencyDiagnosticsSnapshot,
    pub refresh_positions: LatencyDiagnosticsSnapshot,
    pub refresh_open_orders: LatencyDiagnosticsSnapshot,
    pub refresh_executions: LatencyDiagnosticsSnapshot,
    pub get_order: LatencyDiagnosticsSnapshot,
    pub create_order: LatencyDiagnosticsSnapshot,
    pub cancel_order: LatencyDiagnosticsSnapshot,
    pub reconcile_private: LatencyDiagnosticsSnapshot,
    pub refresh_open_interest: LatencyDiagnosticsSnapshot,
}

/// Read-only access to runtime and contention diagnostics.
pub struct DiagnosticsClient<'a> {
    inner: &'a BatMarkets,
}

impl<'a> DiagnosticsClient<'a> {
    pub(crate) const fn new(inner: &'a BatMarkets) -> Self {
        Self { inner }
    }

    #[must_use]
    pub fn snapshot(&self) -> RuntimeDiagnosticsSnapshot {
        let mut snapshot = self.inner.runtime_state.diagnostics.snapshot();
        snapshot.state_reads = self.inner.shared.read_diagnostics();
        snapshot.state_writes = self.inner.shared.write_diagnostics();
        snapshot
    }
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
    fetch_ohlcv: AtomicLatencyDiagnostics,
    refresh_account: AtomicLatencyDiagnostics,
    refresh_positions: AtomicLatencyDiagnostics,
    refresh_open_orders: AtomicLatencyDiagnostics,
    refresh_executions: AtomicLatencyDiagnostics,
    get_order: AtomicLatencyDiagnostics,
    create_order: AtomicLatencyDiagnostics,
    cancel_order: AtomicLatencyDiagnostics,
    reconcile_private: AtomicLatencyDiagnostics,
    refresh_open_interest: AtomicLatencyDiagnostics,
}

impl RuntimeDiagnosticsState {
    pub(crate) fn observe(&self, operation: RuntimeOperation, elapsed: Duration) {
        let diagnostics = match operation {
            RuntimeOperation::RefreshMetadata => &self.refresh_metadata,
            RuntimeOperation::FetchOhlcv => &self.fetch_ohlcv,
            RuntimeOperation::RefreshAccount => &self.refresh_account,
            RuntimeOperation::RefreshPositions => &self.refresh_positions,
            RuntimeOperation::RefreshOpenOrders => &self.refresh_open_orders,
            RuntimeOperation::RefreshExecutions => &self.refresh_executions,
            RuntimeOperation::GetOrder => &self.get_order,
            RuntimeOperation::CreateOrder => &self.create_order,
            RuntimeOperation::CancelOrder => &self.cancel_order,
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
            fetch_ohlcv: self.fetch_ohlcv.snapshot(),
            refresh_account: self.refresh_account.snapshot(),
            refresh_positions: self.refresh_positions.snapshot(),
            refresh_open_orders: self.refresh_open_orders.snapshot(),
            refresh_executions: self.refresh_executions.snapshot(),
            get_order: self.get_order.snapshot(),
            create_order: self.create_order.snapshot(),
            cancel_order: self.cancel_order.snapshot(),
            reconcile_private: self.reconcile_private.snapshot(),
            refresh_open_interest: self.refresh_open_interest.snapshot(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeOperation {
    RefreshMetadata,
    FetchOhlcv,
    RefreshAccount,
    RefreshPositions,
    RefreshOpenOrders,
    RefreshExecutions,
    GetOrder,
    CreateOrder,
    CancelOrder,
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

        let before = client.diagnostics().snapshot();
        assert_eq!(before.state_reads.operations, 0);
        assert_eq!(before.state_writes.operations, 0);

        let _ = client.market().instrument_specs();
        client.write_state(|state| state.mark_rest_success(None));

        let after = client.diagnostics().snapshot();
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
