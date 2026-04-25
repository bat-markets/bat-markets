# bat-markets

`bat-markets` is a futures-first, headless Rust exchange engine for Binance
USD-M and Bybit USDT linear futures.

The v0.3 API follows the CCXT mental model:

- `fetch_*` methods are REST/read operations.
- `watch_*` methods are websocket/live operations.
- `create_*`, `edit_*`, `cancel_*`, `close_*`, and `set_*` methods are commands.
- `advanced()` is the escape hatch for raw lanes, diagnostics, reconciliation,
  and venue-specific native access.

Internally the engine still keeps separate public, private, and command lanes so
state projection, reconciliation, and websocket sharing stay efficient. Users do
not need to learn those lanes before using the crate.

## Install

```toml
[dependencies]
bat-markets = "0.3"
```

Enable a single venue when you want a smaller dependency tree:

```toml
[dependencies]
bat-markets = { version = "0.3", default-features = false, features = ["binance"] }
```

Feature flags:

- `binance`: enable the Binance USD-M linear futures adapter.
- `bybit`: enable the Bybit USDT linear futures adapter.
- `private-trading`: expose authenticated order-entry and private account flows.
- `metrics`: reserved for metrics integrations.
- `serde`: reserved for serde-facing API expansion.

## Quick Start

Static clients are useful for metadata, fixtures, and offline tools:

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

Live clients are async. Public market data does not need credentials; private
REST, private websocket, and command methods use explicit config or the venue
environment variables (`BINANCE_API_KEY`, `BINANCE_API_SECRET`,
`BYBIT_API_KEY`, `BYBIT_API_SECRET`).

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

## API Map

| Family | Methods | Responsibility |
| --- | --- | --- |
| Metadata/cache | `markets`, `load_markets` | local bundled metadata and live venue metadata refresh |
| Public REST | `fetch_ticker`, `fetch_tickers`, `fetch_order_book`, `fetch_ohlcv`, `fetch_trades`, `fetch_mark_price`, `fetch_funding_rate`, `fetch_open_interest`, `fetch_liquidations` | unauthenticated market reads |
| Private REST | `fetch_balance`, `fetch_positions`, `fetch_open_orders`, `fetch_order`, `fetch_my_trades` | authenticated account, position, order, and execution reads |
| Public WS | `watch_ticker`, `watch_tickers`, `watch_trades`, `watch_trades_for_symbols`, `watch_order_book`, `watch_ohlcv`, `watch_ohlcv_for_symbols`, `watch_mark_price`, `watch_funding_rate`, `watch_open_interest`, `watch_liquidations`, `watch_status` | typed live updates over shared websocket hubs |
| Private WS | `watch_balance`, `watch_orders`, `watch_my_trades`, `watch_positions` | authenticated account-stream updates over one shared private hub |
| Commands | `create_order`, `create_orders`, `edit_order`, `edit_orders`, `cancel_order`, `cancel_orders`, `cancel_all_orders`, `close_position`, `validate_order`, `set_leverage`, `set_margin_mode`, `set_position_mode` | write operations with lifecycle-aware `PendingCommandHandle` results |
| WS commands | `create_order_ws`, `create_orders_ws`, `edit_order_ws`, `edit_orders_ws`, `cancel_order_ws`, `cancel_orders_ws`, `cancel_all_orders_ws` | websocket-only command transport; unsupported paths return `Unsupported` instead of falling back to REST |
| Advanced | `advanced().ingest_public_json`, `advanced().ingest_private_json`, `advanced().subscribe_*_events`, `advanced().classify_command_json`, `advanced().reconcile`, `advanced().diagnostics`, `advanced().native` | low-level custom transports, fixture replay, diagnostics, reconciliation, and venue-specific access |

## Command Lifecycle

Command methods return `PendingCommandHandle` instead of hiding exchange
uncertainty:

```rust,no_run
use bat_markets::{
    BatMarkets,
    types::{CreateOrderRequest, Product, Venue},
};

#[tokio::main]
async fn main() -> bat_markets::errors::Result<()> {
    let client = BatMarkets::builder()
        .venue(Venue::Binance)
        .product(Product::LinearUsdt)
        .build_live()
        .await?;

    # let request: CreateOrderRequest = todo!();
    let mut handle = client.create_order(&request).await?;
    let ack = handle.ack();
    let final_receipt = handle.resolved().await?;

    println!("ack transport: {:?}", ack.transport);
    println!("final status: {:?}", final_receipt.status);
    Ok(())
}
```

`UnknownExecution` is intentional. If a timeout or disconnect leaves the
exchange outcome uncertain, the engine records the pending command and resolves
it through private-state or execution-history reconciliation instead of silently
treating it as success or failure.

## Runtime Model

1. Venue adapters decode native Binance/Bybit payloads into typed lane events.
2. Public and private lanes project those events into cached engine state.
3. Root `fetch_*` methods use REST and merge repairable snapshots back into
   state where appropriate.
4. Root `watch_*` methods acquire leases on shared websocket hubs, so duplicate
   watchers do not intentionally create duplicate sockets.
5. The command lane emits acknowledgements, receipts, lifecycle events, and
   recovery evidence.
6. `status()` and `advanced().diagnostics()` expose local operator visibility.

There is no global `un_watch`. Watch handles expose `recv().await` and
`shutdown().await`; dropping a handle releases the local websocket lease.

## Safety Model

`bat-markets` is intentionally conservative:

- Public market reads do not require secrets.
- Authenticated flows require explicit config or venue environment variables.
- Write command uncertainty remains explicit through `UnknownExecution`.
- Websocket-only command variants never fall back to REST silently.
- Venue-specific behavior is available through `advanced().native()` instead of
  being hidden behind fake cross-venue guarantees.

## Workspace Crates

- `bat-markets`: public facade and ergonomic API.
- `bat-markets-core`: domain contracts, error taxonomy, state engine, and adapter traits.
- `bat-markets-binance`: Binance USD-M linear futures adapter.
- `bat-markets-bybit`: Bybit USDT linear futures adapter.
- `bat-markets-testing`: unpublished workspace-only fixtures, smoke helpers, and benches.

## Development

Run the full local quality gate:

```bash
./scripts/check.sh
```

Useful focused checks:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
./scripts/publish-crates.sh --dry-run
```

Publishing is automated from the protected GitHub Actions environment
`crates-io`. Store crates.io tokens only as GitHub Actions secrets or local
environment variables. Never commit tokens to the repository.

## Documentation

- [Architecture](docs/architecture.md)
- [Capability matrix](docs/capability-matrix.md)
- [Error model](docs/error-model.md)
- [v0.3 migration guide](docs/migration-v0.3.md)
- [v0.2 migration guide](docs/migration-v0.2.md)
- [Release process](docs/release.md)
- [Roadmap](docs/roadmap.md)
