#!/usr/bin/env bash
set -euo pipefail
VAULT="/Users/Shared/agent-vault/Agent"
TODAY=$(date +%F)

slugify() { echo "$1" | sed 's/\.md$//;s/[^a-zA-Z0-9]/-/g' | tr '[:upper:]' '[:lower:]' | sed 's/--*/-/g;s/^-//;s/-$//'; }

write_outbox() {
  local slug="$1" classification="$2" planned="$3" executed="$4" git_info="${5:-none}" followups="${6:-none}" orig="${7:-unknown}"
  mkdir -p "$VAULT/Outbox"
  local f="$VAULT/Outbox/${TODAY}-${slug}-result.md"
  cat > "$f" <<INNER
# Outbox Result: $slug
- Date: $TODAY
- Original: $orig
- Classification: $classification
## Planned Actions
$planned
## Executed Actions
$executed
## Git Info
$git_info
## Follow-ups
$followups
INNER
  echo "$f"
}

write_memory() {
  local slug="$1" classification="$2" summary="$3"
  mkdir -p "$VAULT/Memory"
  local f="$VAULT/Memory/${TODAY}-${slug}.md"
  cat > "$f" <<INNER
# Memory: $slug
- Date: $TODAY
- Classification: $classification
- Tags: #decision #experiment
## Summary
$summary
INNER
  if [[ ! -f "$VAULT/Memory/Index.md" ]]; then echo "# Memory Index" > "$VAULT/Memory/Index.md"; fi
  if ! grep -q "## $TODAY" "$VAULT/Memory/Index.md" 2>/dev/null; then
    printf '\n## %s\n' "$TODAY" >> "$VAULT/Memory/Index.md"
  fi
  echo "- [$slug](./${TODAY}-${slug}.md)" >> "$VAULT/Memory/Index.md"
  echo "$f"
}

append_log() {
  local slug="$1" classification="$2" result="$3"
  mkdir -p "$VAULT/Logs"
  local f="$VAULT/Logs/${TODAY}.md"
  if [[ ! -f "$f" ]]; then echo "# Log $TODAY" > "$f"; fi
  echo "- [$(date +%H:%M)] $slug | $classification | $result" >> "$f"
  echo "$f"
}

move_inbox() {
  local filepath="$1" status="$2"
  if [[ "$status" == "success" ]]; then
    mkdir -p "$VAULT/Inbox/Processed"
    mv "$filepath" "$VAULT/Inbox/Processed/"
  else
    mkdir -p "$VAULT/Inbox/Failed"
    mv "$filepath" "$VAULT/Inbox/Failed/"
  fi
}
