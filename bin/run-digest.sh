#!/usr/bin/env bash
set -euo pipefail

ROOT="/Users/Shared/agent-vault/Agent"
INBOX="$ROOT/Inbox"
OUTBOX="$ROOT/Outbox"
LOGS="$ROOT/Logs"
PROCESSED="$INBOX/Processed"
FAILED="$INBOX/Failed"

mkdir -p "$OUTBOX" "$LOGS" "$PROCESSED" "$FAILED"

# Find first *.md file in Inbox (no subfolders)
INBOX_FILE="$(find "$INBOX" -maxdepth 1 -name '*.md' -type f | sort | head -n 1)"

if [[ -z "$INBOX_FILE" ]]; then
  echo "No inbox items."
  exit 0
fi

ORIGINAL_NAME="$(basename "$INBOX_FILE")"
STEM="$(basename "$INBOX_FILE" .md)"
TIMESTAMP="$(date '+%Y-%m-%d_%H%M')"
REPORT="$OUTBOX/${TIMESTAMP}-${STEM}-digest.md"
TODAY="$(date '+%Y-%m-%d')"

# Process the inbox item; on failure move to Failed
if ! {
  # Copy first 200 lines into report
  head -n 200 "$INBOX_FILE" > "$REPORT"

  # Append planned sections
  cat >> "$REPORT" <<'EOF'

## Planned Actions
- (placeholder)

## Next Step
- (placeholder)
EOF
}; then
  mv "$INBOX_FILE" "$FAILED/$ORIGINAL_NAME"
  echo "[$TIMESTAMP] FAILED: $ORIGINAL_NAME" >> "$LOGS/${TODAY}.md"
  echo "Processing failed: $ORIGINAL_NAME (moved to Failed/)"
  exit 0
fi

# Success: move to Processed, log it
mv "$INBOX_FILE" "$PROCESSED/$ORIGINAL_NAME"
echo "[$TIMESTAMP] Digest created: $(basename "$REPORT") from $ORIGINAL_NAME -> Processed/" >> "$LOGS/${TODAY}.md"

echo "Digest written: $REPORT"
echo "Inbox item moved to: $PROCESSED/$ORIGINAL_NAME"
exit 0
