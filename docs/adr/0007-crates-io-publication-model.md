# ADR 0007: Crates.io Publication Model

## Status

Accepted

## Context

The project is moving from a GitHub/source-only `0.1.x` release shape to a
minimal crates.io package surface. The old operator demo and broad example set
were useful during development, but they are not part of the public package
contract and make registry publication noisier and riskier.

The facade still depends on internal workspace crates, so those runtime crates
must become publishable and version-aligned. The testing crate contains fixtures,
live smoke helpers, and benches; it should remain a repository-only validation
tool.

## Decision

Publish the runtime crates in dependency order:

1. `bat-markets-core`
2. `bat-markets-binance`
3. `bat-markets-bybit`
4. `bat-markets`

Keep `bat-markets-testing` unpublished with `publish = false`.

Remove bundled examples and the Bun operator panel from the release surface.
Keep fixtures, tests, and benches in the repository as validation assets.

Use a dedicated GitHub Actions release workflow:

- trigger: `v*` tags or manual dry-run dispatch
- environment: `crates-io`
- secret: `CARGO_REGISTRY_TOKEN` for the first token-backed release
- script: `scripts/publish-crates.sh`

After initial publication, configure crates.io Trusted Publishing for each
published crate and remove the long-lived token secret.

## Consequences

Positive:

- crates.io consumers get a clean facade crate and publishable adapter graph;
- package metadata, docs, and release instructions match the actual distribution model;
- operator-only demos do not ship as public package surface;
- the testing harness remains free to evolve without registry churn.

Negative:

- examples that were previously runnable with `cargo run --example ...` are no longer shipped;
- release order matters because the facade depends on adapter crates and core;
- `0.x` semver still requires careful changelog discipline even while APIs evolve.
- the first publication still needs a crates.io token until Trusted Publishing is configured for the crates.
