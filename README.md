# bat-markets

`bat-markets` is a futures-first, headless Rust exchange engine.

The project intentionally stays narrow:

- initial venue focus: Binance and Bybit,
- initial product focus: linear futures / perpetuals,
- initial architecture focus: typed domain contracts, honest native boundaries, and a small facade crate.

This repository currently implements the initial production-grade foundation:

- a virtual workspace with explicit crate boundaries,
- a strict core model without `f64` in public/state contracts,
- three API layers encoded in code and docs,
- three execution lanes encoded in code and docs,
- fixture-backed Binance and Bybit adapters for linear futures payloads,
- a state engine that applies private events and tracks command uncertainty,
- quality gates for formatting, linting, tests, docs, and benchmarks.

## Release Model

`0.1.x` is currently a GitHub/source release, not a crates.io package.

Use a tagged git dependency:

```toml
[dependencies]
bat-markets = { git = "https://github.com/bat-markets/bat-markets.git", tag = "v0.1.0" }
```

The repository keeps every workspace crate `publish = false` until there is an explicit crates.io strategy for the facade and internal crate boundaries.

## Crates

- `bat-markets`: public facade and ergonomic API
- `bat-markets-core`: internal domain contracts and state engine
- `bat-markets-binance`: Binance linear futures adapter
- `bat-markets-bybit`: Bybit linear futures adapter
- `bat-markets-testing`: shared fixtures, smoke helpers, and benchmarks

## What This Milestone Is

This milestone is the engine-first foundation for `0.1.x`.

It is designed to be:

- honest about venue differences,
- testable without live keys,
- narrow enough to evolve safely,
- ready for transport integration without breaking the core model.

## What This Milestone Is Not

This repository does not yet claim complete live transport coverage for every `0.1.0` operation.
The implemented foundation focuses first on:

- parsing native exchange payloads,
- mapping them into normalized and unified events,
- maintaining market/private state in memory,
- classifying command outcomes, including `UnknownExecution`.

Any live or sandbox checks remain opt-in and env-gated.

## Quick Start

Run the full local quality gate:

```bash
./scripts/check.sh
```

Run the workspace tests:

```bash
cargo test --workspace
```

Read the architecture documents:

- [`docs/architecture.md`](docs/architecture.md)
- [`docs/roadmap.md`](docs/roadmap.md)
- [`docs/error-model.md`](docs/error-model.md)
- [`docs/release.md`](docs/release.md)
- [`blueprint.md`](blueprint.md)
