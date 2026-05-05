# Changelog

## Unreleased

## 0.3.4 - 2026-05-05

- add the `bat-markets-mexc` USDT-M futures adapter and `Venue::Mexc`
- wire MEXC REST metadata, public reads, public/private streams, private read snapshots, auth, and rate-limit buckets into the runtime
- wire MEXC order submit, batch submit, cancel, cancel-all, leverage, margin-mode, and position-mode commands to documented REST endpoints while preserving code notes for endpoints the official docs mark as maintenance
- add fixture-backed MEXC parser coverage and live harness routing for MEXC credentials

## 0.3.3 - 2026-05-01

- align Binance public websocket routing with the split market/public mainnet endpoints
- centralize shared linear-futures capability and lane defaults across venue adapters
- refresh compatible transitive dependencies and keep release checks cwd-independent
- reduce multi-symbol subscription startup allocations and fixture-test boilerplate

## 0.3.2 - 2026-04-26

- keep public event broadcasts consistent with accepted engine state and reject unknown-instrument public events before subscribers see them
- tighten command lifecycle matching so concurrent same-operation commands without shared explicit ids cannot consume each other's events
- fail zero-venue builds with an explicit feature-selection error instead of cfg-related compiler fallout

## 0.3.1 - 2026-04-25

- remove non-essential docs pages (`capability-matrix`, `error-model`, migration guides, roadmap) and keep docs focused on architecture and release flow
- simplify the README documentation block to only stable, working links
- add automatic GitHub Release creation in the publish workflow so tag-driven publishing remains consistent across Releases and crates.io

## 0.3.0 - 2026-04-25

- remove legacy nested facade accessors and lane-client exports from the public crate surface
- keep public usage centered on root `fetch_*`, `watch_*`, command verbs, `status()`, and `advanced()`
- move cached state inspection for tests/custom tooling under explicit `advanced().cached_*` helpers
- remove public amend request aliases from `bat-markets`; public code should use `EditOrderRequest` and `EditOrdersRequest`
- remove legacy top-of-book REST/watch and account-summary watch helpers in favor of `fetch_order_book`, `watch_order_book`, `fetch_balance`, and `watch_balance`
- update repository tests, benches, and live harnesses to exercise the root API and `advanced()` escape hatch only
- align workspace package versions and internal dependency versions for a clean `0.3.0` release

## 0.2.0 - 2026-04-25

- redesign the primary facade around root methods: `fetch_*`, `watch_*`, command verbs, and `advanced()`
- add root metadata, public REST, private REST, public WS, private WS, command, status, and advanced methods on `BatMarkets`
- add public `EditOrderRequest` and `EditOrdersRequest` names while keeping internal amend semantics compatible
- make `fetch_balance()` return a full `AccountSnapshot` with balances and summary
- add websocket-only command variants that return `Unsupported` instead of silently falling back to REST
- move raw lane ingestion, raw event subscriptions, command JSON classification, manual reconcile, diagnostics, and native adapter access under `advanced()`
- document the new API map in README and crate rustdoc
- add `docs/migration-v0.2.md` with direct v0.1-to-v0.2 method mappings
- update architecture, capability, and roadmap docs for the root API shape

## 0.1.1 - 2026-04-25

- bootstrap workspace structure from the blueprint
- add core domain contracts and error taxonomy
- add execution lane and state engine foundation
- add Binance and Bybit linear futures adapters with fixture-backed parsing
- add facade API, tests, fixtures, and quality gates
- batch recent-history `UnknownExecution` repair per instrument to cut repeated REST history calls
- keep periodic private reconcile snapshot-only unless health or pending commands require recent-history repair
- bound recent-history repair to local timestamp windows instead of broad symbol-level pulls
- resolve pending `UnknownExecution` outcomes against local state before issuing remote repair queries
- prefetch recent execution evidence only for local active/recent instruments when the reconcile trigger indicates stream gap or divergence
- tolerate sparse Binance live account position fields and numeric zero-shapes instead of failing reconcile
- prepare `0.1.x` as a crates.io release with publishable runtime crates, package checks, and registry documentation
- add live diagnostics snapshots for shared-state lock wait/hold costs and key runtime latencies to guide future perf decisions
- add unified `market().fetch_ohlcv(...)` for Binance and Bybit REST kline history
- allow `market().fetch_ohlcv(...)` to batch `1..=30` instruments per call while preserving canonical intervals and per-candle `instrument_id`
- make `market().fetch_ohlcv(...)` fully paginate bounded OHLCV ranges whenever both `start_time` and `end_time` are provided; `fetch_ohlcv_window(...)` and `fetch_ohlcv_all(...)` remain compatibility aliases
- add typed `stream().public().watch_ohlcv(...)` for one or many symbols on Binance and Bybit
- add unified `market().fetch_ticker(...)`, `market().fetch_trades(...)`, and `market().fetch_book_top(...)` for public market snapshots
- add typed `stream().public().watch_ticker(...)`, `watch_trades(...)`, and `watch_book_top(...)` for one or many symbols
- normalize OHLCV intervals to canonical values like `1m`, `5m`, `1h`, and `1d` across REST fetches and websocket watches
- fix live Bybit `watch_ohlcv()` parsing when websocket kline payloads omit per-row `symbol` and only surface it in the topic name
- add realistic OHLCV stress harness coverage for multi-symbol live fetch/watch flows and frontend-style `30 symbols x 3 days x 1m` paging
- remove bundled examples and the Bun realtime operator panel from the public package surface
- resolve `UnknownExecution` command handles from reconciliation reports instead of returning stale uncertain receipts
- upgrade `rustls-webpki` and trim optional decimal dependencies to keep the cargo audit surface clean
