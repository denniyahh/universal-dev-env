---
phase: 02-sanitizer-boundaries-security-hardening
verified: 2026-11-05T03:51:18Z
status: passed
score: 113/113 must-haves verified
covered_files:
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-01-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-01-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-02-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-02-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-03-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-03-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-04-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-04-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-05-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-05-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-06-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-06-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-07-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-07-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-08-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-08-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-09-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-09-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-10-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-10-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-11-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-11-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-12-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-12-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-13-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-13-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-14-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-14-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-15-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-15-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-16-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-16-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-17-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-17-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-18-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-18-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-19-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-19-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-20-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-20-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-21-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-21-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-22-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-22-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-23-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-23-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-24-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-24-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-25-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-25-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-26-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-26-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-27-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-27-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-28-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-28-SUMMARY.md"
  - "docs/ARCHITECTURE.md"
  - "src/main.rs"
  - "src/process.rs"
  - "src/sanitizer/entropy.rs"
  - "src/sanitizer/gitleaks.rs"
  - "src/sanitizer/mod.rs"
  - "src/sanitizer/rules/LICENSE-gitleaks"
  - "src/sanitizer/rules/gitleaks.toml"
  - "src/sync/mod.rs"
  - "src/template/mod.rs"
  - "tests/e2e_lifecycle.rs"
covered_digest: "v4:sha256:9a8a5be91f9696e31ffe2a2b68501f1eefa0a794b6ac2a89ec84a2ea9fac80e9"
behavior_unverified: 0
overrides_applied: 0
re_verification:
  previous_status: gaps_found
  previous_score: 70/75
  gaps_closed:
    - "DEF-11 / 02-13 prohibition (CR-400, committed route): a blob or tree in the cache checkout whose stored bytes do not match its id is refused in direct and PR mode; removing the id comparison in read_verified_object makes the four committed-route push rows return Ok again (negative control run here)"
    - "DEF-12 / 02-19 prohibition (CR-400, empty-status route): same, both modes; the same negative control makes the ten empty-status rows return Ok(\"No changes to push to remote repository.\")"
    - "Derived truth 'a push that returns Ok has published the validated content': holds again on every route tried; doc comments of read_blob_by_id, first_path_not_held, require_validated_content, require_fetched_head_holds now true"
  gaps_remaining: []
  regressions: []
---

# Phase 2: Sanitizer, Boundaries & Security Hardening Verification Report

**Phase Goal:** Screen and redact all confidential tokens, raw credentials, and user-specific paths before any remote git staging or push.
**Verified:** 2026-11-05T03:51:18Z
**Status:** passed
**Re-verification:** Yes, after gap plans 02-22 to 02-28 and the review fixes WR-700, IN-700, IN-701, IN-703

## Bottom line

Both gaps of the previous report (CR-400 on the committed and the empty-status route) are closed, and the
fixes are load-bearing: with the id comparison disabled in a scratch copy, 26 tests fail and the CR-400 push
rows return Ok again, exactly the defect the previous report reproduced. The WR-500 swap channel, the
base-side DEF-09 channel and the WR-600 walk bound are each pinned by tests that fail when their check is
removed (controls below). No code finding in this round re-opens a DEF row.

**What argues against this verdict.**
- CR-400 and its relatives are driven by test steps (an object file rewritten in the fixture), not by an
  installed hook through `ude sync push`. Packed-object forgery was not run; the check hashes whatever
  `cat-file` returns, so it is storage-agnostic by construction, not by a run.
- The 02-22 plan's two artifacts (`listed_tree_ids`, `verify_ancestor_trees`) and one key link no longer
  exist: plan 02-24 replaced them with `verified_tree_listing`, which hashes every tree from the root.
  Ruled superseded, not a gap: the truths they served pass and fail under the control (see Required Artifacts).
- Five threat dispositions remain with the operator (T-02-60, -61, -64, -65, -67). By 02-15-PLAN's own
  table they do not block the phase; they are listed as open operator decisions, not verification items.

## Goal Achievement

### Previous gaps

| Gap | Status | Evidence |
|-----|--------|----------|
| DEF-11, committed route (CR-400) | VERIFIED (closed) | read_verified_object (src/sync/mod.rs 3167-3210): `--no-replace-objects cat-file -t`, `cat-file <kind>`, then `hash-object -t <kind> --literally --no-filters --stdin` over the exact bytes, id compared whole, refusal on mismatch. Every blob, tree and commit read by both gate routes goes through it; no production `ls-tree` or `diff-tree` remains (count 0 before `mod tests {`). Rows `test_{direct,pr}_push_refuses_unchanged_{blob,subtree}_not_matching_its_id`, `_rewritten_only_while_listed`, `_added_file_hidden_by_base_tree_...` pass with their controls. Control A fails them with `got Ok("Directly pushed ...")` / `Ok("Pushed changes to branch ...")` |
| DEF-12, empty-status route (CR-400) | VERIFIED (closed) | require_fetched_head_holds (3021-3143) reads the head commit verified, lists from hashed tree bytes, reads recorded blobs verified. Ten `_no_changes_refuses_*_not_matching_its_id` / `_hidden_only_while_listed` / `_subtree_hiding_a_deleted_path` rows pass; control A fails them with `got Ok("No changes to push to remote repository.")` |

### Roadmap Success Criteria

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| SC1 | High-entropy strings and raw Base64 tokens flagged before staging | VERIFIED | `git diff --quiet b8c54e9 HEAD -- src/sanitizer src/template` exit 0 (control: same over src/sync exit 1); full suite green |
| SC2 | Outbound commit inspection catches gitleaks categories across committed blobs, rolls back | VERIFIED | Committed gate scans every changed blob read by verified id (2887-2909); DEF-10 rows incl. `test_post_commit_gate_blocks_gitleaks_only_category`, `..._rolls_back` pass; no route tried published a token or an unvalidated blob |
| SC3 | Path parameterization on word and punctuation boundaries only | VERIFIED | Regression only: code unchanged since b8c54e9; tests pass |
| SC4 | Templated tokens substitute back on initialization | VERIFIED | Regression only: code unchanged since b8c54e9; tests pass |

### Plan must-haves, 02-22 to 02-28

| Plan | Status | Evidence |
|------|--------|----------|
| 02-22 (id check) | VERIFIED | Truths hold via 02-24's stronger form; artifacts `run_bounded_with_input`, `read_verified_object` present; `--literally --no-filters`, no `-w`; test_read_verified_object_checks_bytes_against_the_id asserts the check wrote nothing; SHA-256 and abbreviated-id tests pass; Cargo.toml/Cargo.lock unchanged since 4f9f043 |
| 02-23 (DEF-11/12 rows) | VERIFIED | DEF rows of ARCHITECTURE and 02-20-PLAN byte-identical (diff exit 0); NOT rows unchanged since ab5dc34 (diff exit 0; control: DEF rows since ab5dc34 differ, exit 1); stored mutation output missed=0, caught=35 |
| 02-24 (hashed listing, WR-500/501/502) | VERIFIED | `verified_tree_listing`, `parse_tree_object`, `first_path_under` present and wired (verify.artifacts 4/4, key-links 2/2); swap rows pass, fail under control A; `test_fetched_head_holds_refuses_root_tree_not_matching_its_id` fails under control A (`a root tree not matching its id must be refused`); root push rows assert `git status failed` with a not-held control |
| 02-25 (base-side DEF-09) | VERIFIED | Change list from `listing_diff` over two verified listings (2868-2869); `test_committed_gate_refuses_base_tree_not_matching_its_id` and the two push rows pass, fail under control A; `test_listing_diff_matches_diff_tree` passes |
| 02-26 (rows, controls) | VERIFIED | Rows as above; stored mutation output missed=0, caught=62 |
| 02-27 (walk bound, WR-600) | VERIFIED | ListingBudget (3379-3417) shared by both listings (2740, 2741, 2801-2806) and the empty-status listing (3061-3062); take() before entries are pushed (3302); check_time() before each tree read (3277); listing_diff checks the deadline in both loops. Control B fails all 8 limit tests; control C fails the 3 walk-deadline tests; repeated-path check runs before listing_diff (2843 before 2868) |
| 02-28 (split tests, DEF-09/10) | VERIFIED | `test_listing_diff` gone, 12 `test_listing_diff_*` and 4 `test_verified_tree_listing_*` exist and pass; stored mutation output missed=0, caught=43; no .cargo/mutants.toml |
| 02-01 to 02-21 | VERIFIED (21) | Regression: full suite green; 02-13 and 02-19 groups, FAILED last time, now pass (gaps above) |

### Prohibitions

| Plans | Count | Status | Evidence |
|-------|-------|--------|----------|
| 02-01 to 02-21 | 48 | VERIFIED | The two FAILED last time (02-13 Ok without validated content, 02-19 Ok on empty status) now hold; others regression, unchanged code paths |
| 02-22 | 5 | VERIFIED | Ok-meaning rules (control A); legitimate pushes (no-change publish rows, gc'd-remote packed row, SHA-256) pass; no write while checking (no `-w`, asserted); refusals name record/staged paths (first_path_under); no NOT row widened |
| 02-23, 02-26, 02-28 | 10 | VERIFIED | Stored control tables and mutation outputs (not re-run, see limits); 02-CONTEXT.md and plans 02-01..02-15 unchanged since b8c54e9; NOT rows unchanged; DEF rows identical in plan and doc |
| 02-24 | 6 | VERIFIED | Swap rows; honest listings byte-identical to ls-tree (SHA-1, SHA-256); no tree text in refusals; NOT-10 not widened; RED gate on 02-22 has a recorded operator override (STATE.md 2026-11-04) |
| 02-25 | 4 | VERIFIED | Base-side rows; listing_diff byte-identical to diff-tree; refusals name GATE_REL, not hidden.repo |
| 02-27 | 6 | VERIFIED | NOT rows unchanged; fixed texts (LISTING_TOO_LARGE, LISTING_PAST_DEADLINE) with no tree text; seams `cfg(test)` only (listing_budget 616-627, read_blob_by_id 3152-3155); limits far above this repository's measured listing |

All test-tier; none judgment-tier, so no prohibition routes to a human sign-off.

### Derived truths, re-checked

| Truth | Status | Evidence |
|-------|--------|----------|
| A push that returns Ok has published the validated content | VERIFIED | Was FAILED via CR-400; closed (control A re-opens it) |
| A hook cannot add an unbound name to the published tree | VERIFIED | CR-301 rows; base-side hidden.repo rows; slash-named entry probe below |

**Score:** 113/113 (SC 4; plan groups 28; prohibitions 79; derived 2). 0 present but behaviour-unverified.

### Ruling on the review findings (02-REVIEW.md, 02-27..02-28 review) and older open items

Classified by the closed list's rule in docs/ARCHITECTURE.md section 3.

| Finding | Ruling | Evidence |
|---------|--------|----------|
| WR-700 | Fixed (57ed03a) | Wall-clock asserts removed; refusal text still decides; test passed in both full runs here |
| IN-700 | Fixed (5f64809) | `diff_refusal` pure mapping, pinned by `test_diff_refusal_names_each_defect`; refactor preserves texts (diff read) |
| IN-701 | Fixed (d69322b), comment only | push_tree_entries and ListingBudget docs now say the deadline stops new reads; not a DEF failure (push still refuses) |
| IN-702 | Not taken; Info | Refusal names the fetched head when the commit's listing used the shared budget; fails closed |
| IN-703 | Fixed (e0e2d4c), comment plus a pinning test | Probed here with plain git: a root entry literally named `a/x` lists byte-identical to an honest `a/x`, pushes, clones (exit 0) and checks out the same file; fsck warns `fullPathname`. Control: honest tree clones the same. No new path text is published; Info |
| IN-704 | Owed hand edit (todo); Info | DEF-09 still reads "which also covers the changes"; the change loop is outside the deadline by the 2026-11-04 decision. Wording only |
| WR-400 | Warning, open, not a goal gap | ensure_remote_repo_for_push unchanged since the previous report, which reproduced it; it fails closed (a push from a second directory whose `../x` names another repository), names the wrong cause, persists until the cache is removed. Not a DEF-15 failure: publishing there would build on another repository's head |
| IN-400..IN-403, IN-300, IN-301 | Info / open, unchanged code | stage_publication, ensure, src/main.rs and tests/ unchanged since ab5dc34; rulings of the previous report stand (IN-401 is a DEF-02 wording overclaim; no bypass, and control D shows the id check alone now refuses replace-ref reads) |

### Negative controls run here (scratch `git archive` copy of aed98b0, own target dir, full nextest suite per mutation, each reverted)

Unmodified copy: 598 passed, 0 failed.

| Mutation | Failed | Result |
|----------|--------|--------|
| A: id comparison never refuses (`if false && printed != oid`) | 26 | Every DEF-11/DEF-12 id row, both modes, the base-tree, swap, root and read_verified_object tests; push rows fail on `got Ok(...)`, i.e. CR-400 re-opens |
| B: budget never taken (`budget.take` removed) | 8 | All record- and byte-limit tests; the just-under production row still passes, as it should |
| C: walk deadline never checked (`check_time` removed) | 3 | The commit-listing, carried-deadline and empty-status deadline tests; listing_diff's own deadline tests unaffected, as expected |
| D: `--no-replace-objects` dropped from both cat-file reads (successor of 02-21 rows 10, 11) | 12 | All fail closed: refusals become `does not match its object id`; no Ok where a refusal was expected, so the id check alone also stops replace refs |

### 02-21 control rows listed for re-run at re-verification

Rows 10 and 11 name the `diff-tree`/`ls-tree` reads plan 02-24/02-25 removed; superseded, replaced by control D.
Rows 29, 34, 35, 36, 37, 38, 39, 40, 43: every test name in each row failed. Extra failures are only tests
added by plans 02-22..02-28 whose control halves assert the removed refusal (e.g. under row 43 the
`_no_changes_refuses_blob_not_matching_its_id` control gets `Ok("No changes ...")`), read at their panic sites.

| Row | Row's names | Failed | Extras |
|-----|-------------|--------|--------|
| 29 | 24 | 24 | 0 |
| 34, 35 | 6 each | 6 each | 0 |
| 36, 37 | 9 each | 9 each | 0 |
| 38 | 12 | 24 | 12 newer empty-status id/deletion rows |
| 39 | 3 | 3 | 0 |
| 40 | 3 | 7 | 4 newer deletion-hidden rows |
| 43 | 9 | 18 | 9 newer empty-status id rows and the root-tree row |

### Required Artifacts

| Plan | `verify.artifacts` | `verify.key-links` |
|------|--------------------|--------------------|
| 02-22 | 2/4, exit 1: `listed_tree_ids`, `verify_ancestor_trees` missing | 1/2, exit 1: `verify_ancestor_trees(` |
| 02-23 | 3/3, exit 0 | 2/2, exit 0 |
| 02-24 | 4/4, exit 0 | 2/2, exit 0 |
| 02-25 | 1/1, exit 0 | 2/2, exit 0 |
| 02-26 | 3/3, exit 0 | 2/2, exit 0 |
| 02-27 | 4/4, exit 0 | 3/3, exit 0 |
| 02-28 | 3/3, exit 0 | 2/2, exit 0 |

02-22's misses are superseded, not missing work: plan 02-24 (operator answer `Plan 02-24: both`, 2026-11-04)
removed both functions and checks every tree from the root inside `verified_tree_listing`; the 02-22 tree
rows pass and fail under control A. The todo already records that 02-22's verify commands describe
superseded code.

### Key Link Verification

| From | To | Status | Details |
|------|----|--------|---------|
| gate reads | read_verified_object | WIRED | All blob/tree/commit reads of both routes; none bypass it |
| read_verified_object | run_bounded_with_input | WIRED | via run_local_with_input (3200) |
| verified_tree_listing | parse_tree_object | WIRED | 3288 |
| validate_committed_tree | listing_diff | WIRED | 2868, with the budget's deadline |
| listings | one ListingBudget per gate call | WIRED | 2740 shared by 2741 and 2801; 3061 |
| repeated-path check | before blob reads | WIRED | tree_listing_blobs 2843 before listing_diff 2868 |
| **stored blob bytes** | **the blob id** | **WIRED** | Was NOT WIRED (CR-400) |

### Data-Flow Trace (Level 4)

Not applicable: no rendered dynamic data.

### Behavioral Spot-Checks

| Check | Result |
|-------|--------|
| Full suite in the repository, `cargo nextest run --workspace --no-fail-fast --run-ignored all` | 598 passed; 1 failed: `test_dump_parity_corpus`, the ignored parity dumper that needs UDE_PARITY_DIR (forced on by `--run-ignored`, not a defect). Without it in the copy: 598 passed |
| `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo machete`, `cargo deny check advisories bans sources` | all exit 0 |
| DEF test names | 275 distinct (278 occurrences), each defined exactly once (control: an invented name counts 0) |
| Debt markers in src/sync/mod.rs, src/process.rs, docs/ARCHITECTURE.md | 0 (control: a planted FIXME counts 1) |
| Repository state after runs | .git/config and .git/index hashes unchanged; `git status` unchanged |

### Probe Execution

No `scripts/*/tests/probe-*.sh` is declared by plans 02-22 to 02-28. Not applicable.

### Requirements Coverage

| Requirement | Status | Evidence |
|-------------|--------|----------|
| SEC-01 | SATISFIED | SC1 |
| SEC-02 | SATISFIED | SC2 |
| SEC-03 | SATISFIED | SC3 |
| SEC-04 | SATISFIED | SC4, and the Ok-meaning prohibitions filed under SEC-04 now hold |

All four claimed by plans; none orphaned. REQUIREMENTS.md is inconsistent with itself (SEC-04 checked and
"Complete", SEC-01..03 unchecked and "Gaps Found"); not edited here.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| docs/ARCHITECTURE.md | DEF-09 | "which also covers the changes" | Info | IN-704, owed hand edit |
| docs/ARCHITECTURE.md | DEF-02 | "Every gate read ... past replace refs" | Info | IN-401; staged gate's reads do not; binding gate does |
| src/sync/mod.rs | 1262-1266 | Refusal blames a hook | Warning | WR-400 wrong cause |

### Human Verification Required

None. No judgment-tier item, no visual or external-service behaviour in scope.

### Open operator decisions (not verification items, not gaps)

- T-02-60, T-02-61, T-02-64, T-02-65, T-02-67: still "pending the operator, not accepted" in 02-15-PLAN
  `## Operator answers`. That table says an unanswered row stays OPEN and does not hold up the phase.
- The hand edits in `.planning/todos/pending/2026-11-01-hand-edits-owed-for-phase-02-after-re-verification.md`
  become due now that re-verification passes; 02-SECURITY.md is still to be regenerated by `/gsd-secure-phase`.

### Gaps Summary

None. The single cause of the previous two gaps (gate decisions taken from stored bytes not checked against
their ids) is closed on both routes, and the follow-on channels (swap between reads, base-side tree, walk
size and time) are closed with tests that fail when the checks are removed.

### What these checks do not establish

- Forgeries were made by test steps, not by an installed hook through `ude sync push`. Packed objects were not
  forged; the claim that the check covers them rests on hashing `cat-file`'s output, not on a run.
- Local bare remotes, git 2.56.0, Linux only (NOT-12).
- Each control shows the named tests depend on that check; it does not show the list of channels is complete.
  CR-400 was exactly a channel earlier tests could not see.
- The 02-23, 02-26 and 02-28 control tables and mutation runs were read from stored files, not re-run. The
  production code they covered changed afterwards only by comments and by the behaviour-preserving
  `diff_refusal` extraction (5f64809), which no mutation run covers; a unit test pins its texts.
- Memory under a single very large tree object, the `verified` map and the change loop are outside the
  walk bound by 02-27's "Not claimed" section; the closed list does not say so (beside IN-704). Not run.
- Clippy's result was a cached replay at unchanged sources, not a fresh analysis.
- The slash-named entry probe ran with plain git, not through UDE.

---

_Verified: 2026-11-05T03:51:18Z_
_Verifier: Claude (gsd-verifier)_
