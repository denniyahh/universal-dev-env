---
created: 2026-10-21T17:34:53.965Z
title: Make init and sync writes refuse a symlink created mid-run
area: general
severity: minor
files:
  - src/main.rs:230
  - src/main.rs:1482
---

## Problem

Init's symlink pre-flight is a one-time check. Every later write (`copy_template_file`,
`render_and_write`, `fs::write`, `create_dir_all`) follows links, so a symlink created in
the project while init runs is written through. Confirmed 2026-10-21 on c868d1b: a FIFO
template stalls init after the pre-flight, `.agents/AGENTS.md -> <outside file>` is
planted, and on release the outside file is overwritten with the AGENTS.md render.
Control: with no symlink planted the outside file is unchanged. (Both runs end in exit
124 because init reads the template a second time and the FIFO is then empty; the
write happens before that.) Reported by the codex lane of the 2026-10-21 external
adversarial review; host-reproduced.

Exploiting it needs a concurrent process that can write into the project directory. A
symlink committed in a cloned repo is still refused. Sync's staged writes probably have
the same window between validation and rename (not checked).

Operator decision 2026-10-21: defer to Phase 2 (Boundaries & Security Hardening).

## Solution

Open destinations relative to a directory handle without following links (cap-std, or
rustix openat with O_NOFOLLOW / openat2 RESOLVE_BENEATH on Linux, with a Windows story),
for init and for sync's StagedTransaction. Test-first, using the reproduction below as
the test; negative control: the same run without the planted symlink.

Reproduction (run from the repo after `cargo build`; argument `yes` plants the link,
`no` is the control):

```bash
#!/usr/bin/env bash
# Usage: toctou.sh <plant-symlink:yes|no>
set -uo pipefail
REPO=/var/home/denniyahh/Github/universal-dev-env
SP="${TMPDIR:-/tmp}"
T=$(mktemp -d "$SP/toctou.XXXX")
mkdir -p "$T/home" "$T/project/.agents"
cp -r "$REPO/templates" "$T/project/templates"
echo "OUTSIDE ORIGINAL" > "$T/outside"
TPL="$T/project/templates/projects/agents/agents.md.jinja"
cp "$TPL" "$T/agents.src"; rm "$TPL"; mkfifo "$TPL"
cd "$T/project"
env -u GIT_DIR -u GIT_WORK_TREE HOME="$T/home" XDG_CONFIG_HOME="$T/home/.config" UDE_NO_REMOTE=1 \
  timeout 60 "$REPO/target/debug/ude" init --path "$T/project" --type rust > "$T/init.log" 2>&1 &
PID=$!
# wait until init has opened the FIFO (blocked reading the template)
for _ in $(seq 100); do ls -l /proc/$PID/fd 2>/dev/null | rg -q "agents.md.jinja" && break; sleep 0.1; done
ls -l /proc/$PID/fd 2>/dev/null | rg -q "agents.md.jinja" && echo "init blocked on template read" || echo "init NOT blocked on fifo (pid alive: $(kill -0 $PID 2>/dev/null && echo y || echo n))"
[ "$1" = yes ] && { rm -f .agents/AGENTS.md; ln -s "$T/outside" .agents/AGENTS.md; echo "planted symlink"; }
cat "$T/agents.src" > "$TPL"
wait $PID; echo "init exit=$?"
echo "outside now: $(head -c 60 "$T/outside" | tr '\n' ' ')"
tail -3 "$T/init.log"
rm -rf "$T"
```
