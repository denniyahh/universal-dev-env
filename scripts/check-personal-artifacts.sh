#!/usr/bin/env bash
# Refuse personal and agent artifacts on shared branches (main, develop, feature/*).
#
# Planning state and agent configuration live only on workspace/*, personal/*
# and gsd/* branches. This repository is public, so the shared branches must
# never carry them, in their tree or in any commit being added.
#
# Usage: check-personal-artifacts.sh <tip> [<rev-list range args>...]
#   <tip>   commit whose tree is checked
#   range   optional `git log` revision arguments naming the commits being
#           added (e.g. `base..tip`, or `tip --not --remotes=origin`)
# Exit: 0 clean, 1 artifacts found (listed on stderr), 2 usage or git error.
set -euo pipefail

PERSONAL_PATHS='^(\.agents|\.claude|\.codex|\.gemini|\.gsd|\.planning|\.mcp\.json|CLAUDE\.md|GEMINI\.md|skills)(/|$)'

if [ "$#" -lt 1 ]; then
    echo "usage: $0 <tip> [<rev-list range args>...]" >&2
    exit 2
fi
tip=$1
shift

tree_files=$(git ls-tree -r --name-only "$tip") || exit 2
history_files=""
if [ "$#" -gt 0 ]; then
    history_files=$(git log --format= --name-only "$@") || exit 2
fi

set +e
found=$(printf '%s\n%s\n' "$tree_files" "$history_files" | grep -E "$PERSONAL_PATHS" | sort -u)
status=$?
set -e

case "$status" in
    0)
        echo "refusing personal/agent artifacts on a shared branch:" >&2
        printf '%s\n' "$found" | sed 's/^/  /' >&2
        echo "  They belong on workspace/*, personal/* or gsd/* branches only." >&2
        exit 1
        ;;
    1) exit 0 ;;
    *) echo "check-personal-artifacts: grep failed (exit $status)" >&2; exit 2 ;;
esac
