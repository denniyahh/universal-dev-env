---
created: 2026-10-25T00:00:00.000Z
title: push_updates scans with the host home, not its user_home argument
area: sync
severity: minor
files:
  - src/sync/mod.rs
---

## Problem

`push_updates` builds `dummy_ctx` with `TemplateContext::detect_for_project` (src/sync/mod.rs:1330),
which reads the real host home, and passes it to `scan_project`. The function also takes explicit
`user_home` / `xdg_config` arguments. In production the two are equal; in tests and any caller that
passes a different home, upstream templates are rendered with the host home while local files use
the argument. Raised by the agy lane of the 2026-10-25 Phase 2 plan review
(.planning/reviews/phase-02-plans-adversarial-2026-10-25/); host classified it as pre-existing
Phase 1 behaviour, not a Phase 2 plan defect.

## Solution

Build the context from the arguments once at the top of `push_updates` and use it for both
`scan_project` and plan 02-08's render-back gate. Test: a push with `user_home` different from the
host home and an upstream template containing `{{ user.home }}` reports no drift.
