use serde::{Deserialize, Serialize};

use crate::ids::{AssetCode, ClientOrderId, InstrumentId, OrderId, RequestId, TradeId};
use crate::numeric::{Amount, Leverage, Price, Quantity};
use crate::primitives::TimestampMs;
use crate::types::{
    MarginMode, OrderStatus, OrderType, PositionMode, Side, TimeInForce, TriggerType,
};

/// Unified order snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Order {
    pub order_id: OrderId,
    pub client_order_id: Option<ClientOrderId>,
    pub instrument_id: InstrumentId,
    pub side: Side,
    pub order_type: OrderType,
    pub time_in_force: Option<TimeInForce>,
    pub status: OrderStatus,
    pub price: Option<Price>,
    pub quantity: Quantity,
    pub filled_quantity: Quantity,
    pub average_fill_price: Option<Price>,
    pub reduce_only: bool,
    pub post_only: bool,
    pub created_at: TimestampMs,
    pub updated_at: TimestampMs,
    pub venue_status: Option<Box<str>>,
}

/// Trade execution liquidity tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Liquidity {
    Maker,
    Taker,
}

/// Unified fill/execution.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Execution {
    pub execution_id: TradeId,
    pub order_id: OrderId,
    pub client_order_id: Option<ClientOrderId>,
    pub instrument_id: InstrumentId,
    pub side: Side,
    pub quantity: Quantity,
    pub price: Price,
    pub fee: Option<Amount>,
    pub fee_asset: Option<AssetCode>,
    pub liquidity: Option<Liquidity>,
    pub executed_at: TimestampMs,
}

/// Create-order request for the unified command layer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateOrderRequest {
    pub request_id: Option<RequestId>,
    pub instrument_id: InstrumentId,
    pub client_order_id: Option<ClientOrderId>,
    pub side: Side,
    pub order_type: OrderType,
    pub time_in_force: Option<TimeInForce>,
    pub quantity: Quantity,
    pub price: Option<Price>,
    pub trigger_price: Option<Price>,
    pub trigger_type: Option<TriggerType>,
    pub reduce_only: bool,
    pub post_only: bool,
}

/// Order-target locator used by amend/cancel requests.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderTarget {
    pub instrument_id: InstrumentId,
    pub order_id: Option<OrderId>,
    pub client_order_id: Option<ClientOrderId>,
}

/// Batch create-order request for the unified command layer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateOrdersRequest {
    pub request_id: Option<RequestId>,
    pub orders: Vec<CreateOrderRequest>,
}

/// Amend-order request for the unified command layer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AmendOrderRequest {
    pub request_id: Option<RequestId>,
    pub instrument_id: InstrumentId,
    pub order_id: Option<OrderId>,
    pub client_order_id: Option<ClientOrderId>,
    pub quantity: Option<Quantity>,
    pub price: Option<Price>,
    pub trigger_price: Option<Price>,
}

/// Batch amend-order request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AmendOrdersRequest {
    pub request_id: Option<RequestId>,
    pub orders: Vec<AmendOrderRequest>,
}

/// Edit-order request for the public root API.
///
/// This is the public v0.2 name for an order amendment. The wire behavior is
/// identical to [`AmendOrderRequest`].
pub type EditOrderRequest = AmendOrderRequest;

/// Batch edit-order request for the public root API.
///
/// This is the public v0.2 name for a batch order amendment. The wire behavior
/// is identical to [`AmendOrdersRequest`].
pub type EditOrdersRequest = AmendOrdersRequest;

/// Cancel-order request for the unified command layer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelOrderRequest {
    pub request_id: Option<RequestId>,
    pub instrument_id: InstrumentId,
    pub order_id: Option<OrderId>,
    pub client_order_id: Option<ClientOrderId>,
}

/// Batch cancel-order request for the unified command layer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelOrdersRequest {
    pub request_id: Option<RequestId>,
    pub orders: Vec<OrderTarget>,
}

/// Cancel-all-orders request for the unified command layer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelAllOrdersRequest {
    pub request_id: Option<RequestId>,
    pub instrument_id: Option<InstrumentId>,
}

/// Get-order request for the unified command layer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetOrderRequest {
    pub request_id: Option<RequestId>,
    pub instrument_id: InstrumentId,
    pub order_id: Option<OrderId>,
    pub client_order_id: Option<ClientOrderId>,
}

/// Open-order listing request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListOpenOrdersRequest {
    pub instrument_id: Option<InstrumentId>,
}

/// Execution-history request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListExecutionsRequest {
    pub instrument_id: Option<InstrumentId>,
    pub limit: Option<usize>,
}

/// Set-leverage request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetLeverageRequest {
    pub request_id: Option<RequestId>,
    pub instrument_id: InstrumentId,
    pub leverage: Leverage,
}

/// Set-margin-mode request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetMarginModeRequest {
    pub request_id: Option<RequestId>,
    pub instrument_id: InstrumentId,
    pub margin_mode: MarginMode,
}

/// Close-position request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClosePositionRequest {
    pub request_id: Option<RequestId>,
    pub instrument_id: InstrumentId,
    pub quantity: Option<Quantity>,
    pub client_order_id: Option<ClientOrderId>,
    pub price: Option<Price>,
    pub time_in_force: Option<TimeInForce>,
    pub post_only: bool,
}

/// Order-validation request using venue-native dry-run surfaces where available.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidateOrderRequest {
    pub request_id: Option<RequestId>,
    pub order: CreateOrderRequest,
}

/// Set-position-mode request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetPositionModeRequest {
    pub request_id: Option<RequestId>,
    pub instrument_id: Option<InstrumentId>,
    pub position_mode: PositionMode,
}
