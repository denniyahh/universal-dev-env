---
phase: 02-sanitizer-boundaries-security-hardening
plan: 25
subsystem: sync-publication-gate
tags: [git, object-integrity, diff, sha256, tdd, rust]

requires:
  - phase: 02-sanitizer-boundaries-security-hardening
    provides: "02-24's verified_tree_listing, first_path_under, ListingDefect and read_verified_object (kind commit); 02-22's forged-object test helpers"
provides:
  - "listing_diff: the committed gate's change list computed in process from two verified listings, in diff-tree's raw format"
  - "validate_committed_tree reads the fetched head's commit and trees through the id check and no longer runs git diff-tree"
  - "RED-then-green rows for an added file hidden by a fetched-head tree that does not match its id (both modes) and a direct row for a hidden addition and a hidden deletion"
  - "test_listing_diff and test_listing_diff_matches_diff_tree (byte equality with git, SHA-1 and SHA-256, both directions)"
affects: [02-26]

actuals:
  tokens: 10021
  tasks: 2
  commits: 3
plan_head_before: ebfc0879fc0192dfbc3ec8e8fa770b50f4af2b0d
plan_head_after: 7c9d86f2a1e20ddf469c0690180a77ea35b717aa

tech-stack:
  added: []
  patterns:
    - "Compute the change list from hashed tree bytes in git's own output format, so the existing parser and change loop run unchanged and the result can be compared with git byte for byte"

key-files:
  created: []
  modified:
    - src/sync/mod.rs

key-decisions:
  - "The per-task RED-commit gate tripped before Task 1's GREEN as tdd.md's Rust gap (#4379) predicts; continued under the operator's 2026-11-04 approval because the substitute evidence held (bdf59f8 by exact subject, ancestor of HEAD, 3 markers; all 3 records RED_EVIDENCE_OK). last_gate_trip was not written: state.update reported the field absent from STATE.md"
  - "Only the commit's verified listing (and its after_tree_listing call) moved before the change loop; tree_listing_blobs, count_empty_directories, require_validated_content and the tree binding stay after it, so existing refusal order is unchanged except where the listing itself refuses"
  - "A fetched-head commit with no readable or UTF-8 tree line is refused with the fetched head's Unreadable text (failed to list the tree of the fetched head); the plan did not name a text for that case"
  - "Interpretation recorded by the planner and still open to the operator's objection: 'the trees those diffs read' is met at the committed gate (the binding check), not at the staged gate's git diff --cached"

patterns-established:
  - "listing_diff matches records as a multiset per path, so a repeated entry the base does not hold identically is listed as a change and scanned"

requirements-completed: [SEC-04]

coverage:
  - id: D1
    description: "An added file hidden by a fetched-head tree that does not match its id is refused by the committed gate in direct and PR mode, naming GATE_REL and not the hidden name; no remote head moves, the remote's main does not list it, the remote's generic tree hashes to its id; control refused by the staged gate"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_direct_push_refuses_added_file_hidden_by_base_tree_not_matching_its_id"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_pr_push_refuses_added_file_hidden_by_base_tree_not_matching_its_id"
        status: pass
    human_judgment: false
  - id: D2
    description: "validate_committed_tree refuses a fetched-head templates tree not matching its id when it hides an added file and when it hides a deletion, naming templates/f; each with an honest-base control"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_committed_gate_refuses_base_tree_not_matching_its_id"
        status: pass
    human_judgment: false
  - id: D3
    description: "On honest trees listing_diff is byte-identical to git diff-tree -r -z --raw --no-commit-id --no-renames, both directions, SHA-1 and SHA-256; its rules are pinned on hand-written listings"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_listing_diff_matches_diff_tree"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_listing_diff"
        status: pass
      - kind: unit
        ref: "cargo test (full suite, 452 + 114 passed, 1 ignored pre-existing)"
        status: pass
    human_judgment: false

duration: 19min
completed: 2026-11-04
status: complete
---

# Phase 2 Plan 25: Committed-gate change list from hashed tree bytes Summary

**The committed gate no longer asks git for the diff. It lists the fetched head and the commit from tree bytes hashed against their ids and computes diff-tree's raw records itself (`listing_diff`). A fetched-head tree rewritten to hide an added file or a deletion is now refused before anything is pushed, in both modes. At 02-24's last commit the same push published the hidden file.**

## Performance

- **Duration:** about 19 min
- **Started:** 2026-11-04T17:40:37Z
- **Completed:** 2026-11-04T17:59Z
- **Tasks:** 2
- **Files modified:** 1 (src/sync/mod.rs)

## What changed and what it means

- **The base-side DEF-09 channel is closed at the committed gate.** That gate is the binding check. Before this plan, `git diff-tree` read the fetched head's trees without checking their ids. A rewritten fetched-head tree that listed the commit's added entry at the same blob made the addition invisible. The RED run below shows the push succeeding and the remote listing `hidden.repo`.
- **How it is closed.** `validate_committed_tree` lists the commit first, as before. Then it reads the fetched head's commit through `read_verified_object` and lists its tree with `verified_tree_listing` on the same map. A tree both commits hold is therefore hashed once. The change list is `listing_diff(fetched head, commit)`, parsed by the unchanged `raw_diff_entries` and checked by the unchanged change loop.
- **`git diff-tree` no longer runs on the committed route.** The production count of the quoted `"diff-tree"` argument went from 1 to 0. Its two refusals (`Failed to inspect committed diff-tree in remote checkout`, `git diff-tree failed in remote checkout`) are gone.
- **Not changed:** the staged gate's `git diff --cached` still reads fetched-head trees without the check (T-02-105). It is not the binding check. In the push rows the committed gate refuses before anything is pushed.

## Task Commits

1. **Task 1 (tracer): end-to-end, both modes.**
   - RED `bdf59f8` test(02-25): add RED rows for a file hidden by a base tree
   - GREEN `d936992` feat(02-25): diff the commit from hashed tree bytes
2. **Task 2: equivalence with git and this plan's gates.** `7c9d86f` test(02-25): pin the in-process diff against git diff-tree

Plan metadata: see the docs commits that follow.

## Files Created/Modified

- `src/sync/mod.rs`, production:
  - New: `listing_diff`, `ListingRecord`, `listing_records`, `mode_type_bits`, `diff_record_bytes`.
  - Changed: `validate_committed_tree` and its doc comment. The id-mismatch path naming now builds the sorted recorded and allowlisted lists once and serves both listings.
- `src/sync/mod.rs`, tests:
  - New rows: `test_direct_push_refuses_added_file_hidden_by_base_tree_not_matching_its_id`, `test_pr_push_refuses_added_file_hidden_by_base_tree_not_matching_its_id` (helpers `add_file_hidden_by_base_tree`, `assert_added_file_hidden_by_base_tree_refused`, `HIDDEN_REL`) and `test_committed_gate_refuses_base_tree_not_matching_its_id`.
  - Equivalence tests: `test_listing_diff` and `test_listing_diff_matches_diff_tree` (helpers `listing_record`, `raw_change`, `raw_diff_tree`, `assert_listing_diff_matches`).
  - Changed: the empty-name row of `test_committed_gate_refuses_bad_path_components`.
- Evidence, gitignored: `target/gsd-red/02-25-t1-red.junit.xml` and the 3 records `target/gsd-red/02-25-t1-<row>.json`.

## New refusal format strings

All on the committed route (`validate_committed_tree`):
- `"Post-commit publication gate violation: failed to read the fetched head {base}"`: the fetched head's commit is unreadable.
- `"Post-commit publication gate violation: the fetched head's commit {OBJECT_ID_MISMATCH}"`
- `"Post-commit publication gate violation: failed to list the tree of the fetched head"`: a tree is unreadable, or the commit's tree line is missing or not UTF-8. The plan named no text for the tree-line case, so it reuses this one.
- `"Post-commit publication gate violation: unparseable tree entry in the fetched head"`
- `"Post-commit publication gate violation: an object read for {path} {OBJECT_ID_MISMATCH}"`, where `{path}` is the first recorded path (sorted), then the first allowlisted path (sorted), lying under the mismatched fetched-head tree.
- `"Post-commit publication gate violation: a tree of the fetched head {OBJECT_ID_MISMATCH}"` when no such path lies under it.
- Unchanged and reused: `"Post-commit publication gate violation: unparseable committed entry"`, now also given when `listing_diff` returns None.

`{base}` is the id UDE fetched itself. No name, id or byte read from a tree is printed (DEF-14's rule). The push rows assert that the refusal does not contain `hidden.repo`.

**The empty-name row changed.** In `test_committed_gate_refuses_bad_path_components`, the row for a subtree with an empty name now expects `unparseable tree entry` (was `git diff-tree failed`). The commit's verified listing now comes before any change is listed. It refuses the empty name, as git's own tree decoder does, and `diff-tree` no longer runs. The outcome is the same: refused. The `.` and `..` rows kept their texts (`path component` and `unvalidated file committed`) and pass.

## RED Evidence

**The run.** `PROPTEST_DISABLE_FAILURE_PERSISTENCE=1 timeout -k 30 900 cargo nextest run --bin ude --run-ignored only --no-fail-fast --color never <3 names>` on 02-24's last code (ebfc087) plus the three ignored rows. Exit 100, 3 of 3 failed. The JUnit report is newer than the run start. Each record is classified `RED_EVIDENCE_OK / target_test_failed`.

- `test_direct_push_refuses_added_file_hidden_by_base_tree_not_matching_its_id`: `expected a hard push failure, got Ok("Directly pushed changes to remote branch 'main'.")`, at `assert_push_refused`. Verdict: RED_EVIDENCE_OK.
- `test_pr_push_refuses_added_file_hidden_by_base_tree_not_matching_its_id`: `expected a hard push failure, got Ok("Pushed changes to branch 'ude-sync-14f11ae4-1793814154'. Create a PR on GitHub to merge into 'main'.")`, at `assert_push_refused`. Verdict: RED_EVIDENCE_OK.
- `test_committed_gate_refuses_base_tree_not_matching_its_id`: the failures list was `["hidden addition, forged fetched head: Ok(())", "hidden deletion, forged fetched head: Ok(())"]`. Verdict: RED_EVIDENCE_OK.

**Semantic assessment.** Each target ran and failed on its own assertion.
- In the push rows, the step's own assertion passed first: `--no-replace-objects ls-tree -r --name-only HEAD` listed `hidden.repo`, so the forgery was walked. The push was not refused by the transport, so the planned fallback (a transport refusal recorded as RED without a publication) did not apply.
- In the direct row, neither honest-base control appears in the failures, so both were refused as `unvalidated file committed in HEAD`. Only the two forged cases returned Ok(()).
- No row failed in a fixture step, timed out or failed to build.

**Did the RED push publish `hidden.repo`? Yes, in both modes.** This was measured by a separate probe run, not by the committed rows. The rows panic at `assert_push_refused` before their remote assertions run.
- **How:** a temporary, uncommitted edit printed the remote's heads and `ls-tree -r` before that assertion. It ran on the RED commit. The file was then restored with `git checkout -- src/sync/mod.rs`, and the tree was clean afterwards.
- **Direct mode:** the remote's `main` listed `templates/projects/generic/gitconfig.repo` (the new blob) and `templates/projects/generic/hidden.repo` at the seed blob `b3a7b3c`.
- **PR mode:** `main` was unchanged and listed only the seed template. The pushed `ude-sync-*` branch listed both files.
- **Caveat:** I first misread the probe output as "not published". That was a reading error: `rg` without context showed only the first line of each multi-line value. A second probe that also printed the checkout's index (`hidden.repo` present at the seed blob) confirmed the publication.

## Tests that passed on first run, and why

- `test_listing_diff` and `test_listing_diff_matches_diff_tree` pass on first run. They test a function that exists only after Task 1's fix, so they are not RED rows. Task 2 carries no `tdd` attribute.
- **The equivalence test is not vacuous.** It asserts that git's forward output holds each claimed case:
  - an `A`, `M` (content and mode flip), `D`, and `T` (symlink and gitlink);
  - `a` `A` with `a/x` `D`, and `b` `D` with `b/y` `A`;
  - the nested `M` and `A`;
  - the newline and 0xFF names.
  
  It also asserts that the unchanged tab name, `a.b`, the unchanged nested subtree and the empty subtree are absent. The empty-tree comparison checks every record is `A` forward and `D` reversed.
- **Mutant controls**, run on a scratch copy of the file. The file was restored afterwards and its sha256 checked against the copy.
  - With the sort removed, `test_listing_diff_matches_diff_tree` failed. `test_listing_diff` at first did **not** fail: none of its cases depended on order. So I added the case "records in path order, a deletion before a later addition". With that case the no-sort mutant fails `test_listing_diff` too.
  - With `T` replaced by `M`, both tests failed.
- **No difference from git was found.** `listing_diff` needed no fix.

What these do not establish:
- Equality was shown on the fixtures' honest trees only. A repeated path cannot be compared with git, because git refuses to write such a tree through `mktree`. That case is pinned only by `test_listing_diff`'s hand-written listing.
- The controls that remove the fix on the final code are plan 02-26's.

## Cost (module run `sync::tests:: process::tests::`, beside 02-24's)

| Run | Tests | nextest time | Wall |
|-----|-------|--------------|------|
| 02-24 before Task 1 (e39e860 code) | 341 | 99.7 s | 100.0 s |
| 02-24 after Task 3 | 351 | 76.6 s | 76.8 s |
| 02-25 after Task 1 GREEN (verify run) | 354 | 82.2 s | not taken |
| 02-25 after Task 2 | 356 | 84.3 s | 84.5 s |

**Trees per listing in the seeded fixture.** Measured on a plain-git replica of `seeded_bare_remote`'s commit with `gitconfig.repo` changed, in the session scratch directory, removed afterwards.
- The commit's listing reads **4** trees: the root, templates, projects and generic.
- The fetched head's listing reads **4** more. All four of its trees have ids that differ from the commit's at the same path, and none is shared.
- So the committed route now reads 8 trees (24 git runs) plus 2 commits (6 git runs). Before this plan it read 4 trees plus 1 commit, plus one `diff-tree` run.

**What these numbers do not show.** 02-24 measured about 25% swing between consecutive module runs. The 76.6 s to 84.3 s move is inside that noise, so it does not separate the added reads from run-to-run variance. It supports only "no regression visible above noise".

## Decisions Made

- The commit listing's sorted record/allowlist lists are built once and serve both listings' id-mismatch naming. This is the same rule as 02-24, with no new names.
- The `listing_diff` helpers (`listing_records`, `mode_type_bits`, `diff_record_bytes`) are split out so that each function stays short. The plan's counts hold: one `fn listing_diff`, one call line, three `.verified_tree_listing(` lines, two `raw_diff_entries(` lines.
- `diff_record_bytes` is named so that it does not shadow the test module's existing `raw_diff_record` helper.

## Deviations from Plan

**1. [Rule 2 - Missing test strength] Added a path-order case to test_listing_diff**
- **Found during:** Task 2, mutant control
- **Issue:** with the sort removed, `test_listing_diff` still passed, because no case depended on order. The plan's truth "records are ordered by path bytes" was pinned only by the git comparison.
- **Fix:** one case: a base `b` and a commit `c` give `b` `D` before `c` `A`.
- **Verification:** the no-sort mutant now fails `test_listing_diff` with "records in path order, a deletion before a later addition". The restored code passes.
- **Committed in:** 7c9d86f

**2. [Note] The commit gate runs with `PROPTEST_DISABLE_FAILURE_PERSISTENCE=1` and `TMPDIR` in the session scratch directory**
- This is the same as 02-24. Otherwise the gate is exactly `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && timeout -k 30 1800 cargo test && git commit`, and the commit is gated on its exit code.

**Total deviations:** 1 auto-fixed (test strength), 1 note. **Impact:** no change in behaviour or scope.

## TDD Gate Compliance

| Task | RED commit | GREEN commit | RED markers in the RED commit |
|------|------------|--------------|-------------------------------|
| 1 | `bdf59f8` test(02-25): add RED rows for a file hidden by a base tree | `d936992` feat(02-25): diff the commit from hashed tree bytes | 3 × `#[ignore = "RED: fails until 02-25 Task 1 fix"]` |
| 2 | `7c9d86f` test(02-25): pin the in-process diff against git diff-tree (tests only, no tdd attribute, so the per-task gate does not run) | none | none |

`d936992` descends from `bdf59f8` (`git merge-base --is-ancestor`). No RED marker of this plan survives in src.

**The per-task RED-commit gate, run unchanged before Task 1's GREEN.**
- **How it was bound.** A script copying execute-phase.md's `TDD gate` block verbatim, with `TDD_MODE=true`, `PHASE_NUMBER=02`, `PLAN_ID=25`, `TASK_ID=1` and the plan file as `TASK_FILE`. `task.is-behavior-adding` returned true.
- **Trip line, verbatim:** `TDD GATE TRIPPED: missing RED commit for 25/1`.
- **`last_gate_trip`: none written.** `state.update` replied `{"updated": false, "reason": "Field \"last_gate_trip\" not found in STATE.md", "preserved": []}`. STATE.md's sha256 was the same before and after.
- **Substitute evidence:**
  - `bdf59f87891fe9a9944d304a12005e15b14e6d92`, found by its exact subject (1 match);
  - an ancestor of HEAD;
  - its only file is `src/sync/mod.rs`, with exactly 3 markers;
  - all 3 records are `RED_EVIDENCE_OK`.
- **Control.** The same `evaluation-scope` query without the test-file pathspecs returned `bdf59f8789 test(02-25): add RED rows for a file hidden by a base tree`. So the trip comes from the pathspec (the Rust gap #4379), not from a missing commit.
- GREEN continued under the operator's approval of 2026-11-04. No pathspec was widened and no file under tests/ was touched.

## Threat Flags

| Flag | File | Description |
|------|------|-------------|
| T-02-103 (mitigate) | src/sync/mod.rs | A fetched-head tree not matching its id hiding an added, changed or deleted path from the committed gate's change list. Mitigated: the change list is `listing_diff` over two verified listings. The push rows in both modes, and the direct row for a hidden addition and a hidden deletion, were RED before the fix and each has a control |
| T-02-104 (mitigate) | src/sync/mod.rs | `listing_diff` refusing a legitimate push or missing a change. Mitigated: byte equality with `git diff-tree` in SHA-1 and SHA-256, both directions; `test_listing_diff`; two mutants killed. The deletion, benign-hook, content-kept amend, gc'd-remote and forged commit-graph rows pass, as does the full suite (452 + 114) |
| T-02-105 (mitigate) | src/sync/mod.rs | The staged gate's `git diff --cached` still reads fetched-head trees unchecked. Mitigated by the committed gate refusing before the push. The push rows assert that no remote head moved, that the remote's main does not list `hidden.repo`, and that its generic tree hashes to its id |
| T-02-SC (mitigate) | Cargo.toml, Cargo.lock | No crate added. `git diff --exit-code e39e860 -- Cargo.toml Cargo.lock` exits 0 |

No new security surface beyond the threat model.

## Open to the operator (recorded, not resolved here)

The planner read "apply the same id check to the trees those diffs read" as met at the committed gate, the binding check, and not at the staged gate's `git diff --cached`. That reading remains open to the operator's objection. If the operator wants the staged diff checked too, the same `listing_diff` over the fetched head and the staged tree is a small addition. It would change when the refusal comes, not whether it comes.

## Not claimed

- The staged gate's own diff. It still reads fetched-head trees without the check (above).
- A forged packed object. Not run.
- A forgery made by a real hook through `ude sync push`. The rows drive the engine.
- Hosted remotes, https, ssh and Windows.
- **docs/ARCHITECTURE.md, 02-20-PLAN.md, the controls on the final code and the phase gates belong to plan 02-26.**

## Known Stubs

None.

## Issues Encountered

- The first reading of the publication probe was wrong; see the RED Evidence caveat. It was resolved by a second probe before the SUMMARY.
- One probe log was briefly written to `/var/home/denniyahh/Github/v2.log`, outside the repo and outside scratch, by a wrong relative path. It was moved into the scratch directory at once, and the path no longer exists.

## User Setup Required

None. No external service configuration is required.

## Next Phase Readiness

Ready for 02-26: the closed-list rows, the controls that remove each part of the fix on the final code, docs/ARCHITECTURE.md and the phase gates.

## Self-Check: PASSED

- FOUND: src/sync/mod.rs. `fn listing_diff` is defined once in production.
- FOUND: bdf59f8, d936992 and 7c9d86f, all ancestors of HEAD. `git rev-list --count ebfc087..HEAD` = 3, and `evaluation-scope --plan 02-25` returns 3 commits.
- FOUND: target/gsd-red/02-25-t1-red.junit.xml and the 3 records, each `RED_EVIDENCE_OK`.
- Task 1 verify 1, 2, 3, 4 and 5 exited 0. Verify 1, 4 and 5 were re-run after the last commit; verify 2 and 3 ran on the GREEN code, and the gate's full suite covered the final code.
- Task 2 verify 1 exited 0. Verify 2 is the gated commit of 7c9d86f, and verify 3 exited 0 after the last commit.
- After every probe and run, `git status --porcelain` showed only this plan's own edits, and `git config --local --list | wc -l` = 22.
