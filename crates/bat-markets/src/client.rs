use std::{env, sync::Arc, time::Duration};

use parking_lot::RwLock;
use tokio::sync::{broadcast, watch};

use bat_markets_core::{
    AuthConfig, BatMarketsConfig, CapabilitySet, EngineState, ErrorKind, HealthNotification,
    HealthReport, InstrumentSpec, LaneSet, MarketError, Product, Result, Signer, Venue,
    VenueAdapter,
};

#[cfg(feature = "binance")]
use bat_markets_binance::BinanceLinearFuturesAdapter;
#[cfg(feature = "bybit")]
use bat_markets_bybit::BybitLinearFuturesAdapter;

use crate::{
    account::AccountClient, health::HealthClient, market::MarketClient, native::NativeClient,
    position::PositionClient, runtime, stream::StreamClient, trade::TradeClient,
};

#[derive(Clone)]
pub(crate) enum AdapterHandle {
    #[cfg(feature = "binance")]
    Binance(BinanceLinearFuturesAdapter),
    #[cfg(feature = "bybit")]
    Bybit(BybitLinearFuturesAdapter),
}

impl AdapterHandle {
    pub(crate) fn as_adapter(&self) -> &dyn VenueAdapter {
        match self {
            #[cfg(feature = "binance")]
            Self::Binance(adapter) => adapter,
            #[cfg(feature = "bybit")]
            Self::Bybit(adapter) => adapter,
        }
    }

    pub(crate) fn instrument_specs(&self) -> Vec<InstrumentSpec> {
        self.as_adapter().instrument_specs()
    }

    pub(crate) fn replace_instruments(&self, instruments: Vec<InstrumentSpec>) {
        match self {
            #[cfg(feature = "binance")]
            Self::Binance(adapter) => adapter.replace_instruments(instruments),
            #[cfg(feature = "bybit")]
            Self::Bybit(adapter) => adapter.replace_instruments(instruments),
        }
    }

    pub(crate) fn capabilities(&self) -> CapabilitySet {
        self.as_adapter().capabilities()
    }

    pub(crate) fn lane_set(&self) -> LaneSet {
        self.as_adapter().lane_set()
    }
}

#[derive(Debug)]
pub(crate) struct SharedState {
    state: RwLock<EngineState>,
    health_watch: watch::Sender<HealthReport>,
    health_notifications: broadcast::Sender<HealthNotification>,
}

impl SharedState {
    fn new(state: EngineState) -> Self {
        let snapshot = state.health().clone();
        let (health_watch, _) = watch::channel(snapshot);
        let (health_notifications, _) = broadcast::channel(64);

        Self {
            state: RwLock::new(state),
            health_watch,
            health_notifications,
        }
    }

    pub(crate) fn read<T>(&self, f: impl FnOnce(&EngineState) -> T) -> T {
        let state = self.state.read();
        f(&state)
    }

    pub(crate) fn write<T>(&self, f: impl FnOnce(&mut EngineState) -> T) -> T {
        let (before, after, result) = {
            let mut state = self.state.write();
            let before = state.health().clone();
            let result = f(&mut state);
            let after = state.health().clone();
            (before, after, result)
        };

        if should_publish_health_change(&before, &after) {
            let _ = self.health_watch.send_replace(after.clone());
            let _ = self.health_notifications.send(HealthNotification {
                previous: before,
                current: after,
            });
        }

        result
    }

    pub(crate) fn health_snapshot(&self) -> HealthReport {
        self.read(|state| state.health().clone())
    }

    pub(crate) fn subscribe_health(&self) -> watch::Receiver<HealthReport> {
        self.health_watch.subscribe()
    }

    pub(crate) fn subscribe_health_notifications(&self) -> broadcast::Receiver<HealthNotification> {
        self.health_notifications.subscribe()
    }
}

#[derive(Clone)]
pub(crate) struct LiveContext {
    pub(crate) adapter: AdapterHandle,
    pub(crate) shared: Arc<SharedState>,
    pub(crate) runtime_state: Arc<runtime::LiveRuntimeState>,
    pub(crate) http: reqwest::Client,
    pub(crate) config: BatMarketsConfig,
    pub(crate) api_key: Option<Arc<str>>,
    pub(crate) signer: Option<Arc<dyn Signer>>,
    pub(crate) command_limiter: Arc<runtime::CommandRateLimiter>,
}

/// Public facade for a single venue/product engine instance.
///
/// ```no_run
/// use bat_markets::{BatMarkets, errors::Result, types::{Product, Venue}};
///
/// # #[tokio::main]
/// # async fn main() -> Result<()> {
/// let client = BatMarkets::builder()
///     .venue(Venue::Binance)
///     .product(Product::LinearUsdt)
///     .build_live()
///     .await?;
///
/// println!("loaded {} instruments", client.market().instrument_specs().len());
/// # Ok(())
/// # }
/// ```
pub struct BatMarkets {
    pub(crate) adapter: AdapterHandle,
    pub(crate) shared: Arc<SharedState>,
    pub(crate) runtime_state: Arc<runtime::LiveRuntimeState>,
    pub(crate) http: reqwest::Client,
    pub(crate) config: BatMarketsConfig,
    pub(crate) api_key: Option<Arc<str>>,
    pub(crate) signer: Option<Arc<dyn Signer>>,
    pub(crate) command_limiter: Arc<runtime::CommandRateLimiter>,
}

impl BatMarkets {
    #[must_use]
    pub fn builder() -> BatMarketsBuilder {
        BatMarketsBuilder::default()
    }

    #[must_use]
    pub fn venue(&self) -> Venue {
        self.adapter.as_adapter().venue()
    }

    #[must_use]
    pub fn product(&self) -> Product {
        self.adapter.as_adapter().product()
    }

    #[must_use]
    pub fn capabilities(&self) -> CapabilitySet {
        self.adapter.capabilities()
    }

    #[must_use]
    pub fn lane_set(&self) -> LaneSet {
        self.adapter.lane_set()
    }

    #[must_use]
    pub fn instrument_specs(&self) -> Vec<InstrumentSpec> {
        self.shared.read(EngineState::instrument_specs)
    }

    #[must_use]
    pub fn market(&self) -> MarketClient<'_> {
        MarketClient::new(self)
    }

    #[must_use]
    pub fn stream(&self) -> StreamClient<'_> {
        StreamClient::new(self)
    }

    #[must_use]
    pub fn trade(&self) -> TradeClient<'_> {
        TradeClient::new(self)
    }

    #[must_use]
    pub fn position(&self) -> PositionClient<'_> {
        PositionClient::new(self)
    }

    #[must_use]
    pub fn account(&self) -> AccountClient<'_> {
        AccountClient::new(self)
    }

    #[must_use]
    pub fn health(&self) -> HealthClient<'_> {
        HealthClient::new(self)
    }

    #[must_use]
    pub fn native(&self) -> NativeClient<'_> {
        NativeClient::new(&self.adapter)
    }

    pub(crate) fn read_state<T>(&self, f: impl FnOnce(&EngineState) -> T) -> T {
        self.shared.read(f)
    }

    pub(crate) fn write_state<T>(&self, f: impl FnOnce(&mut EngineState) -> T) -> T {
        self.shared.write(f)
    }

    pub(crate) fn live_context(&self) -> LiveContext {
        LiveContext {
            adapter: self.adapter.clone(),
            shared: Arc::clone(&self.shared),
            runtime_state: Arc::clone(&self.runtime_state),
            http: self.http.clone(),
            config: self.config.clone(),
            api_key: self.api_key.clone(),
            signer: self.signer.clone(),
            command_limiter: Arc::clone(&self.command_limiter),
        }
    }

    async fn bootstrap_live(&self) -> Result<()> {
        runtime::bootstrap_live(&self.live_context()).await
    }

    fn from_handle(adapter: AdapterHandle, config: BatMarketsConfig) -> Result<Self> {
        let shared = Arc::new(SharedState::new(EngineState::new(
            config.venue,
            config.product,
            config.state,
            adapter.instrument_specs(),
        )));
        let runtime_state = Arc::new(runtime::LiveRuntimeState::default());
        let (api_key, signer) = resolve_auth(&config);
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_millis(config.timeouts.connect_ms))
            .timeout(Duration::from_millis(config.timeouts.request_ms))
            .build()
            .map_err(|error| {
                MarketError::new(
                    ErrorKind::TransportError,
                    format!("failed to construct reqwest client: {error}"),
                )
                .with_venue(config.venue, config.product)
            })?;

        Ok(Self {
            adapter,
            shared,
            runtime_state,
            http,
            config: config.clone(),
            api_key,
            signer,
            command_limiter: Arc::new(runtime::CommandRateLimiter::new(
                config.rate_limits.command_burst,
                config.rate_limits.command_refill_per_second,
            )),
        })
    }
}

/// Builder for the public facade.
#[derive(Clone, Debug, Default)]
pub struct BatMarketsBuilder {
    venue: Option<Venue>,
    product: Option<Product>,
    config: Option<BatMarketsConfig>,
}

impl BatMarketsBuilder {
    #[must_use]
    pub fn venue(mut self, venue: Venue) -> Self {
        self.venue = Some(venue);
        self
    }

    #[must_use]
    pub fn product(mut self, product: Product) -> Self {
        self.product = Some(product);
        self
    }

    #[must_use]
    pub fn config(mut self, config: BatMarketsConfig) -> Self {
        self.venue = Some(config.venue);
        self.product = Some(config.product);
        self.config = Some(config);
        self
    }

    pub fn build(self) -> Result<BatMarkets> {
        let config = self.resolve_config(false)?;
        let adapter = build_adapter_handle(config.clone())?;
        BatMarkets::from_handle(adapter, config)
    }

    /// Build a live, async-ready client backed by real REST/WS transport.
    ///
    /// `build_live()` performs:
    ///
    /// - server-time sync,
    /// - live metadata bootstrap,
    /// - HTTP client construction,
    /// - env-backed auth wiring when auth was not configured explicitly.
    pub async fn build_live(self) -> Result<BatMarkets> {
        let config = self.resolve_config(true)?;
        let adapter = build_adapter_handle(config.clone())?;
        let client = BatMarkets::from_handle(adapter, config)?;
        client.bootstrap_live().await?;
        Ok(client)
    }

    fn resolve_config(self, live_mode: bool) -> Result<BatMarketsConfig> {
        let venue = self.venue.ok_or_else(|| {
            MarketError::new(
                ErrorKind::ConfigError,
                "missing venue for BatMarkets builder",
            )
        })?;
        let product = self.product.unwrap_or(Product::LinearUsdt);
        let mut config = self
            .config
            .unwrap_or_else(|| BatMarketsConfig::new(venue, product));
        if live_mode && matches!(config.auth, AuthConfig::None) {
            config.auth = default_env_auth(config.venue);
        }
        Ok(config)
    }
}

fn build_adapter_handle(config: BatMarketsConfig) -> Result<AdapterHandle> {
    match (config.venue, config.product) {
        (Venue::Binance, Product::LinearUsdt) => {
            #[cfg(feature = "binance")]
            {
                Ok(AdapterHandle::Binance(
                    BinanceLinearFuturesAdapter::with_config(config),
                ))
            }
            #[cfg(not(feature = "binance"))]
            {
                Err(MarketError::new(
                    ErrorKind::Unsupported,
                    "binance feature is not enabled",
                ))
            }
        }
        (Venue::Bybit, Product::LinearUsdt) => {
            #[cfg(feature = "bybit")]
            {
                Ok(AdapterHandle::Bybit(
                    BybitLinearFuturesAdapter::with_config(config),
                ))
            }
            #[cfg(not(feature = "bybit"))]
            {
                Err(MarketError::new(
                    ErrorKind::Unsupported,
                    "bybit feature is not enabled",
                ))
            }
        }
    }
}

fn default_env_auth(venue: Venue) -> AuthConfig {
    match venue {
        Venue::Binance => AuthConfig::Env {
            api_key_var: "BINANCE_API_KEY".into(),
            api_secret_var: "BINANCE_API_SECRET".into(),
        },
        Venue::Bybit => AuthConfig::Env {
            api_key_var: "BYBIT_API_KEY".into(),
            api_secret_var: "BYBIT_API_SECRET".into(),
        },
    }
}

fn resolve_auth(config: &BatMarketsConfig) -> (Option<Arc<str>>, Option<Arc<dyn Signer>>) {
    match &config.auth {
        AuthConfig::None => (None, None),
        AuthConfig::Env {
            api_key_var,
            api_secret_var,
        } => {
            let api_key = env::var(api_key_var.as_ref()).ok().map(Arc::<str>::from);
            let signer: Arc<dyn Signer> =
                Arc::new(bat_markets_core::EnvSigner::new(api_secret_var.clone()));
            (api_key, Some(signer))
        }
    }
}

fn should_publish_health_change(before: &HealthReport, after: &HealthReport) -> bool {
    before.status != after.status
        || before.rest_ok != after.rest_ok
        || before.ws_public_ok != after.ws_public_ok
        || before.ws_private_ok != after.ws_private_ok
        || before.clock_skew_ms != after.clock_skew_ms
        || before.reconnect_count != after.reconnect_count
        || before.state_divergence != after.state_divergence
        || before.degraded_reason != after.degraded_reason
}
