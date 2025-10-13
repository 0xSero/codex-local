#!/usr/bin/env bash
# Wrapper to always run the local repo's latest codex-local binary
# Priority: uses release build if present, then debug; otherwise builds and runs via cargo.

set -euo pipefail

REPO_DIR="$HOME/projects/codex-local"
RUST_DIR="$REPO_DIR/codex-rs"
BIN_RELEASE="$RUST_DIR/target/release/codex-local"
BIN_DEBUG="$RUST_DIR/target/debug/codex-local"

if [[ -x "$BIN_RELEASE" ]]; then
  exec "$BIN_RELEASE" "$@"
elif [[ -x "$BIN_DEBUG" ]]; then
  exec "$BIN_DEBUG" "$@"
else
  cd "$RUST_DIR"
  exec cargo run -p codex-cli --bin codex-local --release -- "$@"
fi

