---
created: 2026-10-23T00:00:00.000Z
title: Recalibrate the entropy rule for 64-char base36 tokens
area: security
severity: minor
files:
  - src/sanitizer/entropy.rs
---

## Problem

Phase 02 research measured a gap that the D-05 calibration did not cover. Random tokens of
lowercase-or-uppercase letters plus digits (base36 shape) at exactly 64 characters are
blocked only about 16–20% of the time by the locked R11 constants. The same shape at 20–40
characters is blocked about 78–84%, and mixed-case Base64 at 64 characters about 99%.

The operator accepted this for Phase 2 on 2026-10-23 as risk T-02-07 in plan 02-03. Entropy is a
backstop; the vendored gitleaks rules (plan 02-05) are the primary detector. D-05 stays locked for
Phase 2.

## Solution

In a later phase, re-run the calibration with 64-char (and longer) base36 rows added, decide
whether the length-dependent threshold should change, and check the corpus control
(`templates/` + `template/` → 0 hits) still holds. Source: `02-RESEARCH.md` probe evidence and
plan 02-03 threat T-02-07.
