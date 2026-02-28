#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib/discord.sh"

echo "=== Daily Digest Run: $(date) ==="
output=$("$SCRIPT_DIR/bin/process-inbox.sh" 2>&1) || true
echo "$output"

# Build digest summary
summary="📬 **Daily Digest Run — $(TZ=Europe/Zurich date '+%Y-%m-%d %H:%M') (Bern)**"
summary+=$'\n'"─────────────────────────────"
summary+=$'\n'"$output"

post_digest "$summary"
echo "=== Digest Run Complete ==="
