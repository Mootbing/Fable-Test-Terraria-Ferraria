#!/usr/bin/env bash
# Full verification: fmt, clippy, tests, server build, wasm client build.
# Every PR must pass this before merge.
set -euo pipefail
cd "$(dirname "$0")/.."
scripts/dev.sh cargo fmt --all --check
scripts/dev.sh cargo clippy --workspace --exclude ferraria-client -- -D warnings
scripts/dev.sh cargo clippy -p ferraria-client --target wasm32-unknown-unknown -- -D warnings
scripts/dev.sh cargo test
scripts/dev.sh cargo build -p ferraria-client --target wasm32-unknown-unknown
echo "ALL CHECKS PASSED"
