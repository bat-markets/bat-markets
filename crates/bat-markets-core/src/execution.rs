use serde::{Deserialize, Serialize};

use crate::account::Balance;
use crate::command::{CommandLifecycleEvent, CommandReceipt};
use crate::market::{FastBookTop, FastKline, FastTicker, FastTrade, FundingRate, OpenInterest};
use crate::market::{FastLiquidation, FastMarkPrice, FastOrderBookDelta};
use crate::position::Position;
use crate::primitives::SequenceNumber;
use crate::trade::{Execution, Order};

/// Coarse behavior for a single execution lane.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LanePolicy {
    pub lossless: bool,
    pub coalescing_allowed: bool,
    pub buffer_capacity: usize,
    pub reconnect_required: bool,
    pub idempotent: bool,
}

/// Stable lane policies exposed by the adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaneSet {
    pub public: LanePolicy,
    pub private: LanePolicy,
    pub command: LanePolicy,
}

impl LaneSet {
    /// Standard lane policies shared by current linear futures adapters.
    #[must_use]
    pub const fn linear_futures_defaults() -> Self {
        Self {
            public: LanePolicy {
                lossless: false,
                coalescing_allowed: true,
                buffer_capacity: 4_096,
                reconnect_required: true,
                idempotent: false,
            },
            private: LanePolicy {
                lossless: true,
                coalescing_allowed: false,
                buffer_capacity: 8_192,
                reconnect_required: true,
                idempotent: true,
            },
            command: LanePolicy {
                lossless: true,
                coalescing_allowed: false,
                buffer_capacity: 1_024,
                reconnect_required: true,
                idempotent: true,
            },
        }
    }
}

/// Public market-data lane output.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PublicLaneEvent {
    Ticker(FastTicker),
    Trade(FastTrade),
    BookTop(FastBookTop),
    OrderBookDelta(FastOrderBookDelta),
    Kline(FastKline),
    MarkPrice(FastMarkPrice),
    FundingRate(FundingRate),
    OpenInterest(OpenInterest),
    Liquidation(FastLiquidation),
    Divergence(DivergenceEvent),
}

/// Divergence marker surfaced by live transport or reconciliation paths.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DivergenceEvent {
    ReconcileRequired,
    StateDivergence,
    SequenceGap { at: Option<SequenceNumber> },
}

/// Private state-lane output.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrivateLaneEvent {
    Balance(Balance),
    Position(Position),
    Order(Order),
    Execution(Execution),
    Divergence(DivergenceEvent),
}

/// Command-lane output.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandLaneEvent {
    Receipt(CommandReceipt),
    Lifecycle(CommandLifecycleEvent),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_futures_lane_defaults_keep_public_lossy_and_commands_lossless() {
        let lanes = LaneSet::linear_futures_defaults();

        assert!(!lanes.public.lossless);
        assert!(lanes.public.coalescing_allowed);
        assert_eq!(lanes.public.buffer_capacity, 4_096);
        assert!(lanes.private.lossless);
        assert!(!lanes.private.coalescing_allowed);
        assert_eq!(lanes.private.buffer_capacity, 8_192);
        assert!(lanes.command.lossless);
        assert!(lanes.command.idempotent);
        assert_eq!(lanes.command.buffer_capacity, 1_024);
    }
}
