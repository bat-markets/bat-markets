use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::{
    net::TcpStream,
    sync::{Mutex, mpsc, oneshot},
    time::{interval, timeout},
};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};

use bat_markets_core::{BatMarketsConfig, ErrorKind, MarketError, Product, Signer, Venue};

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub(crate) enum CommandWsRequestError {
    Unavailable(MarketError),
    Uncertain(MarketError),
}

pub(crate) struct CommandTransportHub {
    config: BatMarketsConfig,
    api_key: Option<Arc<str>>,
    signer: Option<Arc<dyn Signer>>,
    next_request_id: AtomicU64,
    session: Mutex<Option<Arc<CommandWsSession>>>,
}

struct CommandWsSession {
    venue: Venue,
    tx: mpsc::UnboundedSender<SessionCommand>,
}

struct PendingResponse {
    operation: Box<str>,
    response: oneshot::Sender<std::result::Result<String, CommandWsRequestError>>,
}

enum SessionCommand {
    Request {
        id: String,
        operation: Box<str>,
        payload: String,
        response: oneshot::Sender<std::result::Result<String, CommandWsRequestError>>,
    },
}

impl CommandTransportHub {
    pub(crate) fn new(
        config: BatMarketsConfig,
        api_key: Option<Arc<str>>,
        signer: Option<Arc<dyn Signer>>,
    ) -> Self {
        Self {
            config,
            api_key,
            signer,
            next_request_id: AtomicU64::new(1),
            session: Mutex::new(None),
        }
    }

    pub(crate) fn next_request_id(&self, prefix: &str) -> String {
        let next = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        format!("{prefix}-{next}")
    }

    pub(crate) async fn request_text(
        &self,
        id: String,
        operation: impl Into<Box<str>>,
        payload: String,
    ) -> std::result::Result<String, CommandWsRequestError> {
        let operation = operation.into();
        for attempt in 0..2 {
            let session = self.session().await?;
            match session
                .request(
                    id.clone(),
                    operation.clone(),
                    payload.clone(),
                    self.config.timeouts.command_ms,
                )
                .await
            {
                Ok(response) => return Ok(response),
                Err(CommandWsRequestError::Unavailable(error)) => {
                    self.invalidate_session(&session).await;
                    if attempt == 0 {
                        continue;
                    }
                    return Err(CommandWsRequestError::Unavailable(error));
                }
                Err(CommandWsRequestError::Uncertain(error)) => {
                    self.invalidate_session(&session).await;
                    return Err(CommandWsRequestError::Uncertain(error));
                }
            }
        }
        Err(CommandWsRequestError::Unavailable(command_ws_error(
            self.config.venue,
            "command_ws.request",
            "command websocket session exhausted",
        )))
    }

    async fn session(&self) -> std::result::Result<Arc<CommandWsSession>, CommandWsRequestError> {
        let mut guard = self.session.lock().await;
        if let Some(session) = guard.as_ref() {
            return Ok(Arc::clone(session));
        }

        let session = Arc::new(self.connect_session().await?);
        *guard = Some(Arc::clone(&session));
        Ok(session)
    }

    async fn invalidate_session(&self, session: &Arc<CommandWsSession>) {
        let mut guard = self.session.lock().await;
        if let Some(current) = guard.as_ref()
            && Arc::ptr_eq(current, session)
        {
            *guard = None;
        }
    }

    async fn connect_session(
        &self,
    ) -> std::result::Result<CommandWsSession, CommandWsRequestError> {
        let api_key = self.api_key.clone().ok_or_else(|| {
            CommandWsRequestError::Unavailable(auth_error(
                self.config.venue,
                "command_ws.connect",
                "missing API key for command websocket flow",
            ))
        })?;
        let signer = self.signer.clone().ok_or_else(|| {
            CommandWsRequestError::Unavailable(auth_error(
                self.config.venue,
                "command_ws.connect",
                "missing signer for command websocket flow",
            ))
        })?;

        let (mut ws, _) = connect_async(self.config.endpoints.command_ws_base.as_ref())
            .await
            .map_err(|error| {
                CommandWsRequestError::Unavailable(command_ws_error(
                    self.config.venue,
                    "command_ws.connect",
                    error.to_string(),
                ))
            })?;

        if self.config.venue == Venue::Bybit {
            authenticate_bybit_trade_session(
                &mut ws,
                api_key.as_ref(),
                signer.as_ref(),
                self.config.venue,
            )
            .await?;
        }

        let (tx, rx) = mpsc::unbounded_channel();
        let venue = self.config.venue;
        tokio::spawn(async move {
            let _ = run_command_ws_session(venue, ws, rx).await;
        });

        Ok(CommandWsSession { venue, tx })
    }
}

impl CommandWsSession {
    async fn request(
        &self,
        id: String,
        operation: Box<str>,
        payload: String,
        timeout_ms: u64,
    ) -> std::result::Result<String, CommandWsRequestError> {
        let (response_tx, response_rx) = oneshot::channel();
        self.tx
            .send(SessionCommand::Request {
                id,
                operation: operation.clone(),
                payload,
                response: response_tx,
            })
            .map_err(|_| {
                CommandWsRequestError::Unavailable(command_ws_error(
                    self.venue,
                    operation.as_ref(),
                    "command websocket session is not running",
                ))
            })?;

        match timeout(Duration::from_millis(timeout_ms.max(1)), response_rx).await {
            Ok(Ok(Ok(response))) => Ok(response),
            Ok(Ok(Err(error))) => Err(error),
            Ok(Err(_)) => Err(CommandWsRequestError::Uncertain(command_ws_error(
                self.venue,
                operation.as_ref(),
                "command websocket closed before response was delivered",
            ))),
            Err(_) => Err(CommandWsRequestError::Uncertain(timeout_error(
                self.venue,
                operation.as_ref(),
                "command websocket response timed out",
            ))),
        }
    }
}

async fn run_command_ws_session(
    venue: Venue,
    mut ws: WsStream,
    mut rx: mpsc::UnboundedReceiver<SessionCommand>,
) -> std::result::Result<(), MarketError> {
    let mut pending = BTreeMap::<String, PendingResponse>::new();
    let mut heartbeat = interval(Duration::from_secs(20));

    loop {
        tokio::select! {
            command = rx.recv() => {
                let Some(command) = command else {
                    let _ = ws.close(None).await;
                    return Ok(());
                };
                match command {
                    SessionCommand::Request { id, operation, payload, response } => {
                        ws.send(Message::Text(payload.into()))
                            .await
                            .map_err(|error| {
                                command_ws_error(venue, operation.as_ref(), format!("failed to send command websocket frame: {error}"))
                            })?;
                        pending.insert(id, PendingResponse { operation, response });
                    }
                }
            }
            _ = heartbeat.tick() => {
                match venue {
                    Venue::Binance => {
                        ws.send(Message::Ping(Vec::new().into()))
                            .await
                            .map_err(|error| command_ws_error(venue, "command_ws.ping", error.to_string()))?;
                    }
                    Venue::Bybit => {
                        let ping = serde_json::to_string(&json!({ "op": "ping" }))
                            .map_err(|error| command_ws_error(venue, "command_ws.ping", format!("failed to serialize heartbeat frame: {error}")))?;
                        ws.send(Message::Text(ping.into()))
                            .await
                            .map_err(|error| command_ws_error(venue, "command_ws.ping", error.to_string()))?;
                    }
                }
            }
            message = ws.next() => {
                let Some(message) = message else {
                    fail_pending(
                        venue,
                        &mut pending,
                        "command websocket closed while requests were in flight",
                    );
                    return Err(command_ws_error(venue, "command_ws.read", "command websocket closed"));
                };

                let message = message
                    .map_err(|error| {
                        fail_pending(
                            venue,
                            &mut pending,
                            format!("command websocket read failed: {error}"),
                        );
                        command_ws_error(venue, "command_ws.read", error.to_string())
                    })?;

                match message {
                    Message::Text(payload) => {
                        let payload = payload.to_string();
                        if let Some(response_id) = extract_response_id(venue, &payload)
                            && let Some(PendingResponse { response, .. }) =
                                pending.remove(&response_id)
                        {
                            let _ = response.send(Ok(payload));
                        }
                    }
                    Message::Ping(payload) => {
                        ws.send(Message::Pong(payload))
                            .await
                            .map_err(|error| command_ws_error(venue, "command_ws.pong", error.to_string()))?;
                    }
                    Message::Close(_) => {
                        fail_pending(
                            venue,
                            &mut pending,
                            "command websocket connection closed by remote peer",
                        );
                        return Err(command_ws_error(venue, "command_ws.read", "command websocket closed by peer"));
                    }
                    _ => {}
                }
            }
        }
    }
}

async fn authenticate_bybit_trade_session(
    ws: &mut WsStream,
    api_key: &str,
    signer: &dyn Signer,
    venue: Venue,
) -> std::result::Result<(), CommandWsRequestError> {
    let expires = now_ms().saturating_add(10_000);
    let signature = signer
        .sign_hex(format!("GET/realtime{expires}").as_bytes())
        .map_err(CommandWsRequestError::Unavailable)?;
    let frame = serde_json::to_string(&json!({
        "reqId": "auth",
        "op": "auth",
        "args": [api_key, expires, signature],
    }))
    .map_err(|error| {
        CommandWsRequestError::Unavailable(command_ws_error(
            venue,
            "bybit.command_ws.auth",
            format!("failed to serialize auth frame: {error}"),
        ))
    })?;

    ws.send(Message::Text(frame.into()))
        .await
        .map_err(|error| {
            CommandWsRequestError::Unavailable(command_ws_error(
                venue,
                "bybit.command_ws.auth",
                error.to_string(),
            ))
        })?;

    let deadline = Duration::from_secs(5);
    let auth = timeout(deadline, async {
        loop {
            let Some(message) = ws.next().await else {
                return Err(command_ws_error(
                    venue,
                    "bybit.command_ws.auth",
                    "connection closed before auth response",
                ));
            };
            let message = message.map_err(|error| {
                command_ws_error(venue, "bybit.command_ws.auth", error.to_string())
            })?;
            match message {
                Message::Text(payload) => {
                    let value = serde_json::from_str::<Value>(&payload).map_err(|error| {
                        command_ws_error(
                            venue,
                            "bybit.command_ws.auth",
                            format!("failed to decode auth response: {error}"),
                        )
                    })?;
                    if is_bybit_auth_success(&value) {
                        return Ok(());
                    }
                    if let Some(op) = value.get("op").and_then(Value::as_str)
                        && (op == "ping" || op == "pong")
                    {
                        continue;
                    }
                    let message = value
                        .get("retMsg")
                        .or_else(|| value.get("ret_msg"))
                        .and_then(Value::as_str)
                        .unwrap_or("bybit trade websocket authentication rejected");
                    return Err(auth_error(venue, "bybit.command_ws.auth", message));
                }
                Message::Ping(payload) => {
                    ws.send(Message::Pong(payload)).await.map_err(|error| {
                        command_ws_error(venue, "bybit.command_ws.auth", error.to_string())
                    })?;
                }
                Message::Close(_) => {
                    return Err(command_ws_error(
                        venue,
                        "bybit.command_ws.auth",
                        "connection closed during auth handshake",
                    ));
                }
                _ => {}
            }
        }
    })
    .await
    .map_err(|_| {
        CommandWsRequestError::Unavailable(timeout_error(
            venue,
            "bybit.command_ws.auth",
            "timed out waiting for auth response",
        ))
    })?;

    auth.map_err(CommandWsRequestError::Unavailable)
}

fn extract_response_id(venue: Venue, payload: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(payload).ok()?;
    match venue {
        Venue::Binance => value_to_id(value.get("id")?),
        Venue::Bybit => value
            .get("reqId")
            .or_else(|| value.get("req_id"))
            .and_then(value_to_id),
    }
}

fn value_to_id(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn is_bybit_auth_success(value: &Value) -> bool {
    value.get("success").and_then(Value::as_bool) == Some(true)
        || value.get("retCode").and_then(Value::as_i64) == Some(0)
}

fn fail_pending(
    venue: Venue,
    pending: &mut BTreeMap<String, PendingResponse>,
    message: impl Into<String>,
) {
    let message = message.into();
    for (_, pending) in std::mem::take(pending) {
        let _ = pending
            .response
            .send(Err(CommandWsRequestError::Uncertain(command_ws_error(
                venue,
                pending.operation.as_ref(),
                message.clone(),
            ))));
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

fn command_ws_error(venue: Venue, operation: &str, message: impl Into<String>) -> MarketError {
    MarketError::new(ErrorKind::TransportError, message.into())
        .with_venue(venue, Product::LinearUsdt)
        .with_operation(operation)
        .with_retriable(true)
}

fn auth_error(venue: Venue, operation: &str, message: impl Into<String>) -> MarketError {
    MarketError::new(ErrorKind::AuthError, message.into())
        .with_venue(venue, Product::LinearUsdt)
        .with_operation(operation)
        .with_retriable(false)
}

fn timeout_error(venue: Venue, operation: &str, message: impl Into<String>) -> MarketError {
    MarketError::new(ErrorKind::Timeout, message.into())
        .with_venue(venue, Product::LinearUsdt)
        .with_operation(operation)
        .with_retriable(true)
}
