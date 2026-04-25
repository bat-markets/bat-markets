# Security Policy

## Secrets

- never commit API keys or secrets
- never print secret material in logs or errors
- use environment variables only
- store crates.io tokens as GitHub Actions secrets or local environment
  variables only; never write tokens into repository files or shell scripts

## Unsafe Operations

Private trading and any future asset-write paths must remain feature-gated and opt-in.

## Reporting

Open a private security report with:

- affected crate/module,
- reproduction steps,
- expected vs actual behavior,
- impact assessment.
