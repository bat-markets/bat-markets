use serde::{Deserialize, Serialize};
use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time};

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

/// Unified OHLCV interval in ccxt-style notation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum KlineInterval {
    Minute1,
    Minute3,
    Minute5,
    Minute15,
    Minute30,
    Hour1,
    Hour2,
    Hour4,
    Hour6,
    Hour12,
    Day1,
    Day3,
    Week1,
    Month1,
}

impl KlineInterval {
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "1" | "1m" => Some(Self::Minute1),
            "3" | "3m" => Some(Self::Minute3),
            "5" | "5m" => Some(Self::Minute5),
            "15" | "15m" => Some(Self::Minute15),
            "30" | "30m" => Some(Self::Minute30),
            "60" | "1h" => Some(Self::Hour1),
            "120" | "2h" => Some(Self::Hour2),
            "240" | "4h" => Some(Self::Hour4),
            "360" | "6h" => Some(Self::Hour6),
            "720" | "12h" => Some(Self::Hour12),
            "D" | "1d" => Some(Self::Day1),
            "3d" => Some(Self::Day3),
            "W" | "1w" => Some(Self::Week1),
            "M" | "1M" => Some(Self::Month1),
            _ => None,
        }
    }

    #[must_use]
    pub const fn as_ccxt_str(self) -> &'static str {
        match self {
            Self::Minute1 => "1m",
            Self::Minute3 => "3m",
            Self::Minute5 => "5m",
            Self::Minute15 => "15m",
            Self::Minute30 => "30m",
            Self::Hour1 => "1h",
            Self::Hour2 => "2h",
            Self::Hour4 => "4h",
            Self::Hour6 => "6h",
            Self::Hour12 => "12h",
            Self::Day1 => "1d",
            Self::Day3 => "3d",
            Self::Week1 => "1w",
            Self::Month1 => "1M",
        }
    }

    #[must_use]
    pub const fn as_binance_str(self) -> &'static str {
        self.as_ccxt_str()
    }

    #[must_use]
    pub const fn as_bybit_str(self) -> &'static str {
        match self {
            Self::Minute1 => "1",
            Self::Minute3 => "3",
            Self::Minute5 => "5",
            Self::Minute15 => "15",
            Self::Minute30 => "30",
            Self::Hour1 => "60",
            Self::Hour2 => "120",
            Self::Hour4 => "240",
            Self::Hour6 => "360",
            Self::Hour12 => "720",
            Self::Day1 => "D",
            Self::Day3 => "3d",
            Self::Week1 => "W",
            Self::Month1 => "M",
        }
    }

    #[must_use]
    pub fn close_time_ms(self, open_time_ms: i64) -> Option<i64> {
        let duration_ms = match self {
            Self::Minute1 => Some(60_000),
            Self::Minute3 => Some(3 * 60_000),
            Self::Minute5 => Some(5 * 60_000),
            Self::Minute15 => Some(15 * 60_000),
            Self::Minute30 => Some(30 * 60_000),
            Self::Hour1 => Some(60 * 60_000),
            Self::Hour2 => Some(120 * 60_000),
            Self::Hour4 => Some(240 * 60_000),
            Self::Hour6 => Some(360 * 60_000),
            Self::Hour12 => Some(720 * 60_000),
            Self::Day1 => Some(24 * 60 * 60_000),
            Self::Day3 => Some(3 * 24 * 60 * 60_000),
            Self::Week1 => Some(7 * 24 * 60 * 60_000),
            Self::Month1 => None,
        };
        if let Some(duration_ms) = duration_ms {
            return Some(open_time_ms + duration_ms - 1);
        }

        let open_time =
            OffsetDateTime::from_unix_timestamp_nanos(i128::from(open_time_ms) * 1_000_000).ok()?;
        let (year, month) = if open_time.month() == Month::December {
            (open_time.year() + 1, Month::January)
        } else {
            (open_time.year(), next_month(open_time.month()))
        };
        let next_month = Date::from_calendar_date(year, month, 1).ok()?;
        let next_open = PrimitiveDateTime::new(next_month, Time::MIDNIGHT).assume_utc();
        Some((next_open.unix_timestamp_nanos() / 1_000_000) as i64 - 1)
    }
}

impl From<KlineInterval> for Box<str> {
    fn from(value: KlineInterval) -> Self {
        value.as_ccxt_str().into()
    }
}

pub const FETCH_OHLCV_MAX_INSTRUMENTS_PER_CALL: usize = 30;

/// Historical OHLCV request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FetchOhlcvRequest {
    pub instrument_ids: Vec<InstrumentId>,
    pub interval: Box<str>,
    pub start_time: Option<TimestampMs>,
    pub end_time: Option<TimestampMs>,
    pub limit: Option<usize>,
}

impl FetchOhlcvRequest {
    #[must_use]
    pub fn for_instrument(
        instrument_id: InstrumentId,
        interval: impl Into<Box<str>>,
        start_time: Option<TimestampMs>,
        end_time: Option<TimestampMs>,
        limit: Option<usize>,
    ) -> Self {
        Self {
            instrument_ids: vec![instrument_id],
            interval: interval.into(),
            start_time,
            end_time,
            limit,
        }
    }

    #[must_use]
    pub fn for_instruments(
        instrument_ids: Vec<InstrumentId>,
        interval: impl Into<Box<str>>,
        start_time: Option<TimestampMs>,
        end_time: Option<TimestampMs>,
        limit: Option<usize>,
    ) -> Self {
        Self {
            instrument_ids,
            interval: interval.into(),
            start_time,
            end_time,
            limit,
        }
    }

    pub fn instrument_ids(&self) -> crate::Result<&[InstrumentId]> {
        if self.instrument_ids.is_empty() {
            return Err(crate::MarketError::new(
                crate::ErrorKind::ConfigError,
                "fetch_ohlcv requires at least one instrument",
            ));
        }
        if self.instrument_ids.len() > FETCH_OHLCV_MAX_INSTRUMENTS_PER_CALL {
            return Err(crate::MarketError::new(
                crate::ErrorKind::Unsupported,
                format!(
                    "fetch_ohlcv supports at most {FETCH_OHLCV_MAX_INSTRUMENTS_PER_CALL} instruments per call"
                ),
            ));
        }
        Ok(self.instrument_ids.as_slice())
    }

    pub fn single_instrument_id(&self) -> crate::Result<&InstrumentId> {
        let instrument_ids = self.instrument_ids()?;
        if instrument_ids.len() != 1 {
            return Err(crate::MarketError::new(
                crate::ErrorKind::Unsupported,
                "single-instrument OHLCV parsing requires exactly one instrument_id",
            ));
        }
        Ok(&instrument_ids[0])
    }
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

fn next_month(month: Month) -> Month {
    match month {
        Month::January => Month::February,
        Month::February => Month::March,
        Month::March => Month::April,
        Month::April => Month::May,
        Month::May => Month::June,
        Month::June => Month::July,
        Month::July => Month::August,
        Month::August => Month::September,
        Month::September => Month::October,
        Month::October => Month::November,
        Month::November => Month::December,
        Month::December => Month::January,
    }
}

#[cfg(test)]
mod tests {
    use crate::{FetchOhlcvRequest, InstrumentId, TimestampMs};

    #[test]
    fn fetch_ohlcv_request_accepts_single_instrument() {
        let request = FetchOhlcvRequest::for_instrument(
            InstrumentId::from("BTC/USDT:USDT"),
            "1m",
            Some(TimestampMs::new(1)),
            Some(TimestampMs::new(2)),
            Some(500),
        );

        let instrument_ids = request
            .instrument_ids()
            .expect("single instrument request should validate");
        assert_eq!(instrument_ids.len(), 1);
        assert_eq!(instrument_ids[0], InstrumentId::from("BTC/USDT:USDT"));
    }

    #[test]
    fn fetch_ohlcv_request_rejects_more_than_30_instruments() {
        let request = FetchOhlcvRequest::for_instruments(
            (0..31)
                .map(|index| InstrumentId::from(format!("SYM{index}/USDT:USDT")))
                .collect(),
            "5m",
            None,
            None,
            Some(1000),
        );

        let error = request
            .instrument_ids()
            .expect_err("31 instruments should be rejected");
        assert_eq!(error.kind, crate::ErrorKind::Unsupported);
    }
}
