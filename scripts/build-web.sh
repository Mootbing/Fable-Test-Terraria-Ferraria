#!/usr/bin/env bash
# Build the wasm client and place it where the server's static dir expects it.
set -euo pipefail
cd "$(dirname "$0")/.."
scripts/dev.sh cargo build -p ferraria-client --target wasm32-unknown-unknown --release
cp target/wasm32-unknown-unknown/release/ferraria-client.wasm web/ferraria-client.wasm
echo "web/ferraria-client.wasm: $(du -h web/ferraria-client.wasm | cut -f1)"
