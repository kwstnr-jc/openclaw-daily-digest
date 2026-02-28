# openclaw-daily-digest

Autonomous daily inbox digest pipeline. Reads items from an Obsidian vault inbox, classifies them, executes work within guardrails, and posts a summary digest to Discord.

## Structure
```
bin/run-digest.sh        # entrypoint
bin/process-inbox.sh     # loops inbox items
bin/work-executor.sh     # executes per classification
lib/classify.sh          # repo-change|research|ambiguous|ops
lib/obsidian.sh          # vault read/write helpers
lib/gitflow.sh           # branch/commit/push/PR
lib/discord.sh           # post digest to Discord channel
config/example.env       # config template
docs/runbook.md          # operational docs
```

## Quick Start
```bash
bin/run-digest.sh
```

See [docs/runbook.md](docs/runbook.md) for details.
