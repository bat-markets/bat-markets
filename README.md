# bat-markets

`bat-markets` is a futures-first, headless Rust exchange engine for Binance USD-M
and Bybit USDT linear futures.

The crate focuses on a narrow production surface:

- typed market, account, position, trade, stream, and order-entry APIs;
- decimal domain types instead of `f64` in public/state contracts;
- fixture-backed native adapters for Binance and Bybit payloads;
- shared public/private websocket lanes with typed watchers;
- explicit command acknowledgements, lifecycle events, and `UnknownExecution`
  recovery semantics;
- in-memory state, health, and diagnostics snapshots for live runners.

## Install

```toml
[dependencies]
bat-markets = "0.1"
```

Default features enable Binance, Bybit, and private trading surfaces:

```toml
[dependencies]
bat-markets = { version = "0.1", default-features = false, features = ["binance"] }
```

Feature flags:

- `binance`: enable the Binance USD-M linear futures adapter.
- `bybit`: enable the Bybit USDT linear futures adapter.
- `private-trading`: expose authenticated order-entry and private account flows.
- `metrics`: reserved for metrics integrations.
- `serde`: reserved for serde-facing API expansion.

## Quick Start

```rust,no_run
use bat_markets::{
    BatMarkets,
    types::{Product, Venue},
};

fn main() -> bat_markets::errors::Result<()> {
    let client = BatMarkets::builder()
        .venue(Venue::Binance)
        .product(Product::LinearUsdt)
        .build()?;

    println!("{:?}", client.capabilities().market);
    Ok(())
}
```

Live REST/WS clients are async and read credentials from environment variables by
default:

```rust,no_run
use bat_markets::{
    BatMarkets,
    types::{Product, Venue},
};

#[tokio::main]
async fn main() -> bat_markets::errors::Result<()> {
    let client = BatMarkets::builder()
        .venue(Venue::Bybit)
        .product(Product::LinearUsdt)
        .build_live()
        .await?;

    let instruments = client.market().instrument_specs();
    println!("loaded {} instruments", instruments.len());
    Ok(())
}
```

## Public API Map

The facade is split by responsibility. Prefer the narrowest surface that matches
the job:

| Surface | Use it for | Network behavior |
| --- | --- | --- |
| `BatMarkets::builder()` | choose venue, product, and config | none |
| `build()` | fixture/static clients for tests and offline tools | none |
| `build_live().await` | live clients with server-time sync and metadata bootstrap | REST during startup |
| `market()` | market snapshots plus public REST reads | snapshot getters are local; `fetch_*` and `refresh_*` use REST |
| `stream().public()` | public websocket ingestion, subscriptions, and typed watchers | `subscribe_*` is local; `watch_*` leases shared WS runners |
| `stream().private()` | account/order/position private stream state and reconcile | `subscribe_*` is local; `watch_*` uses one shared account stream |
| `entry()` | live order-entry and account-setting commands | write path; returns command handles |
| `trade()` | read-side order and execution snapshots | snapshot getters are local; refresh methods use REST |
| `position()` | position snapshots and compatibility settings methods | snapshot getters are local; refresh/settings use REST |
| `account()` | balances and account summary | snapshot getters are local; `refresh()` uses REST |
| `health()` | cheap runtime health snapshots and notifications | local |
| `diagnostics()` | lock/runtime latency snapshots | local |
| `native()` | venue-specific adapter access when unified semantics are not enough | adapter-specific |

## Method Naming Rules

- Snapshot methods such as `market().ticker(...)`, `trade().orders()`, and
  `account().summary()` read cached engine state and never perform network I/O.
- `fetch_*` methods are public-market REST reads and return fresh venue data.
- `refresh_*` methods call REST and merge the result back into engine state.
- `subscribe_*` methods listen to events that are already flowing through the
  local event bus.
- `watch_*` methods acquire a lightweight lease on shared live websocket hubs;
  duplicate watchers do not intentionally create duplicate sockets.
- `entry().*` methods are write commands. They return `PendingCommandHandle`
  values so callers can observe acknowledgement, lifecycle, and recovery.

## Runtime Flow

1. Adapters decode native Binance/Bybit payloads into lane events.
2. Public/private lanes fan out typed events and project them into cached state.
3. REST `fetch_*` and `refresh_*` methods fill gaps or repair snapshots.
4. The command lane classifies write outcomes and keeps `UnknownExecution`
   explicit until reconciliation finds evidence.
5. Health and diagnostics expose cheap operator snapshots without requiring an
   external metrics stack.

## Safety Model

`bat-markets` is intentionally conservative:

- live credentials are supplied through environment variables or explicit config;
- secrets are never required for public market reads;
- live write tests are opt-in and environment-gated;
- exchange-specific semantics remain visible through `native()` instead of being
  hidden behind fake cross-venue guarantees;
- uncertain command outcomes stay explicit until local state or reconciliation
  evidence resolves them.

## Workspace Crates

- `bat-markets`: public facade and ergonomic API.
- `bat-markets-core`: domain contracts, error taxonomy, state engine, and adapter traits.
- `bat-markets-binance`: Binance USD-M linear futures adapter.
- `bat-markets-bybit`: Bybit USDT linear futures adapter.
- `bat-markets-testing`: unpublished workspace-only fixtures, smoke helpers, and benches.

Publishing is automated from the protected GitHub Actions environment
`crates-io`. Runtime crates are published in dependency order:

```bash
./scripts/publish-crates.sh
```

Store crates.io tokens only as GitHub Actions secrets or local environment
variables. Never commit tokens to the repository.

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
cargo audit
cargo package --workspace --exclude bat-markets-testing --allow-dirty
./scripts/publish-crates.sh --dry-run
```

## Documentation

- [Architecture](docs/architecture.md)
- [Capability matrix](docs/capability-matrix.md)
- [Error model](docs/error-model.md)
- [Release process](docs/release.md)
- [Roadmap](docs/roadmap.md)
