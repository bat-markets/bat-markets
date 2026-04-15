# ADR-0004: Internal Live Transport And Snapshot-Driven Reconcile

## Status

Accepted

## Context

The engine-first foundation is already in place:

- typed domain contracts,
- native decoding,
- fast normalization,
- in-memory state,
- command classification.

The next milestone needs real REST/WS transport, metadata bootstrap, and reconnect/reconcile behavior without violating the blueprint rule against early stabilization of low-level adapter or transport contracts.

## Decision

The live runtime is implemented with these rules:

- `bat-markets` owns the async runtime-facing transport layer
- HTTP and WebSocket implementations stay internal
- venue-specific live behavior is selected through internal `AdapterHandle` dispatch
- adapters keep owning venue-native parsing and symbol-resolution logic
- live metadata snapshots replace hardcoded instrument specs at runtime
- reconnect repair is snapshot-driven and explicit

`build()` stays synchronous and reproducible for fixture-backed work.
`build_live().await` becomes the entry point for network-backed runtime construction.

## Consequences

- the public facade stays narrow
- tests can still run without secrets or network
- internal transport/runtime code can evolve during `0.x`
- reconcile semantics become testable without pretending transport alone can guarantee correctness
- metadata is honest in live mode instead of being frozen to placeholder specs
