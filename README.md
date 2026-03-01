# openclaw-daily-digest

Deterministic daily digest runner for OpenClaw. Reads Markdown items from an
Obsidian vault inbox, writes a structured digest report to Outbox, and appends
a timestamped log line.

## Why this approach

- **Predictability over autonomy.** Every run produces the same outputs for the
  same inputs. No LLM calls, no network requests, no side effects beyond
  file I/O.
- **Cheap.** Zero token usage at this stage. LLM enrichment is a later,
  optional layer.
- **Auditable.** Outbox reports and log files form a complete paper trail.
- **Incremental.** We build capability in phases — each phase is a working,
  shippable product.

## Current stage

**Step 1 — Deterministic file IO + report generation** (complete).

The runner picks the first `*.md` file in the vault Inbox, copies its first
200 lines into a timestamped digest report in Outbox, appends placeholder
sections (`Planned Actions`, `Next Step`), and writes a log entry.

## Roadmap

| Phase | Description | Status |
|-------|-------------|--------|
| 1 | Deterministic file loop (read, report, log) | Done |
| 2 | Processing pipeline (move to Processed/Failed, structured report) | Next |
| 3 | LLM enrichment (Planned Actions, questions, next step) | Planned |
| 4 | Cron/launchd trigger for always-on cadence | Planned |
| 5 | Optional: open PRs for repo-change items | Planned |
| 6 | Optional: post summary to Discord | Planned |

## Structure

```
bin/run-digest.sh       # deterministic digest runner (entrypoint)
config/example.env      # configuration template (no secrets)
docs/runbook.md         # operational docs + phased plan
```

## Quick start

```bash
# Create a test inbox item
echo "# Test" > /Users/Shared/agent-vault/Agent/Inbox/test.md

# Run the digest
bin/run-digest.sh

# Verify outputs
ls /Users/Shared/agent-vault/Agent/Outbox/
cat /Users/Shared/agent-vault/Agent/Logs/$(date +%F).md
```

## Vault paths (runtime only — not in this repo)

```
ROOT=/Users/Shared/agent-vault/Agent
├── Inbox/           # source items (*.md)
├── Outbox/          # generated digest reports
└── Logs/            # daily log files (YYYY-MM-DD.md)
```

## Design principle

> Claude Code writes scripts. OpenClaw executes scripts.
>
> The agent never writes its own scripts at runtime. All code lives in this
> repo, is reviewed, committed, and pushed before execution.

See [docs/runbook.md](docs/runbook.md) for operational details.
