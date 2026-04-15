# ADR-0003: Engine-First Foundation Before Live Transport

## Status

Accepted

## Context

The blueprint requires transport abstractions, state, reconciliation semantics, and honest adapter boundaries.
The repository started with no workspace or code.

## Decision

The first implementation milestone prioritizes:

- domain contracts,
- adapter-native decoding,
- fast normalization,
- state application,
- command classification,
- fixture-backed reproducibility.

Live transport integration is deliberately staged behind these contracts instead of being rushed into the first code drop.

## Consequences

- the initial milestone is reproducible without secrets
- quality gates can validate semantics before network complexity is introduced
- this is an implementation-order choice, not a change to the blueprint scope

