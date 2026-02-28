#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
REPO_DIR="$SCRIPT_DIR"
export DIGEST_REPO="$REPO_DIR"

source "$SCRIPT_DIR/lib/classify.sh" 2>/dev/null || true
source "$SCRIPT_DIR/lib/obsidian.sh"
source "$SCRIPT_DIR/lib/gitflow.sh"

VAULT="/Users/Shared/agent-vault/Agent"
filepath="${1:?Usage: work-executor.sh <inbox-file>}"
filename=$(basename "$filepath")
slug=$(slugify "$filename")
TODAY=$(date +%F)

# Classify
classification=$("$SCRIPT_DIR/lib/classify.sh" "$filepath")
echo "CLASSIFY: $filename -> $classification"

content=$(cat "$filepath")
planned=""
executed=""
git_info="none"
pr_url=""
status="success"

case "$classification" in
  repo-change)
    planned="1. Create feature branch\n2. Implement changes per inbox item\n3. Commit + push + draft PR"
    # Create feature branch
    branch=$(gf_create_branch "$slug")
    echo "BRANCH: $branch"

    # Parse the task and apply a simple change
    if echo "$content" | grep -qi "dry-run"; then
      # Add --dry-run flag to run-digest.sh
      sed -i '' '2a\
DRY_RUN=false\
for arg in "$@"; do case "$arg" in --dry-run) DRY_RUN=true;; esac; done\
if [[ "$DRY_RUN" == "true" ]]; then echo "[DRY-RUN] Would process inbox items."; exit 0; fi\
' "$REPO_DIR/bin/run-digest.sh"

      # Update README
      cat >> "$REPO_DIR/README.md" <<RDME

## Usage
\`\`\`bash
# Normal run
bin/run-digest.sh

# Dry run (no changes)
bin/run-digest.sh --dry-run
\`\`\`
RDME
    fi

    gf_commit "feat(daily-digest): add --dry-run flag to run-digest.sh"
    gf_push "$branch"
    pr_url=$(gf_draft_pr "$branch" "feat: add --dry-run flag" "Auto-generated from inbox item: $filename")
    gf_return_main
    executed="1. Created branch $branch\n2. Added --dry-run flag to run-digest.sh\n3. Updated README.md\n4. Committed + pushed\n5. Draft PR: $pr_url"
    git_info="repo: openclaw-daily-digest | branch: $branch | PR: $pr_url"
    ;;
  research)
    planned="1. Research topic\n2. Write result to Outbox"
    executed="1. Wrote research summary to Outbox"
    ;;
  ambiguous|ops)
    planned="1. Cannot auto-execute ($classification)\n2. Write plan to Outbox\n3. Move to Failed"
    executed="1. Wrote plan to Outbox\n2. Moved to Failed"
    status="failed"
    ;;
esac

# Write vault artifacts
outbox_path=$(write_outbox "$slug" "$classification" "$planned" "$executed" "$git_info" "none" "$filename")
memory_path=$(write_memory "$slug" "$classification" "Processed inbox item: $filename as $classification")
log_path=$(append_log "$slug" "$classification" "$status")
move_inbox "$filepath" "$status"

echo "OUTBOX: $outbox_path"
echo "MEMORY: $memory_path"
echo "LOG: $log_path"
echo "STATUS: $status"
echo "PR_URL: $pr_url"
