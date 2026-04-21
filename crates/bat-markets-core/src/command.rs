use serde::{Deserialize, Serialize};

use crate::ids::{ClientOrderId, InstrumentId, OrderId, RequestId};
use crate::primitives::TimestampMs;
use crate::reconcile::ReconcileReport;
use crate::types::{Product, Venue};

/// Supported command-lane operations in `0.1.x`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandOperation {
    CreateOrder,
    CreateOrders,
    AmendOrder,
    AmendOrders,
    CancelOrder,
    CancelOrders,
    CancelAllOrders,
    ClosePosition,
    ValidateOrder,
    GetOrder,
    SetLeverage,
    SetMarginMode,
    SetPositionMode,
}

/// Result of command classification.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandStatus {
    Accepted,
    Rejected,
    UnknownExecution,
}

/// Command-lane output suitable for state hints and caller handling.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandReceipt {
    pub operation: CommandOperation,
    pub status: CommandStatus,
    pub venue: Venue,
    pub product: Product,
    pub instrument_id: Option<InstrumentId>,
    pub order_id: Option<OrderId>,
    pub client_order_id: Option<ClientOrderId>,
    pub request_id: Option<RequestId>,
    pub message: Option<Box<str>>,
    pub native_code: Option<Box<str>>,
    pub retriable: bool,
}

/// Transport path chosen for a command attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandTransport {
    Auto,
    Rest,
    WebSocket,
}

/// Fast acknowledgement returned on the command hot path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandAck {
    pub receipt: CommandReceipt,
    pub transport: CommandTransport,
    pub acknowledged_at: TimestampMs,
}

/// Lifecycle notifications for low-latency command tracking.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandLifecycleEvent {
    Ack(CommandAck),
    Receipt(CommandReceipt),
    RecoveryScheduled(CommandAck),
    RecoveryCompleted {
        ack: CommandAck,
        report: ReconcileReport,
    },
}
