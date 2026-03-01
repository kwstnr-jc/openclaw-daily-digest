# openclaw-daily-digest

Deterministic inbox orchestrator for OpenClaw. Reads Markdown items from a
vault inbox, classifies them (project routing + action type), enriches via LLM,
executes safe handlers, and writes structured reports with full audit trail.

## Why this approach

- **Deterministic outer loop.** The orchestrator owns all state transitions,
  file I/O, and error handling. AI is a pure function dependency — never relied
  upon for correctness.
- **Auditable.** Every run produces an `envelope.json` (single source of truth),
  a digest report, and a log entry.
- **Safe autonomy.** Research and question tasks execute automatically.
  Repo-change and ops tasks are blocked pending approval.
- **Incremental.** Each phase is a working, shippable product.

## Current stage

All 8 phases complete:

| Phase | Description | Status |
|-------|-------------|--------|
| 1 | Deterministic file loop (read, report, log) | Done |
| 2 | Processing pipeline (Processed/Failed, structured report, envelope.json) | Done |
| 3 | LLM enrichment (planned actions, questions, next step via OpenClaw) | Done |
| 4 | Multi-level classification (project routing + action type) | Done |
| 5 | Execution handlers (research/question/blocked) | Done |
| 6 | Testing harness (16 bats tests + 5 Rust tests) | Done |
| 7 | Model selection policy (cheap/mid/expensive tiers) | Done |
| 8 | Rust rewrite + dual trigger (launchd + Discord entrypoint) | Done |

## Structure

```
src/main.rs                 # Rust orchestrator (primary implementation)
bin/run-digest.sh           # thin wrapper → Rust binary (fallback → bash)
bin/run-digest-bash.sh      # bash reference implementation
bin/digest-now.sh           # on-demand trigger for Discord/manual use
config/policy.json          # model selection policy (cheap/mid/expensive)
config/example.env          # configuration template (no secrets)
docs/spec.md                # orchestrator ↔ AI interface specification
docs/runbook.md             # operational docs, trigger modes, troubleshooting
tests/orchestrator.bats     # 16 bats integration tests
tests/fixtures/             # test inbox items for all classification paths
tests/helpers/              # mock OpenClaw script
```

## Quick start

```bash
# Build the Rust binary
cargo build --release

# Create a test inbox item
echo "# Test task" > /Users/Shared/agent-vault/Agent/Inbox/test.md

# Run the digest
bin/run-digest.sh

# Or directly
target/release/openclaw-daily-digest run

# Verify outputs
ls /Users/Shared/agent-vault/Agent/Outbox/
cat /Users/Shared/agent-vault/Agent/Logs/$(date +%F).md
```

## Trigger modes

- **Scheduled:** launchd plist runs daily at 08:00 (`~/Library/LaunchAgents/com.kevinwuestner.digest.plist`)
- **On-demand:** `bin/digest-now.sh` for Discord or manual execution
- **CLI:** `target/release/openclaw-daily-digest run [--root <path>] [--dry-run]`

## Testing

```bash
# Bash integration tests (16 tests)
tests/run-tests.sh

# Rust unit tests (5 tests)
cargo test -- --test-threads=1
```

## Vault paths (runtime only — not in this repo)

```
ROOT=/Users/Shared/agent-vault/Agent
├── Inbox/              # source items (*.md)
│   ├── Processed/      # successfully processed items
│   └── Failed/         # items that failed due to IO errors
├── Outbox/             # digest reports + envelope.json files
├── Logs/               # daily log files (YYYY-MM-DD.md)
└── Projects/           # project directories for routing
```

## Design principle

> Claude Code writes scripts. OpenClaw executes scripts.
>
> The agent never writes its own scripts at runtime. All code lives in this
> repo, is reviewed, committed, and pushed before execution.

See [docs/runbook.md](docs/runbook.md) for operational details and [docs/spec.md](docs/spec.md) for the interface specification.
