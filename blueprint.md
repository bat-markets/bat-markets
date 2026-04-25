# bat-markets Blueprint

**Status:** active
**Audience:** maintainers, contributors, early adopters
**Scope:** open-source Rust exchange engine for linear futures

`bat-markets` is a headless Rust exchange engine for Binance USD-M and Bybit
USDT linear futures. It is a library, not an HTTP server, UI, broker backend,
database layer, or custody/compliance platform.

## Product Goal

Provide a small, typed, futures-first Rust alternative to unified exchange APIs:

- CCXT-style user ergonomics where semantics are stable.
- No fake unification where venues behave differently.
- Low-latency websocket sharing and command handling.
- Explicit reconciliation when an order outcome is uncertain.
- A minimal crates.io package surface that is easy to audit.

## Public API Contract

The primary API lives on `BatMarkets`.

| Family | Methods | Responsibility |
| --- | --- | --- |
| Metadata | `markets`, `load_markets` | cached instrument metadata and live metadata refresh |
| Public REST | `fetch_ticker`, `fetch_tickers`, `fetch_order_book`, `fetch_ohlcv`, `fetch_trades`, `fetch_mark_price`, `fetch_funding_rate`, `fetch_open_interest`, `fetch_liquidations` | unauthenticated market reads |
| Private REST | `fetch_balance`, `fetch_positions`, `fetch_open_orders`, `fetch_order`, `fetch_my_trades` | authenticated account, position, order, and execution reads |
| Public WS | `watch_ticker`, `watch_tickers`, `watch_trades`, `watch_trades_for_symbols`, `watch_order_book`, `watch_ohlcv`, `watch_ohlcv_for_symbols`, `watch_mark_price`, `watch_funding_rate`, `watch_open_interest`, `watch_liquidations`, `watch_status` | typed live updates over shared websocket hubs |
| Private WS | `watch_balance`, `watch_orders`, `watch_my_trades`, `watch_positions` | typed private updates over one shared private hub |
| Commands | `create_order`, `create_orders`, `edit_order`, `edit_orders`, `cancel_order`, `cancel_orders`, `cancel_all_orders`, `close_position`, `validate_order`, `set_leverage`, `set_margin_mode`, `set_position_mode` | write flows returning lifecycle-aware `PendingCommandHandle` values |
| WS commands | `create_order_ws`, `create_orders_ws`, `edit_order_ws`, `edit_orders_ws`, `cancel_order_ws`, `cancel_orders_ws`, `cancel_all_orders_ws` | websocket-only order-entry paths; unsupported venues return `Unsupported` |
| Advanced | `advanced().ingest_*`, `advanced().subscribe_*_events`, `advanced().classify_command_json`, `advanced().reconcile`, `advanced().diagnostics`, `advanced().native` | raw lane ingest, event subscriptions, custom transports, diagnostics, reconciliation, and venue-specific access |

Nested lane clients are implementation details. They are not the documented
application API and should not appear in new examples.

## Architecture

The crate keeps three internal lanes because they are useful for performance and
correctness:

- Public market-data lane: ticker, trade, book, mark-price, funding, open-interest, liquidation, and OHLCV events.
- Private state lane: balances, orders, executions, positions, and account summaries.
- Command lane: acknowledgements, receipts, lifecycle events, uncertainty, and recovery evidence.

Those lanes are internal mechanics. User code should start from method intent:
`fetch_*` for REST reads, `watch_*` for websocket reads, command verbs for writes,
and `advanced()` only when crossing into raw/custom behavior.

## Workspace

| Crate | Role |
| --- | --- |
| `bat-markets` | public facade, live runtime, shared websocket hubs, command lifecycle |
| `bat-markets-core` | domain types, state engine, error model, adapter traits |
| `bat-markets-binance` | Binance USD-M linear futures adapter |
| `bat-markets-bybit` | Bybit USDT linear futures adapter |
| `bat-markets-testing` | unpublished fixtures, stress helpers, live smoke harnesses, benches |

The published package set is limited to the runtime crates. Test harnesses,
fixtures, and benches stay in the repository but are excluded from crates.io.

## Design Rules

- Keep public contracts typed and explicit; no public `HashMap<String, Value>` catch-all models.
- Keep venue-specific behavior in `advanced().native()` or adapter crates.
- Keep side effects at runtime edges; state projection stays deterministic.
- Do not hide command uncertainty. Return `UnknownExecution` and reconcile with evidence.
- Do not create duplicate websocket runners for duplicate watchers. Root `watch_*` methods lease shared hubs.
- Do not add new public methods unless they fit the API table above or clearly belong in `advanced()`.
- Do not ship example servers, dashboards, or operator panels in the crate package.

## Release Rules

`Cargo.toml` workspace version is the single version source of truth. Release
automation must reject mismatched internal dependency versions, mismatched
`Cargo.lock` package entries, missing changelog entries, dirty package builds,
and tag/version drift.

The required local gate is:

```bash
./scripts/check.sh
./scripts/publish-crates.sh --dry-run
```

Real publishing is performed by GitHub Actions from the protected `crates-io`
environment or, if necessary, by the same script from a clean tagged checkout
with `CARGO_REGISTRY_TOKEN` supplied through the environment.
