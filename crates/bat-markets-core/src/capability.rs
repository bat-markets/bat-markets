use serde::{Deserialize, Serialize};

/// Market-data capabilities for a venue adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarketCapabilities {
    pub ticker: bool,
    pub recent_trades: bool,
    pub book_top: bool,
    pub order_book: bool,
    pub klines: bool,
    pub mark_price: bool,
    pub funding_rate: bool,
    pub open_interest: bool,
    pub liquidations: bool,
    pub public_streams: bool,
    pub multi_symbol_streams: bool,
}

/// Trade capabilities for a venue adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TradeCapabilities {
    pub create: bool,
    pub batch_create: bool,
    pub amend: bool,
    pub cancel: bool,
    pub batch_cancel: bool,
    pub cancel_all: bool,
    pub get: bool,
    pub list_open: bool,
    pub history: bool,
    pub validate: bool,
}

/// Position capabilities for a venue adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PositionCapabilities {
    pub read: bool,
    pub leverage_set: bool,
    pub margin_mode_set: bool,
    pub position_mode_set: bool,
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
    pub ws_order_entry: bool,
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
    /// Standard capability profile shared by current linear futures adapters.
    #[must_use]
    pub const fn linear_futures_defaults() -> Self {
        Self {
            market: MarketCapabilities {
                ticker: true,
                recent_trades: true,
                book_top: true,
                order_book: true,
                klines: true,
                mark_price: true,
                funding_rate: true,
                open_interest: true,
                liquidations: false,
                public_streams: true,
                multi_symbol_streams: true,
            },
            trade: TradeCapabilities {
                create: true,
                batch_create: true,
                amend: true,
                cancel: true,
                batch_cancel: true,
                cancel_all: true,
                get: true,
                list_open: true,
                history: true,
                validate: true,
            },
            position: PositionCapabilities {
                read: true,
                leverage_set: true,
                margin_mode_set: true,
                position_mode_set: true,
                hedge_mode: true,
            },
            account: AccountCapabilities {
                read_balances: true,
                read_summary: true,
                private_streams: true,
            },
            asset: AssetCapabilities {
                read: false,
                transfer: false,
                withdraw: false,
                convert: false,
            },
            native: NativeCapabilities {
                fast_stream: true,
                special_orders: true,
                ws_order_entry: true,
            },
        }
    }

    #[must_use]
    pub fn supports_private_trading(self) -> bool {
        self.trade.create && self.account.private_streams
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_futures_defaults_cover_current_adapter_contract() {
        let capabilities = CapabilitySet::linear_futures_defaults();

        assert!(capabilities.supports_private_trading());
        assert!(capabilities.market.ticker);
        assert!(capabilities.market.multi_symbol_streams);
        assert!(!capabilities.market.liquidations);
        assert!(capabilities.trade.batch_create);
        assert!(capabilities.position.hedge_mode);
        assert!(capabilities.account.private_streams);
        assert_eq!(capabilities.asset, AssetCapabilities::default());
        assert!(capabilities.native.ws_order_entry);
    }
}
