#!/usr/bin/env bash
set -euo pipefail
REPO_DIR="$(cd "$(dirname "$0")/.." && pwd)"
exec "$REPO_DIR/tests/bats-core/bin/bats" "$REPO_DIR/tests/orchestrator.bats" "$@"
