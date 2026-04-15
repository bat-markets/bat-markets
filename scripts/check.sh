#!/usr/bin/env bash

set -euo pipefail

cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo doc --workspace --no-deps
cargo check -p bat-markets --no-default-features --features binance
cargo check -p bat-markets --no-default-features --features bybit
cargo run -p bat-markets --example binance_fixture_walkthrough
cargo run -p bat-markets --example bybit_fixture_walkthrough
cargo bench -p bat-markets-testing --no-run
