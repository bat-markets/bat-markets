//! Core domain contracts and engine primitives for `bat-markets`.

pub mod account;
pub mod adapter;
pub mod auth;
pub mod capability;
pub mod catalog;
pub mod command;
pub mod config;
pub mod error;
pub mod execution;
pub mod health;
pub mod ids;
pub mod instrument;
pub mod market;
pub mod numeric;
pub mod position;
pub mod primitives;
pub mod reconcile;
pub mod state;
pub mod trade;
pub mod types;

pub use account::{AccountSummary, Balance};
pub use adapter::VenueAdapter;
pub use auth::{EnvSigner, MemorySigner, Signer};
pub use capability::{
    AccountCapabilities, AssetCapabilities, CapabilitySet, MarketCapabilities, NativeCapabilities,
    PositionCapabilities, TradeCapabilities,
};
pub use catalog::InstrumentCatalog;
pub use command::{CommandOperation, CommandReceipt, CommandStatus};
pub use config::{
    AuthConfig, BatMarketsConfig, EndpointConfig, HealthPolicy, RateLimitPolicy, ReconnectPolicy,
    RetryPolicy, StatePolicy, TimeoutPolicy,
};
pub use error::{ErrorContext, ErrorKind, MarketError, Result};
pub use execution::{DivergenceEvent, LanePolicy, LaneSet, PrivateLaneEvent, PublicLaneEvent};
pub use health::{DegradedReason, HealthNotification, HealthReport, HealthStatus};
pub use ids::{AssetCode, ClientOrderId, InstrumentId, OrderId, PositionId, RequestId, TradeId};
pub use instrument::{InstrumentSpec, InstrumentStatus, InstrumentSupport};
pub use market::{
    BookDelta, BookLevel, BookTop, FETCH_OHLCV_MAX_INSTRUMENTS_PER_CALL, FastBookTop, FastKline,
    FastTicker, FastTrade, FetchOhlcvRequest, FundingRate, Kline, KlineInterval, OpenInterest,
    Ticker, TradeTick,
};
pub use numeric::{
    Amount, FastNotional, FastPrice, FastQuantity, Leverage, Notional, Price, Quantity, Rate,
};
pub use position::Position;
pub use primitives::{SequenceNumber, TimestampMs};
pub use reconcile::{
    AccountSnapshot, PrivateSnapshot, ReconcileOutcome, ReconcileReport, ReconcileTrigger,
};
pub use state::EngineState;
pub use trade::{
    CancelOrderRequest, CreateOrderRequest, Execution, GetOrderRequest, Liquidity,
    ListExecutionsRequest, ListOpenOrdersRequest, Order, SetLeverageRequest, SetMarginModeRequest,
};
pub use types::{
    AggressorSide, MarginMode, MarketType, OrderStatus, OrderType, PositionDirection, PositionMode,
    Product, Side, TimeInForce, TriggerType, Venue,
};
