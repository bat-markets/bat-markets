use serde::{Deserialize, Serialize};

use crate::ids::{InstrumentId, PositionId};
use crate::numeric::{Amount, Leverage, Price, Quantity};
use crate::primitives::TimestampMs;
use crate::types::{MarginMode, PositionDirection, PositionMode};

/// Unified futures position snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub position_id: PositionId,
    pub instrument_id: InstrumentId,
    pub direction: PositionDirection,
    pub size: Quantity,
    pub entry_price: Option<Price>,
    pub mark_price: Option<Price>,
    pub unrealized_pnl: Option<Amount>,
    pub leverage: Option<Leverage>,
    pub margin_mode: MarginMode,
    pub position_mode: PositionMode,
    pub updated_at: TimestampMs,
}
