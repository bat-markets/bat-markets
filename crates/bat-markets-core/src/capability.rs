use serde::{Deserialize, Serialize};

/// Market-data capabilities for a venue adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarketCapabilities {
    pub ticker: bool,
    pub recent_trades: bool,
    pub book_top: bool,
    pub klines: bool,
    pub funding_rate: bool,
    pub open_interest: bool,
    pub public_streams: bool,
}

/// Trade capabilities for a venue adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TradeCapabilities {
    pub create: bool,
    pub cancel: bool,
    pub get: bool,
    pub list_open: bool,
    pub history: bool,
}

/// Position capabilities for a venue adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PositionCapabilities {
    pub read: bool,
    pub leverage_set: bool,
    pub margin_mode_set: bool,
    pub hedge_mode: bool,
}

/// Account capabilities for a venue adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountCapabilities {
    pub read_balances: bool,
    pub read_summary: bool,
    pub private_streams: bool,
}

/// Asset capabilities remain intentionally narrow in `0.1.x`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetCapabilities {
    pub read: bool,
    pub transfer: bool,
    pub withdraw: bool,
    pub convert: bool,
}

/// Native-only capabilities.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeCapabilities {
    pub fast_stream: bool,
    pub special_orders: bool,
}

/// Complete capability set exposed by a single venue/product adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilitySet {
    pub market: MarketCapabilities,
    pub trade: TradeCapabilities,
    pub position: PositionCapabilities,
    pub account: AccountCapabilities,
    pub asset: AssetCapabilities,
    pub native: NativeCapabilities,
}

impl CapabilitySet {
    #[must_use]
    pub fn supports_private_trading(self) -> bool {
        self.trade.create && self.account.private_streams
    }
}
