//! Public facade crate for `bat-markets`.
//!
//! `bat-markets` is a futures-first, headless exchange engine for Binance USD-M
//! and Bybit USDT linear futures.
//!
//! The v0.3 facade follows the CCXT mental model: `fetch_*` is REST/read,
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

/// Low-level advanced facade for custom transports and diagnostics.
pub mod advanced;
/// Re-exported capability contracts from `bat-markets-core`.
pub mod capabilities;
/// Engine facade and builder.
pub mod client;
/// Re-exported runtime config contracts from `bat-markets-core`.
pub mod config;
mod diagnostics;
mod entry;
/// Re-exported error contracts from `bat-markets-core`.
pub mod errors;
mod facade;
mod health;
mod native;
mod runtime;
mod stream;
mod subscriptions;
mod transport;
/// Re-exported domain and request/response types from `bat-markets-core`.
pub mod types;

pub use advanced::AdvancedClient;
pub use client::{BatMarkets, BatMarketsBuilder};
pub use diagnostics::{LockDiagnosticsSnapshot, RuntimeDiagnosticsSnapshot};
pub use entry::PendingCommandHandle;
pub use health::StatusWatch;
pub use native::NativeClient;
pub use stream::{
    BalancesWatch, ExecutionsWatch, FundingRateWatch, LiquidationWatch, MarkPriceWatch, OhlcvWatch,
    OpenInterestWatch, OrderBookWatch, OrdersWatch, PositionsWatch, TickerWatch, TradesWatch,
};
