#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
VAULT="/Users/Shared/agent-vault/Agent"
RESULTS=()

shopt -s nullglob
files=("$VAULT/Inbox"/*.md)
shopt -u nullglob

if [[ ${#files[@]} -eq 0 ]]; then
  echo "NO_ITEMS"
  exit 0
fi

for f in "${files[@]}"; do
  echo "--- Processing: $(basename "$f") ---"
  result=$("$SCRIPT_DIR/bin/work-executor.sh" "$f" 2>&1) || true
  echo "$result"
  RESULTS+=("$result")
  echo "--- Done: $(basename "$f") ---"
done

echo "TOTAL_ITEMS: ${#files[@]}"
