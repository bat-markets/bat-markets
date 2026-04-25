//! Public facade crate for `bat-markets`.
//!
//! `bat-markets` is a futures-first, headless exchange engine for Binance
//! USD-M and Bybit USDT linear futures. The facade keeps application code on a
//! small, typed surface while preserving venue-specific escape hatches when the
//! exchanges do not share identical semantics.
//!
//! # API map
//!
//! | Surface | Owner | Use it for |
//! | --- | --- | --- |
//! | [`BatMarkets`] / [`BatMarketsBuilder`] | engine construction | choose venue, product, config, and live/static mode |
//! | [`MarketClient`] | market read side | cached snapshots, public REST fetches, metadata refresh |
//! | [`StreamClient`] / [`PublicLaneClient`] | public data lane | fixture ingest, local event subscriptions, shared public WS watchers |
//! | [`StreamClient`] / [`PrivateLaneClient`] | private state lane | private feed ingest, account-stream watchers, manual reconcile |
//! | [`CommandLaneClient`] | command event lane | command lifecycle observation and low-level command classification |
//! | [`EntryClient`] | write side | order-entry and account-setting commands with [`PendingCommandHandle`] |
//! | [`TradeClient`] | order/execution read side | cached orders, open orders, executions, and REST refreshes |
//! | [`PositionClient`] | position read side | cached positions and REST refresh |
//! | [`AccountClient`] | account read side | balances, account summary, and account refresh |
//! | [`HealthClient`] | runtime health | cheap health snapshots and health-change subscriptions |
//! | [`DiagnosticsClient`] | local diagnostics | lock and runtime latency snapshots |
//! | [`NativeClient`] | venue-specific access | adapter details that should not be forced into a fake unified API |
//!
//! # Method families
//!
//! - [`BatMarketsBuilder::build`] creates an offline/static client and never
//!   performs network I/O.
//! - [`BatMarketsBuilder::build_live`] bootstraps live transport with server
//!   time, metadata, HTTP, and optional environment-backed auth.
//! - Snapshot getters such as [`MarketClient::ticker`],
//!   [`TradeClient::orders`], and [`AccountClient::summary`] are synchronous
//!   cached-state reads.
//! - `fetch_*` methods perform public REST reads.
//! - `refresh_*` methods perform REST reads and merge the result back into
//!   engine state.
//! - `subscribe_*` methods read events already flowing through the local bus.
//! - `watch_*` methods acquire a shared live websocket lease and return typed
//!   updates.
//! - [`EntryClient`] owns write commands and exposes acknowledgement,
//!   lifecycle, and recovery through [`PendingCommandHandle`].
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
//!     let capabilities = client.capabilities();
//!     assert!(capabilities.market.ticker);
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
//! println!("{} instruments", client.market().instrument_specs().len());
//! # Ok(())
//! # }
//! ```

#![deny(missing_docs)]

/// Account balances and account summary facade.
pub mod account;
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
