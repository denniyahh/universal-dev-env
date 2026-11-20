# Phase 3: Doctor & CLI Hardening - Discussion Log

> **Audit trail only.** Not consumed by downstream agents. Decisions live in 03-CONTEXT.md.

**Date:** 2026-11-20
**Phase:** 03-doctor-cli-hardening
**Areas presented:** Exit-code mapping, SSH key strictness, TypeSafe fail-closed, Include & monorepo

## Area selection
| Question | Options | Selected |
|---|---|---|
| Which areas to discuss? | Exit-code mapping / SSH key strictness / TypeSafe fail-closed / Include & monorepo | "you decide" |
| Fold overlapping todos? | Monorepo init hooks / Init-kept overrides / Neither | "you decide" |

**Outcome:** The operator delegated all four areas and the todo triage. Claude decided D-01…D-21 and
folded the doctor half of the monorepo todo. The reasoning for each is in 03-CONTEXT.md.

## Deferred Ideas
- A user.signingkey ↔ allowed_signers cross-check.
- Init in monorepo subdirectories; init-kept override files (both remain todos).
