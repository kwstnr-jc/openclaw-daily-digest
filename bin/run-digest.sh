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

# --- Dependency check: jq ---
HAS_JQ=true
if ! command -v jq &>/dev/null; then
  echo "jq not found. Attempting install via Homebrew..."
  if command -v brew &>/dev/null; then
    brew install jq 2>/dev/null || HAS_JQ=false
  else
    HAS_JQ=false
  fi
  if [[ "$HAS_JQ" == "false" ]]; then
    echo "Warning: jq unavailable. JSON enrichment will fall back to plain text."
  fi
fi

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

# --- LLM Enrichment via OpenClaw CLI (strict JSON) ---
ENRICHED=false
RAW_JSON=""
RENDERED_ENRICHMENT=""

FALLBACK_ENRICHMENT="## Planned Actions
- (LLM enrichment unavailable — manual review required)

## Clarifying Questions
- None

## Suggested Next Step
- Review inbox item manually and determine actions"

if command -v openclaw &>/dev/null && [[ "$HAS_JQ" == "true" ]]; then
  echo "Calling OpenClaw for JSON enrichment..."
  RAW_JSON="$(openclaw agent \
    --agent main \
    --timeout "$ENRICHMENT_TIMEOUT" \
    --message "You are a strict JSON API. Given the task below, return ONLY a single JSON object. No markdown fences, no prose, no explanation — just the raw JSON object.

Schema:
{
  \"planned_actions\": [\"string\", ...],
  \"clarifying_questions\": [\"string\", ...],
  \"next_step\": \"string\"
}

Rules:
- planned_actions: array of concrete action strings (at least one).
- clarifying_questions: array of question strings. Use [] if the task is clear.
- next_step: single string describing the immediate next action.
- Output MUST be valid JSON. Nothing else.

Task:
$TASK_CONTENT" 2>/dev/null)" || true

  # Strip markdown fences if the LLM wrapped them anyway
  CLEAN_JSON="$(echo "$RAW_JSON" | sed -n '/^[[:space:]]*{/,/}[[:space:]]*$/p')"
  if [[ -z "$CLEAN_JSON" ]]; then
    CLEAN_JSON="$RAW_JSON"
  fi

  # Validate with jq
  if echo "$CLEAN_JSON" | jq empty 2>/dev/null; then
    # Render Planned Actions
    RENDERED_ENRICHMENT="## Planned Actions"$'\n'
    while IFS= read -r action; do
      RENDERED_ENRICHMENT+="- ${action}"$'\n'
    done < <(echo "$CLEAN_JSON" | jq -r '.planned_actions[]' 2>/dev/null)

    # Render Clarifying Questions
    RENDERED_ENRICHMENT+=$'\n'"## Clarifying Questions"$'\n'
    CQ_COUNT="$(echo "$CLEAN_JSON" | jq -r '.clarifying_questions | length' 2>/dev/null)"
    if [[ "$CQ_COUNT" -gt 0 ]]; then
      while IFS= read -r q; do
        RENDERED_ENRICHMENT+="- ${q}"$'\n'
      done < <(echo "$CLEAN_JSON" | jq -r '.clarifying_questions[]' 2>/dev/null)
    else
      RENDERED_ENRICHMENT+="- None"$'\n'
    fi

    # Render Suggested Next Step
    NEXT_STEP="$(echo "$CLEAN_JSON" | jq -r '.next_step' 2>/dev/null)"
    RENDERED_ENRICHMENT+=$'\n'"## Suggested Next Step"$'\n'"- ${NEXT_STEP}"$'\n'

    # Store pretty-printed JSON for audit
    RAW_JSON="$(echo "$CLEAN_JSON" | jq '.' 2>/dev/null)"
    ENRICHED=true
    echo "Enrichment received and parsed as JSON."
  else
    echo "JSON parse failed. Using fallback."
  fi
fi

if [[ "$ENRICHED" == "false" ]]; then
  RENDERED_ENRICHMENT="$FALLBACK_ENRICHMENT"
  echo "Enrichment unavailable or invalid, using fallback."
fi

# --- Build report ---
if ! {
  # Write original content
  echo "$TASK_CONTENT" > "$REPORT"

  # Append rendered enrichment
  printf '\n---\n\n' >> "$REPORT"
  echo "$RENDERED_ENRICHMENT" >> "$REPORT"

  # Append raw JSON audit block (only if we got valid JSON)
  if [[ "$ENRICHED" == "true" && -n "$RAW_JSON" ]]; then
    printf '\n## Enrichment (raw JSON)\n\n```json\n%s\n```\n' "$RAW_JSON" >> "$REPORT"
  fi
}; then
  mv "$INBOX_FILE" "$FAILED/$ORIGINAL_NAME"
  echo "[$TIMESTAMP] $ORIGINAL_NAME -> $(basename "$REPORT") -> Failed/ [error]" >> "$LOGS/${TODAY}.md"
  echo "Processing failed: $ORIGINAL_NAME (moved to Failed/)"
  exit 0
fi

# Success: move to Processed, log it
ENRICH_TAG="[enriched]"
if [[ "$ENRICHED" == "false" ]]; then
  ENRICH_TAG="[unenriched]"
fi

mv "$INBOX_FILE" "$PROCESSED/$ORIGINAL_NAME"
echo "[$TIMESTAMP] $ORIGINAL_NAME -> $(basename "$REPORT") -> Processed/ $ENRICH_TAG" >> "$LOGS/${TODAY}.md"

echo "Digest written: $REPORT"
echo "Inbox item moved to: $PROCESSED/$ORIGINAL_NAME"
exit 0
