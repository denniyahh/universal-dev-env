#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -lt 3 ]; then
  echo "Usage: mise run start:wt <type> <issue-number> <slug>" >&2
  echo "Example: mise run start:wt fix 3158 branch-guard" >&2
  exit 1
fi

TYPE="$1"
ISSUE="$2"
SLUG="$3"
BRANCH="${TYPE}/${ISSUE}-${SLUG}"
ROOT="$(git rev-parse --show-toplevel)"
PARENT_DIR="$(dirname "$ROOT")"
WORKTREE_DIR="${PARENT_DIR}/gsd-core-${SLUG}"

copy_required_file() {
  local source="$1"
  local destination="$2"

  if [ ! -f "$source" ]; then
    echo "❌ Error: Required personal file is missing: $source" >&2
    exit 1
  fi
  mkdir -p "$(dirname "$destination")"
  cp "$source" "$destination"
}

copy_required_directory() {
  local source="$1"
  local destination="$2"

  if [ ! -d "$source" ]; then
    echo "❌ Error: Required personal directory is missing: $source" >&2
    exit 1
  fi
  mkdir -p "$destination"
  cp -R "$source"/. "$destination"/
}

echo "🔄 Fetching latest upstream/next..."
git fetch upstream next

if [ -d "$WORKTREE_DIR" ]; then
  echo "❌ Error: Target worktree directory already exists: $WORKTREE_DIR" >&2
  exit 1
fi

echo "🌿 Creating worktree at $WORKTREE_DIR on branch $BRANCH from upstream/next..."
git worktree add -b "$BRANCH" "$WORKTREE_DIR" upstream/next

echo "📦 Injecting personal workflow capabilities (mise.toml, scratch scripts, .agents/)..."
copy_required_file "$ROOT/mise.toml" "$WORKTREE_DIR/mise.toml"
copy_required_file "$ROOT/scratch/ci-mac.sh" "$WORKTREE_DIR/scratch/ci-mac.sh"
copy_required_file "$ROOT/scratch/check-workflow-budgets.sh" "$WORKTREE_DIR/scratch/check-workflow-budgets.sh"
copy_required_file "$ROOT/scratch/check-publish-boundary.sh" "$WORKTREE_DIR/scratch/check-publish-boundary.sh"
copy_required_file "$ROOT/scratch/sync-personal-env.sh" "$WORKTREE_DIR/scratch/sync-personal-env.sh"
copy_required_directory "$ROOT/.agents" "$WORKTREE_DIR/.agents"

if [ -d "$ROOT/.planning" ]; then
  echo "📋 Copying .planning environment from personal workspace..."
  copy_required_directory "$ROOT/.planning" "$WORKTREE_DIR/.planning"
fi

# Personal overrides must remain available locally but must never become part of
# a contribution commit. A tracked override is hidden from status in this
# worktree's private index; untracked personal files are covered by the push
# guard and remain unstaged unless explicitly added.
if git -C "$WORKTREE_DIR" ls-files --error-unmatch mise.toml >/dev/null 2>&1; then
  git -C "$WORKTREE_DIR" update-index --skip-worktree -- mise.toml
fi

# core.hooksPath is .githooks in this fork, so writing .git/hooks/pre-push is
# ineffective. Use worktree-specific hook storage and preserve the upstream
# pre-push hook before layering personal checks on top.
git -C "$WORKTREE_DIR" config extensions.worktreeConfig true
WORKTREE_GIT_DIR="$(git -C "$WORKTREE_DIR" rev-parse --git-dir)"
HOOK_DIR="${WORKTREE_GIT_DIR}/personal-hooks"
mkdir -p "$HOOK_DIR"
UPSTREAM_HOOKS="$(git -C "$WORKTREE_DIR" config --get core.hooksPath 2>/dev/null || true)"
if [ -z "$UPSTREAM_HOOKS" ]; then
  UPSTREAM_HOOKS="${WORKTREE_GIT_DIR}/hooks"
elif [[ "$UPSTREAM_HOOKS" != /* ]]; then
  UPSTREAM_HOOKS="${WORKTREE_DIR}/${UPSTREAM_HOOKS}"
fi
if [ -f "$UPSTREAM_HOOKS/pre-commit" ]; then
  cp "$UPSTREAM_HOOKS/pre-commit" "$HOOK_DIR/upstream-pre-commit"
fi
if [ -f "$UPSTREAM_HOOKS/pre-push" ]; then
  cp "$UPSTREAM_HOOKS/pre-push" "$HOOK_DIR/upstream-pre-push"
fi
git -C "$WORKTREE_DIR" config --worktree core.hooksPath "$HOOK_DIR"
cat > "$HOOK_DIR/pre-commit" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

HOOK_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [ -f "$HOOK_DIR/upstream-pre-commit" ]; then
  bash "$HOOK_DIR/upstream-pre-commit" "$@"
fi
EOF
cat > "$HOOK_DIR/pre-push" <<EOF
#!/usr/bin/env bash
set -euo pipefail

HOOK_DIR="\$(printf '%q' "$HOOK_DIR")"
WORKTREE_DIR="\$(printf '%q' "$WORKTREE_DIR")"
PUSH_INPUT="\$(mktemp "\$HOOK_DIR/push-input.XXXXXX")"
trap 'rm -f "\$PUSH_INPUT"' EXIT
cat > "\$PUSH_INPUT"

if [ -f "\$HOOK_DIR/upstream-pre-push" ]; then
  bash "\$HOOK_DIR/upstream-pre-push" "\$@" < "\$PUSH_INPUT"
fi
bash "\$WORKTREE_DIR/scratch/check-workflow-budgets.sh"
bash "\$WORKTREE_DIR/scratch/check-publish-boundary.sh" < "\$PUSH_INPUT"

# Reconcile worktree .planning and scratch changes back to personal workspace
if [ -x "\$WORKTREE_DIR/scratch/sync-personal-env.sh" ]; then
  bash "\$WORKTREE_DIR/scratch/sync-personal-env.sh" auto || true
fi
EOF
chmod +x "$HOOK_DIR/pre-commit" "$HOOK_DIR/pre-push"

echo "✅ Worktree initialized with Mac CI runner & personal workflow!"
echo "👉 Next steps:"
echo "   cd $WORKTREE_DIR"
echo "   mise run check"
