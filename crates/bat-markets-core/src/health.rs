use serde::{Deserialize, Serialize};

use crate::primitives::TimestampMs;

/// Stable engine health state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    Healthy,
    DegradedPublicStream,
    DegradedPrivateStream,
    ReconcileRequired,
    CommandUncertain,
    ReadOnlySafe,
    Disconnected,
}

/// Why the engine entered a degraded mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DegradedReason {
    PublicStreamGap,
    PrivateStreamGap,
    ReconcileRequired,
    CommandUncertain,
    SnapshotStale,
    StateDivergence,
    Disconnected,
}

/// Cheap snapshot of engine health.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthReport {
    pub status: HealthStatus,
    pub rest_ok: bool,
    pub ws_public_ok: bool,
    pub ws_private_ok: bool,
    pub clock_skew_ms: Option<i64>,
    pub last_public_msg_at: Option<TimestampMs>,
    pub last_private_msg_at: Option<TimestampMs>,
    pub reconnect_count: u64,
    pub snapshot_age_ms: Option<u64>,
    pub state_divergence: bool,
    pub degraded_reason: Option<DegradedReason>,
}

/// Emitted when the health snapshot changes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthNotification {
    pub previous: HealthReport,
    pub current: HealthReport,
}

impl Default for HealthReport {
    fn default() -> Self {
        Self {
            status: HealthStatus::Disconnected,
            rest_ok: false,
            ws_public_ok: false,
            ws_private_ok: false,
            clock_skew_ms: None,
            last_public_msg_at: None,
            last_private_msg_at: None,
            reconnect_count: 0,
            snapshot_age_ms: None,
            state_divergence: false,
            degraded_reason: Some(DegradedReason::Disconnected),
        }
    }
}

impl HealthReport {
    pub fn observe_public_message(&mut self, timestamp: TimestampMs) {
        self.last_public_msg_at = Some(timestamp);
        self.ws_public_ok = true;
        if matches!(
            self.status,
            HealthStatus::Disconnected
                | HealthStatus::DegradedPublicStream
                | HealthStatus::ReadOnlySafe
        ) && !self.state_divergence
            && !matches!(
                self.degraded_reason,
                Some(
                    DegradedReason::ReconcileRequired
                        | DegradedReason::CommandUncertain
                        | DegradedReason::StateDivergence
                )
            )
        {
            self.status = HealthStatus::Healthy;
            self.degraded_reason = None;
        }
    }

    pub fn observe_private_message(&mut self, timestamp: TimestampMs) {
        self.last_private_msg_at = Some(timestamp);
        self.ws_private_ok = true;
        if matches!(
            self.status,
            HealthStatus::Disconnected
                | HealthStatus::DegradedPrivateStream
                | HealthStatus::ReadOnlySafe
        ) {
            self.status = HealthStatus::Healthy;
            self.degraded_reason = None;
        }
    }

    pub fn observe_rest_success(&mut self, clock_skew_ms: Option<i64>) {
        self.rest_ok = true;
        if clock_skew_ms.is_some() {
            self.clock_skew_ms = clock_skew_ms;
        }
        if matches!(self.status, HealthStatus::Disconnected) {
            self.status = HealthStatus::Healthy;
            self.degraded_reason = None;
        }
    }

    pub fn mark_public_disconnect(&mut self) {
        self.ws_public_ok = false;
        if self.ws_private_ok || self.rest_ok {
            self.status = HealthStatus::DegradedPublicStream;
            self.degraded_reason = Some(DegradedReason::PublicStreamGap);
        } else {
            self.status = HealthStatus::Disconnected;
            self.degraded_reason = Some(DegradedReason::Disconnected);
        }
    }

    pub fn mark_public_gap(&mut self) {
        self.status = HealthStatus::DegradedPublicStream;
        self.ws_public_ok = false;
        self.degraded_reason = Some(DegradedReason::PublicStreamGap);
    }

    pub fn mark_private_disconnect(&mut self) {
        self.ws_private_ok = false;
        if self.ws_public_ok || self.rest_ok {
            self.status = HealthStatus::ReadOnlySafe;
            self.degraded_reason = Some(DegradedReason::PrivateStreamGap);
        } else {
            self.status = HealthStatus::Disconnected;
            self.degraded_reason = Some(DegradedReason::Disconnected);
        }
    }

    pub fn mark_private_gap(&mut self) {
        self.status = HealthStatus::ReconcileRequired;
        self.ws_private_ok = false;
        self.degraded_reason = Some(DegradedReason::PrivateStreamGap);
    }

    pub fn mark_snapshot_age(&mut self, age_ms: u64, stale_after_ms: u64) {
        self.snapshot_age_ms = Some(age_ms);
        if age_ms >= stale_after_ms
            && !matches!(
                self.degraded_reason,
                Some(DegradedReason::CommandUncertain | DegradedReason::StateDivergence)
            )
        {
            self.status = HealthStatus::ReconcileRequired;
            self.degraded_reason = Some(DegradedReason::SnapshotStale);
        }
    }

    pub fn mark_reconnect(&mut self) {
        self.reconnect_count += 1;
    }

    pub fn mark_reconcile_required(&mut self) {
        self.status = HealthStatus::ReconcileRequired;
        self.degraded_reason = Some(DegradedReason::ReconcileRequired);
    }

    pub fn mark_command_uncertain(&mut self) {
        self.status = HealthStatus::CommandUncertain;
        self.degraded_reason = Some(DegradedReason::CommandUncertain);
    }

    pub fn mark_state_divergence(&mut self) {
        self.status = HealthStatus::ReconcileRequired;
        self.state_divergence = true;
        self.degraded_reason = Some(DegradedReason::StateDivergence);
    }

    pub fn mark_reconcile_complete(&mut self) {
        self.state_divergence = false;
        self.snapshot_age_ms = Some(0);
        if self.ws_public_ok && (self.ws_private_ok || self.rest_ok) {
            self.status = HealthStatus::Healthy;
            self.degraded_reason = None;
        } else if self.ws_public_ok {
            self.status = HealthStatus::ReadOnlySafe;
            self.degraded_reason = Some(DegradedReason::PrivateStreamGap);
        } else {
            self.status = HealthStatus::Disconnected;
            self.degraded_reason = Some(DegradedReason::Disconnected);
        }
    }
}
