---
created: 2026-10-25T00:00:00.000Z
title: Add a clean PR-branch cutter for phase work
area: tooling
severity: minor
files:
  - scripts/check-personal-artifacts.sh
---

## Problem

Since the 2026-10-25 git-flow setup, phase work happens on planning-bearing branches
(`workspace/denniyahh`, `gsd/phase-NN-…`), but `develop` only accepts branches with no
`.planning/` or `GEMINI.md` in tree or history (pre-push guard + `hygiene` workflow, required
check). Phase 1 was landed by a one-off history rewrite. Phase 2 will need a repeatable way to
extract its code commits onto a clean `feature/phase-02` branch.

## Solution

Port devflow's `scripts/cut-pr-branch.sh` (phase mode: replay commits scoped to the phase onto a
branch cut from `origin/develop`, drop personal paths, print a fidelity report), or a
`git filter-branch --index-filter` + re-sign recipe like the one used on 2026-10-25. Verify with
`scripts/check-personal-artifacts.sh <tip> origin/develop..<tip>` before pushing.
