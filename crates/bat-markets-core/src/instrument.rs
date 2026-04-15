use serde::{Deserialize, Serialize};

use crate::ids::{AssetCode, InstrumentId};
use crate::numeric::{FastPrice, FastQuantity, Leverage, Notional, Price, Quantity};
use crate::types::{MarketType, Product, Venue};

/// Lifecycle status for the instrument.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstrumentStatus {
    Active,
    Halted,
    Settled,
}

/// Coarse support flags for a single instrument.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstrumentSupport {
    pub public_streams: bool,
    pub private_trading: bool,
    pub leverage_set: bool,
    pub margin_mode_set: bool,
    pub funding_rate: bool,
    pub open_interest: bool,
}

/// Canonical specification used by normalization and validation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstrumentSpec {
    pub venue: Venue,
    pub product: Product,
    pub market_type: MarketType,
    pub instrument_id: InstrumentId,
    pub canonical_symbol: Box<str>,
    pub native_symbol: Box<str>,
    pub base: AssetCode,
    pub quote: AssetCode,
    pub settle: AssetCode,
    pub contract_size: Quantity,
    pub tick_size: Price,
    pub step_size: Quantity,
    pub min_qty: Quantity,
    pub min_notional: Notional,
    pub price_scale: u32,
    pub qty_scale: u32,
    pub quote_scale: u32,
    pub max_leverage: Option<Leverage>,
    pub support: InstrumentSupport,
    pub status: InstrumentStatus,
}

impl InstrumentSpec {
    #[must_use]
    pub fn price_from_fast(&self, value: FastPrice) -> Price {
        value.to_price(self.price_scale)
    }

    #[must_use]
    pub fn quantity_from_fast(&self, value: FastQuantity) -> Quantity {
        value.to_quantity(self.qty_scale)
    }

    #[must_use]
    pub fn matches_native_symbol(&self, symbol: &str) -> bool {
        self.native_symbol.as_ref() == symbol
    }
}
