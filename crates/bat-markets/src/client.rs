use std::{
    env,
    sync::Arc,
    time::{Duration, Instant},
};

use parking_lot::RwLock;
use tokio::sync::{broadcast, watch};

use bat_markets_core::{
    AuthConfig, BatMarketsConfig, CapabilitySet, CommandLaneEvent, EngineState, ErrorKind,
    HealthNotification, HealthReport, InstrumentId, InstrumentSpec, LaneSet, MarketError,
    MemorySigner, PrivateLaneEvent, Product, PublicLaneEvent, Result, Signer, Venue, VenueAdapter,
};

#[cfg(feature = "binance")]
use bat_markets_binance::BinanceLinearFuturesAdapter;
#[cfg(feature = "bybit")]
use bat_markets_bybit::BybitLinearFuturesAdapter;

use crate::{
    diagnostics::SharedStateDiagnostics, runtime, subscriptions::SubscriptionHubs,
    transport::CommandTransportHub,
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
    diagnostics: SharedStateDiagnostics,
    health_watch: watch::Sender<HealthReport>,
    health_notifications: broadcast::Sender<HealthNotification>,
    public_events: broadcast::Sender<PublicLaneEvent>,
    private_events: broadcast::Sender<PrivateLaneEvent>,
    command_events: broadcast::Sender<CommandLaneEvent>,
}

impl SharedState {
    fn new(state: EngineState) -> Self {
        let snapshot = state.health().clone();
        let (health_watch, _) = watch::channel(snapshot);
        let (health_notifications, _) = broadcast::channel(64);
        let (public_events, _) = broadcast::channel(4_096);
        let (private_events, _) = broadcast::channel(4_096);
        let (command_events, _) = broadcast::channel(4_096);

        Self {
            state: RwLock::new(state),
            diagnostics: SharedStateDiagnostics::default(),
            health_watch,
            health_notifications,
            public_events,
            private_events,
            command_events,
        }
    }

    pub(crate) fn read<T>(&self, f: impl FnOnce(&EngineState) -> T) -> T {
        let wait_started = Instant::now();
        let state = self.state.read();
        let wait = wait_started.elapsed();
        let hold_started = Instant::now();
        let result = f(&state);
        let hold = hold_started.elapsed();
        drop(state);
        self.diagnostics.observe_read(wait, hold);
        result
    }

    pub(crate) fn write<T>(&self, f: impl FnOnce(&mut EngineState) -> T) -> T {
        let wait_started = Instant::now();
        let (before, after, wait, hold, result) = {
            let mut state = self.state.write();
            let wait = wait_started.elapsed();
            let hold_started = Instant::now();
            let before = state.health().clone();
            let result = f(&mut state);
            let after = state.health().clone();
            let hold = hold_started.elapsed();
            (before, after, wait, hold, result)
        };
        self.diagnostics.observe_write(wait, hold);

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

    pub(crate) fn read_diagnostics(&self) -> crate::diagnostics::LockDiagnosticsSnapshot {
        self.diagnostics.read_snapshot()
    }

    pub(crate) fn write_diagnostics(&self) -> crate::diagnostics::LockDiagnosticsSnapshot {
        self.diagnostics.write_snapshot()
    }

    pub(crate) fn subscribe_health(&self) -> watch::Receiver<HealthReport> {
        self.health_watch.subscribe()
    }

    pub(crate) fn subscribe_health_notifications(&self) -> broadcast::Receiver<HealthNotification> {
        self.health_notifications.subscribe()
    }

    pub(crate) fn apply_public_events(&self, events: &[PublicLaneEvent]) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }

        self.write(|state| {
            if events.len() > 1 {
                let mut instruments = None;
                for event in events {
                    if let Some(instrument_id) = fallible_public_event_instrument_id(event)
                        && !instruments
                            .get_or_insert_with(|| state.instrument_specs())
                            .iter()
                            .any(|spec| &spec.instrument_id == instrument_id)
                    {
                        return Err(unknown_public_event_error(state, instrument_id));
                    }
                }
            }
            for event in events.iter().cloned() {
                state.apply_public_event(event)?;
            }
            Ok(())
        })?;

        for event in events.iter().cloned() {
            let _ = self.public_events.send(event);
        }

        Ok(())
    }

    pub(crate) fn apply_public_event(&self, event: PublicLaneEvent) -> Result<()> {
        self.write(|state| state.apply_public_event(event.clone()))?;
        let _ = self.public_events.send(event);
        Ok(())
    }

    pub(crate) fn subscribe_public_events(&self) -> broadcast::Receiver<PublicLaneEvent> {
        self.public_events.subscribe()
    }

    pub(crate) fn apply_private_events(&self, events: &[PrivateLaneEvent]) {
        if events.is_empty() {
            return;
        }

        self.write(|state| {
            for event in events.iter().cloned() {
                state.apply_private_event(event);
            }
        });

        for event in events.iter().cloned() {
            let _ = self.private_events.send(event);
        }
    }

    pub(crate) fn apply_private_event(&self, event: PrivateLaneEvent) {
        self.write(|state| {
            state.apply_private_event(event.clone());
        });
        let _ = self.private_events.send(event);
    }

    pub(crate) fn subscribe_private_events(&self) -> broadcast::Receiver<PrivateLaneEvent> {
        self.private_events.subscribe()
    }

    pub(crate) fn emit_command_event(&self, event: CommandLaneEvent) {
        let _ = self.command_events.send(event);
    }

    pub(crate) fn subscribe_command_events(&self) -> broadcast::Receiver<CommandLaneEvent> {
        self.command_events.subscribe()
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
    pub(crate) command_transport: Arc<CommandTransportHub>,
    pub(crate) command_transport_mode: runtime::CommandTransportMode,
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
/// println!("loaded {} instruments", client.markets().len());
/// # Ok(())
/// # }
/// ```
pub struct BatMarkets {
    pub(crate) adapter: AdapterHandle,
    pub(crate) shared: Arc<SharedState>,
    pub(crate) runtime_state: Arc<runtime::LiveRuntimeState>,
    pub(crate) subscription_hubs: Arc<SubscriptionHubs>,
    pub(crate) http: reqwest::Client,
    pub(crate) config: BatMarketsConfig,
    pub(crate) api_key: Option<Arc<str>>,
    pub(crate) signer: Option<Arc<dyn Signer>>,
    pub(crate) command_limiter: Arc<runtime::CommandRateLimiter>,
    pub(crate) command_transport: Arc<CommandTransportHub>,
}

impl BatMarkets {
    /// Start configuring a single venue/product engine instance.
    #[must_use]
    pub fn builder() -> BatMarketsBuilder {
        BatMarketsBuilder::default()
    }

    /// Return the venue selected for this engine instance.
    #[must_use]
    pub fn venue(&self) -> Venue {
        self.adapter.as_adapter().venue()
    }

    /// Return the product family selected for this engine instance.
    #[must_use]
    pub fn product(&self) -> Product {
        self.adapter.as_adapter().product()
    }

    /// Return the adapter capability matrix for this venue/product pair.
    #[must_use]
    pub fn capabilities(&self) -> CapabilitySet {
        self.adapter.capabilities()
    }

    /// Return the public/private/command lane support advertised by the adapter.
    #[must_use]
    pub fn lane_set(&self) -> LaneSet {
        self.adapter.lane_set()
    }

    /// Return the current cached instrument metadata.
    ///
    /// This is a local snapshot read. In live mode use [`Self::load_markets`]
    /// when fresh venue metadata is required.
    #[must_use]
    pub fn instrument_specs(&self) -> Vec<InstrumentSpec> {
        self.shared.read(EngineState::instrument_specs)
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
            command_transport: Arc::clone(&self.command_transport),
            command_transport_mode: runtime::CommandTransportMode::Auto,
        }
    }

    pub(crate) fn live_context_with_command_transport(
        &self,
        command_transport_mode: runtime::CommandTransportMode,
    ) -> LiveContext {
        LiveContext {
            command_transport_mode,
            ..self.live_context()
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
        let command_limiter = Arc::new(runtime::CommandRateLimiter::new(
            config.rate_limits.command_burst,
            config.rate_limits.command_refill_per_second,
        ));
        let command_transport = Arc::new(CommandTransportHub::new(
            config.clone(),
            api_key.clone(),
            signer.clone(),
        ));
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
        let subscription_hubs = Arc::new(SubscriptionHubs::new(LiveContext {
            adapter: adapter.clone(),
            shared: Arc::clone(&shared),
            runtime_state: Arc::clone(&runtime_state),
            http: http.clone(),
            config: config.clone(),
            api_key: api_key.clone(),
            signer: signer.clone(),
            command_limiter: Arc::clone(&command_limiter),
            command_transport: Arc::clone(&command_transport),
            command_transport_mode: runtime::CommandTransportMode::Auto,
        }));

        Ok(Self {
            adapter,
            shared,
            runtime_state,
            subscription_hubs,
            http,
            config: config.clone(),
            api_key,
            signer,
            command_limiter,
            command_transport,
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
    /// Select the exchange venue for the engine instance.
    #[must_use]
    pub fn venue(mut self, venue: Venue) -> Self {
        self.venue = Some(venue);
        self
    }

    /// Select the product family.
    ///
    /// The default is [`Product::LinearUsdt`] when omitted.
    #[must_use]
    pub fn product(mut self, product: Product) -> Self {
        self.product = Some(product);
        self
    }

    /// Replace the full runtime config.
    ///
    /// The config venue/product become the builder venue/product.
    #[must_use]
    pub fn config(mut self, config: BatMarketsConfig) -> Self {
        self.venue = Some(config.venue);
        self.product = Some(config.product);
        self.config = Some(config);
        self
    }

    /// Build an offline/static client without live REST or websocket bootstrap.
    ///
    /// This constructor is intended for fixtures, tests, local payload ingestion,
    /// and applications that only need adapter metadata bundled with the crate.
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
        AuthConfig::Inline {
            api_key,
            api_secret,
        } => {
            let signer: Arc<dyn Signer> = Arc::new(MemorySigner::new(api_secret.as_ref()));
            (Some(Arc::<str>::from(api_key.clone())), Some(signer))
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

fn fallible_public_event_instrument_id(event: &PublicLaneEvent) -> Option<&InstrumentId> {
    match event {
        PublicLaneEvent::Ticker(ticker) => Some(&ticker.instrument_id),
        PublicLaneEvent::Trade(trade) => Some(&trade.instrument_id),
        PublicLaneEvent::BookTop(book_top) => Some(&book_top.instrument_id),
        PublicLaneEvent::Kline(kline) => Some(&kline.instrument_id),
        PublicLaneEvent::MarkPrice(mark_price) => Some(&mark_price.instrument_id),
        PublicLaneEvent::Liquidation(liquidation) => Some(&liquidation.instrument_id),
        PublicLaneEvent::OrderBookDelta(_)
        | PublicLaneEvent::FundingRate(_)
        | PublicLaneEvent::OpenInterest(_)
        | PublicLaneEvent::Divergence(_) => None,
    }
}

fn unknown_public_event_error(state: &EngineState, instrument_id: &InstrumentId) -> MarketError {
    MarketError::new(
        ErrorKind::ConfigError,
        format!(
            "unknown instrument {instrument_id} for {} {}",
            state.venue(),
            state.product()
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::resolve_auth;
    use bat_markets_core::{
        AuthConfig, BatMarketsConfig, EngineState, FastPrice, FastTicker, InstrumentId,
        MemorySigner, Product, PublicLaneEvent, Signer, TimestampMs, Venue,
    };
    use tokio::sync::broadcast::error::TryRecvError;

    #[test]
    fn resolve_auth_supports_inline_credentials() {
        let config = BatMarketsConfig {
            auth: AuthConfig::Inline {
                api_key: "inline-key".into(),
                api_secret: "inline-secret".into(),
            },
            ..BatMarketsConfig::new(Venue::Binance, Product::LinearUsdt)
        };

        let (api_key, signer) = resolve_auth(&config);
        assert_eq!(api_key.as_deref(), Some("inline-key"));

        let signature = signer
            .expect("inline auth should provide a signer")
            .sign_hex(b"payload")
            .expect("inline signer should sign deterministically");
        let expected = MemorySigner::new("inline-secret")
            .sign_hex(b"payload")
            .expect("memory signer should sign deterministically");
        assert_eq!(signature, expected);
    }

    #[test]
    fn rejected_public_event_is_not_published() {
        let config = BatMarketsConfig::new(Venue::Binance, Product::LinearUsdt);
        let shared = super::SharedState::new(EngineState::new(
            Venue::Binance,
            Product::LinearUsdt,
            config.state,
            Vec::new(),
        ));
        let mut receiver = shared.subscribe_public_events();
        let instrument_id = InstrumentId::from("UNKNOWN/USDT:USDT");
        let event = PublicLaneEvent::Ticker(FastTicker {
            instrument_id: instrument_id.clone(),
            last_price: FastPrice::new(1),
            mark_price: None,
            index_price: None,
            volume_24h: None,
            turnover_24h: None,
            event_time: TimestampMs::new(1),
        });

        let error = shared
            .apply_public_event(event)
            .expect_err("unknown-instrument public event must be rejected");

        assert!(error.message.contains(instrument_id.as_ref()));
        assert!(shared.read(|state| state.ticker(&instrument_id).is_none()));
        assert!(matches!(receiver.try_recv(), Err(TryRecvError::Empty)));
    }
}
