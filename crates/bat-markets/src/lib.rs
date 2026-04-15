//! Public facade crate for `bat-markets`.

pub mod account;
pub mod capabilities;
pub mod client;
pub mod config;
pub mod errors;
pub mod health;
pub mod market;
pub mod native;
pub mod position;
mod runtime;
pub mod stream;
pub mod trade;
pub mod types;

pub use account::AccountClient;
pub use client::{BatMarkets, BatMarketsBuilder};
pub use health::HealthClient;
pub use market::MarketClient;
pub use native::NativeClient;
pub use position::PositionClient;
pub use stream::{
    CommandLaneClient, LiveStreamHandle, PrivateLaneClient, PublicLaneClient, PublicSubscription,
    StreamClient,
};
pub use trade::TradeClient;
