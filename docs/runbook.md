# Daily Digest Runbook

## Quick Start
```bash
bin/run-digest.sh          # process inbox + post digest
bin/run-digest.sh --dry-run # preview without changes
```

## How It Works
1. `run-digest.sh` calls `process-inbox.sh`
2. For each `*.md` in `$VAULT/Inbox/`:
   - `classify.sh` determines type: repo-change | research | ambiguous | ops
   - `work-executor.sh` auto-executes (repo-change/research) or plans (ambiguous/ops)
   - Results written to Outbox, Memory, Logs
   - Item moved to Processed or Failed
3. Discord digest posted via bot

## Paths
- Inbox: /Users/Shared/agent-vault/Agent/Inbox
- Outbox: /Users/Shared/agent-vault/Agent/Outbox
- Memory: /Users/Shared/agent-vault/Agent/Memory
- Logs: /Users/Shared/agent-vault/Agent/Logs

## Git Workflow
- Feature branches: `feat/digest-YYYY-MM-DD-<slug>`
- Conventional commits
- Draft PRs via `gh pr create --draft`
