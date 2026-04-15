use serde::{Deserialize, Serialize};

use crate::account::Balance;
use crate::command::CommandReceipt;
use crate::market::{FastBookTop, FastKline, FastTicker, FastTrade, FundingRate, OpenInterest};
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

/// Public market-data lane output.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PublicLaneEvent {
    Ticker(FastTicker),
    Trade(FastTrade),
    BookTop(FastBookTop),
    Kline(FastKline),
    FundingRate(FundingRate),
    OpenInterest(OpenInterest),
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
}
