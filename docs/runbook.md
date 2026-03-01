# Daily Digest Runbook

## What this does

The inbox orchestrator is a deterministic state machine that:

1. Reads the first `*.md` file (alphabetically) from the vault Inbox.
2. If no items exist, prints "No inbox items." and exits 0.
3. Otherwise:
   - **Classifies** the item: project routing (explicit line, tag, folder match, or AI) and action type (keyword overrides or AI).
   - **Enriches** via OpenClaw: planned actions, clarifying questions, next step.
   - **Executes** handler: research/question produce reports; repo-change/ops are blocked pending approval; notes are pass-through.
   - Writes a digest report to Outbox: `YYYY-MM-DD_HHMM-<name>-digest.md`
   - Writes an envelope: `YYYY-MM-DD_HHMM-<name>.envelope.json`
   - Appends a log line to `Logs/YYYY-MM-DD.md`
   - Moves the inbox item to `Processed/` (or `Failed/` on IO error)
4. Exits 0 on success (including unenriched fallback).

## Implementation

The orchestrator has two implementations with identical behavior:

- **Rust binary** (primary): `target/release/openclaw-daily-digest run`
- **Bash reference** (fallback): `bin/run-digest-bash.sh`

`bin/run-digest.sh` is a thin wrapper that delegates to the Rust binary, falling back to bash if the binary is not built.

## How to run

```bash
# Via wrapper (preferred)
bin/run-digest.sh

# Via Rust binary directly
cargo run --release -- run

# With overrides
cargo run --release -- run --root /tmp/test-vault --dry-run

# On-demand trigger
bin/digest-now.sh
```

## Trigger modes

### Scheduled (launchd)

The orchestrator runs daily at 08:00 via launchd:

```
~/Library/LaunchAgents/com.kevinwuestner.digest.plist
```

Management:

```bash
# Check status
launchctl print gui/$(id -u)/com.kevinwuestner.digest

# Manually trigger now
launchctl kickstart gui/$(id -u)/com.kevinwuestner.digest

# Unload
launchctl bootout gui/$(id -u)/com.kevinwuestner.digest

# Reload after plist changes
launchctl bootout gui/$(id -u)/com.kevinwuestner.digest
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.kevinwuestner.digest.plist
```

Logs: `~/Library/Logs/openclaw-digest.log`

### On-demand (Discord)

To trigger manually from Discord, configure OpenClaw to execute:

```
~/work/openclaw-daily-digest/bin/digest-now.sh
```

when receiving a "digest now" command.

The orchestrator is the single source of truth for processing logic. Both Discord and launchd call the same Rust binary entrypoint — no duplicate logic.

### Safety and idempotency

- If no inbox items exist, the runner exits 0 with no side effects.
- Multiple rapid invocations do not corrupt state: each run processes exactly one item and moves it atomically.
- Log file is appended to safely (no truncation).
- Writes use atomic temp-file-then-rename to prevent partial output.

## How to test

```bash
# Run bash integration tests (16 tests)
tests/run-tests.sh

# Run Rust unit tests (5 tests)
cargo test -- --test-threads=1
```

## Vault paths

| Path | Purpose |
|------|---------|
| `$ROOT/Inbox/` | Source items. The runner reads `*.md` files here. |
| `$ROOT/Inbox/Processed/` | Successfully processed items. |
| `$ROOT/Inbox/Failed/` | Items that failed due to IO errors. |
| `$ROOT/Outbox/` | Generated digest reports and envelopes. |
| `$ROOT/Logs/YYYY-MM-DD.md` | Daily log. One line per digest run. |
| `$ROOT/Projects/` | Project directories for routing. |

Where `ROOT=/Users/Shared/agent-vault/Agent` (override with `--root` or `DIGEST_ROOT` env var).

## Model selection policy

`config/policy.json` defines three model tiers:

| Tier | Model | Used for |
|------|-------|----------|
| cheap | gpt-4o-mini | classification, action_type |
| mid | claude-sonnet | enrichment, research, question |
| expensive | claude-opus | deep analysis (#deep tag) |

Override with `DIGEST_POLICY` env var to point to a different policy file.

## Configuration

Copy `config/example.env` to `.env` and fill in values:

```bash
cp config/example.env .env
```

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| "No inbox items." | No `*.md` files in Inbox | Add a `.md` file to the Inbox |
| Permission denied | Script not executable | `chmod +x bin/run-digest.sh` |
| "Rust binary not found" | Not built yet | `cargo build --release` |
| Unenriched output | OpenClaw unavailable or returned invalid JSON | Check `openclaw` is on PATH |
| launchd not firing | Plist not loaded | `launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.kevinwuestner.digest.plist` |
