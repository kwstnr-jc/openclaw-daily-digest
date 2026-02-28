#!/usr/bin/env bash
set -euo pipefail
f="${1:-}"
[[ -n "$f" && -f "$f" ]] || { echo "ambiguous"; exit 0; }
txt="$(tr '[:upper:]' '[:lower:]' < "$f")"
if grep -Eq "(sudo|launchctl|network|iptables|pfctl|sysctl)" <<<"$txt"; then echo "ops"; exit 0; fi
if grep -Eq "(implement|fix|code|repo|script|cli|github|git)" <<<"$txt"; then echo "repo-change"; exit 0; fi
if grep -Eq "(research|compare|summarize|investigate)" <<<"$txt"; then echo "research"; exit 0; fi
echo "ambiguous"
