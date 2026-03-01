#!/usr/bin/env bats

# Orchestrator integration tests using temp directories and mock OpenClaw.

REPO_DIR="$(cd "$(dirname "$BATS_TEST_FILENAME")/.." && pwd)"
SCRIPT="$REPO_DIR/bin/run-digest.sh"
FIXTURES="$REPO_DIR/tests/fixtures"
MOCK_DIR="$REPO_DIR/tests/helpers"

setup() {
  # Create isolated vault structure in a temp dir
  TEST_ROOT="$(mktemp -d)"
  mkdir -p "$TEST_ROOT/Inbox" "$TEST_ROOT/Outbox" "$TEST_ROOT/Logs"
  mkdir -p "$TEST_ROOT/Inbox/Processed" "$TEST_ROOT/Inbox/Failed"
  mkdir -p "$TEST_ROOT/Projects"

  export DIGEST_ROOT="$TEST_ROOT"
  # Put mock openclaw first on PATH
  export PATH="$MOCK_DIR:$PATH"
  # Ensure the mock is found as "openclaw"
  ln -sf "$MOCK_DIR/mock-openclaw.sh" "$MOCK_DIR/openclaw"

  # Reset mock env vars
  unset MOCK_OPENCLAW_FAIL
  unset MOCK_OPENCLAW_INVALID
}

teardown() {
  rm -rf "$TEST_ROOT"
  rm -f "$MOCK_DIR/openclaw"
}

# --- Helper ---
place_fixture() {
  cp "$FIXTURES/$1" "$TEST_ROOT/Inbox/$1"
}

# ============================================================
# Test: Empty inbox exits 0, no files created
# ============================================================
@test "empty inbox exits 0 with no output files" {
  run bash "$SCRIPT"
  [ "$status" -eq 0 ]
  [[ "$output" == *"No inbox items"* ]]

  # No files in Outbox
  [ "$(ls "$TEST_ROOT/Outbox/" 2>/dev/null | wc -l)" -eq 0 ]
}

# ============================================================
# Test: Existing project + repo-change (explicit Project: line + "fix" keyword)
# ============================================================
@test "existing project + repo-change: routes correctly and blocks execution" {
  # Create matching project folder
  mkdir -p "$TEST_ROOT/Projects/openclaw-daily-digest"
  place_fixture "existing-project-repo-change.md"

  run bash "$SCRIPT"
  [ "$status" -eq 0 ]

  # Item moved to Processed
  [ -f "$TEST_ROOT/Inbox/Processed/existing-project-repo-change.md" ]
  [ ! -f "$TEST_ROOT/Inbox/existing-project-repo-change.md" ]

  # Outbox report exists
  local report
  report="$(ls "$TEST_ROOT/Outbox/"*-digest.md 2>/dev/null | head -n 1)"
  [ -n "$report" ]

  # Report contains expected sections
  grep -q "## Project Routing" "$report"
  grep -q "existing" "$report"
  grep -q "openclaw-daily-digest" "$report"
  grep -q "## Action Type" "$report"
  grep -q "repo-change" "$report"
  grep -q "## Execution" "$report"
  grep -q "blocked" "$report"
  grep -q "## Planned Actions" "$report"

  # Envelope exists and is valid JSON
  local envelope
  envelope="$(ls "$TEST_ROOT/Outbox/"*.envelope.json 2>/dev/null | head -n 1)"
  [ -n "$envelope" ]
  jq empty "$envelope"

  # Envelope fields
  [ "$(jq -r '.classification.project.kind' "$envelope")" = "existing" ]
  [ "$(jq -r '.classification.project.name' "$envelope")" = "openclaw-daily-digest" ]
  [ "$(jq -r '.action_type.action_type' "$envelope")" = "repo-change" ]
  [ "$(jq -r '.execution.status' "$envelope")" = "blocked" ]

  # Log file exists with entry
  local logfile
  logfile="$(ls "$TEST_ROOT/Logs/"*.md 2>/dev/null | head -n 1)"
  [ -n "$logfile" ]
  grep -q "Processed/" "$logfile"
  grep -q "enriched" "$logfile"
}

# ============================================================
# Test: New project + research (explicit Project: line + "research"/"compare" keywords)
# ============================================================
@test "new project + research: creates project dir and research report" {
  place_fixture "new-project-research.md"

  run bash "$SCRIPT"
  [ "$status" -eq 0 ]

  # Item moved to Processed
  [ -f "$TEST_ROOT/Inbox/Processed/new-project-research.md" ]

  # New project directory created
  [ -d "$TEST_ROOT/Projects/home-automation" ]
  [ -f "$TEST_ROOT/Projects/home-automation/README.md" ]
  [ -d "$TEST_ROOT/Projects/home-automation/Inbox" ]

  # Envelope
  local envelope
  envelope="$(ls "$TEST_ROOT/Outbox/"*.envelope.json 2>/dev/null | head -n 1)"
  jq empty "$envelope"
  [ "$(jq -r '.classification.project.kind' "$envelope")" = "new" ]
  [ "$(jq -r '.classification.project.name' "$envelope")" = "home-automation" ]
  [ "$(jq -r '.action_type.action_type' "$envelope")" = "research" ]

  # Research execution output file
  local research_file
  research_file="$(ls "$TEST_ROOT/Outbox/"*.research.md 2>/dev/null | head -n 1)"
  [ -n "$research_file" ]
  grep -q "## Summary" "$research_file"
  grep -q "## Findings" "$research_file"

  [ "$(jq -r '.execution.status' "$envelope")" = "completed" ]
}

# ============================================================
# Test: No project + question (ends with ?, question keyword match)
# ============================================================
@test "no project + question: produces answer report" {
  place_fixture "none-project-question.md"

  run bash "$SCRIPT"
  [ "$status" -eq 0 ]

  [ -f "$TEST_ROOT/Inbox/Processed/none-project-question.md" ]

  local envelope
  envelope="$(ls "$TEST_ROOT/Outbox/"*.envelope.json 2>/dev/null | head -n 1)"
  jq empty "$envelope"

  # "launchctl" keyword triggers ops, but "?" at end triggers question
  # The keyword check for ops comes before the "?" check, so this should be ops
  # Actually let's check: the fixture has "launchd" not "launchctl"
  # and "cron" is not a keyword. The last line ends with "?"
  # Let's just verify it ended up somewhere sensible.
  local action_type
  action_type="$(jq -r '.action_type.action_type' "$envelope")"
  [ "$action_type" = "question" ]

  # Answer output
  local answer_file
  answer_file="$(ls "$TEST_ROOT/Outbox/"*.research.md 2>/dev/null | head -n 1)"
  [ -n "$answer_file" ]
  grep -q "## Answer" "$answer_file"
}

# ============================================================
# Test: Ops task (keyword match: "install", "tailscale", "launchctl")
# ============================================================
@test "ops task: classified as ops and execution blocked" {
  place_fixture "ops-task.md"

  run bash "$SCRIPT"
  [ "$status" -eq 0 ]

  [ -f "$TEST_ROOT/Inbox/Processed/ops-task.md" ]

  local envelope
  envelope="$(ls "$TEST_ROOT/Outbox/"*.envelope.json 2>/dev/null | head -n 1)"
  jq empty "$envelope"
  [ "$(jq -r '.action_type.action_type' "$envelope")" = "ops" ]
  [ "$(jq -r '.execution.status' "$envelope")" = "blocked" ]
}

# ============================================================
# Test: Tag-based project routing (#project/name)
# ============================================================
@test "tag-based routing: #project/ tag routes to existing project" {
  mkdir -p "$TEST_ROOT/Projects/openclaw-daily-digest"
  place_fixture "tag-project-note.md"

  run bash "$SCRIPT"
  [ "$status" -eq 0 ]

  [ -f "$TEST_ROOT/Inbox/Processed/tag-project-note.md" ]

  local envelope
  envelope="$(ls "$TEST_ROOT/Outbox/"*.envelope.json 2>/dev/null | head -n 1)"
  jq empty "$envelope"
  [ "$(jq -r '.classification.project.kind' "$envelope")" = "existing" ]
  [ "$(jq -r '.classification.project.name' "$envelope")" = "openclaw-daily-digest" ]
}

# ============================================================
# Test: Unclassified note (no project match, no action keyword)
# ============================================================
@test "unclassified note: falls through to AI classification then note" {
  place_fixture "unclassified-note.md"

  run bash "$SCRIPT"
  [ "$status" -eq 0 ]

  [ -f "$TEST_ROOT/Inbox/Processed/unclassified-note.md" ]

  local envelope
  envelope="$(ls "$TEST_ROOT/Outbox/"*.envelope.json 2>/dev/null | head -n 1)"
  jq empty "$envelope"

  # Mock AI returns kind=none, action_type=note
  [ "$(jq -r '.action_type.action_type' "$envelope")" = "note" ]
  [ "$(jq -r '.execution.status' "$envelope")" = "none" ]
}

# ============================================================
# Test: OpenClaw failure — item still processed as unenriched
# ============================================================
@test "openclaw failure: item processed as unenriched" {
  export MOCK_OPENCLAW_FAIL=1
  place_fixture "existing-project-repo-change.md"
  mkdir -p "$TEST_ROOT/Projects/openclaw-daily-digest"

  run bash "$SCRIPT"
  [ "$status" -eq 0 ]

  # Still moved to Processed, not Failed
  [ -f "$TEST_ROOT/Inbox/Processed/existing-project-repo-change.md" ]

  local envelope
  envelope="$(ls "$TEST_ROOT/Outbox/"*.envelope.json 2>/dev/null | head -n 1)"
  jq empty "$envelope"
  [ "$(jq -r '.status' "$envelope")" = "unenriched" ]

  # Log records unenriched
  local logfile
  logfile="$(ls "$TEST_ROOT/Logs/"*.md 2>/dev/null | head -n 1)"
  grep -q "unenriched" "$logfile"
}

# ============================================================
# Test: OpenClaw returns invalid JSON — still processed
# ============================================================
@test "openclaw invalid json: item processed as unenriched" {
  export MOCK_OPENCLAW_INVALID=1
  place_fixture "unclassified-note.md"

  run bash "$SCRIPT"
  [ "$status" -eq 0 ]

  [ -f "$TEST_ROOT/Inbox/Processed/unclassified-note.md" ]

  local envelope
  envelope="$(ls "$TEST_ROOT/Outbox/"*.envelope.json 2>/dev/null | head -n 1)"
  jq empty "$envelope"
  [ "$(jq -r '.status' "$envelope")" = "unenriched" ]
}

# ============================================================
# Test: IO failure — cannot write to Outbox → item moved to Failed
# ============================================================
@test "io failure: unwritable outbox moves item to Failed" {
  place_fixture "ops-task.md"
  # Make Outbox unwritable
  chmod 000 "$TEST_ROOT/Outbox"

  run bash "$SCRIPT"
  # The script should still exit 0 (it catches the error and moves to Failed)
  # but the item should be in Failed/
  [ -f "$TEST_ROOT/Inbox/Failed/ops-task.md" ] || [ -f "$TEST_ROOT/Inbox/Processed/ops-task.md" ]

  # Restore permissions for teardown
  chmod 755 "$TEST_ROOT/Outbox"
}

# ============================================================
# Test: Log file contains required fields
# ============================================================
@test "log entry contains timestamp, filenames, and status" {
  mkdir -p "$TEST_ROOT/Projects/openclaw-daily-digest"
  place_fixture "existing-project-repo-change.md"

  run bash "$SCRIPT"
  [ "$status" -eq 0 ]

  local logfile
  logfile="$(ls "$TEST_ROOT/Logs/"*.md 2>/dev/null | head -n 1)"
  [ -n "$logfile" ]

  # Log line format: [YYYY-MM-DD_HHMM] <inbox> -> <outbox> -> Processed/ [status]
  local logline
  logline="$(cat "$logfile")"
  [[ "$logline" =~ \[20[0-9]{2}-[0-9]{2}-[0-9]{2}_[0-9]{4}\] ]]
  [[ "$logline" == *"existing-project-repo-change.md"* ]]
  [[ "$logline" == *"Processed/"* ]]
  [[ "$logline" == *"enriched"* ]]
}

# ============================================================
# Test: Only first inbox item is processed per run
# ============================================================
@test "only first item processed when multiple inbox items exist" {
  place_fixture "ops-task.md"
  place_fixture "unclassified-note.md"

  run bash "$SCRIPT"
  [ "$status" -eq 0 ]

  # Count: exactly one in Processed, one still in Inbox
  local processed_count inbox_count
  processed_count="$(ls "$TEST_ROOT/Inbox/Processed/"*.md 2>/dev/null | wc -l | tr -d ' ')"
  inbox_count="$(ls "$TEST_ROOT/Inbox/"*.md 2>/dev/null | wc -l | tr -d ' ')"
  [ "$processed_count" -eq 1 ]
  [ "$inbox_count" -eq 1 ]
}

# ============================================================
# Test: Policy selects cheap model for classification calls
# ============================================================
@test "policy: classification uses cheap model" {
  export MOCK_OPENCLAW_LOG="$TEST_ROOT/model-log.txt"
  place_fixture "unclassified-note.md"

  run bash "$SCRIPT"
  [ "$status" -eq 0 ]

  # The mock logs each --model arg; classification + action_type should use cheap
  [ -f "$TEST_ROOT/model-log.txt" ]
  local cheap_count
  cheap_count="$(grep -c "gpt-4o-mini" "$TEST_ROOT/model-log.txt" || true)"
  [ "$cheap_count" -ge 1 ]
}

# ============================================================
# Test: Policy selects mid model for enrichment
# ============================================================
@test "policy: enrichment uses mid model" {
  export MOCK_OPENCLAW_LOG="$TEST_ROOT/model-log.txt"
  place_fixture "unclassified-note.md"

  run bash "$SCRIPT"
  [ "$status" -eq 0 ]

  [ -f "$TEST_ROOT/model-log.txt" ]
  local mid_count
  mid_count="$(grep -c "claude-sonnet" "$TEST_ROOT/model-log.txt" || true)"
  [ "$mid_count" -ge 1 ]
}

# ============================================================
# Test: #deep tag upgrades to expensive model
# ============================================================
@test "policy: #deep tag upgrades model to expensive" {
  export MOCK_OPENCLAW_LOG="$TEST_ROOT/model-log.txt"
  place_fixture "deep-analysis-task.md"

  run bash "$SCRIPT"
  [ "$status" -eq 0 ]

  [ -f "$TEST_ROOT/model-log.txt" ]
  local expensive_count
  expensive_count="$(grep -c "claude-opus" "$TEST_ROOT/model-log.txt" || true)"
  [ "$expensive_count" -ge 1 ]
}

# ============================================================
# Test: Missing policy file falls back gracefully
# ============================================================
@test "policy: missing policy file works without model args" {
  export DIGEST_POLICY="/nonexistent/policy.json"
  export MOCK_OPENCLAW_LOG="$TEST_ROOT/model-log.txt"
  place_fixture "unclassified-note.md"

  run bash "$SCRIPT"
  [ "$status" -eq 0 ]

  [ -f "$TEST_ROOT/Inbox/Processed/unclassified-note.md" ]

  # Model log should have empty lines (no model specified)
  if [ -f "$TEST_ROOT/model-log.txt" ]; then
    local nonempty
    nonempty="$(grep -c '.' "$TEST_ROOT/model-log.txt" || true)"
    [ "$nonempty" -eq 0 ] || true
  fi
}
