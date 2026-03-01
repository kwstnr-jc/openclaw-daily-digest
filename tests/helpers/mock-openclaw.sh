#!/usr/bin/env bash
# Mock OpenClaw CLI for testing.
# Responds to "agent" subcommand with canned JSON based on prompt content.
# Set MOCK_OPENCLAW_FAIL=1 to simulate failure.
# Set MOCK_OPENCLAW_INVALID=1 to return invalid JSON.

set -euo pipefail

if [[ "${MOCK_OPENCLAW_FAIL:-}" == "1" ]]; then
  echo "Error: mock failure" >&2
  exit 1
fi

if [[ "${MOCK_OPENCLAW_INVALID:-}" == "1" ]]; then
  echo "This is not valid JSON at all {{{broken"
  exit 0
fi

# Parse args — capture --message and --model
MESSAGE=""
MODEL=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --message) MESSAGE="$2"; shift 2 ;;
    --model) MODEL="$2"; shift 2 ;;
    *) shift ;;
  esac
done

# Log model selection for test assertions
if [[ -n "${MOCK_OPENCLAW_LOG:-}" ]]; then
  echo "$MODEL" >> "$MOCK_OPENCLAW_LOG"
fi

# Detect which type of call this is based on prompt keywords
if echo "$MESSAGE" | grep -q "Classify the action type"; then
  # Action type classification call
  cat <<'JSON'
{
  "action_type": "note",
  "confidence": 0.8,
  "rationale": "Mock classification",
  "suggested_repo": null
}
JSON

elif echo "$MESSAGE" | grep -q "Classify the following task into a project"; then
  # Project classification call
  cat <<'JSON'
{
  "project": { "kind": "none", "name": null },
  "confidence": 0.7,
  "rationale": "Mock: no project match"
}
JSON

elif echo "$MESSAGE" | grep -q "research assistant"; then
  # Research execution handler
  cat <<'RESEARCH'
## Summary

Mock research report for testing.

## Findings

- Finding 1
- Finding 2

## Sources

- https://example.com/source1
- https://example.com/source2

## Next Steps

- Next step 1
RESEARCH

elif echo "$MESSAGE" | grep -q "expert assistant"; then
  # Question execution handler
  cat <<'ANSWER'
## Answer

Mock answer for testing.

## Details

- Detail 1
- Detail 2

## Sources

- https://example.com/source1

## Follow-up Questions

- Follow-up 1
ANSWER

else
  # Default: enrichment call
  cat <<'JSON'
{
  "planned_actions": ["Mock action 1", "Mock action 2"],
  "clarifying_questions": [],
  "next_step": "Mock next step"
}
JSON
fi
