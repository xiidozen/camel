#!/usr/bin/env bash
# Build, test and document the Rust workspace. Run before every push on the `rust` branch.
set -euo pipefail
cd "$(dirname "$0")/../.."
cargo fmt --all -- --check
cargo build --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
