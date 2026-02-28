#!/usr/bin/env bash
set -euo pipefail
REPO_DIR="${DIGEST_REPO:-/Users/agent/work/openclaw-daily-digest}"
TODAY=$(date +%F)

gf_create_branch() {
  local slug="$1"
  local branch="feat/digest-${TODAY}-${slug}"
  git -C "$REPO_DIR" checkout -b "$branch" 2>&1
  echo "$branch"
}

gf_commit() {
  local msg="$1"
  git -C "$REPO_DIR" add -A 2>&1
  git -C "$REPO_DIR" commit -m "$msg" 2>&1
}

gf_push() {
  local branch="$1"
  git -C "$REPO_DIR" push -u origin "$branch" 2>&1
}

gf_draft_pr() {
  local branch="$1" title="$2" body="$3"
  gh pr create --repo kwstnr-jc/openclaw-daily-digest --draft \
    --title "$title" --body "$body" --head "$branch" 2>&1
}

gf_return_main() {
  git -C "$REPO_DIR" checkout main 2>&1
}
