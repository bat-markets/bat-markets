# ADR-0001: Project Shape

## Status

Accepted

## Context

The blueprint defines `bat-markets` as a futures-first, headless Rust exchange engine with a small public facade and explicit adapter boundaries.

## Decision

The repository uses:

- one public facade crate,
- one internal core crate,
- one adapter crate per venue,
- one testing crate for fixtures and benchmarks.

The core crate is not treated as a public product contract.

## Consequences

- internal contracts can evolve during `0.x`
- the public API surface remains small
- new venues can be added without polluting the facade with venue-specific models

