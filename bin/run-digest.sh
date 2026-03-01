#!/usr/bin/env bash
# Thin wrapper that delegates to the Rust binary.
# Keeps existing cron/launchd entrypoints working.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
BINARY="$REPO_DIR/target/release/openclaw-daily-digest"

if [[ ! -x "$BINARY" ]]; then
  echo "Error: Rust binary not found at $BINARY"
  echo "Build it with: cd $REPO_DIR && cargo build --release"
  exit 1
fi

exec "$BINARY" run "$@"
