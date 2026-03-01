#!/usr/bin/env bash
set -euo pipefail

ROOT="/Users/Shared/agent-vault/Agent"
INBOX="$ROOT/Inbox"
OUTBOX="$ROOT/Outbox"
LOGS="$ROOT/Logs"
PROCESSED="$INBOX/Processed"
FAILED="$INBOX/Failed"
ENRICHMENT_TIMEOUT=120

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

# Read inbox content (first 200 lines)
TASK_CONTENT="$(head -n 200 "$INBOX_FILE")"

# --- LLM Enrichment via OpenClaw CLI ---
FALLBACK_ENRICHMENT="## Planned Actions
- (LLM enrichment unavailable — manual review required)

## Clarifying Questions
- (none — enrichment skipped)

## Suggested Next Step
- Review inbox item manually and determine actions"

ENRICHMENT=""
if command -v openclaw &>/dev/null; then
  echo "Calling OpenClaw for enrichment..."
  ENRICHMENT="$(openclaw agent \
    --agent main \
    --timeout "$ENRICHMENT_TIMEOUT" \
    --message "Given the following task, produce exactly three sections with these headings:

## Planned Actions
(bullet list of concrete actions)

## Clarifying Questions
(bullet list, or \"- None\" if the task is clear)

## Suggested Next Step
(single bullet: the immediate next action)

Task:
$TASK_CONTENT" 2>/dev/null)" || true
fi

# Use fallback if enrichment is empty or failed
if [[ -z "${ENRICHMENT// /}" ]]; then
  echo "Enrichment unavailable, using fallback."
  ENRICHMENT="$FALLBACK_ENRICHMENT"
else
  echo "Enrichment received."
fi

# --- Build report ---
if ! {
  # Write original content
  echo "$TASK_CONTENT" > "$REPORT"

  # Append enrichment
  printf '\n---\n\n' >> "$REPORT"
  echo "$ENRICHMENT" >> "$REPORT"
}; then
  mv "$INBOX_FILE" "$FAILED/$ORIGINAL_NAME"
  echo "[$TIMESTAMP] FAILED: $ORIGINAL_NAME" >> "$LOGS/${TODAY}.md"
  echo "Processing failed: $ORIGINAL_NAME (moved to Failed/)"
  exit 0
fi

# Success: move to Processed, log it
mv "$INBOX_FILE" "$PROCESSED/$ORIGINAL_NAME"
echo "[$TIMESTAMP] Digest created: $(basename "$REPORT") from $ORIGINAL_NAME -> Processed/ [enriched]" >> "$LOGS/${TODAY}.md"

echo "Digest written: $REPORT"
echo "Inbox item moved to: $PROCESSED/$ORIGINAL_NAME"
exit 0
