# Roadmap

## Current Milestone

Stabilize the live futures-first `0.1.x` foundation and ship it honestly as a GitHub/source release before any crates.io packaging work.

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
- GitHub/source release workflow and documentation for tagged `0.1.x` cuts

## Explicitly Out Of Scope

- spot
- asset transfers
- withdrawals
- deposits
- convert
- options
- persistence inside core
- fake cross-venue abstractions for unstable semantics
- crates.io publication before a dedicated registry strategy exists

## Ordered Backlog

1. keep fixture/static mode stable while adding live mode
2. bootstrap live metadata and server-time skew checks
3. add real public/private REST/WS transport for Binance and Bybit
4. add reconnect-triggered and periodic reconcile/state repair
5. add env-gated sandbox read and minimal write integration tests
6. add manual read-only mainnet smoke harness
7. expand fixtures and docs to cover new runtime behavior
8. formalize GitHub/source release workflow for `0.1.x`
