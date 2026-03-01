# Daily Digest Runbook

## What this does

`bin/run-digest.sh` is a deterministic shell script that:

1. Reads the first `*.md` file (alphabetically) from the vault Inbox.
2. If no items exist, prints "No inbox items." and exits 0.
3. Otherwise:
   - Creates a digest report in Outbox named `YYYY-MM-DD_HHMM-<name>-digest.md`.
   - Copies the first 200 lines of the inbox item into the report.
   - Appends `## Planned Actions` and `## Next Step` placeholder sections.
   - Appends a timestamped log line to `Logs/YYYY-MM-DD.md`.
4. Exits 0 on success.

## How to run

```bash
cd ~/work/openclaw-daily-digest
bin/run-digest.sh
```

## How to test locally

```bash
# 1. Create a test inbox item
cat > /Users/Shared/agent-vault/Agent/Inbox/test-item.md <<'EOF'
# Test Item
This is a test brief for the digest runner.
EOF

# 2. Run the script
bin/run-digest.sh

# 3. Verify the digest report was created
ls -la /Users/Shared/agent-vault/Agent/Outbox/

# 4. Verify the log entry
cat /Users/Shared/agent-vault/Agent/Logs/$(date +%F).md

# 5. Clean up test data
rm /Users/Shared/agent-vault/Agent/Inbox/test-item.md
```

## Vault paths

| Path | Purpose |
|------|---------|
| `$ROOT/Inbox/` | Source items. The runner reads `*.md` files here. |
| `$ROOT/Outbox/` | Generated digest reports. |
| `$ROOT/Logs/YYYY-MM-DD.md` | Daily log. One line per digest run. |
| `$ROOT/Inbox/Processed/` | (Phase 2) Successfully processed items move here. |
| `$ROOT/Inbox/Failed/` | (Phase 2) Items that fail processing move here. |

Where `ROOT=/Users/Shared/agent-vault/Agent`.

## Phased extension plan

### Phase 2 — Processing pipeline (next)

- After generating the report, move the inbox item to `Processed/`.
- On error, move to `Failed/` and log the failure.
- Add structured frontmatter to the digest report (date, source, status).
- Still fully deterministic — no LLM calls.

### Phase 3 — LLM enrichment

- Replace placeholder sections with real content via OpenRouter API.
- Generate: Planned Actions, clarifying questions, concrete Next Step.
- Keep the deterministic path as a fallback (offline mode).

### Phase 4 — Always-on cadence

- Add a launchd plist (or cron entry) to trigger `bin/run-digest.sh` on a
  schedule (e.g., daily at 08:00).
- Log rotation if needed.

### Phase 5 — Repo-change items (optional)

- For inbox items classified as code changes: create a feature branch, apply
  changes, open a draft PR.
- Requires `gh` CLI and repo write access.

### Phase 6 — Discord posting (optional)

- Post a summary of each digest run to a Discord channel via webhook.
- Requires a webhook URL in `.env`.

## Configuration

Copy `config/example.env` to `.env` and fill in values:

```bash
cp config/example.env .env
```

Currently only `VAULT_ROOT` is used. Other variables are reserved for future
phases.

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| "No inbox items." | No `*.md` files in Inbox | Add a `.md` file to the Inbox |
| Permission denied | Script not executable | `chmod +x bin/run-digest.sh` |
| Outbox empty after run | Script errored silently | Check `set -euo pipefail` output |
