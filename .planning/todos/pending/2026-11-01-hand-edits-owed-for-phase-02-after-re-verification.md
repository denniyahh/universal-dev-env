---
created: 2026-11-02T19:09:58.520Z
title: Hand edits owed for phase 02 after re-verification
area: planning
severity: major
files:
  - .planning/phases/02-sanitizer-boundaries-security-hardening/02-15-PLAN.md
  - .planning/phases/02-sanitizer-boundaries-security-hardening/02-21-PLAN.md
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

## Added by plan 02-21

The edits in this todo are owed by 02-15-PLAN.md's "Who updates which document, and when" and by 02-21-PLAN.md's `## Who updates which document`. They are still conditional on re-verification of phase 02 passing; if it finds gaps they wait.

- T-02-52 and AR-02-10 stay as written above. AR-02-10's note also records that the operator ruled on 2026-11-02 that its reasoning does not extend to CR-300 or CR-301.
- 02-SECURITY.md, Accepted Risks Log, once `/gsd-secure-phase` has created each threat row:
  - T-02-77: operator, 2026-11-02, widened 2026-11-03. Quote its Status cell in `## Operator answers` of 02-21-PLAN.md word for word: accepted by the operator 2026-11-02 for global `url.*.insteadOf` (the option recorded as "fix all four causes", whose text included "Global `url.*.insteadOf` stays open and would be recorded as a residual."); widened 2026-11-03 to every global redirect key. Question: "Two git settings in your GLOBAL config (~/.gitconfig), not the cache checkout's, can redirect UDE's push. Global `url.*.pushInsteadOf` sends the validated commit to another repository before the read-back refuses; this was measured. Global `remote.<url>.url` makes the remote-head check read another repository; this was measured with plain git, and whether UDE then returns Ok is not known. You already accepted global `url.*.insteadOf` as a residual. Anything that can write your global config can also run git itself. How should these two be handled?" Selected "Accept with insteadOf", whose option text read "Widen the accepted global-config residual to cover all global redirect keys: same writer, same privilege. The plans record it as accepted, and the gate's promise stays scoped to the cache checkout and its hooks."
  - T-02-78: operator, 2026-11-02 (one or two extra `git ls-remote` reads per push). The rationale quotes the text of the option the operator selected: "Push source becomes a form no ref can shadow, base is checked against `git ls-remote`, push goes to the caller's URL and is confirmed. Costs one or two network reads per push. Global `url.*.insteadOf` stays open and would be recorded as a residual."
  - T-02-80: operator, 2026-11-03. Quote its Status cell in `## Operator answers` of 02-21-PLAN.md word for word: accepted by the operator 2026-11-03. Question: "New risk from pushing to the caller's URL and reading the ref back: a hook-written `url.*.pushInsteadOf` in the cache checkout still sends the validated commit to another repository before the read-back refuses. `ude sync push` returns no Ok, and only validated bytes travel. How should this be handled? (Planner's finding, severity low, not a defect.)" Selected "Accept (Recommended)", whose option text read "Record it as an accepted residual. A refusal before the push would need a list of config keys and would still miss a process that writes the key later." Scope, the same day, a question ending "Does your acceptance cover all of them?" over every repository-level redirect key in NOT-04 (`url.*.pushInsteadOf`, `url.*.insteadOf`, `remote.<url>.pushurl`, and the reasoned `core.sshCommand` and `http.proxy`): selected "All redirect keys (Recommended)", whose option text read "Same effect and the same protection (no Ok, only validated bytes travel). A refusal for each key would need a key list and would still miss a process writing later."
  - T-02-81: operator, 2026-11-03. Quote its Status cell in `## Operator answers` of 02-21-PLAN.md word for word: accepted by the operator 2026-11-03. Question: "Second new risk: pushing to the caller's URL puts that URL on the git command line for every push and read, where before it appeared only at first clone. If the URL embeds a token, other local users can see it in the process list. How should this be handled? (Planner's finding, severity low, not a defect.)" Selected "Accept (Recommended)", whose option text read "Record it as an accepted risk. Safe if you don't use token-bearing URLs on a shared host; please say so under notes if you do." No notes were given.
- 02-REVIEW-DISPOSITION.md: CR-300 (plan 02-16), CR-301 (02-17), WR-300 (02-19), WR-301 (02-18) and WR-302 (02-19) to `fixed`, with the plan and the confirming commit in the Source cell. The fix commits named by those plans' SUMMARYs: CR-300 a891d58 and f1628d9; CR-301 7eb7c6b; WR-300 c9acbbf; WR-301 9a83d3f and 4a681a8; WR-302 84c2c86. IN-300 and IN-301 stay open.
- When re-verifying: name the closed list in 02-20-PLAN.md and docs/ARCHITECTURE.md (section 3, `What the publication gate promises`) to the verifier, and say a finding is classified by that list's rule. Re-run each control row 02-21-SUMMARY.md lists after `Re-run at re-verification:` (rows 10, 11, 29, 34, 35, 36, 37, 38, 39, 40 and 43, all corrected by added names only).

When these are done, move this todo to completed and run gsd_run query state.resolve-blocker --text "Hand edits owed after re-verification".
