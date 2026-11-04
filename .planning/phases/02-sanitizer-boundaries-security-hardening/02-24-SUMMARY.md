---
phase: 02-sanitizer-boundaries-security-hardening
plan: 24
subsystem: sync-publication-gate
tags: [git, object-integrity, tree-listing, sha256, tdd, rust]

requires:
  - phase: 02-sanitizer-boundaries-security-hardening
    provides: "02-22's read_verified_object, ObjectDefect, OBJECT_ID_MISMATCH and the forged-object test helpers; 02-23's controls"
provides:
  - "parse_tree_object, ListingDefect, SyncEngine::verified_tree_listing, first_path_under: both publication routes decide only from tree bytes hashed against their ids, root included"
  - "Both routes read their commit through read_verified_object (kind commit)"
  - "A cfg(test) listing hook (with_after_tree_listing) and the ListingSwap test helper"
  - "Tests that pin the gate's own root-tree check (WR-501) and the stdin helper's deadline (WR-502)"
affects: [02-25, 02-26]

actuals:
  tokens: 17400
  tasks: 3
  commits: 5
plan_head_before: 646621bc41a77f9247769f63efa791d1ca3cc5f5
plan_head_after: 680ac12229aeae2964d222591cbb36698860d0eb

tech-stack:
  added: []
  patterns:
    - "Decide from the bytes that were hashed: a listing is built in process from verified reads, never taken from a second, unchecked git read"
    - "A cfg(test) hook at the read boundary makes a between-reads swap deterministic"

key-files:
  created: []
  modified:
    - src/sync/mod.rs
    - src/process.rs

key-decisions:
  - "The per-task RED-commit gate tripped on Tasks 1 and 2 as tdd.md's Rust gap (#4379) predicts; continued under the operator's 2026-11-04 approval because the substitute evidence held (RED commit by exact subject, ancestor of HEAD, exact markers; every record RED_EVIDENCE_OK). last_gate_trip was not written: state.update reported the field absent from STATE.md"
  - "An id-mismatch refusal names the first recorded path (sorted), then a staged or allowlisted path, under the mismatched tree, or none; a name from the tree is never printed"
  - "The gate script binds PLAN_ID=24 so its --plan argument is 02-24; its STATE key would be 24/<task>"

patterns-established:
  - "Tree walk: an explicit stack of pending entries, pushed in reverse stored order, gives git's pre-order listing with no recursion"

requirements-completed: [SEC-04]

coverage:
  - id: D1
    description: "Empty-status route: a fetched-head subtree swapped back after the listing is refused in both modes, the remote untouched (WR-500)"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_direct_push_no_changes_refuses_subtree_rewritten_only_while_listed"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_pr_push_no_changes_refuses_subtree_rewritten_only_while_listed"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_direct_push_no_changes_refuses_deletion_hidden_only_while_listed"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_pr_push_no_changes_refuses_deletion_hidden_only_while_listed"
        status: pass
    human_judgment: false
  - id: D2
    description: "Committed route: an unchanged subtree swapped back after the listing is refused in both modes; a commit object not matching its id is refused (WR-500, T-02-100)"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_direct_push_refuses_unchanged_subtree_rewritten_only_while_listed"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_pr_push_refuses_unchanged_subtree_rewritten_only_while_listed"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_committed_gate_refuses_commit_not_matching_its_id"
        status: pass
    human_judgment: false
  - id: D3
    description: "The built listing is byte-identical to git's ls-tree -r -t -z --full-tree on honest trees (SHA-1 and SHA-256), and the raw-tree decoder refuses what git refuses"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_verified_tree_listing_matches_ls_tree"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_parse_tree_object"
        status: pass
      - kind: unit
        ref: "cargo test (full suite, 447 + 114 passed)"
        status: pass
    human_judgment: false
  - id: D4
    description: "WR-501: the gate's own root-tree check is pinned by a direct test with a same-bytes-own-id control; the two push rows discriminate a forged root (git status) from none"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_fetched_head_holds_refuses_root_tree_not_matching_its_id"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_direct_push_refuses_fetched_root_tree_not_matching_its_id"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_pr_push_refuses_fetched_root_tree_not_matching_its_id"
        status: pass
    human_judgment: false
  - id: D5
    description: "WR-502: the stdin helper's deadline has a test that the synchronous-write mutant fails"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/process.rs#test_run_bounded_with_input_times_out_when_the_child_neither_reads_nor_exits"
        status: pass
      - kind: other
        ref: "target/controls-02-24-x1-mutant.log (mutant: exit=100, only the new test failed)"
        status: pass
    human_judgment: false

duration: 26min
completed: 2026-11-04
status: complete
---

# Phase 2 Plan 24: Hashed-bytes tree listing for both publication routes Summary

**Both publication gates now build their tree listing in process from tree bytes hashed against their ids, root included, starting from a commit read through the same check. A tree object swapped between git's listing and the id check (review WR-500) is refused on both routes in both modes. WR-501 and WR-502 each got a test that fails when its property is removed.**

## Performance

- **Duration:** about 26 min
- **Started:** 2026-11-04T17:08:42Z
- **Completed:** 2026-11-04T17:34Z
- **Tasks:** 3
- **Files modified:** 2

## What changed and what it means

- **WR-500 is closed on both routes.** Each route used to take its blob ids, modes and deleted-path check from an `ls-tree` read, and git does not check subtrees below the root on that read. The id check then hashed a second read and threw those bytes away. Now `verified_tree_listing` reads each tree with `read_verified_object` and builds the exact `ls-tree -r -t -z --full-tree` records from those bytes. The existing parsers and refusal texts run unchanged on that output. `git ls-tree` no longer runs on either route: the production count of `"ls-tree"` went from 2 to 0.
- **The commit is read against its id on both routes.** On the committed route, the parent, message, header and tree binding now come from those bytes (T-02-100).
- **`verify_ancestor_trees`, `listed_tree_ids` and the `listed_trees` parameter are removed.** Every listed tree is hashed when it is read, so the ancestor re-reads were redundant.
- **The six seam rows are deterministic through the engine.** A `cfg(test)` hook runs right after each route's listing and puts the honest bytes back. The hook is absent from non-test builds.

## Task Commits

1. **Task 1 (tracer): empty-status route.** RED `c9e74a0` (test), GREEN `146fad2` (feat).
2. **Task 2: committed route.** RED `0737048` (test), GREEN `7ee7eb3` (feat).
3. **Task 3: WR-501 and WR-502 tests and the plan's gates.** `680ac12` (test).

Plan metadata: see the final docs commit.

## Files Created/Modified

- `src/sync/mod.rs`, production:
  - New: `TreeEntry`, `parse_tree_object`, `canonical_mode` and its mode constants, `ListingDefect`, `lower_hex`, `first_path_under`, `SyncEngine::verified_tree_listing`, `SyncEngine::push_tree_entries` (the walk's per-tree step), and `SyncEngine::after_tree_listing`. Under `cfg(test)`: the `after_tree_listing` field and `with_after_tree_listing`.
  - Changed: `require_fetched_head_holds` and `validate_committed_tree` read their commit through `read_verified_object` and their listing through `verified_tree_listing`. `first_path_not_held` and `require_validated_content` lose `listed_trees`. Doc comments are updated, including `read_verified_object`'s kinds.
  - Removed: `verify_ancestor_trees`, `listed_tree_ids`.
- `src/sync/mod.rs`, tests:
  - New helpers: `SavedObjectFile`, `ListingSwap`, `checkout_generic_tree_id`, `raw_tree_entry`, `raw_ls_tree`, `raw_id`, `built_listing`, `holds_bytes`, `root_listing_validated_blob`.
  - New tests: the 6 swap rows, `test_committed_gate_refuses_commit_not_matching_its_id`, `test_parse_tree_object`, `test_verified_tree_listing_matches_ls_tree`, `test_fetched_head_holds_refuses_root_tree_not_matching_its_id`.
  - Reworked (same names): `test_direct_push_refuses_fetched_root_tree_not_matching_its_id` and `test_pr_push_refuses_fetched_root_tree_not_matching_its_id`. `test_committed_gate_requires_validated_content_refuses_tag_object` now makes the narrower call.
  - Removed: `test_listed_tree_ids`. Its helper `tree_listing_record` stays, because `test_tree_listing_blobs` uses it.
- `src/process.rs`: `test_run_bounded_with_input_times_out_when_the_child_neither_reads_nor_exits`.
- Evidence, gitignored: `target/gsd-red/02-24-t1-*` (4 records and `02-24-t1-red.junit.xml`), `target/gsd-red/02-24-t2-*` (2 records and `02-24-t2-red.junit.xml`), and `target/controls-02-24-x1-mutant.log`.

## New refusal format strings

Empty-status route. `PREFIX` = `Publication gate violation: git status reports nothing to push, but`:
- `"{PREFIX} the fetched head's commit {OBJECT_ID_MISMATCH}"`
- `"{PREFIX} an object read for {path} {OBJECT_ID_MISMATCH}"`, where `path` is the first recorded path (sorted), then the first staged path with no record, that lies under the mismatched tree.
- `"{PREFIX} a tree of the fetched head {OBJECT_ID_MISMATCH}"` when no such path lies under it.
- Reused: `"{PREFIX} the fetched head's tree cannot be listed"`, for an unreadable commit or tree, or a missing or non-UTF-8 root line. Also reused: the existing unparseable text, for a tree the decoder refuses.

Committed route:
- `"Post-commit publication gate violation: the commit to publish {OBJECT_ID_MISMATCH}"`
- `"Post-commit publication gate violation: failed to read commit {commit}"` for an unreadable commit. It no longer carries git's stderr excerpt, because `read_verified_object` returns no stderr. The plan prescribed this text.
- `"Post-commit publication gate violation: unparseable commit {commit}"` for a missing or non-UTF-8 root line.
- `"Post-commit publication gate violation: failed to list the tree of {commit}"` for an unreadable tree. `"... unparseable tree entry in {commit}"` for a tree the decoder refuses.
- `"Post-commit publication gate violation: an object read for {path} {OBJECT_ID_MISMATCH}"`, where `path` is the first recorded path (sorted), then the first allowlisted path (sorted), under the mismatched tree.
- `"Post-commit publication gate violation: a tree of the commit to publish {OBJECT_ID_MISMATCH}"` when none lies under it.

Each refusal names only a recorded, staged or allowlisted path, or no path at all. `{commit}` is the id UDE resolved itself. No name, id or bytes read from a tree is printed (DEF-14's rule).

## RED Evidence

The records were built from the nextest JUnit report of each run and classified with `gsd-tools check tdd-red-evidence`. All 6 returned `RED_EVIDENCE_OK / target_test_failed`. Each run started after its recorded start time, and its report is newer than that start.

**Task 1**, on e39e860 plus the seam (exit 100, 4 of 4 failed, each at `assert_empty_status_refused`):
- test_direct_push_no_changes_refuses_subtree_rewritten_only_while_listed: `expected a hard push failure, got Ok("No changes to push to remote repository.")`
- test_pr_push_no_changes_refuses_subtree_rewritten_only_while_listed: `expected a hard push failure, got Ok("No changes to push to remote repository.")`
- test_direct_push_no_changes_refuses_deletion_hidden_only_while_listed: `expected a hard push failure, got Ok("No changes to push to remote repository.")`
- test_pr_push_no_changes_refuses_deletion_hidden_only_while_listed: `expected a hard push failure, got Ok("No changes to push to remote repository.")`

**Task 2**, on 146fad2 plus the seam call (exit 100, 2 of 2 failed, each at `assert_push_refused`):
- test_direct_push_refuses_unchanged_subtree_rewritten_only_while_listed: `expected a hard push failure, got Ok("Directly pushed changes to remote branch 'main'.")`
- test_pr_push_refuses_unchanged_subtree_rewritten_only_while_listed: `expected a hard push failure, got Ok("Pushed changes to branch 'ude-sync-14f11ae4-1793812910'. Create a PR on GitHub to merge into 'main'.")`

**Semantic assessment.** Each target ran and failed on its own refusal assertion. Three assertions run before that one, and all passed:
- the walk inside the step saw the forgery;
- the hook ran;
- the hook restored the honest tree, checked by hashing it in the checkout.

So the false Ok is the one WR-500 describes: the listing read forged bytes, and the id check hashed honest ones. No row failed in a fixture step, timed out or failed to build.

**A note on the Task 1 records.** The first gated commit of Task 1 failed clippy (`type_complexity` on `ListingSwap`'s slot type). The fix was a type alias only. The RED rows were re-run on the corrected code and the records rebuilt, so the committed RED code is exactly what the records measured. The failure lines were identical.

## Tests that passed on first run, and why

These pin behaviour that exists once the fix is in. They are not RED:
- `test_committed_gate_refuses_commit_not_matching_its_id`, `test_parse_tree_object` and `test_verified_tree_listing_matches_ls_tree` were written beside Task 2's fix, as the plan prescribes.
  - The listing test carries its own negative control. After one subtree's object file is given another tree's bytes, `ls-tree` still exits 0, while the built listing returns `IdMismatch(b"templates/projects/generic")`.
  - The fixture also asserts that git's listing holds each claimed case: the legacy 100664 entry listed as 100644, the empty subtree, the gitlink, the symlink, the executable, and the names with 0xFF and with a newline. So the byte-equality covers those cases and is not a vacuous comparison.
- `test_fetched_head_holds_refuses_root_tree_not_matching_its_id` holds its control in the same test. The same root bytes under their own id read as held (Ok). So the refusal on the forged base root comes from the id check alone.
- The two reworked root push rows assert `git status failed`, which is git's own check and runs before the gate. Their control, the same step without the forgery, reaches the gate and is refused as not held.
- `test_run_bounded_with_input_times_out_when_the_child_neither_reads_nor_exits`. **Mutant control (WR-502):** spawn_prepared's detached writer was replaced with a write on the calling thread, followed by `drop(stdin)`. Run under `timeout -k 30 300`, exit 100:
  - only the new test failed: `a child that neither reads nor exits returned Ok after 10.005900518s: ExitStatus(unix_wait_status(0))`;
  - `test_run_bounded_with_input_feeds_stdin` and `test_run_bounded_with_input_returns_when_the_child_ignores_stdin` passed.

  `src/process.rs` was restored from the copy. Afterwards it held no `drop(stdin)` and no removed lines, and after the commit `git diff --exit-code -- src/process.rs` exits 0. The copy was deleted.

What these do not establish:
- Plan 02-26 owns the controls that remove each part of the fix on the final code. Task 3's direct root test was not run against e39e860 here.
- The plan's interfaces record the pre-fix behaviour of step 3 (refused with `cannot be listed`). That was not re-run.

## Cost (Task 1 baseline vs after Task 3)

| Run | Tests | nextest time | Wall |
|-----|-------|--------------|------|
| Before Task 1 (e39e860 code) | 341 | 99.7 s | 100.0 s |
| After Task 1 GREEN | 345 | 79.1 s | not taken |
| After Task 2 GREEN | 349 | 99.6 s | not taken |
| After Task 3 | 351 | 76.6 s | 76.8 s |

**Trees per listing in the seeded fixture: 4.** That is 3 `040000 tree` records in `git ls-tree -r -t` (templates, projects, generic) plus the root, measured on a scratch copy of `seeded_bare_remote`'s commit. So one listing costs 4 × 3 = 12 git runs, plus 3 for the commit read.

**What these numbers do not show:** the module times swing by about 25% between consecutive runs (79 s to 99.6 s to 76.6 s, with 2 to 4 tests added each time). That noise hides the per-tree cost. The numbers support only "no regression visible above run-to-run noise", not "no added cost".

## Decisions Made

- The id-mismatch path is chosen from sorted recorded paths, then from staged-without-record paths (empty-status route) or sorted allowlisted paths (committed route). Sorting makes the named path deterministic, because the record is a HashMap. This is within the plan's "the recorded paths, then ..." rule.
- `verified_tree_listing` delegates each tree's read and decode to `push_tree_entries`, so the walk stays iterative and each function stays short. The plan's counts (one definition, one or two call lines) hold.

## Deviations from Plan

**1. [Rule 3 - Blocking] The clippy type_complexity failure on ListingSwap**
- **Found during:** Task 1, first gated RED commit
- **Issue:** `Arc<Mutex<Option<(PathBuf, Vec<u8>)>>>` tripped `-D clippy::type-complexity`, so the gate refused the commit. Nothing was committed.
- **Fix:** a `SavedObjectFile` type alias. The RED rows were re-run, the records rebuilt, and the gate retried.
- **Files modified:** src/sync/mod.rs
- **Committed in:** c9e74a0

**2. [Note] The commit gate runs `cargo test` with `PROPTEST_DISABLE_FAILURE_PERSISTENCE=1`**
- This keeps proptest regression files out of the working tree, which Task 3's last verify requires to be clean. The gate is otherwise exactly `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && timeout -k 30 1800 cargo test && git commit`, and the commit is gated on its exit code.

**Total deviations:** 1 auto-fixed (blocking), 1 note. **Impact:** none on behaviour or scope.

## TDD Gate Compliance

**Commit order.** Each feat commit descends from its RED commit (`git merge-base --is-ancestor`):

| Task | RED commit | GREEN commit | RED markers in the RED commit |
|------|------------|--------------|-------------------------------|
| 1 | `c9e74a0` test(02-24): add RED rows for a head subtree swapped after listing | `146fad2` feat(02-24): list the fetched head from hashed tree bytes | 4 × `#[ignore = "RED: fails until 02-24 Task 1 fix"]` |
| 2 | `0737048` test(02-24): add RED rows for a commit subtree swapped after listing | `7ee7eb3` feat(02-24): list the commit to publish from hashed tree bytes | 2 × `#[ignore = "RED: fails until 02-24 Task 2 fix"]` |
| 3 | `680ac12` test(02-24): pin the root-tree check and the stdin deadline (tests only, no tdd attribute, so the per-task gate does not run) | none | none |

No RED marker of this plan survives in src.

**The per-task RED-commit gate (execute-phase.md, `TDD gate`), run unchanged before each GREEN.**
- **How it was bound.** It ran as a script that copies the gate block verbatim, with `TDD_MODE=true`, `PHASE_NUMBER=02`, `PLAN_ID=24` and the plan file as `TASK_FILE`. `task.is-behavior-adding` returned true.
- **Task 1.** Trip line, verbatim: `TDD GATE TRIPPED: missing RED commit for 24/1`.
  - `last_gate_trip`: **none written**. `state.update` printed `{"updated": false, "reason": "Field \"last_gate_trip\" not found in STATE.md"}`, and STATE.md was byte-identical before and after.
  - Substitute evidence: `c9e74a036a4c715ed91b9dcb002e97c6697efcd5`, found by its exact subject (1 match), an ancestor of HEAD, with 4 markers. All 4 records are `RED_EVIDENCE_OK`.
  - Control: the same `evaluation-scope` query without the test-file pathspecs returned `c9e74a036a test(02-24): add RED rows for a head subtree swapped after listing`. So the trip comes from the pathspec (the Rust gap #4379), not from a missing commit.
- **Task 2.** Trip line, verbatim: `TDD GATE TRIPPED: missing RED commit for 24/2`.
  - `last_gate_trip`: **none written**, for the same reason; STATE.md was unchanged.
  - Substitute evidence: `0737048beb9f2da47c4a9ef7f1db8aedc2b7ca3c` (1 match), an ancestor of HEAD, with 2 markers. Both records are `RED_EVIDENCE_OK`.
- GREEN continued under the operator's approval of 2026-11-04. No pathspec was widened and no file under tests/ was touched.

## Threat Flags

| Flag | File | Description |
|------|------|-------------|
| T-02-98 (mitigate) | src/sync/mod.rs | Decisions taken from a listing git read while the id check hashed a second read (WR-500). Mitigated: both routes decide only from `verified_tree_listing`, built from hashed tree bytes from a commit read the same way. Six seam rows in both modes, RED before the fix, each with a control |
| T-02-99 (mitigate) | src/sync/mod.rs | The built listing differing from git's on a legitimate tree. Mitigated: the decoder follows git's tree decoder and canon_mode and fails closed on an overflowing mode. Byte equality with git in SHA-1 and SHA-256 repositories covers a legacy mode, a gitlink, a symlink, odd names, an empty subtree and an empty commit. The no-change, deletion, benign-hook and gc'd-remote rows still pass, and so does the full suite |
| T-02-100 (mitigate) | src/sync/mod.rs | A commit object swapped on the committed route. Mitigated: the commit is read through `read_verified_object` (kind commit). Pinned by `test_committed_gate_refuses_commit_not_matching_its_id`, which includes the opposite direction |
| T-02-101 (mitigate) | src/sync/mod.rs | A new refusal printing tree-chosen text. Mitigated: `first_path_under` names only a recorded, staged or allowlisted path, otherwise the text is fixed. The rows assert GATE_REL |
| T-02-102 (mitigate) | src/sync/mod.rs | A deep or wide tree. Mitigated: the walk is iterative, each tree is read once per id per call, and each read is bounded by the local tier's timeout |
| T-02-SC (mitigate) | Cargo.toml, Cargo.lock | No crate added. `git diff --exit-code e39e860 -- Cargo.toml Cargo.lock` exits 0 |

No new security surface beyond the threat model. The listing hook exists only under `cfg(test)`. In other builds, `after_tree_listing` is an empty function.

## Not claimed (from the plan)

- An object file overwritten after the gate's last read, during the push: NOT-10, unchanged.
- **The base-side channel** (a fetched-head tree hiding an added file from the diff) belongs to **plan 02-25**. The committed route's `diff-tree` read stays as it was.
- **docs/ARCHITECTURE.md, 02-20-PLAN.md, the controls on the final code and the phase gates** belong to **plan 02-26**.
- A forged packed object was not run. A forgery made by a real hook through `ude sync push` was not run either; the rows drive the engine through the seam. Hosted remotes, https, ssh and Windows are not covered. Review IN-501 was not taken.

## Known Stubs

None.

## Issues Encountered

The clippy failure on the first RED commit attempt; see Deviation 1.

## User Setup Required

None. No external service configuration is required.

## Next Phase Readiness

Ready for 02-25 (the base-side channel). `verified_tree_listing` and `read_verified_object` are the checks it can reuse for the base-side reads.

## Self-Check: PASSED

- FOUND: src/sync/mod.rs, src/process.rs. `parse_tree_object`, `verified_tree_listing` and `first_path_under` are each defined once in production. `verify_ancestor_trees` and `listed_tree_ids` are absent from the file.
- FOUND: c9e74a0, 146fad2, 0737048, 7ee7eb3, 680ac12, all ancestors of HEAD. `git rev-list --count 646621b..HEAD` = 5.
- Every `<verify>` command of Tasks 1, 2 and 3 exited 0. Task 3's working-tree invariant was run after the last code commit.
