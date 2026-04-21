# Roadmap

## Current Milestone

Stabilize the live, WS-first futures `0.1.x` foundation and ship it honestly as a GitHub/source release before any crates.io packaging work.

## In Scope

- async live REST/WS transport behind the facade
- metadata bootstrap from Binance and Bybit snapshots
- reconnect-aware shared public/private stream runners
- sequence-aware transport gap detection and snapshot-driven repair foundations
- recent execution / order-history repair where venue makes it available
- periodic health/reconcile/metadata maintenance inside live runners
- low-latency `entry()` command plane with lifecycle tracking
- websocket-first command routing where the venue supports it
- env-gated sandbox integration tests
- operator-oriented mainnet smoke and stress harnesses for approved testing subaccounts
- Bun-based realtime operator panel and expanded runnable examples
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

1. tighten local realtime account and position projection when Binance omits `ACCOUNT_UPDATE`
2. reduce allocation and serialization cost in the command hot path
3. keep fixture/static mode stable while live/runtime coverage expands
4. expand operator docs and rustdoc around the new `entry()` and stream surfaces
5. add more venue-native stress and latency probes for focused production workflows
6. formalize GitHub/source release workflow for `0.1.x`
