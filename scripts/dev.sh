#!/usr/bin/env bash
# Run any command inside the ferraria-dev container, in the caller's cwd.
# The host has no C toolchain, so all cargo invocations must go through this.
# Usage: scripts/dev.sh cargo build -p ferraria-server
set -euo pipefail

IMG=ferraria-dev:latest
NAME=ferraria-dev

if ! docker ps --format '{{.Names}}' | grep -qx "$NAME"; then
  docker rm -f "$NAME" >/dev/null 2>&1 || true
  docker run -d --name "$NAME" \
    -v /home/mootbing/code:/home/mootbing/code \
    -v ferraria-cargo-registry:/usr/local/cargo/registry \
    -v ferraria-cargo-git:/usr/local/cargo/git \
    -w /home/mootbing/code/ferraria \
    "$IMG" sleep infinity >/dev/null
fi

exec docker exec -w "$(pwd)" "$NAME" "$@"
