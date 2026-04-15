# Roadmap

## Current Milestone

Integrate live transport, metadata bootstrap, and reconcile foundations on top of the existing engine contracts for the `0.1.x` futures-first scope.

## In Scope

- async live REST/WS transport behind the facade
- metadata bootstrap from Binance and Bybit snapshots
- reconnect-aware public/private stream runners
- sequence-aware transport gap detection and snapshot-driven repair foundations
- recent execution / order-history repair where venue makes it available
- periodic health/reconcile/metadata maintenance inside live runners
- env-gated sandbox integration tests
- manual read-only mainnet smoke harness for operator validation
- capability matrix and public rustdoc examples
- expanded fixtures for negative and contradictory scenarios

## Explicitly Out Of Scope

- spot
- asset transfers
- withdrawals
- deposits
- convert
- options
- persistence inside core
- fake cross-venue abstractions for unstable semantics

## Ordered Backlog

1. keep fixture/static mode stable while adding live mode
2. bootstrap live metadata and server-time skew checks
3. add real public/private REST/WS transport for Binance and Bybit
4. add reconnect-triggered and periodic reconcile/state repair
5. add env-gated sandbox read and minimal write integration tests
6. add manual read-only mainnet smoke harness
7. expand fixtures and docs to cover new runtime behavior
