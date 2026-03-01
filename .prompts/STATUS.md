# Project Status — 2026-03-01

All 12 prompts are **complete**. The project is a deterministic inbox orchestrator
that runs on a Mac mini, processing Markdown tasks from a vault inbox.

## Architecture

```
Inbox/*.md → Orchestrator → Outbox/ (digest + envelope.json) + Logs/
                │
                ├─ Level 1: Project classification (explicit line → tag → folder match → AI)
                ├─ Level 2: Action type (keyword overrides → AI fallback)
                ├─ Enrichment: planned_actions, clarifying_questions, next_step (AI)
                ├─ Execution: research/question auto-execute, repo-change opens PRs, ops with safety rails
                └─ Discord: summary posted via bot REST API after each run
```

**Core principle:** The deterministic outer loop owns all state. AI is a pure
function — its failure degrades output but never breaks the run.

## Implementation

- **Rust binary** (sole implementation): `src/main.rs` (~2100 lines, 23 tests)
  with clap CLI (`run --root --dry-run --max-items --no-discord`), serde JSON,
  reqwest for Discord, atomic writes, `OPENCLAW_CMD` env var for mock injection.
- **Thin wrapper**: `bin/run-digest.sh` → delegates to Rust binary, errors if not built.
- Bash reference implementation has been **removed** (prompt 11+).

## Key files

| File | Purpose |
|------|---------|
| `src/main.rs` | Rust orchestrator (~2100 lines, 23 tests) |
| `bin/run-digest.sh` | Wrapper (delegates to Rust binary) |
| `bin/digest-now.sh` | On-demand trigger for Discord/manual |
| `config/policy.json` | Model tiers: cheap/mid/expensive |
| `tests/helpers/mock-openclaw.sh` | Mock with FAIL/INVALID/LOG modes |
| `tests/fixtures/` | 7 test inbox items |
| `.cargo/config.toml` | Enforces single-threaded tests |

## Trigger modes

- **launchd**: `~/Library/LaunchAgents/com.kevinwuestner.digest.plist` — daily 08:00
- **Discord**: via `bin/digest-now.sh` (OpenClaw executes it)
- **CLI**: `target/release/openclaw-daily-digest run`

## Vault paths (runtime, not in repo)

```
ROOT=/Users/Shared/agent-vault/Agent
├── Inbox/           → source *.md files
│   ├── Processed/   → success
│   └── Failed/      → IO errors only
├── Outbox/          → digest reports + envelope.json
├── Logs/            → YYYY-MM-DD.md (one line per run)
└── Projects/        → project directories for routing
```

## Test coverage

23 Rust tests covering:
- Empty inbox, happy path enriched, OpenClaw failure/invalid JSON → unenriched
- Dry run, IO failure → Failed, multiple items, max items limit
- Discord message formatting, Discord token failure (graceful)
- New project creation, research output, question handler
- Tag routing, unclassified note, log entry format
- Repo-change skip (no git repo), ops execution, ops safety (dangerous task)
- Policy: cheap/mid/expensive model selection, `#deep` tag, missing policy file

Run: `cargo test`

## Env vars for testing/overrides

| Var | Default | Purpose |
|-----|---------|---------|
| `DIGEST_ROOT` | `/Users/Shared/agent-vault/Agent` | Override vault root |
| `DIGEST_POLICY` | `config/policy.json` (repo-relative) | Override policy file |
| `OPENCLAW_CMD` | `openclaw` | Override OpenClaw binary (used by tests) |
| `DISCORD_TOKEN_FILE` | `~/.digest-bot-token` | Bot token file path |
| `DISCORD_CHANNEL_ID` | `1477340656350396668` | Target Discord channel |
| `MOCK_OPENCLAW_FAIL` | unset | Set to `1` for mock failure |
| `MOCK_OPENCLAW_INVALID` | unset | Set to `1` for invalid JSON |
| `MOCK_OPENCLAW_LOG` | unset | File path to log `--model` args |

## Prompt completion log

| # | Prompt | Status |
|---|--------|--------|
| 00 | North star (architecture principles) | Reference doc |
| 01 | Interface spec + envelope.json | Done |
| 02 | Multi-level project classification | Done |
| 03 | Action type classification | Done |
| 04 | Execution handlers | Done |
| 05 | Testing harness (bats-core) | Done |
| 06 | Model selection policy | Done |
| 07 | Rust rewrite | Done |
| 08 | Dual trigger (launchd + Discord) | Done |
| 09 | Loop processing (all inbox items per run) | Done |
| 10 | Discord posting (bot REST API) | Done |
| 11 | Autonomous execution (repo-change PRs, ops with safety) | Done |
| 12 | Discord format cleanup (per-item detail lines) | Done |

## What's next (not yet prompted)

- Discord embeds (richer formatting) instead of plain content
- Log rotation
- Config-driven project rules (beyond folder matching)
- Approval workflow for blocked execution types
