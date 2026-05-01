#!/usr/bin/env bash

set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

./scripts/verify-release.sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo doc --workspace --no-deps
cargo clippy -p bat-markets --no-default-features --features binance -- -D warnings
cargo clippy -p bat-markets --no-default-features --features bybit -- -D warnings
cargo audit
cargo package --workspace --exclude bat-markets-testing --allow-dirty
cargo bench -p bat-markets-testing --no-run
