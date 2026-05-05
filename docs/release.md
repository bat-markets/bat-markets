# Release Process

## Crates.io Contract

`bat-markets` is prepared as a crates.io workspace release.

Published crates:

- `bat-markets-core`
- `bat-markets-binance`
- `bat-markets-bybit`
- `bat-markets-mexc`
- `bat-markets`

Workspace-only crate:

- `bat-markets-testing` remains `publish = false` because it contains fixtures,
  live smoke helpers, and benches for repository validation.

## Versioning

All runtime crates share the workspace version. For `0.x`, public API changes can
still happen, but each published crate must keep dependency versions aligned with
the workspace release. Release automation treats the workspace version as the
single source of truth and refuses to publish when any internal dependency,
`Cargo.lock`, changelog entry, or `v*` tag is out of sync.

Dependency order:

1. `bat-markets-core`
2. `bat-markets-binance`
3. `bat-markets-bybit`
4. `bat-markets-mexc`
5. `bat-markets`

## Local Release Gate

Run:

```bash
./scripts/check.sh
```

This gate runs:

- release consistency verification,
- formatting,
- all-target/all-feature clippy,
- workspace tests and doctests,
- workspace docs,
- single-venue clippy,
- `cargo audit`,
- strict workspace packaging,
- benchmark compilation.

Before publishing, run package verification for the whole runtime graph:

```bash
./scripts/publish-crates.sh --dry-run
```

The dry-run packages all publishable runtime crates together and verifies the
same dependency graph that crates.io will receive. It does not rely on expected
downstream failures.

## Publishing

Publishing is automated by `.github/workflows/publish-crates.yml`.

Create a protected GitHub Actions environment named `crates-io`, then add a
repository or environment secret named `CARGO_REGISTRY_TOKEN`.

Using GitHub CLI:

```bash
gh secret set CARGO_REGISTRY_TOKEN --env crates-io
```

Use a freshly created crates.io token. If a token was pasted into chat, logs, or
any non-secret channel, revoke it before adding a replacement secret.

Run a workflow dry-run from GitHub Actions first. A manual dispatch with
`dry_run=true` verifies packaging without uploading.

To publish, tag the exact workspace version:

```bash
VERSION="$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[] | select(.name == "bat-markets") | .version')"
git tag -a "v${VERSION}" -m "v${VERSION}"
git push origin "v${VERSION}"
```

The publish workflow verifies the repository, publishes in dependency order, and
waits for each crate version to become visible before publishing dependants.
It runs with workflow concurrency so only one crates.io publication can happen at
a time.

Manual fallback:

```bash
CARGO_REGISTRY_TOKEN=... ./scripts/publish-crates.sh
```

The manual fallback also refuses dirty worktrees, mismatched tags, stale
lockfiles, and internal dependency version drift. Never commit tokens, write
them into scripts, or paste them into logs.

## Trusted Publishing

After the initial manual/token-backed publication, prefer crates.io Trusted
Publishing for future releases. Configure each crate on crates.io with:

- owner/repository: `bat-markets/bat-markets`
- workflow: `publish-crates.yml`
- environment: `crates-io`

Trusted Publishing removes the long-lived `CARGO_REGISTRY_TOKEN` secret and uses
GitHub Actions OIDC to mint short-lived publish credentials.
