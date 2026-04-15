use serde::{Deserialize, Serialize};

use crate::account::{AccountSummary, Balance};
use crate::position::Position;
use crate::primitives::TimestampMs;
use crate::trade::Order;

/// Why the engine initiated a reconcile cycle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReconcileTrigger {
    Manual,
    Reconnect,
    SequenceGap,
    UnknownExecution,
    Periodic,
}

/// Outcome of a repair attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReconcileOutcome {
    Synchronized,
    StillUncertain,
    Diverged,
}

/// Result of a reconcile pass.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReconcileReport {
    pub trigger: ReconcileTrigger,
    pub outcome: ReconcileOutcome,
    pub repaired_at: TimestampMs,
    pub note: Option<Box<str>>,
}

/// REST-backed account snapshot used by repair flows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AccountSnapshot {
    pub balances: Vec<Balance>,
    pub summary: Option<AccountSummary>,
}

/// Minimal repairable private-state snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PrivateSnapshot {
    pub account: Option<AccountSnapshot>,
    pub positions: Vec<Position>,
    pub open_orders: Vec<Order>,
}
