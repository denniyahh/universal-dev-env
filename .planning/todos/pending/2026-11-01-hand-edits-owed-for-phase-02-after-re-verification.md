---
created: 2026-11-02T19:09:58.520Z
title: Hand edits owed for phase 02 after re-verification
area: planning
severity: major
files:
  - .planning/phases/02-sanitizer-boundaries-security-hardening/02-15-PLAN.md
---

## Problem

No workflow step triggers these edits. They are owed only after re-verification of phase 02 passes, and whoever runs them may start from a context reset.

## Solution

Do the hand edits in 02-15-PLAN.md, "Who updates which document, and when", once re-verification passes; if it finds gaps they wait.

- T-02-52 to `mitigate` naming T-02-62, and AR-02-10 marked superseded, in 02-SECURITY.md.
- Each risk accepted in `## Operator answers` written to 02-SECURITY.md once `/gsd-secure-phase` has created its row.
- The two lines of 02-VALIDATION.md.
- CR-200, CR-201, WR-200 and WR-203 to `fixed` and the note on IN-205 in 02-REVIEW-DISPOSITION.md.

These four apply whatever re-verification finds:

- If re-verification finds gaps, revert SEC-02 and SEC-04 in REQUIREMENTS.md.
- No phase.complete before re-verification passes.
- At re-verification, re-run each control row the SUMMARY's ## Control table lists after "Re-run at re-verification:", as "Who updates which document" says.
- Any of T-02-60, T-02-61, T-02-64, T-02-65 and T-02-67 whose Status cell in 02-15-PLAN.md's ## Operator answers still says pending still needs the operator's answer: ask again, and list it as still open in each report until it is answered or the operator drops it.

When these are done, move this todo to completed and run gsd_run query state.resolve-blocker --text "Hand edits owed after re-verification".
