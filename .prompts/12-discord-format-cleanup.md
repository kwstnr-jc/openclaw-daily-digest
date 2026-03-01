Task: Clean up Discord message formatting.

Problem:
The bash implementation sent literal `\n` text instead of actual newlines.
The Rust implementation uses actual newlines but the message format could be improved.

Current issues:
- Bash used double-quoted `\n` which doesn't produce real newlines (fixed: now uses $'...' syntax)
- Messages are plain text — Discord supports markdown but not full formatting
- No per-item detail lines (just a count summary)
- Bold markers (**) render correctly in Discord but the overall layout is cramped

Desired format (Discord renders markdown subset):
```
**Daily Digest** — 2026-03-01 12:31

Processed 3 items:
- `fix-bug.md` → **my-project** (repo-change) — PR opened: <url>
- `research-ai.md` → **none** (research) — completed
- `grocery-list.md` → **none** (note) — filed

Enriched: 2 | Unenriched: 1 | Failed: 0
```

Fixes applied:
1. ~~Bash $'...' syntax for real newlines~~ (DONE)
2. ~~Rebuild release binary so Rust path is used~~ (DONE)
3. ~~Bash script removed entirely — Rust binary is sole implementation~~ (DONE)
4. ~~Updated format_discord_message() to use per-item detail lines with markdown dashes~~ (DONE)
5. Consider using Discord embeds (richer formatting) instead of plain content in a future iteration

Status: Complete. Format matches desired spec. Verify on next run.
