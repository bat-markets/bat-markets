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

For `0.1.x`, publish runtime crates to crates.io only after the local gate and
package dry-runs pass.

- publish in dependency order: `bat-markets-core`, adapters, then `bat-markets`
- keep `bat-markets-testing` unpublished
- never commit or print crates.io tokens; use the protected GitHub environment secret `CARGO_REGISTRY_TOKEN`
- publish by pushing a version tag that matches `[workspace.package].version`

## Scope Discipline

The current focus is the `0.1.x` futures-first foundation.
Do not add spot, asset write flows, or wide venue abstractions without an ADR and an explicit roadmap update.
