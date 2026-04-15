# Contributing

## Ground Rules

- blueprint first, implementation second
- no hidden globals
- no `f64` in public or state contracts
- no fake unification across venues
- secrets only through environment variables
- every non-trivial architectural change needs an ADR

## Local Checks

Use the same checks as CI:

```bash
./scripts/check.sh
```

## Release Discipline

For `0.1.x`, cut releases from GitHub tags and source archives only.

- do not run `cargo publish`
- keep workspace crates `publish = false` until there is an ADR for registry publication
- use `./scripts/source-release.sh <tag>` to build the archive and checksum shape used by GitHub releases

## Scope Discipline

The current focus is the `0.1.x` futures-first foundation.
Do not add spot, asset write flows, or wide venue abstractions without an ADR and an explicit roadmap update.
