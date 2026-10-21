---
created: 2026-10-21T16:37:58.391Z
title: Refuse or resolve a symlinked project root in init
area: general
severity: major
files:
  - src/main.rs:563
  - src/sync/mod.rs:1963
---

## Problem

`refuse_symlink_under(root, path)` never checks `root` itself
(`take_while(|p| *p != root)`). `ude init --path link` with `link -> real` passes
init's symlink pre-flight, writes every file into `real`, then fails to save
`.ude/state.json` (`validate_safe_write_target` rejects the symlinked parent), prints
only a warning, and exits 0. The project is left with managed files and no state ledger.

Reproduced 2026-10-21 on HEAD and on the pre-fix code (pre-existing).
Source: 2026-10-21 external adversarial review (agy lane), host-verified.

## Solution

Canonicalize the project root once at the start of init (and sync) and use that path
everywhere, or refuse a symlinked root with exit 2 before writing. Separately, a failed
state.json save should not exit 0 (it also drops the unmanaged markers init just
decided). Test-first: init through a symlinked root asserts the chosen outcome and that
state.json exists or nothing was written; negative control: a plain directory.
