# openclaw-daily-digest

Deterministic inbox orchestrator. Reads Markdown items from an Obsidian inbox,
classifies them (project routing + action type), enriches via LLM, and pushes
tasks to the [task-orchestrator](https://github.com/kwstnr-jc/task-orchestrator) API.

## Why this approach

- **Deterministic outer loop.** The orchestrator owns all state transitions,
  file I/O, and error handling. AI is a pure function dependency -- never relied
  upon for correctness.
- **API-first.** Classified tasks are pushed to the task-orchestrator API for
  tracking and project management.
- **Incremental.** Each phase is a working, shippable product.

## Structure

```
src/main.rs                 # Rust orchestrator
src/api.rs                  # HTTP client for task-orchestrator API
src/classify.rs             # Project + action type classification
src/enrich.rs               # LLM enrichment
src/report.rs               # Digest report builder
src/types.rs                # Shared types
src/util.rs                 # File I/O utilities
src/discord.rs              # Discord webhook posting
src/execute.rs              # (dead code) Execution handlers
src/git.rs                  # (dead code) Git operations
tests/helpers/mock-openclaw.sh  # Mock LLM script for tests
```

## Quick start

```bash
# Build the Rust binary
cargo build --release

# Create inbox/outbox directories
mkdir -p /path/to/inbox /path/to/outbox

# Create a test inbox item
echo "# Test task" > /path/to/inbox/test.md

# Run the digest
target/release/openclaw-daily-digest run --inbox /path/to/inbox --outbox /path/to/outbox

# With API integration
API_URL=https://your-api.example.com API_KEY=your-key \
  target/release/openclaw-daily-digest run --inbox /path/to/inbox --outbox /path/to/outbox
```

## CLI

```
target/release/openclaw-daily-digest run \
  --inbox <path>      # Required: source directory for *.md items
  --outbox <path>     # Required: output directory for digest reports
  --dry-run           # Don't move inbox items after processing
  --max-items <n>     # Max items per run (default: 10, 0 = unlimited)
  --no-discord        # Skip Discord posting
```

## Environment variables

| Variable | Description | Default |
|----------|-------------|---------|
| `DIGEST_INBOX_DIR` | Inbox directory (alternative to `--inbox`) | (required) |
| `DIGEST_OUTBOX_DIR` | Outbox directory (alternative to `--outbox`) | (required) |
| `LLM_CMD` | Path to LLM CLI binary | `claude` |
| `API_URL` | Task orchestrator API base URL | (empty = skip push) |
| `API_KEY` | API key for task orchestrator | (empty) |
| `DIGEST_LOG_RETENTION_DAYS` | Days to keep log files | `30` |
| `DISCORD_TOKEN_FILE` | Path to Discord bot token file | `~/.digest-bot-token` |
| `DISCORD_CHANNEL_ID` | Discord channel ID for summaries | (hardcoded default) |

## Testing

```bash
# Run all tests (single-threaded required)
cargo test -- --test-threads=1

# Lint
cargo clippy --all-targets -- -D warnings

# Format
cargo fmt --check
```

## Design principle

> The orchestrator never writes its own scripts at runtime. All code lives in this
> repo, is reviewed, committed, and pushed before execution. AI is a pure function
> dependency -- the deterministic outer loop handles all state transitions.
