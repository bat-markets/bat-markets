# Roadmap

## Current Milestone

Ship a minimal, registry-ready `0.1.x` foundation for Binance and Bybit linear
futures without bundling operator demos, unpublished harnesses, or broad venue
promises into the public package surface.

## In Scope

- async live REST/WS transport behind the facade
- metadata bootstrap from Binance and Bybit snapshots
- reconnect-aware shared public/private stream runners
- sequence-aware transport gap detection and snapshot-driven repair foundations
- recent execution and order-history repair where a venue makes it available
- low-latency `entry()` command plane with lifecycle tracking
- websocket-first command routing where the venue supports it
- env-gated sandbox and mainnet validation in the unpublished testing crate
- crates.io package metadata, package checks, and release documentation
- public rustdoc and concise package README documentation

## Explicitly Out Of Scope

- spot
- asset transfers
- withdrawals
- deposits
- convert
- options
- persistence inside core
- fake cross-venue abstractions for unstable semantics
- bundled web/operator demos in published crates

## Ordered Backlog

1. tighten local realtime account and position projection when Binance omits `ACCOUNT_UPDATE`
2. reduce allocation and serialization cost in the command hot path
3. keep fixture/static mode stable while live/runtime coverage expands
4. expand rustdoc around `entry()` and stream surfaces
5. add more venue-native stress and latency probes inside `bat-markets-testing`
6. add release automation once manual crates.io publication is proven
