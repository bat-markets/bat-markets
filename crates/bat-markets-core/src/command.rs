use serde::{Deserialize, Serialize};

use crate::ids::{ClientOrderId, InstrumentId, OrderId, RequestId};
use crate::types::{Product, Venue};

/// Supported command-lane operations in `0.1.x`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandOperation {
    CreateOrder,
    CancelOrder,
    GetOrder,
    SetLeverage,
    SetMarginMode,
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
