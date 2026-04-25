//! Shared fixtures, smoke helpers, and integration-style tests.

mod live_trade_cycle;

use std::{
    env,
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use bat_markets::types::{Product, Venue};
use bat_markets::{
    BatMarkets, BatMarketsBuilder,
    config::{AuthConfig, BatMarketsConfig, EndpointConfig, RateLimitPolicy, TimeoutPolicy},
    types::{
        CancelOrdersRequest, ClientOrderId, CreateOrderRequest, CreateOrdersRequest, InstrumentId,
        OrderTarget, OrderType, Price, Quantity, Side, TimeInForce, ValidateOrderRequest,
    },
};
use rust_decimal::Decimal;
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::{Message, accept};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveTestEndpointMode {
    Sandbox,
    Mainnet,
}

/// Binance fixture payloads.
pub mod binance {
    pub const PUBLIC_TICKER: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/binance/public_ticker.json"
    ));
    pub const PUBLIC_TRADE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/binance/public_trade.json"
    ));
    pub const PUBLIC_BOOK_TICKER: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/binance/public_book_ticker.json"
    ));
    pub const PUBLIC_MARK_PRICE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/binance/public_mark_price.json"
    ));
    pub const PUBLIC_LIQUIDATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/binance/public_liquidation.json"
    ));
    pub const PUBLIC_KLINE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/binance/public_kline.json"
    ));
    pub const OPEN_INTEREST: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/binance/open_interest.json"
    ));
    pub const PRIVATE_ACCOUNT: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/binance/private_account_update.json"
    ));
    pub const PRIVATE_ORDER: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/binance/private_order_trade_update.json"
    ));
    pub const COMMAND_CREATE_OK: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/binance/command_create_ok.json"
    ));
    pub const COMMAND_AMEND_OK: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/binance/command_amend_ok.json"
    ));
    pub const COMMAND_BATCH_CREATE_OK: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/binance/command_batch_create_ok.json"
    ));
    pub const COMMAND_BATCH_AMEND_OK: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/binance/command_batch_amend_ok.json"
    ));
    pub const COMMAND_BATCH_CANCEL_OK: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/binance/command_batch_cancel_ok.json"
    ));
    pub const COMMAND_REJECT: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/binance/command_reject.json"
    ));
}

/// Bybit fixture payloads.
pub mod bybit {
    pub const PUBLIC_TICKER: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/bybit/public_ticker.json"
    ));
    pub const PUBLIC_TRADE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/bybit/public_trade.json"
    ));
    pub const PUBLIC_ORDERBOOK: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/bybit/public_orderbook.json"
    ));
    pub const PUBLIC_LIQUIDATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/bybit/public_liquidation.json"
    ));
    pub const PUBLIC_KLINE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/bybit/public_kline.json"
    ));
    pub const PRIVATE_WALLET: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/bybit/private_wallet.json"
    ));
    pub const PRIVATE_POSITION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/bybit/private_position.json"
    ));
    pub const PRIVATE_ORDER: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/bybit/private_order.json"
    ));
    pub const PRIVATE_ORDER_CANCELED: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/bybit/private_order_canceled.json"
    ));
    pub const PRIVATE_EXECUTION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/bybit/private_execution.json"
    ));
    pub const PRIVATE_EXECUTION_LATE_AFTER_CANCEL: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/bybit/private_execution_late_after_cancel.json"
    ));
    pub const PUBLIC_ORDERBOOK_GAP: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/bybit/public_orderbook_gap.json"
    ));
    pub const COMMAND_CREATE_OK: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/bybit/command_create_ok.json"
    ));
    pub const COMMAND_AMEND_OK: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/bybit/command_amend_ok.json"
    ));
    pub const COMMAND_BATCH_CREATE_OK: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/bybit/command_batch_create_ok.json"
    ));
    pub const COMMAND_BATCH_AMEND_OK: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/bybit/command_batch_amend_ok.json"
    ));
    pub const COMMAND_BATCH_CANCEL_OK: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/bybit/command_batch_cancel_ok.json"
    ));
    pub const COMMAND_REJECT: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/bybit/command_reject.json"
    ));
}

pub use live_trade_cycle::{
    BinanceExtendedStressReport, BurstRoundReport, LatencySummary, LiveTradeCyclePlan,
    LiveTradeCycleReport, LiveTradeEventCounts, ProtectiveOrderReport, StreamLatencyObservation,
    binance_mainnet_extended_stress_enabled, binance_mainnet_trade_cycle_enabled,
    prepare_binance_trade_cycle, run_binance_extended_stress, run_binance_trade_cycle,
};

#[must_use]
pub fn build_binance() -> BatMarkets {
    BatMarketsBuilder::default()
        .venue(Venue::Binance)
        .product(Product::LinearUsdt)
        .build()
        .unwrap_or_else(|error| panic!("failed to build binance fixture engine: {error}"))
}

#[must_use]
pub fn build_bybit() -> BatMarkets {
    BatMarketsBuilder::default()
        .venue(Venue::Bybit)
        .product(Product::LinearUsdt)
        .build()
        .unwrap_or_else(|error| panic!("failed to build bybit fixture engine: {error}"))
}

#[must_use]
pub fn has_binance_live_env() -> bool {
    std::env::var_os("BINANCE_API_KEY").is_some()
        && std::env::var_os("BINANCE_API_SECRET").is_some()
}

#[must_use]
pub fn has_bybit_live_env() -> bool {
    std::env::var_os("BYBIT_API_KEY").is_some() && std::env::var_os("BYBIT_API_SECRET").is_some()
}

#[must_use]
pub fn live_test_config(venue: Venue, default_mode: LiveTestEndpointMode) -> BatMarketsConfig {
    let mut config = BatMarketsConfig::new(venue, Product::LinearUsdt);
    config.auth = live_test_auth_config(venue);
    config.endpoints = live_test_endpoint_config(venue, default_mode);
    config
}

#[must_use]
pub fn live_test_endpoint_config(
    venue: Venue,
    default_mode: LiveTestEndpointMode,
) -> EndpointConfig {
    let rest_base = env_var_non_empty(live_test_rest_base_var(venue));
    let resolved_mode = infer_live_test_endpoint_mode(rest_base.as_deref(), default_mode);
    let mut endpoints = match resolved_mode {
        LiveTestEndpointMode::Sandbox => EndpointConfig::sandbox_defaults(venue),
        LiveTestEndpointMode::Mainnet => EndpointConfig::mainnet_defaults(venue),
    };

    if let Some(rest_base) = rest_base {
        endpoints.rest_base = rest_base.into();
    }
    if let Some(public_ws_base) = first_non_empty_env(live_test_public_ws_vars(venue)) {
        endpoints.public_ws_base = public_ws_base.into();
    }
    if let Some(private_ws_base) = first_non_empty_env(live_test_private_ws_vars(venue)) {
        endpoints.private_ws_base = private_ws_base.into();
    }
    if let Some(command_ws_base) = first_non_empty_env(live_test_command_ws_vars(venue)) {
        endpoints.command_ws_base = command_ws_base.into();
    }

    endpoints
}

#[must_use]
pub fn live_test_uses_sandbox(venue: Venue, default_mode: LiveTestEndpointMode) -> bool {
    live_test_endpoint_config(venue, default_mode).sandbox
}

/// Minimal local REST stub for measuring the real runtime command path without venue network hops.
pub struct RuntimeRestStub {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl RuntimeRestStub {
    pub fn spawn() -> std::io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let address = listener.local_addr()?;
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = Arc::clone(&stop);
        let thread = thread::spawn(move || run_runtime_rest_stub(listener, stop_flag));

        Ok(Self {
            address,
            stop,
            thread: Some(thread),
        })
    }

    #[must_use]
    pub fn rest_base(&self) -> String {
        format!("http://{}", self.address)
    }
}

impl Drop for RuntimeRestStub {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(self.address);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Minimal local command websocket stub for measuring the real persistent WS entry path.
pub struct RuntimeCommandWsStub {
    venue: Venue,
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl RuntimeCommandWsStub {
    pub fn spawn(venue: Venue) -> std::io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let address = listener.local_addr()?;
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = Arc::clone(&stop);
        let thread = thread::spawn(move || run_runtime_command_ws_stub(listener, stop_flag, venue));

        Ok(Self {
            venue,
            address,
            stop,
            thread: Some(thread),
        })
    }

    #[must_use]
    pub fn command_ws_base(&self) -> String {
        format!("ws://{}", self.address)
    }

    #[must_use]
    pub const fn venue(&self) -> Venue {
        self.venue
    }
}

impl Drop for RuntimeCommandWsStub {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(self.address);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[must_use]
pub fn build_runtime_stub_client(venue: Venue, rest_base: &str) -> BatMarkets {
    let mut config = BatMarketsConfig::new(venue, Product::LinearUsdt);
    config.auth = AuthConfig::Inline {
        api_key: format!("{venue:?}-bench-key").into(),
        api_secret: format!("{venue:?}-bench-secret").into(),
    };
    config.endpoints = EndpointConfig {
        rest_base: rest_base.into(),
        public_ws_base: "ws://127.0.0.1/unused-public".into(),
        private_ws_base: "ws://127.0.0.1/unused-private".into(),
        command_ws_base: "ws://127.0.0.1/unused-command".into(),
        sandbox: true,
    };
    config.rate_limits = RateLimitPolicy {
        command_burst: 10_000,
        command_refill_per_second: 10_000,
    };
    config.timeouts = TimeoutPolicy {
        connect_ms: 250,
        request_ms: 500,
        command_ms: 500,
        ws_handshake_ms: 500,
        ws_idle_ms: 1_000,
    };

    BatMarketsBuilder::default()
        .config(config)
        .build()
        .unwrap_or_else(|error| {
            panic!("failed to build runtime stub client for {venue:?}: {error}")
        })
}

#[must_use]
pub fn build_runtime_dual_stub_client(
    venue: Venue,
    rest_base: &str,
    command_ws_base: &str,
) -> BatMarkets {
    let mut config = BatMarketsConfig::new(venue, Product::LinearUsdt);
    config.auth = AuthConfig::Inline {
        api_key: format!("{venue:?}-bench-key").into(),
        api_secret: format!("{venue:?}-bench-secret").into(),
    };
    config.endpoints = EndpointConfig {
        rest_base: rest_base.into(),
        public_ws_base: "ws://127.0.0.1/unused-public".into(),
        private_ws_base: "ws://127.0.0.1/unused-private".into(),
        command_ws_base: command_ws_base.into(),
        sandbox: true,
    };
    config.rate_limits = RateLimitPolicy {
        command_burst: 10_000,
        command_refill_per_second: 10_000,
    };
    config.timeouts = TimeoutPolicy {
        connect_ms: 250,
        request_ms: 500,
        command_ms: 500,
        ws_handshake_ms: 500,
        ws_idle_ms: 1_000,
    };

    BatMarketsBuilder::default()
        .config(config)
        .build()
        .unwrap_or_else(|error| {
            panic!("failed to build runtime dual stub client for {venue:?}: {error}")
        })
}

fn live_test_auth_config(venue: Venue) -> AuthConfig {
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

fn live_test_rest_base_var(venue: Venue) -> &'static str {
    match venue {
        Venue::Binance => "BINANCE_BASE_URL",
        Venue::Bybit => "BYBIT_BASE_URL",
    }
}

fn live_test_public_ws_vars(venue: Venue) -> &'static [&'static str] {
    match venue {
        Venue::Binance => &[
            "BINANCE_PUBLIC_WS_BASE_URL",
            "BINANCE_PUBLIC_WS_URL",
            "BINANCE_WS_PUBLIC_URL",
        ],
        Venue::Bybit => &[
            "BYBIT_PUBLIC_WS_BASE_URL",
            "BYBIT_PUBLIC_WS_URL",
            "BYBIT_WS_PUBLIC_URL",
        ],
    }
}

fn live_test_private_ws_vars(venue: Venue) -> &'static [&'static str] {
    match venue {
        Venue::Binance => &[
            "BINANCE_PRIVATE_WS_BASE_URL",
            "BINANCE_PRIVATE_WS_URL",
            "BINANCE_WS_PRIVATE_URL",
        ],
        Venue::Bybit => &[
            "BYBIT_PRIVATE_WS_BASE_URL",
            "BYBIT_PRIVATE_WS_URL",
            "BYBIT_WS_PRIVATE_URL",
        ],
    }
}

fn live_test_command_ws_vars(venue: Venue) -> &'static [&'static str] {
    match venue {
        Venue::Binance => &[
            "BINANCE_COMMAND_WS_BASE_URL",
            "BINANCE_COMMAND_WS_URL",
            "BINANCE_WS_COMMAND_URL",
        ],
        Venue::Bybit => &[
            "BYBIT_COMMAND_WS_BASE_URL",
            "BYBIT_COMMAND_WS_URL",
            "BYBIT_WS_COMMAND_URL",
        ],
    }
}

fn infer_live_test_endpoint_mode(
    rest_base: Option<&str>,
    default_mode: LiveTestEndpointMode,
) -> LiveTestEndpointMode {
    let Some(rest_base) = rest_base else {
        return default_mode;
    };
    let rest_base = rest_base.trim().to_ascii_lowercase();
    if rest_base.contains("testnet") || rest_base.contains("demo-") || rest_base.contains("demo.") {
        LiveTestEndpointMode::Sandbox
    } else {
        LiveTestEndpointMode::Mainnet
    }
}

fn first_non_empty_env(candidates: &[&str]) -> Option<String> {
    candidates.iter().find_map(|name| env_var_non_empty(name))
}

fn env_var_non_empty(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[must_use]
pub fn runtime_stub_batch_create_request(venue: Venue) -> CreateOrdersRequest {
    let (instrument_id, prefix) = runtime_stub_identity(venue);
    CreateOrdersRequest {
        request_id: None,
        orders: vec![
            CreateOrderRequest {
                request_id: None,
                instrument_id: instrument_id.clone(),
                client_order_id: Some(ClientOrderId::from(format!("{prefix}-1"))),
                side: Side::Buy,
                order_type: OrderType::Limit,
                time_in_force: Some(TimeInForce::Gtc),
                quantity: Quantity::new(Decimal::new(1, 3)),
                price: Some(Price::new(Decimal::new(70000, 0))),
                trigger_price: None,
                trigger_type: None,
                reduce_only: false,
                post_only: true,
            },
            CreateOrderRequest {
                request_id: None,
                instrument_id,
                client_order_id: Some(ClientOrderId::from(format!("{prefix}-2"))),
                side: Side::Buy,
                order_type: OrderType::Limit,
                time_in_force: Some(TimeInForce::Gtc),
                quantity: Quantity::new(Decimal::new(1, 3)),
                price: Some(Price::new(Decimal::new(69990, 0))),
                trigger_price: None,
                trigger_type: None,
                reduce_only: false,
                post_only: true,
            },
        ],
    }
}

#[must_use]
pub fn runtime_stub_batch_cancel_request(venue: Venue) -> CancelOrdersRequest {
    let create = runtime_stub_batch_create_request(venue);
    CancelOrdersRequest {
        request_id: None,
        orders: create
            .orders
            .into_iter()
            .map(|order| OrderTarget {
                instrument_id: order.instrument_id,
                order_id: None,
                client_order_id: order.client_order_id,
            })
            .collect(),
    }
}

#[must_use]
pub fn runtime_stub_validate_requests(venue: Venue) -> Vec<ValidateOrderRequest> {
    runtime_stub_batch_create_request(venue)
        .orders
        .into_iter()
        .map(|order| ValidateOrderRequest {
            request_id: None,
            order,
        })
        .collect()
}

fn runtime_stub_identity(venue: Venue) -> (InstrumentId, &'static str) {
    match venue {
        Venue::Binance => (InstrumentId::from("BTC/USDT:USDT"), "stub-binance"),
        Venue::Bybit => (InstrumentId::from("BTC/USDT:USDT"), "stub-bybit"),
    }
}

fn run_runtime_rest_stub(listener: TcpListener, stop: Arc<AtomicBool>) {
    while let Ok((stream, _)) = listener.accept() {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let _ = handle_runtime_stub_connection(stream);
    }
}

fn run_runtime_command_ws_stub(listener: TcpListener, stop: Arc<AtomicBool>, venue: Venue) {
    while let Ok((stream, _)) = listener.accept() {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let _ = handle_runtime_command_ws_connection(stream, venue);
    }
}

fn handle_runtime_command_ws_connection(
    mut stream: TcpStream,
    venue: Venue,
) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_millis(250)))?;
    stream.set_write_timeout(Some(Duration::from_millis(250)))?;

    let mut ws = accept(&mut stream)
        .map_err(|error| std::io::Error::other(format!("websocket accept failed: {error}")))?;

    loop {
        match ws.read() {
            Ok(Message::Text(payload)) => {
                if let Some(response) = runtime_command_ws_response(venue, &payload) {
                    ws.send(Message::Text(response.into())).map_err(|error| {
                        std::io::Error::other(format!("websocket send failed: {error}"))
                    })?;
                }
            }
            Ok(Message::Ping(payload)) => {
                ws.send(Message::Pong(payload)).map_err(|error| {
                    std::io::Error::other(format!("websocket pong failed: {error}"))
                })?;
            }
            Ok(Message::Close(_)) => return Ok(()),
            Ok(_) => {}
            Err(tokio_tungstenite::tungstenite::Error::Io(error))
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                return Ok(());
            }
            Err(error) => {
                return Err(std::io::Error::other(format!(
                    "websocket read failed: {error}"
                )));
            }
        }
    }
}

fn handle_runtime_stub_connection(mut stream: TcpStream) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_millis(250)))?;
    stream.set_write_timeout(Some(Duration::from_millis(250)))?;

    let mut buffer = Vec::with_capacity(2048);
    let mut chunk = [0_u8; 4096];
    loop {
        let request_end = loop {
            if let Some(index) = find_bytes(&buffer, b"\r\n\r\n") {
                break index + 4;
            }

            match stream.read(&mut chunk) {
                Ok(0) => return Ok(()),
                Ok(read) => buffer.extend_from_slice(&chunk[..read]),
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    return Ok(());
                }
                Err(error) => return Err(error),
            }
        };

        let header = String::from_utf8_lossy(&buffer[..request_end]).into_owned();
        let content_length = header
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                if name.eq_ignore_ascii_case("content-length") {
                    value.trim().parse::<usize>().ok()
                } else {
                    None
                }
            })
            .unwrap_or(0);
        while buffer.len() < request_end + content_length {
            match stream.read(&mut chunk) {
                Ok(0) => return Ok(()),
                Ok(read) => buffer.extend_from_slice(&chunk[..read]),
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    return Ok(());
                }
                Err(error) => return Err(error),
            }
        }

        let request_line = header.lines().next().unwrap_or_default();
        let mut parts = request_line.split_whitespace();
        let method = parts.next().unwrap_or_default();
        let target = parts.next().unwrap_or_default();
        let (status_line, body) = runtime_stub_response(method, target);

        let response = format!(
            "{status_line}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: keep-alive\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes())?;
        stream.flush()?;
        buffer.drain(..request_end + content_length);
    }
}

fn runtime_stub_response(method: &str, target: &str) -> (&'static str, &'static str) {
    let path = target.split('?').next().unwrap_or(target);
    match (method, path) {
        ("POST", "/fapi/v1/order/test") => ("HTTP/1.1 200 OK", "{}"),
        ("POST", "/fapi/v1/order") => ("HTTP/1.1 200 OK", binance::COMMAND_CREATE_OK),
        ("PUT", "/fapi/v1/order") => ("HTTP/1.1 200 OK", binance::COMMAND_AMEND_OK),
        ("DELETE", "/fapi/v1/order") => ("HTTP/1.1 200 OK", binance::COMMAND_CREATE_OK),
        ("POST", "/fapi/v1/batchOrders") => ("HTTP/1.1 200 OK", binance::COMMAND_BATCH_CREATE_OK),
        ("PUT", "/fapi/v1/batchOrders") => ("HTTP/1.1 200 OK", binance::COMMAND_BATCH_AMEND_OK),
        ("DELETE", "/fapi/v1/batchOrders") => ("HTTP/1.1 200 OK", binance::COMMAND_BATCH_CANCEL_OK),
        ("POST", "/v5/order/pre-check") => (
            "HTTP/1.1 200 OK",
            "{\"retCode\":0,\"retMsg\":\"OK\",\"result\":{}}",
        ),
        ("POST", "/v5/order/create") => ("HTTP/1.1 200 OK", bybit::COMMAND_CREATE_OK),
        ("POST", "/v5/order/amend") => ("HTTP/1.1 200 OK", bybit::COMMAND_AMEND_OK),
        ("POST", "/v5/order/cancel") => ("HTTP/1.1 200 OK", bybit::COMMAND_CREATE_OK),
        ("POST", "/v5/order/create-batch") => ("HTTP/1.1 200 OK", bybit::COMMAND_BATCH_CREATE_OK),
        ("POST", "/v5/order/amend-batch") => ("HTTP/1.1 200 OK", bybit::COMMAND_BATCH_AMEND_OK),
        ("POST", "/v5/order/cancel-batch") => ("HTTP/1.1 200 OK", bybit::COMMAND_BATCH_CANCEL_OK),
        _ => (
            "HTTP/1.1 404 Not Found",
            "{\"error\":\"unsupported runtime stub path\"}",
        ),
    }
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn runtime_command_ws_response(venue: Venue, payload: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(payload).ok()?;
    match venue {
        Venue::Binance => runtime_command_ws_response_binance(&value),
        Venue::Bybit => runtime_command_ws_response_bybit(&value),
    }
}

fn runtime_command_ws_response_binance(value: &Value) -> Option<String> {
    let id = value.get("id")?.clone();
    let method = value.get("method")?.as_str()?;
    let params = value.get("params")?.as_object()?;
    let symbol = params
        .get("symbol")
        .and_then(Value::as_str)
        .unwrap_or("BTCUSDT");
    let order_id = params
        .get("orderId")
        .and_then(runtime_value_as_i64)
        .unwrap_or(123456);
    let client_order_id = params
        .get("newClientOrderId")
        .or_else(|| params.get("origClientOrderId"))
        .and_then(runtime_value_as_string)
        .unwrap_or_else(|| format!("stub-{method}-{order_id}"));

    let result = json!({
        "symbol": symbol,
        "orderId": order_id,
        "clientOrderId": client_order_id,
    });
    Some(
        json!({
            "id": id,
            "status": 200,
            "result": result,
        })
        .to_string(),
    )
}

fn runtime_command_ws_response_bybit(value: &Value) -> Option<String> {
    let op = value.get("op")?.as_str()?;
    let req_id = value
        .get("reqId")
        .or_else(|| value.get("req_id"))
        .and_then(runtime_value_as_string)?;

    if op == "auth" {
        return Some(
            json!({
                "reqId": req_id,
                "op": "auth",
                "success": true,
                "retCode": 0,
                "retMsg": "OK",
                "connId": "stub-bybit",
            })
            .to_string(),
        );
    }

    if op == "ping" {
        return Some(json!({ "op": "pong" }).to_string());
    }

    let args = value.get("args")?.as_array()?;
    let first = args.first()?;
    match op {
        "order.create" | "order.amend" | "order.cancel" => {
            let symbol = first
                .get("symbol")
                .and_then(Value::as_str)
                .unwrap_or("BTCUSDT");
            let order_link_id = first
                .get("orderLinkId")
                .or_else(|| first.get("orderLinkID"))
                .and_then(runtime_value_as_string)
                .unwrap_or_else(|| "stub-bybit-1".to_owned());
            let order_id = first
                .get("orderId")
                .and_then(runtime_value_as_string)
                .unwrap_or_else(|| format!("{op}-{order_link_id}"));
            Some(
                json!({
                    "reqId": req_id,
                    "op": op,
                    "retCode": 0,
                    "retMsg": "OK",
                    "result": {
                        "symbol": symbol,
                        "orderId": order_id,
                        "orderLinkId": order_link_id,
                    },
                    "connId": "stub-bybit",
                })
                .to_string(),
            )
        }
        "order.create-batch" | "order.amend-batch" | "order.cancel-batch" => {
            let list = first
                .get("request")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let result_list = list
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    let symbol = item
                        .get("symbol")
                        .and_then(Value::as_str)
                        .unwrap_or("BTCUSDT");
                    let order_link_id = item
                        .get("orderLinkId")
                        .or_else(|| item.get("orderLinkID"))
                        .and_then(runtime_value_as_string)
                        .unwrap_or_else(|| format!("stub-bybit-{index}"));
                    let order_id = item
                        .get("orderId")
                        .and_then(runtime_value_as_string)
                        .unwrap_or_else(|| format!("{op}-{index}"));
                    json!({
                        "symbol": symbol,
                        "orderId": order_id,
                        "orderLinkId": order_link_id,
                    })
                })
                .collect::<Vec<_>>();
            let ext_list = result_list
                .iter()
                .map(|_| json!({ "code": 0, "msg": "OK" }))
                .collect::<Vec<_>>();
            Some(
                json!({
                    "reqId": req_id,
                    "op": op,
                    "retCode": 0,
                    "retMsg": "OK",
                    "result": { "list": result_list },
                    "retExtInfo": { "list": ext_list },
                    "connId": "stub-bybit",
                })
                .to_string(),
            )
        }
        _ => Some(
            json!({
                "reqId": req_id,
                "op": op,
                "retCode": 0,
                "retMsg": "OK",
                "result": {},
                "connId": "stub-bybit",
            })
            .to_string(),
        ),
    }
}

fn runtime_value_as_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn runtime_value_as_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str()?.parse::<i64>().ok())
}

#[cfg(test)]
mod tests {
    use tokio::sync::broadcast::error::TryRecvError;
    use tokio::time::{Duration, timeout};

    use bat_markets::types::{CancelAllOrdersRequest, CommandLifecycleEvent};
    use bat_markets::{BatMarkets, errors::Result};
    use bat_markets_core::{
        CommandOperation, CommandStatus, CommandTransport, ErrorKind, HealthStatus, OrderStatus,
    };

    use super::{
        LiveTestEndpointMode, RuntimeCommandWsStub, RuntimeRestStub, binance, build_binance,
        build_bybit, build_runtime_dual_stub_client, bybit, infer_live_test_endpoint_mode,
        runtime_stub_batch_cancel_request, runtime_stub_batch_create_request,
        runtime_stub_validate_requests,
    };

    #[test]
    fn binance_fixtures_drive_market_and_private_state() -> Result<()> {
        let client = build_binance();
        ingest_binance(&client)?;

        let instrument = bat_markets::types::InstrumentId::from("BTC/USDT:USDT");
        let ticker = client
            .market()
            .ticker(&instrument)
            .expect("binance ticker missing after ingest");
        assert_eq!(ticker.last_price.to_string(), "70100.50");
        assert_eq!(client.account().balances().len(), 1);
        assert_eq!(client.position().list().len(), 1);
        assert_eq!(client.trade().open_orders().len(), 1);
        assert_eq!(client.trade().executions().len(), 1);
        assert_eq!(client.status().status, HealthStatus::Healthy);

        Ok(())
    }

    #[test]
    fn bybit_fixtures_drive_market_and_private_state() -> Result<()> {
        let client = build_bybit();
        ingest_bybit(&client)?;

        let instrument = bat_markets::types::InstrumentId::from("BTC/USDT:USDT");
        let ticker = client
            .market()
            .ticker(&instrument)
            .expect("bybit ticker missing after ingest");
        assert_eq!(
            ticker.mark_price.expect("mark price missing").to_string(),
            "70108.50"
        );
        assert_eq!(client.account().balances().len(), 1);
        assert_eq!(client.position().list().len(), 1);
        assert_eq!(client.trade().open_orders().len(), 1);
        assert_eq!(client.trade().executions().len(), 1);
        assert_eq!(client.status().status, HealthStatus::Healthy);

        Ok(())
    }

    #[test]
    fn live_test_endpoint_mode_infers_mainnet_and_sandbox_from_rest_base() {
        assert_eq!(
            infer_live_test_endpoint_mode(
                Some("https://fapi.binance.com"),
                LiveTestEndpointMode::Sandbox,
            ),
            LiveTestEndpointMode::Mainnet
        );
        assert_eq!(
            infer_live_test_endpoint_mode(
                Some("https://api-testnet.bybit.com"),
                LiveTestEndpointMode::Mainnet,
            ),
            LiveTestEndpointMode::Sandbox
        );
    }

    #[test]
    fn command_lane_surfaces_unknown_execution() -> Result<()> {
        let client = build_binance();
        let receipt =
            client
                .advanced()
                .classify_command_json(CommandOperation::CreateOrder, None, None)?;

        assert_eq!(receipt.status, CommandStatus::UnknownExecution);
        assert_eq!(client.status().status, HealthStatus::CommandUncertain);

        Ok(())
    }

    #[test]
    fn reject_paths_stay_explicit() -> Result<()> {
        let binance = build_binance();
        let receipt = binance.advanced().classify_command_json(
            CommandOperation::CreateOrder,
            Some(binance::COMMAND_REJECT),
            None,
        )?;
        assert_eq!(receipt.status, CommandStatus::Rejected);
        assert_eq!(receipt.native_code.as_deref(), Some("-2019"));

        let bybit = build_bybit();
        let receipt = bybit.advanced().classify_command_json(
            CommandOperation::CreateOrder,
            Some(bybit::COMMAND_REJECT),
            None,
        )?;
        assert_eq!(receipt.status, CommandStatus::Rejected);
        assert_eq!(receipt.native_code.as_deref(), Some("110007"));

        Ok(())
    }

    #[test]
    fn duplicate_private_execution_is_idempotent() -> Result<()> {
        let client = build_binance();
        client
            .advanced()
            .ingest_private_json(binance::PRIVATE_ORDER)?;
        client
            .advanced()
            .ingest_private_json(binance::PRIVATE_ORDER)?;

        assert_eq!(client.trade().executions().len(), 1);
        assert_eq!(client.trade().orders().len(), 1);

        Ok(())
    }

    #[test]
    fn duplicate_public_trade_is_coalesced() -> Result<()> {
        let client = build_bybit();
        let instrument = bat_markets::types::InstrumentId::from("BTC/USDT:USDT");
        client.advanced().ingest_public_json(bybit::PUBLIC_TRADE)?;
        client.advanced().ingest_public_json(bybit::PUBLIC_TRADE)?;

        let trades = client
            .market()
            .recent_trades(&instrument)
            .expect("recent trades missing after duplicate public ingest");
        assert_eq!(trades.len(), 1);

        Ok(())
    }

    #[test]
    fn contradictory_command_and_stream_outcome_stays_explicit() -> Result<()> {
        let client = build_binance();
        let receipt =
            client
                .advanced()
                .classify_command_json(CommandOperation::CreateOrder, None, None)?;
        assert_eq!(receipt.status, CommandStatus::UnknownExecution);

        client
            .advanced()
            .ingest_private_json(binance::PRIVATE_ORDER)?;

        assert_eq!(client.trade().executions().len(), 1);
        assert_eq!(client.status().status, HealthStatus::CommandUncertain);

        Ok(())
    }

    #[test]
    fn liquidation_fixtures_fill_recent_market_cache() -> Result<()> {
        let binance = build_binance();
        binance
            .advanced()
            .ingest_public_json(binance::PUBLIC_LIQUIDATION)?;
        let liquidations = binance
            .market()
            .liquidations(&bat_markets::types::InstrumentId::from("BTC/USDT:USDT"))
            .expect("binance liquidation cache should be populated");
        assert_eq!(liquidations.len(), 1);

        let bybit = build_bybit();
        bybit
            .advanced()
            .ingest_public_json(bybit::PUBLIC_LIQUIDATION)?;
        let liquidations = bybit
            .market()
            .liquidations(&bat_markets::types::InstrumentId::from("BTC/USDT:USDT"))
            .expect("bybit liquidation cache should be populated");
        assert_eq!(liquidations.len(), 1);

        Ok(())
    }

    #[test]
    fn amend_command_classification_is_supported_on_both_venues() -> Result<()> {
        let binance = build_binance();
        let receipt = binance.advanced().classify_command_json(
            CommandOperation::AmendOrder,
            Some(binance::COMMAND_AMEND_OK),
            None,
        )?;
        assert_eq!(receipt.status, CommandStatus::Accepted);

        let bybit = build_bybit();
        let receipt = bybit.advanced().classify_command_json(
            CommandOperation::AmendOrder,
            Some(bybit::COMMAND_AMEND_OK),
            None,
        )?;
        assert_eq!(receipt.status, CommandStatus::Accepted);

        Ok(())
    }

    #[test]
    fn batch_command_surface_is_supported_on_both_venues() -> Result<()> {
        let binance = build_binance();
        let create = binance.advanced().classify_command_json(
            CommandOperation::CreateOrders,
            Some(binance::COMMAND_BATCH_CREATE_OK),
            None,
        )?;
        let amend = binance.advanced().classify_command_json(
            CommandOperation::AmendOrders,
            Some(binance::COMMAND_BATCH_AMEND_OK),
            None,
        )?;
        let cancel = binance.advanced().classify_command_json(
            CommandOperation::CancelOrders,
            Some(binance::COMMAND_BATCH_CANCEL_OK),
            None,
        )?;
        assert_eq!(create.status, CommandStatus::Accepted);
        assert_eq!(amend.status, CommandStatus::Accepted);
        assert_eq!(cancel.status, CommandStatus::Accepted);

        let bybit = build_bybit();
        let create = bybit.advanced().classify_command_json(
            CommandOperation::CreateOrders,
            Some(bybit::COMMAND_BATCH_CREATE_OK),
            None,
        )?;
        let amend = bybit.advanced().classify_command_json(
            CommandOperation::AmendOrders,
            Some(bybit::COMMAND_BATCH_AMEND_OK),
            None,
        )?;
        let cancel = bybit.advanced().classify_command_json(
            CommandOperation::CancelOrders,
            Some(bybit::COMMAND_BATCH_CANCEL_OK),
            None,
        )?;
        assert_eq!(create.status, CommandStatus::Accepted);
        assert_eq!(amend.status, CommandStatus::Accepted);
        assert_eq!(cancel.status, CommandStatus::Accepted);

        Ok(())
    }

    #[test]
    fn health_notifications_emit_only_for_structural_changes() -> Result<()> {
        let client = build_binance();
        let mut notifications = client.health().notifications();
        client
            .advanced()
            .ingest_public_json(binance::PUBLIC_TICKER)?;
        let first = notifications
            .try_recv()
            .expect("first public transition should emit a health notification");
        assert_eq!(first.previous.status, HealthStatus::Disconnected);
        assert_eq!(first.current.status, HealthStatus::Healthy);
        assert!(first.current.ws_public_ok);

        client
            .advanced()
            .ingest_public_json(binance::PUBLIC_BOOK_TICKER)?;
        assert!(matches!(notifications.try_recv(), Err(TryRecvError::Empty)));

        Ok(())
    }

    #[test]
    fn late_fill_after_cancel_stays_explicit() -> Result<()> {
        let client = build_bybit();
        client
            .advanced()
            .ingest_private_json(bybit::PRIVATE_ORDER)?;
        client
            .advanced()
            .ingest_private_json(bybit::PRIVATE_ORDER_CANCELED)?;
        client
            .advanced()
            .ingest_private_json(bybit::PRIVATE_EXECUTION_LATE_AFTER_CANCEL)?;

        let orders = client.trade().orders();
        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].status, OrderStatus::Canceled);
        assert_eq!(client.trade().executions().len(), 1);

        Ok(())
    }

    #[tokio::test]
    async fn runtime_stub_entry_batch_path_covers_validate_create_and_cancel() -> Result<()> {
        let rest_stub = RuntimeRestStub::spawn().expect("local runtime stub should bind");
        let rest_base = rest_stub.rest_base();

        for venue in [
            bat_markets::types::Venue::Binance,
            bat_markets::types::Venue::Bybit,
        ] {
            let command_ws_stub =
                RuntimeCommandWsStub::spawn(venue).expect("local command ws stub should bind");
            let client = build_runtime_dual_stub_client(
                venue,
                &rest_base,
                &command_ws_stub.command_ws_base(),
            );

            for (index, request) in runtime_stub_validate_requests(venue)
                .into_iter()
                .enumerate()
            {
                let mut handle = client.validate_order(&request).await?;
                assert_eq!(handle.ack().receipt.status, CommandStatus::Accepted);
                if index == 0 {
                    let lifecycle = timeout(Duration::from_secs(1), handle.next_lifecycle())
                        .await
                        .expect(
                            "pre-subscribed validate handle should not miss its lifecycle ack",
                        )?;
                    assert!(matches!(lifecycle, CommandLifecycleEvent::Ack(_)));
                }
            }

            let mut create_handles = client
                .create_orders(&runtime_stub_batch_create_request(venue))
                .await?;
            assert_eq!(create_handles.len(), 2);
            assert!(
                create_handles
                    .iter()
                    .all(|handle| handle.ack().receipt.status == CommandStatus::Accepted)
            );
            assert!(
                create_handles
                    .iter()
                    .all(|handle| handle.ack().transport == CommandTransport::WebSocket)
            );
            let lifecycle = timeout(Duration::from_secs(1), create_handles[0].next_lifecycle())
                .await
                .expect("pre-subscribed command handle should not miss its lifecycle ack")?;
            assert!(matches!(lifecycle, CommandLifecycleEvent::Ack(_)));

            let ws_create_handles = client
                .create_orders_ws(&runtime_stub_batch_create_request(venue))
                .await?;
            assert_eq!(ws_create_handles.len(), 2);
            assert!(
                ws_create_handles
                    .iter()
                    .all(|handle| handle.ack().transport == CommandTransport::WebSocket)
            );

            let mut cancel_handles = client
                .cancel_orders(&runtime_stub_batch_cancel_request(venue))
                .await?;
            assert_eq!(cancel_handles.len(), 2);
            assert!(
                cancel_handles
                    .iter()
                    .all(|handle| handle.ack().receipt.status == CommandStatus::Accepted)
            );
            assert!(
                cancel_handles
                    .iter()
                    .all(|handle| handle.ack().transport == CommandTransport::WebSocket)
            );
            let lifecycle = timeout(Duration::from_secs(1), cancel_handles[0].next_lifecycle())
                .await
                .expect("pre-subscribed cancel handle should not miss its lifecycle ack")?;
            assert!(matches!(lifecycle, CommandLifecycleEvent::Ack(_)));

            let ws_cancel_handles = client
                .cancel_orders_ws(&runtime_stub_batch_cancel_request(venue))
                .await?;
            assert_eq!(ws_cancel_handles.len(), 2);
            assert!(
                ws_cancel_handles
                    .iter()
                    .all(|handle| handle.ack().transport == CommandTransport::WebSocket)
            );
        }

        Ok(())
    }

    #[tokio::test]
    async fn websocket_only_cancel_all_returns_unsupported_without_rest_fallback() -> Result<()> {
        let client = build_binance();
        let error = match client
            .cancel_all_orders_ws(&CancelAllOrdersRequest {
                request_id: None,
                instrument_id: None,
            })
            .await
        {
            Ok(_) => panic!("cancel_all_orders_ws should not fall back to REST"),
            Err(error) => error,
        };

        assert_eq!(error.kind, ErrorKind::Unsupported);
        Ok(())
    }

    fn ingest_binance(client: &BatMarkets) -> Result<()> {
        let advanced = client.advanced();
        advanced.ingest_public_json(binance::PUBLIC_TICKER)?;
        advanced.ingest_public_json(binance::PUBLIC_TRADE)?;
        advanced.ingest_public_json(binance::PUBLIC_BOOK_TICKER)?;
        advanced.ingest_public_json(binance::PUBLIC_MARK_PRICE)?;
        advanced.ingest_public_json(binance::PUBLIC_KLINE)?;
        advanced.ingest_public_json(binance::OPEN_INTEREST)?;
        advanced.ingest_private_json(binance::PRIVATE_ACCOUNT)?;
        advanced.ingest_private_json(binance::PRIVATE_ORDER)?;
        let receipt = advanced.classify_command_json(
            CommandOperation::CreateOrder,
            Some(binance::COMMAND_CREATE_OK),
            None,
        )?;
        assert_eq!(receipt.status, CommandStatus::Accepted);
        Ok(())
    }

    fn ingest_bybit(client: &BatMarkets) -> Result<()> {
        let advanced = client.advanced();
        advanced.ingest_public_json(bybit::PUBLIC_TICKER)?;
        advanced.ingest_public_json(bybit::PUBLIC_TRADE)?;
        advanced.ingest_public_json(bybit::PUBLIC_ORDERBOOK)?;
        advanced.ingest_public_json(bybit::PUBLIC_KLINE)?;
        advanced.ingest_private_json(bybit::PRIVATE_WALLET)?;
        advanced.ingest_private_json(bybit::PRIVATE_POSITION)?;
        advanced.ingest_private_json(bybit::PRIVATE_ORDER)?;
        advanced.ingest_private_json(bybit::PRIVATE_EXECUTION)?;
        let receipt = advanced.classify_command_json(
            CommandOperation::CreateOrder,
            Some(bybit::COMMAND_CREATE_OK),
            None,
        )?;
        assert_eq!(receipt.status, CommandStatus::Accepted);
        Ok(())
    }
}
