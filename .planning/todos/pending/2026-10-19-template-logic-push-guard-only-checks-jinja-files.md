---
created: 2026-10-19T20:09:59.000Z
title: Template-logic push guard only checks .jinja files
area: general
severity: major
files:
  - src/sync/mod.rs
---

## Problem

Phase 01 re-review warning W3 (.planning/phases/01-sync-engine-state-hermeticity/01-REVIEW-FIXES.md).
The CR-02 push guard (3c9e79a) refuses to overwrite templates with logic only when the upstream
file is `.jinja`, but status and pull render any template containing `{{`. Reproduced by the
reviewer: a `{{ git_email }}` placeholder added to `gitconfig.repo` was replaced upstream by
the rendered `developer@example.com` on push. Latent: no shipped .repo or hook template contains
`{{` yet.

## Solution

Apply the same template-logic guard to every upstream file the render path treats as a template
(anything rendering would change), not only `.jinja`. Test-first with the reviewer's repro plus a
plain-.repo negative control that still publishes.
