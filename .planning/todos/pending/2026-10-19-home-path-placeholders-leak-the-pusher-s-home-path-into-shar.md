---
created: 2026-10-19T20:09:59.000Z
title: Home-path placeholders leak the pusher's home path into shared templates
area: general
severity: minor
files:
  - src/sync/mod.rs
---

## Problem

Phase 01 re-review warning W2 (.planning/phases/01-sync-engine-state-hermeticity/01-REVIEW-FIXES.md).
The CR-02 push guard (3c9e79a) exempts templates whose only constructs are the
`{{ user.home }}` / `{{ user.xdg_config }}` placeholders, because push writes those back.
But when a placeholder is followed by `,` `;` `.` `)` or `}`, push does not reverse the
substitution, so the pusher's absolute home path is published into the shared template.
Reproduced by the reviewer with a modified template; a placeholder followed by `/` on a
neighbouring line round-tripped correctly. Latent: no shipped template uses these placeholders yet.

## Solution

Make the placeholder reversal delimiter-agnostic (match the rendered path as a whole token, not
only before `/`), or drop the exemption and refuse such templates like other logic templates.
Test-first with the reviewer's repro plus a `/`-followed negative control.
