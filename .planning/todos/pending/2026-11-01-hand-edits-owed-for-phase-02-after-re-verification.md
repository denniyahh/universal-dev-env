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

## Added by plan 02-23

The edits in this todo are owed by 02-15-PLAN.md's "Who updates which document, and when", by 02-21-PLAN.md's `## Who updates which document` and by 02-23-PLAN.md's `## Who updates which document`. They are still conditional on re-verification of phase 02 passing; if it finds gaps they wait.

- 02-REVIEW-DISPOSITION.md: CR-400 to `fixed`, with plan 02-22 and the confirming commit in the Source cell. The fix commits named by 02-22-SUMMARY.md: 91446c2 (blobs on both routes) and 3abb023 (the trees above recorded and deleted paths); efe86b6 pins git's own refusal of a rewritten fetched root tree. No other row changes because of plans 02-22 and 02-23.
- When re-verifying, tell the verifier three things:
  - DEF-11 and DEF-12 now name the object-id channel: a blob or tree in the cache checkout whose stored bytes do not match its object id (02-23-PLAN.md `## The two rows`, and docs/ARCHITECTURE.md section 3). A finding is classified by the closed list's rule.
  - The channel includes subtrees, against 02-VERIFICATION.md's earlier conclusion that on git 2.56.0 it is blobs only (blobs read by `cat-file`, concluded from a forged root tree). A rewritten subtree is read silently: `ls-tree -r` exits 0 listing the forged blob (measured at planning and by the plan checker); only a root tree or commit makes git refuse. 02-22's tree rows pin it.
  - Re-run each control row 02-23-SUMMARY.md lists after `Re-run at re-verification:`; 02-21's list above still stands.
- Still open with the operator, to be listed in each report until it is answered: the base-side channel in 02-22-PLAN.md's `Not in this plan` bullet (a further gap plan, or a NOT row). A base-side tree whose stored bytes do not match its id hides an added file from the staged diff and from `diff-tree`. Measured with plain git, not through UDE:
  - With `pack.window=0` the push published the added path, but its blob was one the remote already held.
  - With a blob the remote did not hold, the push was refused (`missing necessary objects`, the plan checker's probe).
  - So no new bytes reached the remote in what was run; what is published is an unvalidated path.

## Added by plan 02-26

The edits in this todo are owed by 02-15-PLAN.md's "Who updates which document, and when", by 02-21-PLAN.md's and 02-23-PLAN.md's `## Who updates which document`, and by 02-26-PLAN.md's `## Who updates which document`. They are still conditional on re-verification of phase 02 passing; if it finds gaps they wait.

- 02-REVIEW-DISPOSITION.md: WR-500, WR-501 and WR-502 to `fixed`, with plan 02-24 and the confirming commits in the Source cell. The commits named by 02-24-SUMMARY.md: WR-500 146fad2 (the empty-status route lists the fetched head from hashed tree bytes; RED c9e74a0) and 7ee7eb3 (the committed route does the same for the commit to publish; RED 0737048); WR-501 and WR-502 680ac12 (tests pinning the gate's own root-tree check and the stdin helper's deadline). IN-501 is not taken (02-24-PLAN.md's decisions: telling a type mismatch from a failed read needs ObjectDefect split, which changes texts DEF-11's rows pin); its two ancestor-tree sites are gone with the code 02-24 removed (verify_ancestor_trees, listed_tree_ids). IN-500 stays out of scope by the operator's answer of 2026-11-04 (02-24-PLAN.md `## Operator answers`). No other row changes because of plans 02-24 to 02-26.
- The base-side channel from 02-22-PLAN.md's `Not in this plan` bullet: answered by the operator on 2026-11-04 (`Plan 02-24: both`) and closed by plan 02-25 with the commits its SUMMARY names: d936992 (the committed gate's change list computed from hashed tree bytes; RED bdf59f8) and 7c9d86f (byte equality with git diff-tree). Once re-verification holds 02-25's rows, drop it from every still-open list, including the `## Added by plan 02-23` bullet above that lists it as open.
- Answered by the operator on 2026-11-04 (recorded in STATE.md), no longer open: the staged gate's `git diff --cached` stays unchecked; the id check on the trees the diff reads is met at the committed gate, which is the binding check (02-25-SUMMARY.md, T-02-105).
- 02-VALIDATION.md, when its validation strategy is updated for plans 02-24 to 02-26: drop test_listed_tree_ids from the SEC-04 row (plan 02-24 removed that test, listed_tree_ids and verify_ancestor_trees). 02-SECURITY.md gets rows for plans 02-24 to 02-26 from `/gsd-secure-phase`, not by hand.
- When re-verifying, tell the verifier:
  - DEF-09, DEF-11 and DEF-12 changed (02-26-PLAN.md `## The three rows`, and docs/ARCHITECTURE.md section 3). A finding is classified by the closed list's rule.
  - Neither gate route runs `ls-tree` or `diff-tree` any more. Each builds its listing, and the committed route its change list, in process from tree bytes hashed against their ids, starting from a commit read the same way. test_verified_tree_listing_matches_ls_tree and test_listing_diff_matches_diff_tree pin byte-identity with git's output on honest trees.
  - The two root-tree push rows now assert git status's refusal, and test_fetched_head_holds_refuses_root_tree_not_matching_its_id pins the gate's own root check.
  - Re-run each control row 02-26-SUMMARY.md lists after `Re-run at re-verification:` (none: all 13 rows came out `same`, so no corrected row and no T-02-109); the lists of 02-21 and 02-23 still stand.
  - 02-22-PLAN.md's verify commands and 02-23-PLAN.md's control rows 3, 4 and 8 name code plan 02-24 removed (verify_ancestor_trees, listed_tree_ids, test_listed_tree_ids). They describe superseded code and are not re-run; 02-23's SUMMARY lists none of them for re-running.
