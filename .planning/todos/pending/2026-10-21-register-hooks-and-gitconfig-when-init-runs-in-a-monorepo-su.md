---
created: 2026-10-21T16:37:58.391Z
title: Register hooks and gitconfig when init runs in a monorepo subdirectory
area: general
severity: major
files:
  - src/main.rs:467
  - src/audit/mod.rs:199
---

## Problem

Init step 3 (`include.path`, `core.hooksPath`, `update-index --chmod=+x`) is gated on
`project_path.join(".git").exists()`. In a monorepo subdirectory there is no `.git` of
its own, so none of it runs, yet init prints "Configured tracked .gitconfig & chained
scripts/hooks" and exits 0. `ude doctor --repo <subdir>` then fails with "is not a git
repository" (same `.git` check). 8e7c6dd made committed-file detection monorepo-aware,
which makes this case look supported while the hooks it keeps are never active.

Reproduced 2026-10-21 on HEAD and on the pre-fix code (pre-existing): git init at
`mono/`, init `mono/svc` → `git -C mono config --get core.hooksPath` unset.
Source: 2026-10-21 external adversarial review (agy, pi lanes), host-verified.

## Solution

Decide the design first: in a monorepo, `core.hooksPath` and `include.path` are
repo-wide, so pointing them at one subproject's hooks affects every project in the repo.
Options: register relative to the repo root (`git rev-parse --show-toplevel`) only when
the subproject is the only ude project; or skip with an explicit warning instead of the
success line. Either way, the success line must reflect what ran, and doctor must use the
same repo detection. Test-first: monorepo init asserts the chosen outcome; negative
control: a project with its own .git still registers.
