#!/usr/bin/env bash
set -euo pipefail

ROOT="/Users/Shared/agent-vault/Agent"
INBOX="$ROOT/Inbox"
OUTBOX="$ROOT/Outbox"
LOGS="$ROOT/Logs"

mkdir -p "$OUTBOX" "$LOGS"

# Find first *.md file in Inbox (no subfolders)
INBOX_FILE="$(find "$INBOX" -maxdepth 1 -name '*.md' -type f | sort | head -n 1)"

if [[ -z "$INBOX_FILE" ]]; then
  echo "No inbox items."
  exit 0
fi

ORIGINAL_NAME="$(basename "$INBOX_FILE" .md)"
TIMESTAMP="$(date '+%Y-%m-%d_%H%M')"
REPORT="$OUTBOX/${TIMESTAMP}-${ORIGINAL_NAME}-digest.md"
TODAY="$(date '+%Y-%m-%d')"

# Copy first 200 lines into report
head -n 200 "$INBOX_FILE" > "$REPORT"

# Append planned sections
cat >> "$REPORT" <<'EOF'

## Planned Actions
- (placeholder)

## Next Step
- (placeholder)
EOF

# Append log line
echo "[$TIMESTAMP] Digest created: $(basename "$REPORT") from $(basename "$INBOX_FILE")" >> "$LOGS/${TODAY}.md"

echo "Digest written: $REPORT"
exit 0
