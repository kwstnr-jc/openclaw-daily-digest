# openclaw-daily-digest

## Architecture

- **Rust CLI** that reads Obsidian inbox items (Markdown files), classifies them by project and action type, enriches via LLM, and pushes tasks to the task-orchestrator API
- **Deterministic outer loop**: the orchestrator owns all state transitions, file I/O, and error handling. AI is a pure function dependency, never relied upon for correctness
- **LLM integration**: uses `claude` CLI (Claude Code) in print mode (`claude -p`) for classification and enrichment. Configurable via `LLM_CMD` env var
- **API integration**: pushes classified tasks to task-orchestrator API via `API_URL` and `API_KEY` env vars

## Commands

- `cargo build` -- build the binary
- `cargo test -- --test-threads=1` -- run tests (must be single-threaded due to env var mutation)
- `cargo clippy --all-targets -- -D warnings` -- lint
- `cargo fmt` -- format code

## Before Pushing

All of the following must pass locally before committing/pushing:

```bash
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test -- --test-threads=1
```

## After Creating a PR

Monitor the CI pipeline and fix failures immediately before moving on.

## Key Conventions

- Deterministic outer loop: AI is a pure function, never relied upon for correctness
- No ORM: direct HTTP calls to the task-orchestrator API
- Configuration via env vars or CLI args (`--inbox`, `--outbox`, `--dry-run`, `--max-items`, `--no-discord`)
- Tests use a mock LLM script at `tests/helpers/mock-openclaw.sh` with `LLM_CMD` env var
- Tests must run single-threaded (`--test-threads=1`) due to process-global env var mutation

## Configuration

| Variable | Description | Default |
|----------|-------------|---------|
| `LLM_CMD` | Path to LLM CLI binary | `claude` |
| `API_URL` | Task orchestrator API base URL | (empty = skip push) |
| `API_KEY` | API key for task orchestrator | (empty) |
| `DIGEST_INBOX_DIR` | Inbox directory (or `--inbox` flag) | (required) |
| `DIGEST_OUTBOX_DIR` | Outbox directory (or `--outbox` flag) | (required) |
| `DIGEST_LOG_RETENTION_DAYS` | Days to keep log files | `30` |
| `DISCORD_TOKEN_FILE` | Path to Discord bot token file | `~/.digest-bot-token` |
| `DISCORD_CHANNEL_ID` | Discord channel ID for summaries | (hardcoded default) |
