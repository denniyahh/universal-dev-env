---
created: 2026-10-21T19:57:07.994Z
title: Triage the five info findings from the first phase 01 code review
area: general
severity: minor
files:
  - src/sync/mod.rs
  - src/main.rs
  - mise.toml
---

## Problem

The first phase 01 code review (01-REVIEW.md at 97fe52a) reported five info findings that were
never triaged. The 2026-10-21 re-review reused the IDs IN-01..IN-07 for new findings, so the
disposition ledger no longer has rows for these; this todo keeps them tracked.

- **IN-01 (needs an operator decision):** a local deletion of a templated file (for example
  mise.toml) makes push delete the shared `.jinja` template every project renders from. Status
  and init warn, and D-14 specifies delete-on-push. Decide: apply the template-logic gate to
  deletions too, or record in D-14 that templated sources are intentionally deletable.
- **IN-02:** init's managed-file list duplicates `sync::MANAGED_FILES`; build it from that constant.
- **IN-03:** when init's scan fails, the missing-file warning predicts "push will delete" instead of
  "push outcome unknown"; `missing_tracked_files` follows links (`fs::metadata`).
- **IN-04:** `template_has_logic`, `check_for_override` and `detect_for_project`'s state read follow
  symlinks while state loading refuses them; route through one policy.
- **IN-05:** no committed `mise.lock` for the pinned gitleaks / cargo-deny; `install-hooks` silently
  overrides a global `core.hooksPath`.

## Solution

Get the IN-01 decision first. Then fix IN-02..IN-05 test-first (each finding's "Fix" line in the
97fe52a report is a starting point); IN-05 (a) is `mise lock` plus a commit.

## Update 2026-10-21

IN-01 is done: the operator chose to block it, and 65b9931 makes push skip deleting a template
that has logic (D-14 amended). IN-02..IN-05 remain.
