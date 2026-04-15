use serde::{Deserialize, Serialize};

use crate::ids::{AssetCode, ClientOrderId, InstrumentId, OrderId, RequestId, TradeId};
use crate::numeric::{Amount, Leverage, Price, Quantity};
use crate::primitives::TimestampMs;
use crate::types::{MarginMode, OrderStatus, OrderType, Side, TimeInForce};

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
    pub reduce_only: bool,
    pub post_only: bool,
}

/// Cancel-order request for the unified command layer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelOrderRequest {
    pub request_id: Option<RequestId>,
    pub instrument_id: InstrumentId,
    pub order_id: Option<OrderId>,
    pub client_order_id: Option<ClientOrderId>,
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
