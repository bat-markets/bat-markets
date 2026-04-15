# ADR-0002: API Layers And Execution Lanes

## Status

Accepted

## Context

The blueprint rejects one-model-fits-all exchange design.

## Decision

The implementation explicitly separates:

- native payloads and adapters,
- fast normalized events,
- unified ergonomic snapshots.

It also separates:

- public market data lane,
- private state lane,
- command lane.

## Consequences

- hot-path data stays compact
- private state stays lossless at the model level
- command uncertainty is visible and testable
- venue-specific behavior does not leak into the unified layer

