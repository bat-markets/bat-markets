use serde::{Deserialize, Serialize};

use crate::types::{Product, Venue};

/// How the engine loads credentials when private APIs are enabled later.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthConfig {
    None,
    Env {
        api_key_var: Box<str>,
        api_secret_var: Box<str>,
    },
}

/// Network endpoints for the selected venue/product.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndpointConfig {
    pub rest_base: Box<str>,
    pub public_ws_base: Box<str>,
    pub private_ws_base: Box<str>,
    pub sandbox: bool,
}

/// Health-related thresholds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthPolicy {
    pub clock_skew_threshold_ms: u64,
    pub snapshot_stale_after_ms: u64,
    pub health_check_interval_ms: u64,
    pub periodic_private_reconcile_ms: Option<u64>,
    pub periodic_metadata_refresh_ms: Option<u64>,
}

/// Request and socket timeout policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeoutPolicy {
    pub connect_ms: u64,
    pub request_ms: u64,
    pub command_ms: u64,
    pub ws_handshake_ms: u64,
    pub ws_idle_ms: u64,
}

/// Coarse command-lane rate limit policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RateLimitPolicy {
    pub command_burst: u32,
    pub command_refill_per_second: u32,
}

/// Coarse retry policy for transport operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryPolicy {
    pub rest_retries: u32,
    pub rest_initial_backoff_ms: u64,
    pub rest_max_backoff_ms: u64,
}

/// Reconnect policy for websocket lanes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReconnectPolicy {
    pub ws_initial_backoff_ms: u64,
    pub ws_max_backoff_ms: u64,
    pub max_reconnect_attempts: Option<u32>,
    pub refresh_metadata_on_reconnect: bool,
    pub reconcile_private_on_reconnect: bool,
}

/// In-memory state sizing and retention policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatePolicy {
    pub recent_trade_capacity: usize,
    pub execution_capacity: usize,
}

/// Top-level configuration for a single engine instance.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatMarketsConfig {
    pub venue: Venue,
    pub product: Product,
    pub auth: AuthConfig,
    pub endpoints: EndpointConfig,
    pub health: HealthPolicy,
    pub timeouts: TimeoutPolicy,
    pub rate_limits: RateLimitPolicy,
    pub retry: RetryPolicy,
    pub reconnect: ReconnectPolicy,
    pub state: StatePolicy,
}

impl BatMarketsConfig {
    #[must_use]
    pub fn new(venue: Venue, product: Product) -> Self {
        Self {
            venue,
            product,
            auth: AuthConfig::None,
            endpoints: EndpointConfig::sandbox_defaults(venue),
            health: HealthPolicy {
                clock_skew_threshold_ms: 250,
                snapshot_stale_after_ms: 5_000,
                health_check_interval_ms: 1_000,
                periodic_private_reconcile_ms: Some(15_000),
                periodic_metadata_refresh_ms: Some(300_000),
            },
            timeouts: TimeoutPolicy {
                connect_ms: 2_000,
                request_ms: 5_000,
                command_ms: 7_500,
                ws_handshake_ms: 10_000,
                ws_idle_ms: 30_000,
            },
            rate_limits: RateLimitPolicy {
                command_burst: 8,
                command_refill_per_second: 4,
            },
            retry: RetryPolicy {
                rest_retries: 2,
                rest_initial_backoff_ms: 100,
                rest_max_backoff_ms: 1_000,
            },
            reconnect: ReconnectPolicy {
                ws_initial_backoff_ms: 250,
                ws_max_backoff_ms: 5_000,
                max_reconnect_attempts: None,
                refresh_metadata_on_reconnect: true,
                reconcile_private_on_reconnect: true,
            },
            state: StatePolicy {
                recent_trade_capacity: 128,
                execution_capacity: 1_024,
            },
        }
    }
}

impl EndpointConfig {
    #[must_use]
    pub fn sandbox_defaults(venue: Venue) -> Self {
        match venue {
            Venue::Binance => Self {
                rest_base: "https://demo-fapi.binance.com".into(),
                public_ws_base: "wss://stream.binancefuture.com/ws".into(),
                private_ws_base: "wss://stream.binancefuture.com/ws".into(),
                sandbox: true,
            },
            Venue::Bybit => Self {
                rest_base: "https://api-testnet.bybit.com".into(),
                public_ws_base: "wss://stream-testnet.bybit.com/v5/public/linear".into(),
                private_ws_base: "wss://stream-testnet.bybit.com/v5/private".into(),
                sandbox: true,
            },
        }
    }

    #[must_use]
    pub fn mainnet_defaults(venue: Venue) -> Self {
        match venue {
            Venue::Binance => Self {
                rest_base: "https://fapi.binance.com".into(),
                public_ws_base: "wss://fstream.binance.com/ws".into(),
                private_ws_base: "wss://fstream.binance.com/ws".into(),
                sandbox: false,
            },
            Venue::Bybit => Self {
                rest_base: "https://api.bybit.com".into(),
                public_ws_base: "wss://stream.bybit.com/v5/public/linear".into(),
                private_ws_base: "wss://stream.bybit.com/v5/private".into(),
                sandbox: false,
            },
        }
    }
}
