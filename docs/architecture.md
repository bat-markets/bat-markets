# Architecture

## Source Of Truth

The source of truth order for this repository is:

1. `blueprint.md`
2. repository code and tests
3. Rust ecosystem best practices
4. documented engineering decisions in ADRs

## Workspace Shape

The workspace uses a virtual root manifest and five crates:

- `bat-markets`: public facade crate
- `bat-markets-core`: core contracts, state, and adapter interfaces
- `bat-markets-binance`: Binance linear futures adapter
- `bat-markets-bybit`: Bybit linear futures adapter
- `bat-markets-testing`: fixtures, smoke helpers, and benchmarks

Live transport is implemented inside the facade crate rather than split into a new public crate.
This keeps HTTP/WS choices replaceable during `0.x` and avoids prematurely freezing a transport contract.

## Public API Philosophy

The public facade must stay:

- narrow,
- explicit,
- typed,
- honest about exchange differences.

Unified APIs exist only where semantics are actually stable. The primary user
surface follows the CCXT-style root API: `fetch_*` for REST reads, `watch_*` for
live websocket reads, command verbs for writes, and `advanced()` for low-level
escape hatches. Venue-specific behavior stays in `advanced().native()`.

## Public Facade Ownership

Each public surface owns one job. If a new method does not fit one row, the API
boundary should be reconsidered before it is added.

| Surface | Responsibility | Avoid putting here |
| --- | --- | --- |
| `BatMarkets` root methods | metadata, public/private REST, public/private WS, commands, and status | raw payload classification or adapter-specific protocol details |
| `advanced()` | raw lane ingest/subscriptions, command classification, manual reconcile, diagnostics, and native adapter access | common user workflows that fit root `fetch_*`, `watch_*`, or command methods |

## Method Contract Rules

- `build()` is offline/static and must not perform network I/O.
- `build_live().await` performs live bootstrap before returning the facade.
- `markets()` is a synchronous cached metadata read.
- `load_markets().await` refreshes live metadata through REST.
- Root `fetch_*` methods are REST reads; private `fetch_*` methods may merge
  repairable snapshots into state.
- Root `watch_*` methods acquire shared websocket-hub leases and return typed
  RAII handles.
- Root command methods return lifecycle-aware `PendingCommandHandle` values.
- Explicit `*_ws` command variants must force websocket transport and return
  `Unsupported` rather than silently falling back to REST.
- `advanced().subscribe_*_events` attaches to an existing local event bus.
- `advanced().native()` is the only documented venue-specific escape hatch.

## Three API Layers

### Native

Native types and decoders preserve exchange semantics.
This layer exists for:

- exchange-specific payloads,
- exchange-specific flags and quirks,
- low-level adapter inspection.

### Fast Normalized

Fast normalized types are compact and lane-friendly.
They are used for:

- ticker and trade fan-out,
- health snapshots,
- state application inputs.

This layer prefers quantized integer values plus stable identifiers.

### Unified Ergonomic

Unified ergonomic types provide a clean application-facing contract.
They use typed wrappers, explicit enums, and immutable snapshots.

## Three Execution Lanes

### Public Market Data Lane

Input:

- public websocket payloads,
- market snapshots,
- venue metadata refreshes.

Output:

- fast normalized market events,
- cached ticker and book top state,
- optional health updates.

### Private State Lane

Input:

- private websocket payloads,
- REST-derived account or position snapshots,
- reconciliation repairs,
- reconnect-triggered snapshot refreshes.

Output:

- orders,
- executions,
- balances,
- positions,
- divergence and health signals.

### Command Lane

Input:

- command responses,
- command timeouts,
- reconciliation confirmations,
- rate-limited live REST submissions.

Output:

- accepted/rejected command receipts,
- state hints,
- explicit `UnknownExecution` classifications when outcome is uncertain.

## Numeric Model

The architecture uses two numeric forms:

- public/state value objects based on `Decimal`
- fast normalized quantized integers for hot-path friendly events

No public or state contract uses `f64`.

## Live Transport Shape

The live runtime follows these rules:

- `build()` remains the fixture/static constructor used by unit tests and offline harnesses
- `build_live().await` performs server-time sync and metadata bootstrap before the facade is returned
- HTTP/WS clients remain internal implementation details
- venue-specific transport details stay behind `AdapterHandle` dispatch rather than a public universal trait
- reconnect and reconcile remain explicit engine concerns, not hidden transport behavior

Internally the facade runtime is split into transport, subscription, feed, entry, reconcile, and diagnostics concerns even though they remain private modules inside `bat-markets`.

### Subscription Hubs

Live websocket usage is multiplexed through shared hubs rather than “one task per watcher”.

- one public hub fans out typed market-data events
- one private hub fans out order, execution, balance, position, and account events
- typed `watch_*` methods hold lightweight leases over those shared hubs
- duplicate watchers do not intentionally multiply exchange websocket subscriptions

### Feed And Projection Flow

The runtime publishes lane events before higher-level projections are queried by consumers.

- public feed updates fan out through the shared public bus
- private feed updates fan out through the shared private bus
- synchronous snapshot readers stay cheap through cached state access
- diagnostics and health remain operator-facing snapshots rather than a required external metrics stack

### Command Plane

The root command API is intentionally separated from read-side `fetch_*` methods.

- root command methods return fast acknowledgements through `PendingCommandHandle`
- command lifecycle events are broadcast through the shared command lane
- websocket order entry is used where the venue supports and the runtime validates it
- REST remains the fallback path for venue-specific gaps and for settings/validation flows that are still REST-native
- explicit `*_ws` command methods disable REST fallback
- uncertain outcomes stay explicit and schedule reconcile in the background rather than blocking the hot path
- command and account-setting flows route through root methods; nested lane clients are internal implementation details
- manual stream runners are internal test and transport primitives; normal application code should use `watch_*` leases so shared hub subscriptions are preserved and accidental duplicate sockets are avoided

### Metadata Bootstrap

Instrument metadata is no longer treated as a hardcoded adapter constant in live mode.
The runtime fetches venue snapshots and updates both:

- the adapter-side symbol resolver used by decoders
- the engine-side `InstrumentSpec` registry used by normalization and validation

### Reconcile Foundation

Reconcile is triggered by:

- reconnect after a private stream drop,
- explicit sequence-gap markers,
- periodic maintenance while live streams are running,
- `UnknownExecution`,
- explicit manual refresh calls.

The repair path uses REST snapshots to refresh:

- account balances and summary,
- positions,
- open orders,
- recent execution history,
- order-history evidence where the venue exposes it.

### Health Access

Health remains cheap to query as a snapshot, but live mode also exposes subscriptions for health changes.
Snapshot reads are synchronous; notifications are async and best-effort.
Live stream runners also perform periodic maintenance ticks for idle detection, metadata refresh, and stale private-state reconcile.

### Diagnostics Access

Live mode also exposes a cheap diagnostics snapshot for:

- shared-state read/write lock wait and hold costs,
- key runtime REST and reconcile latencies,
- command-lane acknowledgement and lifecycle latency tracking,
- operator-facing live stress sanity checks without an external metrics backend.

## Testing Strategy

The current foundation relies on:

- unit tests for domain logic and quantization,
- fixture-backed protocol tests for Binance and Bybit payloads,
- state-engine tests for order, balance, position, and execution transitions,
- smoke tests for the facade,
- benchmarks for decode, normalization, and state apply.

## Validation Assets

Executable examples are not part of the public package surface. Repository
validation lives in tests, fixtures, and benches:

- benches: `crates/bat-markets-testing/benches/`
- fixtures: `fixtures/`
- integration and live smoke tests: `crates/bat-markets-testing/tests/`
