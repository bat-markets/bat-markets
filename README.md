# bat-markets

Headless Rust exchange engine for Binance USD-M and Bybit USDT linear futures.

`bat-markets` gives applications a typed root client for:

- REST market reads
- REST private/account reads
- shared websocket watchers
- order and account commands
- deterministic state projection and reconciliation
- low-level adapter access when a venue-specific escape hatch is needed

## Install

```toml
[dependencies]
bat-markets = "0.3"
```

Single-venue build:

```toml
[dependencies]
bat-markets = { version = "0.3", default-features = false, features = ["binance"] }
```

Feature flags:

| Feature | Purpose |
| --- | --- |
| `binance` | Binance USD-M linear futures adapter |
| `bybit` | Bybit USDT linear futures adapter |
| `private-trading` | Authenticated private reads and commands |
| `metrics` | Reserved for metrics integrations |
| `serde` | Reserved for serde-facing API expansion |

At least one venue feature must be enabled. The default build enables both
venues; use `default-features = false` with `features = ["binance"]` or
`features = ["bybit"]` for a single-venue build.

## Quick Start

Offline/static client:

```rust
use bat_markets::{
    BatMarkets,
    types::{Product, Venue},
};

fn main() -> bat_markets::errors::Result<()> {
    let client = BatMarkets::builder()
        .venue(Venue::Binance)
        .product(Product::LinearUsdt)
        .build()?;

    println!("{} bundled markets", client.markets().len());
    Ok(())
}
```

Live public data:

```rust,no_run
use bat_markets::{
    BatMarkets,
    types::{InstrumentId, Product, Venue},
};

#[tokio::main]
async fn main() -> bat_markets::errors::Result<()> {
    let client = BatMarkets::builder()
        .venue(Venue::Bybit)
        .product(Product::LinearUsdt)
        .build_live()
        .await?;

    let symbol = InstrumentId::from("BTC/USDT:USDT");
    let ticker = client.fetch_ticker(&symbol).await?;
    let mut trades = client.watch_trades(symbol).await?;

    println!("last price: {:?}", ticker.last_price);
    println!("next trade: {:?}", trades.recv().await?);
    trades.shutdown().await?;
    Ok(())
}
```

## Runtime Rules

| Area | Rule |
| --- | --- |
| `build()` | Offline constructor. No network I/O. Useful for fixtures, metadata, and local ingestion. |
| `build_live().await` | Live constructor. Builds transport, resolves auth, syncs time, refreshes metadata. |
| Public reads | Do not require credentials. |
| Private reads, private watches, commands | Use explicit config or `BINANCE_API_KEY`, `BINANCE_API_SECRET`, `BYBIT_API_KEY`, `BYBIT_API_SECRET`. |
| Watch handles | Use `recv().await` for the next typed event. Use `shutdown().await` or drop the handle to release the shared lease. |
| Command handles | Immediate `ack()` is available. `receipt().await`, `next_lifecycle().await`, and `resolved().await` observe command outcome. |
| Uncertain writes | Timeouts/disconnects can return `UnknownExecution`; reconciliation resolves what local evidence can prove. |

## API Reference

### Builder And Identity

| Method | Input | Output | Purpose |
| --- | --- | --- | --- |
| `BatMarkets::builder` | none | `BatMarketsBuilder` | Start client configuration. |
| `BatMarketsBuilder::venue` | `Venue` | `BatMarketsBuilder` | Select exchange venue. |
| `BatMarketsBuilder::product` | `Product` | `BatMarketsBuilder` | Select product family. Defaults to `LinearUsdt`. |
| `BatMarketsBuilder::config` | `BatMarketsConfig` | `BatMarketsBuilder` | Replace full runtime config. |
| `BatMarketsBuilder::build` | none | `Result<BatMarkets>` | Build offline/static client. |
| `BatMarketsBuilder::build_live` | none | `Result<BatMarkets>` | Build live client with REST/bootstrap. |
| `venue` | none | `Venue` | Selected venue. |
| `product` | none | `Product` | Selected product family. |

### Metadata, Status, Capabilities

| Method | Input | Output | Purpose |
| --- | --- | --- | --- |
| `markets` | none | `Vec<InstrumentSpec>` | Cached market metadata. No network I/O. |
| `instrument_specs` | none | `Vec<InstrumentSpec>` | Same metadata projection as `markets`. |
| `load_markets` | none | `Result<Vec<InstrumentSpec>>` | Refresh venue metadata through REST. |
| `capabilities` | none | `CapabilitySet` | Venue/product support matrix. |
| `lane_set` | none | `LaneSet` | Public/private/command lane support. |
| `status` | none | `HealthReport` | Cheap synchronous runtime health snapshot. |
| `watch_status` | none | `StatusWatch` | Health change watcher. |

### Public REST Reads

| Method | Input | Output | Payload |
| --- | --- | --- | --- |
| `fetch_ticker` | `&InstrumentId` | `Result<Ticker>` | Latest ticker snapshot. |
| `fetch_tickers` | `Vec<InstrumentId>` | `Result<Vec<Ticker>>` | Batch ticker snapshots. |
| `fetch_order_book` | `&InstrumentId`, `Option<usize>` | `Result<OrderBookSnapshot>` | Focused depth snapshot. |
| `fetch_ohlcv` | `&FetchOhlcvRequest` | `Result<Vec<Kline>>` | Historical candles; supports bounded window paging. |
| `fetch_trades` | `&InstrumentId`, `Option<usize>` | `Result<Vec<TradeTick>>` | Recent public trades. |
| `fetch_mark_price` | `&InstrumentId` | `Result<MarkPrice>` | Latest mark-price snapshot. |
| `fetch_funding_rate` | `&InstrumentId` | `Result<FundingRate>` | Latest funding-rate snapshot. |
| `fetch_open_interest` | `&InstrumentId` | `Result<OpenInterest>` | Latest open-interest snapshot. |
| `fetch_liquidations` | `&InstrumentId`, `Option<usize>` | `Result<Vec<Liquidation>>` | Recent cached liquidation events. |

### Private REST Reads

| Method | Input | Output | Payload |
| --- | --- | --- | --- |
| `fetch_balance` | none | `Result<AccountSnapshot>` | Balances plus optional account summary. |
| `fetch_positions` | none | `Result<Vec<Position>>` | Current positions. |
| `fetch_open_orders` | `Option<&ListOpenOrdersRequest>` | `Result<Vec<Order>>` | Open orders scoped by request. |
| `fetch_order` | `&GetOrderRequest` | `Result<Order>` | Single order snapshot. |
| `fetch_my_trades` | `Option<&ListExecutionsRequest>` | `Result<Vec<Execution>>` | Private execution history. |

### Public Websocket Watches

All public watchers use the shared public websocket hub.

| Method | Input | Output | Event from `recv().await` |
| --- | --- | --- | --- |
| `watch_ticker` | `InstrumentId` | `Result<TickerWatch>` | `Ticker` |
| `watch_tickers` | `Vec<InstrumentId>` | `Result<TickerWatch>` | `Ticker` |
| `watch_trades` | `InstrumentId` | `Result<TradesWatch>` | `TradeTick` |
| `watch_trades_for_symbols` | `Vec<InstrumentId>` | `Result<TradesWatch>` | `TradeTick` |
| `watch_order_book` | `InstrumentId`, `Option<usize>` | `Result<OrderBookWatch>` | `OrderBookDelta` |
| `watch_ohlcv` | `InstrumentId`, interval string | `Result<OhlcvWatch>` | `Kline` |
| `watch_ohlcv_for_symbols` | `Vec<InstrumentId>`, interval string | `Result<OhlcvWatch>` | `Kline` |
| `watch_mark_price` | `InstrumentId` | `Result<MarkPriceWatch>` | `MarkPrice` |
| `watch_funding_rate` | `InstrumentId` | `Result<FundingRateWatch>` | `FundingRate` |
| `watch_open_interest` | `InstrumentId` | `Result<OpenInterestWatch>` | `OpenInterest` |
| `watch_liquidations` | `InstrumentId` | `Result<LiquidationWatch>` | `Liquidation` |

### Private Websocket Watches

All private watchers use the shared authenticated private websocket hub.

| Method | Input | Output | Event from `recv().await` |
| --- | --- | --- | --- |
| `watch_balance` | none | `Result<BalancesWatch>` | `Balance` |
| `watch_orders` | none | `Result<OrdersWatch>` | `Order` |
| `watch_my_trades` | none | `Result<ExecutionsWatch>` | `Execution` |
| `watch_positions` | none | `Result<PositionsWatch>` | `Position` |

### Watch Handles

| Method | Output | Applies to | Purpose |
| --- | --- | --- | --- |
| `recv().await` | `Result<T>` | all typed watch handles | Wait for the next matching event. |
| `shutdown().await` | `Result<()>` | root watch handles | Explicitly release the local subscription lease. |
| `current()` | `HealthReport` | `StatusWatch` | Read the current health snapshot without waiting. |
| `abort()` | `()` | stream-compatible public handles | Stop waiting on the handle; shared hubs own the socket. |
| `wait().await` | `Result<()>` | stream-compatible public handles | Complete the handle lifecycle. Prefer `shutdown()` in new code. |

### Commands

Default command methods choose the best supported live transport and may use REST
fallback where the venue requires it.

| Method | Input | Output | Effect |
| --- | --- | --- | --- |
| `create_order` | `&CreateOrderRequest` | `Result<PendingCommandHandle>` | Submit one order. |
| `create_orders` | `&CreateOrdersRequest` | `Result<Vec<PendingCommandHandle>>` | Submit batch orders. |
| `edit_order` | `&EditOrderRequest` | `Result<PendingCommandHandle>` | Modify one order. |
| `edit_orders` | `&EditOrdersRequest` | `Result<Vec<PendingCommandHandle>>` | Modify batch orders. |
| `cancel_order` | `&CancelOrderRequest` | `Result<PendingCommandHandle>` | Cancel one order. |
| `cancel_orders` | `&CancelOrdersRequest` | `Result<Vec<PendingCommandHandle>>` | Cancel batch orders. |
| `cancel_all_orders` | `&CancelAllOrdersRequest` | `Result<PendingCommandHandle>` | Cancel all orders in request scope. |
| `close_position` | `&ClosePositionRequest` | `Result<PendingCommandHandle>` | Close a position using an actionable order. |
| `validate_order` | `&ValidateOrderRequest` | `Result<PendingCommandHandle>` | Validate without intentionally opening a position. |
| `set_leverage` | `&SetLeverageRequest` | `Result<PendingCommandHandle>` | Set instrument leverage. |
| `set_margin_mode` | `&SetMarginModeRequest` | `Result<PendingCommandHandle>` | Set margin mode. |
| `set_position_mode` | `&SetPositionModeRequest` | `Result<PendingCommandHandle>` | Set one-way or hedge mode. |

Websocket-only command methods never fall back to REST. Unsupported paths return
`ErrorKind::Unsupported`.

| Method | Input | Output |
| --- | --- | --- |
| `create_order_ws` | `&CreateOrderRequest` | `Result<PendingCommandHandle>` |
| `create_orders_ws` | `&CreateOrdersRequest` | `Result<Vec<PendingCommandHandle>>` |
| `edit_order_ws` | `&EditOrderRequest` | `Result<PendingCommandHandle>` |
| `edit_orders_ws` | `&EditOrdersRequest` | `Result<Vec<PendingCommandHandle>>` |
| `cancel_order_ws` | `&CancelOrderRequest` | `Result<PendingCommandHandle>` |
| `cancel_orders_ws` | `&CancelOrdersRequest` | `Result<Vec<PendingCommandHandle>>` |
| `cancel_all_orders_ws` | `&CancelAllOrdersRequest` | `Result<PendingCommandHandle>` |

### Command Handle

| Method | Output | Purpose |
| --- | --- | --- |
| `ack` | `&CommandAck` | Immediate acknowledgement and initial receipt. |
| `receipt().await` | `Result<CommandReceipt>` | Initial receipt first, then matching receipt events. |
| `next_lifecycle().await` | `Result<CommandLifecycleEvent>` | Ack, recovery scheduling, recovery completion, and receipt lifecycle. |
| `resolved().await` | `Result<CommandReceipt>` | Best known final receipt after local recovery evidence. |

### Advanced

`advanced()` is for custom transports, fixture replay, diagnostics, and
venue-specific access. Normal application code should prefer root methods.

| Method | Input | Output | Purpose |
| --- | --- | --- | --- |
| `advanced` | none | `AdvancedClient` | Enter low-level API. |
| `ingest_public_json` | `&str` | `Result<Vec<PublicLaneEvent>>` | Decode and apply public native JSON. |
| `ingest_private_json` | `&str` | `Result<Vec<PrivateLaneEvent>>` | Decode and apply private native JSON. |
| `subscribe_public_events` | none | `broadcast::Receiver<PublicLaneEvent>` | Raw public event stream. |
| `subscribe_private_events` | none | `broadcast::Receiver<PrivateLaneEvent>` | Raw private event stream. |
| `subscribe_command_events` | none | `broadcast::Receiver<CommandLaneEvent>` | Raw command event stream. |
| `subscribe_health_notifications` | none | `broadcast::Receiver<HealthNotification>` | Structural health changes. |
| `require_instrument` | `&InstrumentId` | `Result<InstrumentSpec>` | Resolve known instrument metadata. |
| `cached_ticker` | `&InstrumentId` | `Option<Ticker>` | Cached ticker. |
| `cached_recent_trades` | `&InstrumentId` | `Option<Vec<TradeTick>>` | Cached recent public trades. |
| `cached_book_top` | `&InstrumentId` | `Option<BookTop>` | Cached top of book. |
| `cached_funding_rate` | `&InstrumentId` | `Option<FundingRate>` | Cached funding rate. |
| `cached_mark_price` | `&InstrumentId` | `Option<MarkPrice>` | Cached mark price. |
| `cached_open_interest` | `&InstrumentId` | `Option<OpenInterest>` | Cached open interest. |
| `cached_liquidations` | `&InstrumentId` | `Option<Vec<Liquidation>>` | Cached liquidation events. |
| `cached_balances` | none | `Vec<Balance>` | Cached balances. |
| `cached_account_summary` | none | `Option<AccountSummary>` | Cached account summary. |
| `cached_positions` | none | `Vec<Position>` | Cached positions. |
| `cached_orders` | none | `Vec<Order>` | Cached orders. |
| `cached_open_orders` | none | `Vec<Order>` | Cached open orders. |
| `cached_executions` | none | `Vec<Execution>` | Cached private executions. |
| `classify_command_json` | `CommandOperation`, `Option<&str>`, `Option<RequestId>` | `Result<CommandReceipt>` | Decode raw command response and apply state hint. |
| `reconcile().await` | none | `Result<ReconcileReport>` | Manual REST-backed private-state repair. |
| `diagnostics` | none | `RuntimeDiagnosticsSnapshot` | Runtime latency and state-lock diagnostics. |
| `native` | none | `NativeClient` | Venue-specific adapter access. |
| `native().binance()` | Binance client | `Result<&BinanceLinearFuturesAdapter>` | Binance adapter access. |
| `native().bybit()` | Bybit client | `Result<&BybitLinearFuturesAdapter>` | Bybit adapter access. |

## Runtime Architecture

```text
Application
  -> BatMarkets root methods
     -> live runtime: REST, WS runners, command transport, reconcile
     -> shared hubs: one public stream plan, one private stream lease set
     -> SharedState: EngineState, event buses, health, diagnostics
        -> bat-markets-core domain contracts
     -> venue adapter: Binance or Bybit native decoding/classification
```

Core rules:

- `bat-markets-core` owns domain types, state, errors, capabilities, and adapter traits.
- `bat-markets-binance` and `bat-markets-bybit` own native payload decoding and venue behavior.
- `bat-markets` owns the public facade and live transport runtime.
- `bat-markets-testing` is unpublished and owns fixtures, smoke tests, and benches.

## Quality Gate

```bash
./scripts/check.sh
```

The gate verifies release consistency, formatting, clippy, tests, docs,
single-venue clippy, audit, packaging, and benchmark compilation.

Focused commands:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
./scripts/publish-crates.sh --dry-run
```

Publishing is automated from the protected GitHub Actions environment
`crates-io`. Store crates.io tokens only as GitHub Actions secrets or local
environment variables. Never commit tokens.

## Documentation

- [Architecture](https://github.com/bat-markets/bat-markets/blob/main/docs/architecture.md)
- [Release process](https://github.com/bat-markets/bat-markets/blob/main/docs/release.md)
