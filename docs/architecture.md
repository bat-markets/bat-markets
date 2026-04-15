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
- `bat-markets-core`: internal contracts, state, and adapter interfaces
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

Unified APIs exist only where semantics are actually stable.
Venue-specific behavior stays in `native()`.

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

- `build()` remains the fixture/static constructor used by unit tests and offline examples
- `build_live().await` performs server-time sync and metadata bootstrap before the facade is returned
- HTTP/WS clients remain internal implementation details
- venue-specific transport details stay behind `AdapterHandle` dispatch rather than a public universal trait
- reconnect and reconcile remain explicit engine concerns, not hidden transport behavior

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
- operator-facing live stress sanity checks without an external metrics backend.

## Testing Strategy

The current foundation relies on:

- unit tests for domain logic and quantization,
- fixture-backed protocol tests for Binance and Bybit payloads,
- state-engine tests for order, balance, position, and execution transitions,
- smoke tests for the facade,
- benchmarks for decode, normalization, and state apply.

## Intentional Deviation From The Illustrative Blueprint Tree

The blueprint shows root-level `examples/` and `benches/`.
This repository uses a virtual workspace root, so executable examples and benches live with their owning crates:

- examples: `crates/bat-markets/examples/`
- benches: `crates/bat-markets-testing/benches/`

This keeps the workspace root clean without changing the architectural intent.
