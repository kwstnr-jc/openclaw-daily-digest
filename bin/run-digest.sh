#!/usr/bin/env bash
# Thin wrapper that delegates to the Rust binary.
# Keeps existing cron/launchd entrypoints working.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
BINARY="$REPO_DIR/target/release/openclaw-daily-digest"

if [[ ! -x "$BINARY" ]]; then
  echo "Rust binary not found at $BINARY. Falling back to bash implementation."
  exec bash "$SCRIPT_DIR/run-digest-bash.sh" "$@"
fi

exec "$BINARY" run "$@"
