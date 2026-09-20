#!/usr/bin/env bash
# scratch/sync-personal-env.sh
# Bidirectional reconciliation between task worktrees and the personal workspace.
# Usage:
#   sync-personal-env.sh pull    (personal-workspace -> current worktree)
#   sync-personal-env.sh push    (current worktree -> personal-workspace)
#   sync-personal-env.sh auto    (hook mode: pushes changed planning/scratch files if in a worktree)

set -euo pipefail

MODE="${1:-pull}"
PERSONAL_WORKSPACE="${GSD_PERSONAL_WORKSPACE:-/var/home/denniyahh/Github/gsd-core-personal-workspace}"

if [ ! -d "$PERSONAL_WORKSPACE" ]; then
  echo "⚠️  Personal workspace not found at: $PERSONAL_WORKSPACE" >&2
  exit 0
fi

CURRENT_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"

# If invoked inside the personal workspace itself, nothing to sync against itself
if [ "$CURRENT_ROOT" = "$PERSONAL_WORKSPACE" ]; then
  if [ "$MODE" != "auto" ]; then
    echo "ℹ️  Already inside personal workspace ($PERSONAL_WORKSPACE). Nothing to sync."
  fi
  exit 0
fi

case "$MODE" in
  pull)
    echo "📥 Pulling personal environment (.planning, scratch, .agents, mise.toml)..."
    echo "   Source: $PERSONAL_WORKSPACE"
    echo "   Target: $CURRENT_ROOT"

    if [ -d "$PERSONAL_WORKSPACE/.planning" ]; then
      mkdir -p "$CURRENT_ROOT/.planning"
      rsync -au --info=NAME "$PERSONAL_WORKSPACE/.planning/" "$CURRENT_ROOT/.planning/"
    fi

    if [ -d "$PERSONAL_WORKSPACE/scratch" ]; then
      mkdir -p "$CURRENT_ROOT/scratch"
      rsync -au --info=NAME "$PERSONAL_WORKSPACE/scratch/" "$CURRENT_ROOT/scratch/"
    fi

    if [ -d "$PERSONAL_WORKSPACE/.agents" ]; then
      mkdir -p "$CURRENT_ROOT/.agents"
      rsync -au --info=NAME "$PERSONAL_WORKSPACE/.agents/" "$CURRENT_ROOT/.agents/"
    fi

    if [ -f "$PERSONAL_WORKSPACE/mise.toml" ]; then
      cp -u "$PERSONAL_WORKSPACE/mise.toml" "$CURRENT_ROOT/mise.toml"
    fi

    echo "✅ Pull complete."
    ;;

  push|auto)
    if [ "$MODE" = "push" ]; then
      echo "📤 Pushing worktree changes to personal workspace..."
      echo "   Source: $CURRENT_ROOT"
      echo "   Target: $PERSONAL_WORKSPACE"
    fi

    # Sync .planning
    if [ -d "$CURRENT_ROOT/.planning" ]; then
      mkdir -p "$PERSONAL_WORKSPACE/.planning"
      rsync -au --info=NAME "$CURRENT_ROOT/.planning/" "$PERSONAL_WORKSPACE/.planning/"
    fi

    # Sync scratch scripts (exclude temporary review outputs, diffs, and patches)
    if [ -d "$CURRENT_ROOT/scratch" ]; then
      mkdir -p "$PERSONAL_WORKSPACE/scratch"
      rsync -au --info=NAME \
        --exclude='*.patch' \
        --exclude='*.diff' \
        --exclude='push-input.*' \
        "$CURRENT_ROOT/scratch/" "$PERSONAL_WORKSPACE/scratch/"
    fi

    # Sync .agents if present
    if [ -d "$CURRENT_ROOT/.agents" ]; then
      mkdir -p "$PERSONAL_WORKSPACE/.agents"
      rsync -au --info=NAME "$CURRENT_ROOT/.agents/" "$PERSONAL_WORKSPACE/.agents/"
    fi

    if [ "$MODE" = "push" ]; then
      echo "✅ Push and reconciliation complete."
    else
      echo "🔄 [hook] Reconciled worktree .planning and scratch back to personal workspace."
    fi
    ;;

  *)
    echo "Usage: $0 [pull|push|auto]" >&2
    exit 1
    ;;
esac
