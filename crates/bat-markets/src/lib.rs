//! Public facade crate for `bat-markets`.

pub mod account;
pub mod capabilities;
pub mod client;
pub mod config;
pub mod diagnostics;
pub mod entry;
pub mod errors;
pub mod health;
pub mod market;
pub mod native;
pub mod position;
mod runtime;
pub mod stream;
mod subscriptions;
pub mod trade;
mod transport;
pub mod types;

pub use account::AccountClient;
pub use client::{BatMarkets, BatMarketsBuilder};
pub use diagnostics::{DiagnosticsClient, LockDiagnosticsSnapshot, RuntimeDiagnosticsSnapshot};
pub use entry::{EntryClient, PendingCommandHandle};
pub use health::HealthClient;
pub use market::MarketClient;
pub use native::NativeClient;
pub use position::PositionClient;
pub use stream::{
    AccountUpdates, AccountWatch, BalanceUpdates, BalancesWatch, BookTopUpdates, BookTopWatch,
    CommandLaneClient, ExecutionUpdates, ExecutionsWatch, FastFeedUpdates, FastFeedWatch,
    FundingRateUpdates, FundingRateWatch, LiquidationUpdates, LiquidationWatch, LiveStreamHandle,
    MarkPriceUpdates, MarkPriceWatch, OhlcvUpdates, OhlcvWatch, OpenInterestUpdates,
    OpenInterestWatch, OrderBookUpdates, OrderBookWatch, OrderUpdates, OrdersWatch,
    PositionUpdates, PositionsWatch, PrivateLaneClient, PublicLaneClient, PublicSubscription,
    StreamClient, TickerUpdates, TickerWatch, TradeUpdates, TradesWatch, WatchInstrumentsRequest,
    WatchOhlcvRequest,
};
pub use trade::TradeClient;
