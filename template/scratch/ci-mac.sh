#!/usr/bin/env bash
set -euo pipefail

# Configuration
REMOTE="${GSD_CI_REMOTE:-mac}"
REMOTE_DIR="${GSD_CI_REMOTE_DIR:-~/builds/gsd-core}"
LOCAL_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Quick connectivity probe (1 second timeout)
if ! ssh -T -q -o ConnectTimeout=2 -o BatchMode=yes "$REMOTE" exit 0; then
  echo "❌ Error: Cannot connect to Mac remote '$REMOTE' via SSH." >&2
  echo "Please verify that the Mac is awake and connected to the local network." >&2
  exit 1
fi

# Ensure remote directory exists
ssh -T "$REMOTE" "mkdir -p $REMOTE_DIR/.cache"

# Fast LAN rsync of the working tree. .git is deliberately EXCLUDED and
# handled separately below: when LOCAL_ROOT is a linked git worktree (not
# the main clone), its .git is a pointer FILE to this machine's own
# .git/worktrees/<name> directory, which cannot resolve on a remote host —
# rsyncing it verbatim leaves the remote with a git repo that fails on
# every git invocation ("fatal: not a git repository: ...").
rsync -a --delete \
  --filter='P /node_modules' \
  --filter='P /gsd-core/bin/lib' \
  --filter='P /.cache' \
  --filter='P /*.tsbuildinfo' \
  --filter='P /.git' \
  --filter='- /node_modules' \
  --filter='- /.local' \
  --filter='- /.cache' \
  --filter='- /*.tsbuildinfo' \
  --filter='- /.git' \
  "$LOCAL_ROOT/" "$REMOTE:$REMOTE_DIR/"

# Give the remote copy real, working git history — matching this worktree's
# current branch/commit and remotes — without disturbing the files rsync
# just wrote. `reset --mixed` (not --hard) moves the branch ref and index to
# the target commit but leaves the working tree alone, so any uncommitted
# local changes rsync carried over survive as a diff instead of being wiped.
#
# History travels as a git bundle, not a fetch from origin: a contribution
# branch worked from a linked worktree is routinely local-only (unpushed), so
# `git fetch origin <branch>` fails outright when origin has never seen it.
# A bundle of everything reachable from HEAD is self-contained and correct
# whether or not the branch exists on any remote.
LOCAL_BRANCH="$(git -C "$LOCAL_ROOT" rev-parse --abbrev-ref HEAD)"
ORIGIN_URL="$(git -C "$LOCAL_ROOT" remote get-url origin)"
UPSTREAM_URL="$(git -C "$LOCAL_ROOT" remote get-url upstream 2>/dev/null || true)"

BUNDLE_LOCAL="$(mktemp)"
BUNDLE_REFS=("HEAD")
if [ -n "$UPSTREAM_URL" ] && git -C "$LOCAL_ROOT" rev-parse --verify upstream/next >/dev/null 2>&1; then
  BUNDLE_REFS+=("upstream/next")
fi
git -C "$LOCAL_ROOT" bundle create -q "$BUNDLE_LOCAL" "${BUNDLE_REFS[@]}"
REMOTE_BUNDLE=/tmp/gsd-ci-git.bundle
scp -q "$BUNDLE_LOCAL" "$REMOTE:$REMOTE_BUNDLE"

# Built as a real script and shipped as a file (not an inline ssh command
# string): the remote's login shell is fish, which cannot parse the
# bash/zsh quoting ($'...' escapes, nested single-quotes) an inline
# multi-line command would need — a file sidesteps that entirely.
REMOTE_GIT_SCRIPT_LOCAL="$(mktemp)"

# `node --test` re-execs itself as a per-file worker, carrying a large
# internal flag set (--test-isolation=process --test-timeout=0 among them) —
# that's how it hands the resolved option set to the child. gsd-core's own
# bounded-timeout guard test (tests/prohibition-enforcement.test.cjs:685,
# tracked as gsd-core#3660) kills the direct child on timeout but not this
# re-exec'd worker, so a hung fixture inside it survives forever, reparented
# to PID 1, spinning at ~95-99% CPU. `npm test` runs that check every time,
# so every remote CI run leaks one. Confirmed on this Mac and independently
# on the maintainer's own machine (macOS, #3660's original report) and this
# author's Linux worktree (2026-08-22, three orphans, ~20.5 CPU-hours before
# being found and killed) — same defect, three machines.
#
# Sweep on every exit path (success, failure, or this script erroring out
# earlier via `set -e`), not just after a passing run — a leaked worker
# doesn't care whether the run it came from was green. Matched on BOTH flags
# together so this can't catch an ordinary developer-invoked `node --test`.
# Plain SIGTERM is sufficient (verified: none of the reproductions above
# needed SIGKILL — the busy-loop never installs a handler that swallows it).
#
# The pattern below brackets one character in each flag name
# (isolatio[n], timeou[t]). Without that, `pgrep -f`'s search text is embedded
# verbatim in the argv of the process running this cleanup, and matches
# ITSELF — confirmed live: an earlier unbracketed version killed its own
# remote shell before the real sweep ran. A bracketed character class still
# matches the literal character in a genuine leaked worker's argv, but breaks
# the contiguous-substring self-match against this script's own source text
# (same trick as gsd-core#3660's own repro, which uses `prohib[-]hang` for
# the identical reason).
#
# This runs as a FILE (`sh /tmp/....sh`), not an inline `ssh host sh -c
# '<script>'`. Confirmed live that inline fails: ssh does not preserve the
# local shell's quoting when it forwards a multi-word remote command — it
# space-joins the trailing argv into ONE string and sends that, so the
# quotes protecting the multi-line body vanish before the remote's login
# shell (fish) ever parses it, and fish then chokes on the bare `pids=$(...)`
# inside ("Unsupported use of '='") — the exact failure mode the existing
# REMOTE_GIT_SCRIPT_LOCAL bootstrap above already works around, for the same
# reason. A file sidesteps it: `ssh host sh /path` has only one argument
# after the interpreter, so there is nothing left for naive space-joining to
# corrupt.
REMOTE_CLEANUP_SCRIPT_LOCAL="$(mktemp)"
cat > "$REMOTE_CLEANUP_SCRIPT_LOCAL" <<CLEANUP
pids=\$(pgrep -f "test-isolatio[n]=process.*test-timeou[t]=0" 2>/dev/null || true)
runner_pids=\$(pgrep -f "builds/gsd-core.*node.*scripts/run-test[s]" 2>/dev/null || true)
all_pids=\$(echo "\$pids \$runner_pids" | tr -s ' ' '\n' | sort -u | grep -v '^\$' || true)
if [ -n "\$all_pids" ]; then
  echo "🧹 killing leaked test-runner worker(s) on $REMOTE: \$all_pids" >&2
  kill -TERM \$all_pids 2>/dev/null || true
  sleep 1
  surviving_pids=\$(echo "\$all_pids" | while read -r pid; do kill -0 "\$pid" 2>/dev/null && echo "\$pid"; done || true)
  if [ -n "\$surviving_pids" ]; then
    echo "🚨 escalating to SIGKILL for unresponsive worker(s) on $REMOTE: \$surviving_pids" >&2
    kill -9 \$surviving_pids 2>/dev/null || true
  fi
fi
CLEANUP

REMOTE_RUN_SCRIPT_LOCAL="$(mktemp)"

cleanup_remote_test_runners() {
  ssh -T "$REMOTE" "cat > /tmp/gsd-ci-cleanup.sh" < "$REMOTE_CLEANUP_SCRIPT_LOCAL" 2>&1 || true
  ssh -T "$REMOTE" sh /tmp/gsd-ci-cleanup.sh 2>&1 || true
  rm -f "$REMOTE_GIT_SCRIPT_LOCAL" "$BUNDLE_LOCAL" "$REMOTE_CLEANUP_SCRIPT_LOCAL" "$REMOTE_RUN_SCRIPT_LOCAL"
}
trap cleanup_remote_test_runners EXIT

# Run pre-flight sweep before starting new CI run
ssh -T "$REMOTE" "cat > /tmp/gsd-ci-cleanup.sh" < "$REMOTE_CLEANUP_SCRIPT_LOCAL" 2>&1 || true
ssh -T "$REMOTE" sh /tmp/gsd-ci-cleanup.sh 2>&1 || true

{
  echo "set -e"
  echo "cd $REMOTE_DIR"
  echo "if ! git rev-parse --git-dir >/dev/null 2>&1; then"
  echo "  echo '🔧 No usable git repo on remote — initializing'"
  echo "  rm -rf .git"
  echo "  git init -q"
  echo "fi"
  echo "git remote set-url origin '$ORIGIN_URL' 2>/dev/null || git remote add origin '$ORIGIN_URL'"
  if [ -n "$UPSTREAM_URL" ]; then
    echo "git remote set-url upstream '$UPSTREAM_URL' 2>/dev/null || git remote add upstream '$UPSTREAM_URL'"
  fi
  if [ -n "$UPSTREAM_URL" ] && git -C "$LOCAL_ROOT" rev-parse --verify upstream/next >/dev/null 2>&1; then
    echo "git fetch --quiet '$REMOTE_BUNDLE' HEAD '+refs/remotes/upstream/next:refs/remotes/upstream/next' '+refs/remotes/upstream/next:refs/remotes/origin/next'"
  else
    echo "git fetch --quiet '$REMOTE_BUNDLE' HEAD"
  fi
  echo "git reset --mixed --quiet FETCH_HEAD"
  echo "git branch -M '$LOCAL_BRANCH'"
  echo "git add -A"
  echo "rm -f '$REMOTE_BUNDLE'"
} > "$REMOTE_GIT_SCRIPT_LOCAL"

ssh -T "$REMOTE" "cat > /tmp/gsd-ci-git-adopt.zsh" < "$REMOTE_GIT_SCRIPT_LOCAL"
ssh -T "$REMOTE" zsh /tmp/gsd-ci-git-adopt.zsh

# Compute local package-lock.json hash
LOCKFILE_HASH="$(sha256sum "$LOCAL_ROOT/package-lock.json" | cut -d' ' -f1)"

# Command to execute
if [ $# -gt 0 ]; then
  COMMAND="$*"
else
  # The Mac runner executes all test chunks serially. Its timing table can lag
  # behind expensive state/property suites, which packed state.test.cjs with
  # enough peers to exceed run-tests' fixed 600s per-chunk cap. Halve the
  # chunk-weight budget here without changing the repository default; callers
  # may tune it further through the local GSD_CI_RUN_TESTS_MAX_FILES_PER_CHUNK.
  TEST_CHUNK_WEIGHT="${GSD_CI_RUN_TESTS_MAX_FILES_PER_CHUNK:-30}"
  COMMAND="npm run check:env && npm run build && npm run lint:ci && RUN_TESTS_MAX_FILES_PER_CHUNK=${TEST_CHUNK_WEIGHT} npm test"
fi

# Execute on Mac in a hermetic clean-room environment (Option 1).
#
# Hermeticity guarantees:
# 1. Non-interactive SSH (`ssh -T`) with no TTY allocation: `process.stdout.isTTY`
#    evaluates to undefined/false, faithfully matching headless GitHub Actions CI.
# 2. Non-login shell invocation: sources NO personal dotfiles (~/.zshrc, ~/.zprofile).
# 3. `env -i`: Completely strips the environment so no personal API keys (Tavily,
#    Exa, Firecrawl, Google, Anthropic, etc.) or editor/pager configs leak into tests.
# 4. Scratch HOME directory (`/tmp/gsd-ci-sandbox.XXXXXX`) isolates ~/.gsd, ~/.config,
#    ~/.gitconfig, and runtime configuration. Cleaned up on exit.
# 5. Git config blanking: GIT_CONFIG_NOSYSTEM=1, GIT_CONFIG_GLOBAL=/dev/null,
#    GIT_CONFIG_SYSTEM=/dev/null, and dummy CI author/committer identities.
# 6. Persistent build cache: npm cache is scoped to `$RESOLVED_REMOTE_DIR/.cache/npm`
#    and lockfile hash gating skips redundant `npm ci` runs without touching ~/.npm.
cat > "$REMOTE_RUN_SCRIPT_LOCAL" <<PREAMBLE
REMOTE_DIR_INPUT="$REMOTE_DIR"
LOCKFILE_HASH="$LOCKFILE_HASH"
COMMAND=\$(cat <<'CMD_EOF'
$COMMAND
CMD_EOF
)
PREAMBLE

cat >> "$REMOTE_RUN_SCRIPT_LOCAL" <<'RUNSCRIPT'
set -euo pipefail

RESOLVED_REMOTE_DIR="${REMOTE_DIR_INPUT/#\~/$HOME}"
RESOLVED_REMOTE_DIR="$(cd "$RESOLVED_REMOTE_DIR" && pwd)"
cd "$RESOLVED_REMOTE_DIR"

mkdir -p "$RESOLVED_REMOTE_DIR/.cache/npm"
CACHED_HASH_FILE="$RESOLVED_REMOTE_DIR/.cache/gsd-core-pkg-lock.sha256"

NODE_BIN="$(dirname "$(/usr/local/opt/mise/bin/mise which node 2>/dev/null || mise which node 2>/dev/null || which node)")"
SCRATCH_HOME="$(mktemp -d /tmp/gsd-ci-sandbox.XXXXXX)"
trap 'rm -rf "$SCRATCH_HOME"' EXIT

clean_exec() {
  local cmd="$1"
  env -i \
    HOME="$SCRATCH_HOME" \
    TMPDIR="/tmp" \
    PATH="$NODE_BIN:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin" \
    USER="ci" \
    CI="true" \
    TERM="dumb" \
    LANG="en_US.UTF-8" \
    LC_ALL="en_US.UTF-8" \
    GSD_EMITTED_BASE="upstream/next" \
    GSD_AFFECTED_BASE="upstream/next" \
    MISE_YES="1" \
    MISE_DATA_DIR="$HOME/.local/share/mise" \
    MISE_CONFIG_DIR="$HOME/.config/mise" \
    npm_config_cache="$RESOLVED_REMOTE_DIR/.cache/npm" \
    GIT_CONFIG_NOSYSTEM="1" \
    GIT_CONFIG_GLOBAL="/dev/null" \
    GIT_CONFIG_SYSTEM="/dev/null" \
    GIT_AUTHOR_NAME="CI" \
    GIT_AUTHOR_EMAIL="ci@example.com" \
    GIT_COMMITTER_NAME="CI" \
    GIT_COMMITTER_EMAIL="ci@example.com" \
    sh -c "cd '$RESOLVED_REMOTE_DIR' && $cmd"
}

if [ ! -d node_modules ] || [ ! -f "$CACHED_HASH_FILE" ] || [ "$(cat "$CACHED_HASH_FILE" 2>/dev/null)" != "$LOCKFILE_HASH" ]; then
  echo "🔄 Lockfile change or clean install detected. Running npm ci on Mac..."
  clean_exec "npm ci --silent"
  echo "$LOCKFILE_HASH" > "$CACHED_HASH_FILE"
fi

# rsync intentionally preserves the emitted runtime and incremental state so
# dependency installation is cached. Source changes otherwise leave tsc with a
# cache that can claim the old emitted CJS is current (#4316 Mac CI). Invalidate
# only the rebuildable incremental state, then compile the synced source once.
rm -f tsconfig.build.tsbuildinfo
clean_exec "npm run build:lib --silent"

echo "🚀 Running CI command in clean-room environment on Mac: $COMMAND"
clean_exec "$COMMAND"
RUNSCRIPT

ssh -T "$REMOTE" "cat > /tmp/gsd-ci-run.zsh" < "$REMOTE_RUN_SCRIPT_LOCAL"
ssh -T "$REMOTE" zsh /tmp/gsd-ci-run.zsh
