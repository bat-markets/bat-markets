use serde::{Deserialize, Serialize};

use crate::ids::{InstrumentId, TradeId};
use crate::instrument::InstrumentSpec;
use crate::numeric::{FastNotional, FastPrice, FastQuantity, Notional, Price, Quantity, Rate};
use crate::primitives::TimestampMs;
use crate::types::AggressorSide;

/// Fast normalized ticker snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FastTicker {
    pub instrument_id: InstrumentId,
    pub last_price: FastPrice,
    pub mark_price: Option<FastPrice>,
    pub index_price: Option<FastPrice>,
    pub volume_24h: Option<FastQuantity>,
    pub turnover_24h: Option<FastNotional>,
    pub event_time: TimestampMs,
}

/// Fast normalized trade snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FastTrade {
    pub instrument_id: InstrumentId,
    pub trade_id: TradeId,
    pub price: FastPrice,
    pub quantity: FastQuantity,
    pub aggressor_side: AggressorSide,
    pub event_time: TimestampMs,
}

/// Fast normalized top-of-book snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FastBookTop {
    pub instrument_id: InstrumentId,
    pub bid_price: FastPrice,
    pub bid_quantity: FastQuantity,
    pub ask_price: FastPrice,
    pub ask_quantity: FastQuantity,
    pub event_time: TimestampMs,
}

/// Fast normalized kline snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FastKline {
    pub instrument_id: InstrumentId,
    pub interval: Box<str>,
    pub open: FastPrice,
    pub high: FastPrice,
    pub low: FastPrice,
    pub close: FastPrice,
    pub volume: FastQuantity,
    pub open_time: TimestampMs,
    pub close_time: TimestampMs,
    pub closed: bool,
}

/// Unified ticker snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ticker {
    pub instrument_id: InstrumentId,
    pub last_price: Price,
    pub mark_price: Option<Price>,
    pub index_price: Option<Price>,
    pub volume_24h: Option<Quantity>,
    pub turnover_24h: Option<Notional>,
    pub event_time: TimestampMs,
}

/// Unified trade tick.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TradeTick {
    pub instrument_id: InstrumentId,
    pub trade_id: TradeId,
    pub price: Price,
    pub quantity: Quantity,
    pub aggressor_side: AggressorSide,
    pub event_time: TimestampMs,
}

/// Single book level used by unified deltas.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookLevel {
    pub price: Price,
    pub quantity: Quantity,
}

/// Unified top-of-book snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookTop {
    pub instrument_id: InstrumentId,
    pub bid: BookLevel,
    pub ask: BookLevel,
    pub event_time: TimestampMs,
}

/// Unified book delta event.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookDelta {
    pub instrument_id: InstrumentId,
    pub bids: Vec<BookLevel>,
    pub asks: Vec<BookLevel>,
    pub event_time: TimestampMs,
}

/// Unified candlestick snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kline {
    pub instrument_id: InstrumentId,
    pub interval: Box<str>,
    pub open: Price,
    pub high: Price,
    pub low: Price,
    pub close: Price,
    pub volume: Quantity,
    pub open_time: TimestampMs,
    pub close_time: TimestampMs,
    pub closed: bool,
}

/// Unified funding-rate snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FundingRate {
    pub instrument_id: InstrumentId,
    pub value: Rate,
    pub mark_price: Option<Price>,
    pub event_time: TimestampMs,
}

/// Unified open-interest snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenInterest {
    pub instrument_id: InstrumentId,
    pub value: Quantity,
    pub event_time: TimestampMs,
}

impl FastTicker {
    #[must_use]
    pub fn to_unified(&self, spec: &InstrumentSpec) -> Ticker {
        Ticker {
            instrument_id: self.instrument_id.clone(),
            last_price: spec.price_from_fast(self.last_price),
            mark_price: self.mark_price.map(|value| spec.price_from_fast(value)),
            index_price: self.index_price.map(|value| spec.price_from_fast(value)),
            volume_24h: self.volume_24h.map(|value| spec.quantity_from_fast(value)),
            turnover_24h: self
                .turnover_24h
                .map(|value| value.to_notional(spec.quote_scale)),
            event_time: self.event_time,
        }
    }
}

impl FastTrade {
    #[must_use]
    pub fn to_unified(&self, spec: &InstrumentSpec) -> TradeTick {
        TradeTick {
            instrument_id: self.instrument_id.clone(),
            trade_id: self.trade_id.clone(),
            price: spec.price_from_fast(self.price),
            quantity: spec.quantity_from_fast(self.quantity),
            aggressor_side: self.aggressor_side,
            event_time: self.event_time,
        }
    }
}

impl FastBookTop {
    #[must_use]
    pub fn to_unified(&self, spec: &InstrumentSpec) -> BookTop {
        BookTop {
            instrument_id: self.instrument_id.clone(),
            bid: BookLevel {
                price: spec.price_from_fast(self.bid_price),
                quantity: spec.quantity_from_fast(self.bid_quantity),
            },
            ask: BookLevel {
                price: spec.price_from_fast(self.ask_price),
                quantity: spec.quantity_from_fast(self.ask_quantity),
            },
            event_time: self.event_time,
        }
    }
}

impl FastKline {
    #[must_use]
    pub fn to_unified(&self, spec: &InstrumentSpec) -> Kline {
        Kline {
            instrument_id: self.instrument_id.clone(),
            interval: self.interval.clone(),
            open: spec.price_from_fast(self.open),
            high: spec.price_from_fast(self.high),
            low: spec.price_from_fast(self.low),
            close: spec.price_from_fast(self.close),
            volume: spec.quantity_from_fast(self.volume),
            open_time: self.open_time,
            close_time: self.close_time,
            closed: self.closed,
        }
    }
}
