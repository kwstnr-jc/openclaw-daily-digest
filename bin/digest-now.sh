#!/usr/bin/env bash
# On-demand trigger for the inbox orchestrator.
# Called by Discord (via OpenClaw) or manually.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
BINARY="$REPO_DIR/target/release/openclaw-daily-digest"

if [[ ! -x "$BINARY" ]]; then
  echo "Error: Rust binary not found at $BINARY"
  echo "Run: cd $REPO_DIR && cargo build --release"
  exit 1
fi

echo "Running digest..."
OUTPUT=$("$BINARY" run 2>&1) || EXIT_CODE=$?
EXIT_CODE=${EXIT_CODE:-0}

echo "$OUTPUT"

if [[ "$EXIT_CODE" -eq 0 ]]; then
  echo "Digest completed successfully."
else
  echo "Digest failed."
fi

exit "$EXIT_CODE"
