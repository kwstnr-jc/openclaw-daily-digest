#!/usr/bin/env bash
set -euo pipefail

ROOT="${DIGEST_ROOT:-/Users/Shared/agent-vault/Agent}"
INBOX="$ROOT/Inbox"
OUTBOX="$ROOT/Outbox"
LOGS="$ROOT/Logs"
PROCESSED="$INBOX/Processed"
FAILED="$INBOX/Failed"
ENRICHMENT_TIMEOUT=120
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
POLICY_FILE="${DIGEST_POLICY:-$REPO_DIR/config/policy.json}"

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

# --- Model selection policy ---
HAS_POLICY=false
if [[ "$HAS_JQ" == "true" && -f "$POLICY_FILE" ]] && jq empty "$POLICY_FILE" 2>/dev/null; then
  HAS_POLICY=true
  echo "Policy loaded: $POLICY_FILE"
fi

# _select_model <step> [task_content]
# Returns the model name for the given step, applying overrides.
# Steps: classification, action_type, enrichment, research, question, deep_analysis
_select_model() {
  local step="$1"
  local content="${2:-}"

  if [[ "$HAS_POLICY" != "true" ]]; then
    echo ""
    return
  fi

  local tier
  tier="$(jq -r ".routing.\"$step\" // \"mid\"" "$POLICY_FILE")"

  # Override: #deep tag → expensive model
  local deep_tag
  deep_tag="$(jq -r '.overrides.deep_tag // "#deep"' "$POLICY_FILE")"
  if [[ -n "$content" ]] && echo "$content" | grep -qF "$deep_tag" 2>/dev/null; then
    tier="$(jq -r '.overrides.deep_tag_model // "expensive"' "$POLICY_FILE")"
  fi

  jq -r ".models.\"$tier\".name // empty" "$POLICY_FILE"
}

_model_max_tokens() {
  local step="$1"
  if [[ "$HAS_POLICY" != "true" ]]; then echo ""; return; fi
  local tier
  tier="$(jq -r ".routing.\"$step\" // \"mid\"" "$POLICY_FILE")"
  jq -r ".models.\"$tier\".max_tokens // empty" "$POLICY_FILE"
}

MAX_ITEMS="${DIGEST_MAX_ITEMS:-10}"
ITEMS_PROCESSED=0
ITEMS_ENRICHED=0
ITEMS_UNENRICHED=0
ITEMS_FAILED=0

while true; do
  if [[ "$MAX_ITEMS" -gt 0 && "$ITEMS_PROCESSED" -ge "$MAX_ITEMS" ]]; then
    break
  fi

  # Find first *.md file in Inbox (no subfolders)
  INBOX_FILE="$(find "$INBOX" -maxdepth 1 -name '*.md' -type f | sort | head -n 1)"

  if [[ -z "$INBOX_FILE" ]]; then
    break
  fi

  ITEMS_PROCESSED=$((ITEMS_PROCESSED + 1))
  echo ""
  echo "--- Item $ITEMS_PROCESSED ---"

ORIGINAL_NAME="$(basename "$INBOX_FILE")"
STEM="$(basename "$INBOX_FILE" .md)"
TIMESTAMP="$(date '+%Y-%m-%d_%H%M')"
REPORT="$OUTBOX/${TIMESTAMP}-${STEM}-digest.md"
ENVELOPE="$OUTBOX/${TIMESTAMP}-${STEM}.envelope.json"
TODAY="$(date '+%Y-%m-%d')"

# Read inbox content (first 200 lines)
TASK_CONTENT="$(head -n 200 "$INBOX_FILE")"

# --- Project Classification (Level 1) ---
PROJECTS_DIR="$ROOT/Projects"
PROJECT_KIND="none"
PROJECT_NAME=""
CLASSIFICATION_METHOD=""
CLASSIFICATION_JSON="null"

# Rule 1: Explicit "Project: <name>" line
EXPLICIT_PROJECT="$(echo "$TASK_CONTENT" | grep -iE '^Project:[[:space:]]+' | head -n 1 | sed 's/^[Pp]roject:[[:space:]]*//' | xargs || true)"
if [[ -n "$EXPLICIT_PROJECT" ]]; then
  PROJECT_NAME="$EXPLICIT_PROJECT"
  CLASSIFICATION_METHOD="explicit-line"
  if [[ -d "$PROJECTS_DIR/$PROJECT_NAME" ]]; then
    PROJECT_KIND="existing"
  else
    PROJECT_KIND="new"
  fi
fi

# Rule 2: #project/<name> tag
if [[ -z "$PROJECT_NAME" ]]; then
  TAG_PROJECT="$(echo "$TASK_CONTENT" | grep -oE '#project/[A-Za-z0-9_-]+' | head -n 1 | sed 's|^#project/||' || true)"
  if [[ -n "$TAG_PROJECT" ]]; then
    PROJECT_NAME="$TAG_PROJECT"
    CLASSIFICATION_METHOD="tag"
    if [[ -d "$PROJECTS_DIR/$PROJECT_NAME" ]]; then
      PROJECT_KIND="existing"
    else
      PROJECT_KIND="new"
    fi
  fi
fi

# Rule 3: Case-insensitive substring match against existing project folder names
if [[ -z "$PROJECT_NAME" && -d "$PROJECTS_DIR" ]]; then
  TASK_LOWER="$(echo "$TASK_CONTENT" | tr '[:upper:]' '[:lower:]')"
  for proj_dir in "$PROJECTS_DIR"/*/; do
    [[ -d "$proj_dir" ]] || continue
    proj_name="$(basename "$proj_dir")"
    proj_lower="$(echo "$proj_name" | tr '[:upper:]' '[:lower:]')"
    if echo "$TASK_LOWER" | grep -qF "$proj_lower" 2>/dev/null; then
      PROJECT_NAME="$proj_name"
      PROJECT_KIND="existing"
      CLASSIFICATION_METHOD="folder-match"
      break
    fi
  done
fi

# Rule 4: AI-assisted classification via OpenClaw
if [[ -z "$PROJECT_NAME" ]] && command -v openclaw &>/dev/null && [[ "$HAS_JQ" == "true" ]]; then
  echo "Calling OpenClaw for project classification..."
  CLASSIFY_MODEL="$(_select_model classification "$TASK_CONTENT")"
  CLASSIFY_MODEL_ARGS=()
  [[ -n "$CLASSIFY_MODEL" ]] && CLASSIFY_MODEL_ARGS=(--model "$CLASSIFY_MODEL")
  CLASSIFY_RAW="$(openclaw agent \
    --agent main \
    --timeout "$ENRICHMENT_TIMEOUT" \
    "${CLASSIFY_MODEL_ARGS[@]}" \
    --message "You are a strict JSON API. Classify the following task into a project.

Return ONLY a JSON object:
{
  \"project\": { \"kind\": \"existing\"|\"new\"|\"none\", \"name\": \"string or null\" },
  \"confidence\": 0.0,
  \"rationale\": \"string\"
}

Existing projects: $(ls "$PROJECTS_DIR" 2>/dev/null | tr '\n' ', ')

Rules:
- kind=existing if the task clearly belongs to one of the existing projects.
- kind=new if the task requires a new project that doesn't exist yet. Provide a kebab-case name.
- kind=none if it's personal admin, a question, or doesn't warrant a project.
- Output MUST be valid JSON. Nothing else.

Task:
$TASK_CONTENT" 2>/dev/null)" || true

  # Strip markdown fences
  CLASSIFY_CLEAN="$(echo "$CLASSIFY_RAW" | sed -n '/^[[:space:]]*{/,/}[[:space:]]*$/p')"
  [[ -z "$CLASSIFY_CLEAN" ]] && CLASSIFY_CLEAN="$CLASSIFY_RAW"

  if echo "$CLASSIFY_CLEAN" | jq empty 2>/dev/null; then
    AI_KIND="$(echo "$CLASSIFY_CLEAN" | jq -r '.project.kind // "none"')"
    AI_NAME="$(echo "$CLASSIFY_CLEAN" | jq -r '.project.name // empty')"
    if [[ "$AI_KIND" == "existing" || "$AI_KIND" == "new" ]] && [[ -n "$AI_NAME" ]]; then
      PROJECT_KIND="$AI_KIND"
      PROJECT_NAME="$AI_NAME"
      CLASSIFICATION_METHOD="ai"
      CLASSIFICATION_JSON="$(echo "$CLASSIFY_CLEAN" | jq '.')"
    elif [[ "$AI_KIND" == "none" ]]; then
      PROJECT_KIND="none"
      CLASSIFICATION_METHOD="ai"
      CLASSIFICATION_JSON="$(echo "$CLASSIFY_CLEAN" | jq '.')"
    fi
    echo "AI classification: kind=$PROJECT_KIND name=$PROJECT_NAME"
  else
    echo "AI classification JSON invalid, skipping."
  fi
fi

# Default: unclassified
if [[ -z "$CLASSIFICATION_METHOD" ]]; then
  CLASSIFICATION_METHOD="default"
fi

# Build classification JSON for envelope
if [[ "$CLASSIFICATION_JSON" == "null" ]]; then
  if [[ -n "$PROJECT_NAME" ]]; then
    CLASSIFICATION_JSON="{\"project\":{\"kind\":\"$PROJECT_KIND\",\"name\":\"$PROJECT_NAME\"},\"confidence\":1.0,\"rationale\":\"Matched via $CLASSIFICATION_METHOD\"}"
  else
    CLASSIFICATION_JSON="{\"project\":{\"kind\":\"none\",\"name\":null},\"confidence\":1.0,\"rationale\":\"No project match ($CLASSIFICATION_METHOD)\"}"
  fi
fi

echo "Project routing: kind=$PROJECT_KIND name=${PROJECT_NAME:-<none>} method=$CLASSIFICATION_METHOD"

# Create new project directory if classified as "new"
if [[ "$PROJECT_KIND" == "new" && -n "$PROJECT_NAME" ]]; then
  NEW_PROJECT_DIR="$PROJECTS_DIR/$PROJECT_NAME"
  if [[ ! -d "$NEW_PROJECT_DIR" ]]; then
    mkdir -p "$NEW_PROJECT_DIR/Inbox"
    cat > "$NEW_PROJECT_DIR/README.md" <<PROJEOF
# $PROJECT_NAME

Created: $TODAY
Source: $ORIGINAL_NAME

## Description

(Auto-created by inbox orchestrator. Update this with project details.)
PROJEOF
    echo "Created new project: $NEW_PROJECT_DIR"
  fi
fi

# --- Action Type Classification (Level 2) ---
ACTION_TYPE=""
ACTION_TYPE_METHOD=""
ACTION_TYPE_JSON="null"
TASK_LOWER="$(echo "$TASK_CONTENT" | tr '[:upper:]' '[:lower:]')"

# Deterministic keyword overrides
if echo "$TASK_LOWER" | grep -qE '\b(fix|implement|add flag|refactor|pr)\b' 2>/dev/null; then
  ACTION_TYPE="repo-change"
  ACTION_TYPE_METHOD="keyword"
elif echo "$TASK_LOWER" | grep -qE '\b(compare|research|find out|summarize)\b' 2>/dev/null; then
  ACTION_TYPE="research"
  ACTION_TYPE_METHOD="keyword"
elif echo "$TASK_LOWER" | grep -qE '\b(install|brew|launchctl|tailscale)\b' 2>/dev/null; then
  ACTION_TYPE="ops"
  ACTION_TYPE_METHOD="keyword"
elif echo "$TASK_CONTENT" | grep -qE '\?\s*$' 2>/dev/null; then
  ACTION_TYPE="question"
  ACTION_TYPE_METHOD="keyword"
fi

# AI fallback if no deterministic match
if [[ -z "$ACTION_TYPE" ]] && command -v openclaw &>/dev/null && [[ "$HAS_JQ" == "true" ]]; then
  echo "Calling OpenClaw for action type classification..."
  ACTION_MODEL="$(_select_model action_type "$TASK_CONTENT")"
  ACTION_MODEL_ARGS=()
  [[ -n "$ACTION_MODEL" ]] && ACTION_MODEL_ARGS=(--model "$ACTION_MODEL")
  ACTION_RAW="$(openclaw agent \
    --agent main \
    --timeout "$ENRICHMENT_TIMEOUT" \
    "${ACTION_MODEL_ARGS[@]}" \
    --message "You are a strict JSON API. Classify the action type for the following task.

Return ONLY a JSON object:
{
  \"action_type\": \"repo-change\"|\"research\"|\"ops\"|\"question\"|\"note\",
  \"confidence\": 0.0,
  \"rationale\": \"...\",
  \"suggested_repo\": \"string or null\"
}

Rules:
- repo-change: task requires code changes, PRs, or modifications to a repository
- research: task requires investigation, comparison, or summarization
- ops: task requires infrastructure, tooling, or system administration
- question: task is asking a question that needs an answer
- note: everything else (personal admin, reminders, etc.)
- Output MUST be valid JSON. Nothing else.

Task:
$TASK_CONTENT" 2>/dev/null)" || true

  ACTION_CLEAN="$(echo "$ACTION_RAW" | sed -n '/^[[:space:]]*{/,/}[[:space:]]*$/p')"
  [[ -z "$ACTION_CLEAN" ]] && ACTION_CLEAN="$ACTION_RAW"

  if echo "$ACTION_CLEAN" | jq empty 2>/dev/null; then
    AI_ACTION="$(echo "$ACTION_CLEAN" | jq -r '.action_type // "note"')"
    case "$AI_ACTION" in
      repo-change|research|ops|question|note)
        ACTION_TYPE="$AI_ACTION"
        ACTION_TYPE_METHOD="ai"
        ACTION_TYPE_JSON="$(echo "$ACTION_CLEAN" | jq '.')"
        ;;
      *)
        ACTION_TYPE="note"
        ACTION_TYPE_METHOD="ai-fallback"
        ;;
    esac
    echo "AI action type: $ACTION_TYPE"
  else
    echo "AI action type JSON invalid, defaulting to note."
    ACTION_TYPE="note"
    ACTION_TYPE_METHOD="default"
  fi
fi

# Final default
if [[ -z "$ACTION_TYPE" ]]; then
  ACTION_TYPE="note"
  ACTION_TYPE_METHOD="default"
fi

# Build action type JSON for envelope
if [[ "$ACTION_TYPE_JSON" == "null" ]]; then
  ACTION_TYPE_JSON="{\"action_type\":\"$ACTION_TYPE\",\"confidence\":1.0,\"rationale\":\"Matched via $ACTION_TYPE_METHOD\",\"suggested_repo\":null}"
fi

echo "Action type: $ACTION_TYPE method=$ACTION_TYPE_METHOD"

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
  ENRICH_MODEL="$(_select_model enrichment "$TASK_CONTENT")"
  ENRICH_MODEL_ARGS=()
  [[ -n "$ENRICH_MODEL" ]] && ENRICH_MODEL_ARGS=(--model "$ENRICH_MODEL")
  RAW_JSON="$(openclaw agent \
    --agent main \
    --timeout "$ENRICHMENT_TIMEOUT" \
    "${ENRICH_MODEL_ARGS[@]}" \
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

  # Validate with jq (must be non-empty)
  if [[ -n "$CLEAN_JSON" ]] && echo "$CLEAN_JSON" | jq empty 2>/dev/null; then
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

# --- Execution Handlers ---
EXECUTION_RESULT=""
EXECUTION_JSON="{}"
EXECUTION_FILE=""

case "$ACTION_TYPE" in
  research)
    EXECUTION_FILE="$OUTBOX/${TIMESTAMP}-${STEM}.research.md"
    echo "Executing research handler..."
    if command -v openclaw &>/dev/null; then
      RESEARCH_MODEL="$(_select_model research "$TASK_CONTENT")"
      RESEARCH_MODEL_ARGS=()
      [[ -n "$RESEARCH_MODEL" ]] && RESEARCH_MODEL_ARGS=(--model "$RESEARCH_MODEL")
      RESEARCH_RAW="$(openclaw agent \
        --agent main \
        --timeout "$ENRICHMENT_TIMEOUT" \
        "${RESEARCH_MODEL_ARGS[@]}" \
        --message "You are a research assistant. Given the task below, produce a structured research report.

Format your response as markdown with these exact sections:
## Summary
(2-3 sentence overview)

## Findings
(bulleted list of key findings)

## Sources
(bulleted list — use placeholder URLs for now)

## Next Steps
(bulleted list of recommended follow-up actions)

Task:
$TASK_CONTENT" 2>/dev/null)" || true
      if [[ -n "$RESEARCH_RAW" ]]; then
        echo "$RESEARCH_RAW" > "$EXECUTION_FILE"
        EXECUTION_RESULT="completed"
        EXECUTION_JSON="{\"handler\":\"research\",\"status\":\"completed\",\"output_file\":\"$(basename "$EXECUTION_FILE")\"}"
        echo "Research report written: $EXECUTION_FILE"
      else
        EXECUTION_RESULT="failed"
        EXECUTION_JSON="{\"handler\":\"research\",\"status\":\"failed\",\"reason\":\"OpenClaw returned empty response\"}"
      fi
    else
      EXECUTION_RESULT="skipped"
      EXECUTION_JSON="{\"handler\":\"research\",\"status\":\"skipped\",\"reason\":\"OpenClaw not available\"}"
    fi
    ;;

  question)
    EXECUTION_FILE="$OUTBOX/${TIMESTAMP}-${STEM}.research.md"
    echo "Executing question handler..."
    if command -v openclaw &>/dev/null; then
      QUESTION_MODEL="$(_select_model question "$TASK_CONTENT")"
      QUESTION_MODEL_ARGS=()
      [[ -n "$QUESTION_MODEL" ]] && QUESTION_MODEL_ARGS=(--model "$QUESTION_MODEL")
      ANSWER_RAW="$(openclaw agent \
        --agent main \
        --timeout "$ENRICHMENT_TIMEOUT" \
        "${QUESTION_MODEL_ARGS[@]}" \
        --message "You are an expert assistant. Given the question below, produce a structured answer.

Format your response as markdown with these exact sections:
## Answer
(clear, direct answer to the question)

## Details
(supporting explanation with bullet points)

## Sources
(bulleted list — use placeholder URLs for now)

## Follow-up Questions
(bulleted list of related questions worth exploring)

Question:
$TASK_CONTENT" 2>/dev/null)" || true
      if [[ -n "$ANSWER_RAW" ]]; then
        echo "$ANSWER_RAW" > "$EXECUTION_FILE"
        EXECUTION_RESULT="completed"
        EXECUTION_JSON="{\"handler\":\"question\",\"status\":\"completed\",\"output_file\":\"$(basename "$EXECUTION_FILE")\"}"
        echo "Answer report written: $EXECUTION_FILE"
      else
        EXECUTION_RESULT="failed"
        EXECUTION_JSON="{\"handler\":\"question\",\"status\":\"failed\",\"reason\":\"OpenClaw returned empty response\"}"
      fi
    else
      EXECUTION_RESULT="skipped"
      EXECUTION_JSON="{\"handler\":\"question\",\"status\":\"skipped\",\"reason\":\"OpenClaw not available\"}"
    fi
    ;;

  repo-change)
    EXECUTION_RESULT="blocked"
    EXECUTION_JSON="{\"handler\":\"repo-change\",\"status\":\"blocked\",\"reason\":\"Execution blocked: requires approval\"}"
    echo "Execution blocked: repo-change requires approval"
    ;;

  ops)
    EXECUTION_RESULT="blocked"
    EXECUTION_JSON="{\"handler\":\"ops\",\"status\":\"blocked\",\"reason\":\"Execution blocked: requires approval\"}"
    echo "Execution blocked: ops requires approval"
    ;;

  note|*)
    EXECUTION_RESULT="none"
    EXECUTION_JSON="{\"handler\":\"note\",\"status\":\"none\",\"reason\":\"No execution required for notes\"}"
    ;;
esac

echo "Execution: $EXECUTION_RESULT"

# --- Envelope writer function ---
_write_envelope() {
  local status="$1"
  local enrichment_json="null"
  if [[ "$ENRICHED" == "true" && -n "$RAW_JSON" ]]; then
    enrichment_json="$RAW_JSON"
  fi
  # Escape task content for JSON embedding
  local escaped_task
  escaped_task="$(printf '%s' "$TASK_CONTENT" | jq -Rs '.' 2>/dev/null || printf '"%s"' "$(printf '%s' "$TASK_CONTENT" | sed 's/\\/\\\\/g; s/"/\\"/g; s/\t/\\t/g' | tr '\n' '\\' | sed 's/\\/\\n/g')")"

  cat > "$ENVELOPE" <<ENVEOF
{
  "version": "1.0.0",
  "timestamp": "${TIMESTAMP}",
  "source_file": "${ORIGINAL_NAME}",
  "task_text": ${escaped_task},
  "classification": ${CLASSIFICATION_JSON},
  "action_type": ${ACTION_TYPE_JSON},
  "planning": null,
  "enrichment": ${enrichment_json},
  "execution": ${EXECUTION_JSON},
  "status": "${status}"
}
ENVEOF
}

# --- Build report ---
if ! {
  # Write original content
  echo "$TASK_CONTENT" > "$REPORT"

  # Append project routing section
  printf '\n---\n\n' >> "$REPORT"
  echo "## Project Routing" >> "$REPORT"
  echo "" >> "$REPORT"
  echo "- **Kind:** $PROJECT_KIND" >> "$REPORT"
  if [[ -n "$PROJECT_NAME" ]]; then
    echo "- **Project:** $PROJECT_NAME" >> "$REPORT"
  fi
  echo "- **Method:** $CLASSIFICATION_METHOD" >> "$REPORT"
  echo "" >> "$REPORT"

  # Append action type section
  echo "## Action Type" >> "$REPORT"
  echo "" >> "$REPORT"
  echo "- **Type:** $ACTION_TYPE" >> "$REPORT"
  echo "- **Method:** $ACTION_TYPE_METHOD" >> "$REPORT"
  echo "" >> "$REPORT"

  # Append execution status section
  echo "## Execution" >> "$REPORT"
  echo "" >> "$REPORT"
  echo "- **Handler:** $ACTION_TYPE" >> "$REPORT"
  echo "- **Status:** $EXECUTION_RESULT" >> "$REPORT"
  if [[ -n "$EXECUTION_FILE" && -f "$EXECUTION_FILE" ]]; then
    echo "- **Output:** $(basename "$EXECUTION_FILE")" >> "$REPORT"
  fi
  echo "" >> "$REPORT"

  # Append rendered enrichment
  echo "$RENDERED_ENRICHMENT" >> "$REPORT"

  # Append raw JSON audit block (only if we got valid JSON)
  if [[ "$ENRICHED" == "true" && -n "$RAW_JSON" ]]; then
    printf '\n## Enrichment (raw JSON)\n\n```json\n%s\n```\n' "$RAW_JSON" >> "$REPORT"
  fi
}; then
  mv "$INBOX_FILE" "$FAILED/$ORIGINAL_NAME"
  echo "[$TIMESTAMP] $ORIGINAL_NAME -> $(basename "$REPORT") -> Failed/ [error]" >> "$LOGS/${TODAY}.md"
  # Write failure envelope
  _write_envelope "failed"
  echo "Processing failed: $ORIGINAL_NAME (moved to Failed/)"
  ITEMS_FAILED=$((ITEMS_FAILED + 1))
  continue
fi

# Success: move to Processed, log it
ENVELOPE_STATUS="unenriched"
if [[ "$ENRICHED" == "true" ]]; then
  ENVELOPE_STATUS="enriched"
  ITEMS_ENRICHED=$((ITEMS_ENRICHED + 1))
else
  ITEMS_UNENRICHED=$((ITEMS_UNENRICHED + 1))
fi

# --- Write envelope.json ---
_write_envelope "$ENVELOPE_STATUS"

mv "$INBOX_FILE" "$PROCESSED/$ORIGINAL_NAME"
echo "[$TIMESTAMP] $ORIGINAL_NAME -> $(basename "$REPORT") -> Processed/ [$ENVELOPE_STATUS]" >> "$LOGS/${TODAY}.md"

echo "Digest written: $REPORT"
echo "Envelope written: $ENVELOPE"
echo "Inbox item moved to: $PROCESSED/$ORIGINAL_NAME"

done  # end of while loop

if [[ "$ITEMS_PROCESSED" -eq 0 ]]; then
  echo "No inbox items."
  exit 0
fi

echo ""
echo "--- Summary ---"
echo "Processed $ITEMS_PROCESSED items ($ITEMS_ENRICHED enriched, $ITEMS_UNENRICHED unenriched, $ITEMS_FAILED failed)."

# Check if more items remain
REMAINING="$(find "$INBOX" -maxdepth 1 -name '*.md' -type f | wc -l | tr -d ' ')"
if [[ "$REMAINING" -gt 0 ]]; then
  echo "$REMAINING items remaining. Run again to continue."
fi

# --- Discord posting ---
DISCORD_TOKEN_FILE="${DISCORD_TOKEN_FILE:-$HOME/.digest-bot-token}"
DISCORD_CHANNEL_ID="${DISCORD_CHANNEL_ID:-1477340656350396668}"
NO_DISCORD="${NO_DISCORD:-false}"

if [[ "$NO_DISCORD" != "true" && "$ITEMS_PROCESSED" -gt 0 ]]; then
  if [[ -f "$DISCORD_TOKEN_FILE" ]]; then
    BOT_TOKEN="$(cat "$DISCORD_TOKEN_FILE" | tr -d '[:space:]')"
    if [[ -n "$BOT_TOKEN" ]]; then
      MSG="**Daily Digest — $(date '+%Y-%m-%d %H:%M')**\n\nProcessed $ITEMS_PROCESSED items.\n\nEnriched: $ITEMS_ENRICHED | Unenriched: $ITEMS_UNENRICHED | Failed: $ITEMS_FAILED"
      PAYLOAD="$(jq -n --arg content "$MSG" '{"content":$content}')"
      HTTP_CODE="$(curl -s -o /dev/null -w '%{http_code}' \
        -X POST "https://discord.com/api/v10/channels/${DISCORD_CHANNEL_ID}/messages" \
        -H "Authorization: Bot ${BOT_TOKEN}" \
        -H "Content-Type: application/json" \
        -d "$PAYLOAD" 2>/dev/null || true)"
      if [[ "$HTTP_CODE" == "200" || "$HTTP_CODE" == "204" ]]; then
        echo "Discord summary posted."
      else
        echo "Discord post failed (HTTP $HTTP_CODE) — non-fatal." >&2
      fi
    else
      echo "Discord token file empty — skipping post." >&2
    fi
  else
    echo "Discord token file not found — skipping post." >&2
  fi
fi

exit 0
