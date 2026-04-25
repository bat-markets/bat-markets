//! Public facade crate for `bat-markets`.
//!
//! `bat-markets` is a futures-first, headless exchange engine for Binance USD-M
//! and Bybit USDT linear futures.
//!
//! The v0.2 facade follows the CCXT mental model: `fetch_*` is REST/read,
//! `watch_*` is websocket/live, `create/edit/cancel/set_*` are commands, and
//! [`BatMarkets::advanced`] is the low-level escape hatch.
//!
//! # API map
//!
//! | Family | Primary methods | Responsibility |
//! | --- | --- | --- |
//! | Metadata/cache | [`BatMarkets::markets`], [`BatMarkets::load_markets`] | local bundled metadata and live venue metadata refresh |
//! | Public REST | `fetch_ticker`, `fetch_tickers`, `fetch_order_book`, `fetch_ohlcv`, `fetch_trades`, `fetch_mark_price`, `fetch_funding_rate`, `fetch_open_interest`, `fetch_liquidations` | unauthenticated market reads |
//! | Private REST | `fetch_balance`, `fetch_positions`, `fetch_open_orders`, `fetch_order`, `fetch_my_trades` | authenticated account, position, order, and execution reads |
//! | Public WS | `watch_ticker`, `watch_tickers`, `watch_trades`, `watch_trades_for_symbols`, `watch_order_book`, `watch_ohlcv`, `watch_ohlcv_for_symbols`, `watch_mark_price`, `watch_funding_rate`, `watch_open_interest`, `watch_liquidations`, `watch_status` | typed live updates over shared websocket hubs |
//! | Private WS | `watch_balance`, `watch_orders`, `watch_my_trades`, `watch_positions` | authenticated account-stream updates over one shared private hub |
//! | Commands | `create_order`, `create_orders`, `edit_order`, `edit_orders`, `cancel_order`, `cancel_orders`, `cancel_all_orders`, `close_position`, `validate_order`, `set_leverage`, `set_margin_mode`, `set_position_mode` | write operations with lifecycle-aware [`PendingCommandHandle`] results |
//! | Advanced | [`BatMarkets::advanced`] | raw lane ingest, subscriptions, command classification, reconcile, diagnostics, and native access |
//!
//! # Safety model
//!
//! Public market reads do not require secrets. Live authenticated flows read
//! credentials from explicit config or venue-specific environment variables in
//! live mode. Command outcomes that cannot be proven are surfaced as
//! `UnknownExecution` and are resolved through reconciliation evidence instead
//! of being silently treated as success or failure.
//!
//! # Examples
//!
//! Static/offline client:
//!
//! ```
//! use bat_markets::{BatMarkets, errors::Result, types::{Product, Venue}};
//!
//! fn main() -> Result<()> {
//!     let client = BatMarkets::builder()
//!         .venue(Venue::Binance)
//!         .product(Product::LinearUsdt)
//!         .build()?;
//!
//!     assert!(!client.markets().is_empty());
//!     Ok(())
//! }
//! ```
//!
//! Live client:
//!
//! ```no_run
//! use bat_markets::{BatMarkets, errors::Result, types::{Product, Venue}};
//!
//! # #[tokio::main]
//! # async fn main() -> Result<()> {
//! let client = BatMarkets::builder()
//!     .venue(Venue::Bybit)
//!     .product(Product::LinearUsdt)
//!     .build_live()
//!     .await?;
//!
//! println!("{} instruments", client.markets().len());
//! # Ok(())
//! # }
//! ```

#![deny(missing_docs)]

/// Account balances and account summary facade.
pub mod account;
/// Low-level advanced facade for custom transports and diagnostics.
pub mod advanced;
/// Re-exported capability contracts from `bat-markets-core`.
pub mod capabilities;
/// Engine facade and builder.
pub mod client;
/// Re-exported runtime config contracts from `bat-markets-core`.
pub mod config;
/// Runtime and shared-state diagnostics facade.
pub mod diagnostics;
/// Order-entry and account-setting command facade.
pub mod entry;
/// Re-exported error contracts from `bat-markets-core`.
pub mod errors;
mod facade;
/// Runtime health facade.
pub mod health;
/// Market-data snapshot and REST facade.
pub mod market;
/// Venue-specific native adapter access.
pub mod native;
/// Position snapshot and compatibility settings facade.
pub mod position;
mod runtime;
/// Public, private, and command stream-lane facade.
pub mod stream;
mod subscriptions;
/// Read-side order and execution facade.
pub mod trade;
mod transport;
/// Re-exported domain and request/response types from `bat-markets-core`.
pub mod types;

pub use account::AccountClient;
pub use advanced::AdvancedClient;
pub use client::{BatMarkets, BatMarketsBuilder};
pub use diagnostics::{DiagnosticsClient, LockDiagnosticsSnapshot, RuntimeDiagnosticsSnapshot};
pub use entry::{EntryClient, PendingCommandHandle};
pub use health::{HealthClient, StatusWatch};
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
