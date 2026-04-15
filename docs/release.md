# Release Model

## 0.1.x Contract

`bat-markets` `0.1.x` is distributed as a GitHub/source release, not a crates.io package.

This is intentional:

- the public facade still depends on internal workspace crates;
- those internal crates are deliberately `publish = false`;
- the repository is still allowed to reshape internal boundaries during `0.x`.

The release model stays honest to the current architecture instead of pretending the registry surface is already stable.

## Supported Consumption Paths

Use a tagged git dependency:

```toml
[dependencies]
bat-markets = { git = "https://github.com/bat-markets/bat-markets.git", tag = "v0.1.0" }
```

Or consume a GitHub source archive from the tagged release page and verify its SHA-256 checksum.

## How A Source Release Is Cut

1. Run the full local gate with `./scripts/check.sh`.
2. Create an annotated version tag such as `v0.1.0`.
3. Push the tag to GitHub.
4. The release workflow reruns checks, creates a source tarball from the tagged git tree, writes a SHA-256 file, and uploads both assets to the GitHub release.

## What A Source Release Guarantees

- the tagged git tree passed the repository quality gate;
- the uploaded archive is derived from the tagged source tree;
- checksums are published for operator verification.

## What It Does Not Guarantee Yet

- crates.io publication;
- long-term stability of internal workspace crate boundaries;
- registry-friendly semver for every internal crate.
