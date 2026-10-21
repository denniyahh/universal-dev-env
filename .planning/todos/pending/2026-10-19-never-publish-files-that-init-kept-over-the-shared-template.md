---
created: 2026-10-19T18:23:46.501Z
title: Never publish files that init kept over the shared template
area: general
severity: major
files:
  - src/main.rs
  - src/sync/mod.rs
  - src/sync/state.rs
---

## Problem

After the phase 01 CR-03 fix (commit 984f0c7), a pre-existing custom `.gitignore`
that `ude init` keeps is recorded with the template as its ancestor, so it reads as
Locally Modified. Pull no longer silently overwrites it (the CR-03 bug), but the next
plain `ude sync push` publishes that one project's `.gitignore` as the shared
`gitignore.repo` template for every project. The executor confirmed this in a scratch
run: after push, upstream `gitignore.repo` began with `custom_rule_one/`.
`ude sync status` shows "Locally Modified (ready to push)" beforehand, and an upstream
`git revert` undoes it, but nothing stops it.

The operator accepted this for phase 01 on 2026-10-19 and asked for a later fix.

## Solution

Mark files that `ude init` kept as project-only overrides (a new state class) that push
skips with a message and exit 1, following the D-10 skip-reporting pattern. Pull should
still merge upstream template changes into them or produce a conflict sidecar, never
silently replace them. Test-first: an e2e running init over a custom .gitignore, then push,
asserting upstream `gitignore.repo` stays byte-identical, with a negative control that an
ordinary local edit still publishes.
Source: phase 01 code-review fix report; also logged in
.planning/phases/01-sync-engine-state-hermeticity/deferred-items.md.

## Update 2026-10-21

The same hole exists when the template is absent: an uncommitted pre-existing
`.gitignore` with no `gitignore.repo` records nothing at init, reads as
"Locally Modified (ready to push)", and push creates the template from it (reproduced on
HEAD and pre-fix code; external adversarial review, agy and pi lanes).
`test_init_keeps_uncommitted_gitignore_when_template_is_absent` stops at pull and never
runs push, so it does not catch this; add a push assertion when fixing.
Committed kept files with no template, and uncommitted non-.gitignore managed files with
no template, are already left Unmanaged (539a3d7, b5c11a3).
