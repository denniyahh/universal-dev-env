---
created: 2026-10-21T16:37:58.391Z
title: Make init refusals happen before any file is written
area: general
severity: minor
files:
  - src/main.rs:230
  - src/main.rs:333
---

## Problem

Init's symlink pre-flight refuses before touching anything, but other refusals happen
mid-way: an unreadable `.gitignore` exits 2 after `.gitconfig` and scaffolding were
written (reproduced 2026-10-21, also on pre-fix code with exit 1), and the new
"Cannot read committed X" refusal (b5c11a3) runs after the writes too. The pre-flight
comment ("Every path init may create or write is checked before anything is touched")
covers symlinks only. Exit codes for "init could not proceed" are also mixed (1 for
template/git failures, 2 for the newer refusals).
Source: 2026-10-21 external adversarial review (pi lane), host-verified.

## Solution

Move every read init depends on (existing .gitignore, kept committed files) into the
pre-flight, so any refusal leaves the project untouched; then settle one exit code for
init refusals. Test-first: extend test_init_refuses_unreadable_gitignore and
test_init_refuses_unreadable_committed_file_without_template to assert no managed file
was written, mirroring the symlink refusal tests.

## Update 2026-10-21

The exit-code half is fixed: 7e44cf1 makes every init failure exit 2. The ordering half (reads
init depends on happen after writes) remains; the no-templates refusal (464bfc4) already runs
before any write.
