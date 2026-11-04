---
created: 2026-11-03T21:36:17.102Z
title: Add end-of-options before the clone URL in ensure_remote_repo
area: sync
severity: minor
files:
  - src/sync/mod.rs
---

## Problem

`ensure_remote_repo` runs `git clone --depth 1 -b <branch> <remote_url> <target>` with no `--` before `remote_url` (src/sync/mod.rs, the clone in the `!target_checkout.join(".git").exists()` branch). The URL can come from a project manifest, so a value beginning with `-` would be read as an option. Found by the phase 02 gap-closure planner (plan 02-17 decisions note) and confirmed by the plan checker by reading the argument list. Its effect has been reasoned, not run. The new `git ls-remote` and `git push` commands in plans 02-17 and 02-18 already use `--`.

## Solution

Insert `--` before `remote_url` (and `-b <branch>` before it). Add a test with a URL starting with `-` that is refused or treated as a path, with a control that clones normally.
