# ADR 0006: GitHub-First Source Release Model For 0.1.x

## Status

Accepted

## Context

The `0.1.x` milestone is production-shaped in architecture and testing, but it is not yet a valid crates.io publication target:

- the public facade depends on internal workspace crates that are intentionally `publish = false`;
- the project still wants freedom to reshape internal crate boundaries during `0.x`;
- the blueprint prefers architectural honesty over pretending the package surface is more stable than it is.

Shipping a crates.io package now would create a false signal about registry readiness and versioning guarantees.

## Decision

For `0.1.x`, `bat-markets` is released as a GitHub/source release:

- all workspace crates are marked `publish = false`;
- releases are cut from signed Git tags on GitHub, not from `cargo publish`;
- source archives are produced from the git tree with SHA-256 checksums;
- users consume the project from a tagged git dependency or a GitHub source archive.

## Consequences

Positive:

- the release story matches the real repository shape;
- operators get reproducible source artifacts and checksums;
- the team keeps freedom to evolve internal crates without registry churn.

Negative:

- there is no crates.io install path for `0.1.x`;
- future registry publication needs a separate ADR and crate-boundary strategy.
