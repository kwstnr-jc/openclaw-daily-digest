# Orchestrator ↔ AI Interface Specification

**Version:** 1.0.0

## Overview

The orchestrator is a deterministic state machine that owns all file I/O,
state transitions, and error handling. AI (OpenClaw) is invoked as a pure
function: given structured input, return structured JSON. The orchestrator
never depends on AI being available or reliable.

## Input Payload Schema

The orchestrator sends a prompt string to OpenClaw containing:

```json
{
  "version": "1.0.0",
  "task_text": "string — full text of the inbox item (first 200 lines)",
  "metadata": {
    "source_file": "string — original filename",
    "timestamp": "string — ISO 8601 (YYYY-MM-DDTHH:MM)",
    "file_path": "string — absolute path to inbox file"
  },
  "hints": {
    "project": "string | null — explicit Project: line if present",
    "tags": ["string — any #tag/value tokens found in the text"]
  }
}
```

### Field Rules

| Field | Required | Notes |
|-------|----------|-------|
| `version` | Yes | Semver; bump on breaking schema changes |
| `task_text` | Yes | First 200 lines of the inbox `.md` file |
| `metadata.source_file` | Yes | Basename only, no path |
| `metadata.timestamp` | Yes | Processing timestamp, not file mtime |
| `metadata.file_path` | Yes | Full path for audit trail |
| `hints.project` | No | Extracted from `Project: <name>` line |
| `hints.tags` | No | Extracted from `#tag/value` patterns |

## AI Output Schemas

### a) Enrichment (current — v1.0)

```json
{
  "planned_actions": ["string"],
  "clarifying_questions": ["string"],
  "next_step": "string"
}
```

### b) Classification (v1.1 — planned)

```json
{
  "project": {
    "kind": "existing" | "new" | "none",
    "name": "string | null"
  },
  "confidence": 0.0,
  "rationale": "string"
}
```

### c) Action Type (v1.2 — planned)

```json
{
  "action_type": "repo-change" | "research" | "ops" | "question" | "note",
  "confidence": 0.0,
  "rationale": "string",
  "suggested_repo": "string | null"
}
```

## Error Handling Rules

1. **AI call fails (timeout, non-zero exit, network error):**
   Continue with fallback enrichment. Mark item as `[unenriched]`.
   Item still moves to `Processed/`, not `Failed/`.

2. **AI returns invalid JSON:**
   Same as failure — use fallback, mark `[unenriched]`.

3. **IO error (cannot read input, write outbox, write log, move file):**
   Move item to `Failed/`. Exit non-zero.

4. **No inbox items:**
   Exit 0 with message. No files created.

5. **Principle:** AI failure is degraded mode, not an error.
   Only real I/O failures constitute errors.

## Versioning Strategy

- The `version` field in the input payload uses semantic versioning.
- **Patch** (1.0.x): Documentation, comment, or cosmetic changes.
- **Minor** (1.x.0): New optional fields added. Old consumers unaffected.
- **Major** (x.0.0): Required field changes or removal. Requires orchestrator update.
- The envelope.json always records which schema version produced it.
- Old envelope files remain valid; the orchestrator never re-processes them.

## Envelope File

Every run produces an envelope file alongside the digest report:

```
Outbox/<timestamp>-<slug>.envelope.json
```

Contents:

```json
{
  "version": "1.0.0",
  "timestamp": "YYYY-MM-DDTHH:MM",
  "source_file": "original-filename.md",
  "task_text": "full original task text",
  "classification": null,
  "planning": null,
  "enrichment": {
    "planned_actions": [],
    "clarifying_questions": [],
    "next_step": ""
  },
  "execution": {},
  "status": "enriched" | "unenriched" | "failed"
}
```

The envelope is the single source of truth for what happened during processing.
Later phases populate `classification`, `planning`, and `execution` fields.
