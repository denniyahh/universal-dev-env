---
phase: 02-sanitizer-boundaries-security-hardening
plan: 23
subsystem: sync
tags: [git, publication-gate, object-integrity, controls, cargo-mutants, coverage]

requires:
  - phase: 02-sanitizer-boundaries-security-hardening
    provides: "02-22 read_verified_object on both push routes; 02-20 closed list; 02-21 control procedure"
provides:
  - "DEF-11 and DEF-12 in docs/ARCHITECTURE.md and 02-20-PLAN.md name the object-id channel and its tests"
  - "10 control rows on the final code, all same"
  - "Scoped mutation run: 49 listed, 35 caught, 0 missed, 14 unviable"
  - "Phase gates after the gate's last change, and a coverage report"
  - "The hand-edits todo carried forward with plan 02-23's items"
affects: [02 re-verification, docs/ARCHITECTURE.md section 3]

actuals:
  tokens: 8130
  tasks: 3
  commits: 2
plan_head_before: 0f1c0c1e5673db61c09d8abb88d0c4da1373e846
plan_head_after: 6b32f57fbda09cd38864b898d8cd3db5b6f29d1e

tech-stack:
  added: []
  patterns:
    - "Control edits applied by exact single-match string replacement from a script, so a drifted source fails loudly instead of taking a different edit"

key-files:
  created:
    - .planning/phases/02-sanitizer-boundaries-security-hardening/02-23-SUMMARY.md
  modified:
    - docs/ARCHITECTURE.md
    - .planning/phases/02-sanitizer-boundaries-security-hardening/02-20-PLAN.md
    - .planning/todos/pending/2026-11-01-hand-edits-owed-for-phase-02-after-re-verification.md

key-decisions:
  - "02-23: all 10 control rows on the final id-check code came out same (no corrected row, no dropped name, T-02-96 unused); no row needs re-running at re-verification"
  - "02-23: scoped mutation run over the id check (49 listed, 35 caught, 0 missed, 14 unviable, 0 timeouts) needed no new test"
  - "02-23: control rows 3 and 4 also remove the `verified` set that only the removed call used, because the set alone no longer infers a type (E0282); the first attempt at row 3 exited 101 and was redone, not restarted"

patterns-established: []

requirements-completed: [SEC-04]

coverage:
  - id: D1
    description: "DEF-11 and DEF-12 in docs/ARCHITECTURE.md and 02-20-PLAN.md name the object-id channel with its tests, byte-identical in both files; every DEF test name exists once and passes"
    requirement: SEC-04
    verification:
      - kind: other
        ref: "02-23-PLAN.md Task 1 verify 1 (rows equal ## The two rows in both files; numstat 2/2 and 3/3 since 4f9f043)"
        status: pass
      - kind: integration
        ref: "02-23-PLAN.md Task 1 verify 2 (230 names, each listed once; 232 tests run, 232 passed)"
        status: pass
      - kind: other
        ref: "02-23-PLAN.md Task 1 verify 3 (02-CONTEXT.md and 02-01..02-15 equal b8c54e9)"
        status: pass
    human_judgment: false
  - id: D2
    description: "Each part of the id check is load-bearing on the final code: 10 control rows, each failing exactly its named tests on their own assertions"
    requirement: SEC-04
    verification:
      - kind: integration
        ref: "02-23-PLAN.md Task 2 verify 1 over target/controls-02-23.tsv (same=10 corrected=0)"
        status: pass
    human_judgment: false
  - id: D3
    description: "No mutant of the id check survives the scoped tests"
    requirement: SEC-04
    verification:
      - kind: other
        ref: "cargo mutants (Task 2 verify 2), target/mutants-tree-02-23/mutants.out: 35 caught, missed.txt empty"
        status: pass
      - kind: other
        ref: "02-23-PLAN.md Task 2 verify 3 (index and local config unchanged, temp dir gone, no mutants.toml)"
        status: pass
    human_judgment: false
  - id: D4
    description: "Phase gates pass after the gate's last change: hermeticity with decoy, fmt, clippy, full cargo test, cargo deny, machete, Cargo files and sanitizer/template unchanged; coverage report recorded"
    requirement: SEC-04
    verification:
      - kind: integration
        ref: "02-23-PLAN.md Task 3 verify 1 (hermeticity: 354 sync tests passed, decoy 0 object or ref files)"
        status: pass
      - kind: other
        ref: "02-23-PLAN.md Task 3 verify 2-4 and 6"
        status: pass
    human_judgment: false
  - id: D5
    description: "The hand edits owed after re-verification, and the open operator item, are in the todo for a reader after a context reset"
    verification:
      - kind: other
        ref: "02-23-PLAN.md Task 3 verify 5 (section words, every 4f9f043 line kept, STATE.md concern line); negative control: a copy missing one old line is detected"
        status: pass
    human_judgment: true
    rationale: "Whether the todo's wording is enough for the re-verifier to act on is a reading judgment; the verify checks only named words and preserved lines"

duration: 31min
completed: 2026-11-04
status: complete
---

# Phase 02 Plan 23: Object-id channel in the closed list, controls and final gates Summary

**DEF-11 and DEF-12 now name the object-id channel with their tests, all 10 controls on the final id check failed exactly their named tests, the scoped mutation run missed nothing (35 caught, 14 unviable), and the phase gates pass on the final state; the hand edits are owed after re-verification and are in the todo.**

## Performance

- **Duration:** 31 min
- **Started:** 2026-11-04T14:37:15Z
- **Completed:** 2026-11-04T15:08:33Z
- **Tasks:** 3
- **Files modified:** 3 (docs/ARCHITECTURE.md, 02-20-PLAN.md, the todo), plus this SUMMARY

## Accomplishments

- DEF-11 and DEF-12 in docs/ARCHITECTURE.md and 02-20-PLAN.md are the two lines of `## The two rows`, byte for byte. One sentence was added to 02-20-PLAN.md's `How the rows were kept apart`. No NOT row changed, and NOT-10 says what it said.
- Every part of the id check is shown to be load-bearing on the final code. Ten control rows each removed one part, and each failed exactly the tests its row names.
- The scoped mutation run over the id check reported no missed mutant, so no new test was needed.
- The phase gates passed after the last code change, and the coverage report is recorded.

## Task Commits

1. **Task 1: name the object-id channel in DEF-11 and DEF-12**: `733a58a` (docs)
2. **Task 2: controls and the scoped mutation run**: no commit. The task changes no tracked file; its evidence is the gitignored output listed below. No mutant forced a test.
3. **Task 3: final gates, and the hand edits carried forward**: `6b32f57` (docs, the todo)

Each commit ran behind `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && timeout -k 30 1800 cargo test && git commit`. Both runs: 437 bin tests passed (1 ignored) and 114 e2e tests passed.

## Control table

All rows ran `cargo nextest run --bin ude --no-fail-fast sync::tests:: process::tests::` with the row's edit alone. Each exited 100 and failed exactly the row's set. **same: 10. corrected: 0.**

| # | Edit | State | Failing tests |
|---|------|-------|---------------|
| 1 | read_verified_object: a blob skips the id comparison | same | test_committed_gate_refuses_changed_blob_not_matching_its_id, test_direct_push_no_changes_refuses_blob_not_matching_its_id, test_direct_push_refuses_unchanged_blob_not_matching_its_id, test_pr_push_no_changes_refuses_blob_not_matching_its_id, test_pr_push_refuses_unchanged_blob_not_matching_its_id, test_read_verified_object_checks_bytes_against_the_id, test_read_verified_object_in_sha256_repository, test_read_verified_object_refuses_an_abbreviated_id |
| 2 | read_verified_object: a tree skips the id comparison | same | test_direct_push_no_changes_refuses_subtree_hiding_a_deleted_path, test_direct_push_no_changes_refuses_subtree_not_matching_its_id, test_direct_push_refuses_unchanged_subtree_not_matching_its_id, test_pr_push_no_changes_refuses_subtree_hiding_a_deleted_path, test_pr_push_no_changes_refuses_subtree_not_matching_its_id, test_pr_push_refuses_unchanged_subtree_not_matching_its_id, test_read_verified_object_checks_bytes_against_the_id |
| 3 | first_path_not_held: verify_ancestor_trees call removed | same | test_direct_push_no_changes_refuses_subtree_not_matching_its_id, test_direct_push_refuses_unchanged_subtree_not_matching_its_id, test_pr_push_no_changes_refuses_subtree_not_matching_its_id, test_pr_push_refuses_unchanged_subtree_not_matching_its_id |
| 4 | require_fetched_head_holds, staged-path loop: verify_ancestor_trees call removed | same | test_direct_push_no_changes_refuses_subtree_hiding_a_deleted_path, test_pr_push_no_changes_refuses_subtree_hiding_a_deleted_path |
| 5 | require_fetched_head_holds: IdMismatch refused with the held-content text | same | test_direct_push_no_changes_refuses_blob_not_matching_its_id, test_direct_push_no_changes_refuses_subtree_not_matching_its_id, test_pr_push_no_changes_refuses_blob_not_matching_its_id, test_pr_push_no_changes_refuses_subtree_not_matching_its_id |
| 6 | require_validated_content: IdMismatch refused with the held-content text | same | test_direct_push_refuses_unchanged_blob_not_matching_its_id, test_direct_push_refuses_unchanged_subtree_not_matching_its_id, test_pr_push_refuses_unchanged_blob_not_matching_its_id, test_pr_push_refuses_unchanged_subtree_not_matching_its_id |
| 7 | validate_committed_tree, changed-blob loop: IdMismatch refused with the inspect text | same | test_committed_gate_refuses_changed_blob_not_matching_its_id |
| 8 | listed_tree_ids returns an empty map | same | test_direct_push_no_changes_refuses_subtree_hiding_a_deleted_path, test_direct_push_no_changes_refuses_subtree_not_matching_its_id, test_direct_push_refuses_unchanged_subtree_not_matching_its_id, test_listed_tree_ids, test_pr_push_no_changes_refuses_subtree_hiding_a_deleted_path, test_pr_push_no_changes_refuses_subtree_not_matching_its_id, test_pr_push_refuses_unchanged_subtree_not_matching_its_id |
| 9 | read_verified_object: `--no-replace-objects` dropped from `cat-file <kind>` | same | test_committed_gate_reads_blob_past_replace_ref, test_push_refuses_hook_replaced_blob |
| 10 | read_verified_object: hash-object run with `self.git(None)` | same | test_read_verified_object_in_sha256_repository |

Each failure is the test's own assertion; I read the panic messages per row, beyond what the verify checks:
- `expected a hard push failure, got Ok(...)` in the push rows.
- `assert_eq` failures in the unit tests.
- The wrong refusal text in rows 5, 6, 7 and 9.

No failure came from a fixture or setup step. Row 9 kept the planned set: with the id check in place, the replaced blob is refused with the id-mismatch text, and the two named tests fail on that text.

Procedure:
- Start of table: the tree was clean outside `.planning`, then the table hash, the copies and the hash of the code before `mod tests {` were recorded.
- After each row, the three files were restored from the copies and `git diff --exit-code -- src` was confirmed.
- Task 2 verify 1 exited 0 (same=10 corrected=0). Evidence: target/controls-02-23.tsv, target/controls-02-23-rows/{1..10}.1.log, target/controls-02-23.nontest.sha256 and target/controls-02-23.table.sha256.

What the verify does not establish: that the edit made was the row's edit (the script applies each edit by an exact single-match replacement, see the decisions), or that a failure is the test's own assertion (checked by reading the panic messages, above).

No corrected rows.

Re-run at re-verification: none (no corrected rows). 02-21's list still stands.

## Mutation result

- **Listed:** 49 (`cargo mutants --list` with the plan's regex over src/sync/mod.rs and src/process.rs).
- **Run:** the plan's Task 2 verify 2 command, verbatim. Exit 0 after 346 s (about 6 minutes): baseline 44 s build plus 6 s test, then an auto-set per-test timeout of 32 s.
- **Result:** 35 caught, 0 missed, 14 unviable, 0 timeouts.
- **Caught, by function:**
  - read_verified_object 9
  - require_fetched_head_holds 7
  - listed_tree_ids 6
  - first_path_not_held 4
  - spawn_prepared 4
  - read_blob_by_id 3
  - require_validated_content 1
  - verify_ancestor_trees 1
- **Unviable:**
  - Nine `Default`/leak replacements of listed_tree_ids and first_path_not_held, which do not type-check.
  - `Ok(Default::default())` for run_bounded_with_input, spawn_prepared and run_local_with_input, because RunOutput has no Default.
  - `+` to `*` on the deadline in spawn_prepared.
- **Tests added:** none.
- **Output:** target/mutants-tree-02-23/mutants.out/ (caught.txt, missed.txt empty, unviable.txt, timeout.txt empty), target/mutants-tree-02-23-run.log, target/mutants-tree-02-23-config.before.
- **Cleanup and hermeticity:** /var/home/denniyahh/.cache/ude-mutants-02-23-tmp was removed. Task 2 verify 3 exited 0: nothing staged, local config equal to the saved copy, no `.cargo/mutants.toml`.
- **Not mutated here, as the plan states:** validate_committed_tree beyond its changed-blob arm (02-21 ran its other mutants; row 7 pins the arm), and src/main.rs.

## Gates (Task 3, in the plan's order)

| Gate | Exit | Result |
|------|------|--------|
| Hermeticity with decoy | 0 | `cargo test --bin ude sync::` under an inherited GIT_DIR: 354 passed, 84 filtered out. Decoy objects/ and refs/: 0 files; packed-refs absent. target/hermetic-gate removed before and after |
| Coverage | 0 | `cargo +nightly llvm-cov --bin ude --branch --summary-only --show-missing-lines` (nightly built, so `--branch` was kept). target/llvm-cov-target removed afterwards |
| cargo fmt --check | 0 | |
| cargo clippy --workspace --all-targets -- -D warnings | 0 | |
| cargo test (1800 s bound) | 0 | 437 passed, 1 ignored; e2e 114 passed |
| cargo deny check advisories bans sources (600 s bound) | 0 | advisories ok, bans ok, sources ok |
| git diff --exit-code 4f9f043 -- Cargo.toml Cargo.lock | 0 | Unchanged |
| git diff --quiet b8c54e9 HEAD -- src/sanitizer src/template | 0 | Unchanged: SEC-01 and SEC-03 regression-only |
| cargo machete | 0 | `Analyzing dependencies` printed, no unused dependency |
| Todo check (verify 5) | 0 | Negative control: a copy with one 4f9f043 line deleted is detected |
| Final check (verify 6) | 0 | Re-run after the todo commit. The code before `mod tests {` equals the control-start record, src/tests/docs are committed, and nothing outside `.planning` shows in `git status` |

The todo commit (6b32f57) came after the gate list and touched only `.planning`. `git diff --stat 733a58a HEAD -- . ':(exclude).planning'` is empty, so nothing the gates read changed after they ran. The commit itself ran behind fmt, clippy and the full `cargo test`.

### Coverage (no threshold)

| File | Regions | Lines | Branches |
|------|---------|-------|----------|
| process.rs | 95.82% (22 of 526 missed) | 96.82% (14 of 440 missed) | 76.92% (6 of 26 missed) |
| sync/mod.rs | 96.36% (506 of 13907 missed) | 95.78% (474 of 11228 missed) | 77.31% (147 of 648 missed) |

Uncovered lines inside the functions of Task 2's regex:
- process.rs:187-188, spawn_prepared: the child has no stdin pipe to write to (an internal-misuse path).
- process.rs:210-211, spawn_prepared: `try_wait` itself failing.
- sync/mod.rs:2823, first_path_not_held: verify_ancestor_trees returning Unreadable (a tree above a recorded path that cannot be read).
- sync/mod.rs:2911, 2913-2916, require_fetched_head_holds: `ls-tree` failing to spawn or exiting non-zero.
- sync/mod.rs:2918-2920, 2922, 2924, 2927, require_fetched_head_holds: the fetched head's listing unparseable or repeating a path, on this route.
- sync/mod.rs:2970, require_fetched_head_holds: an unreadable tree above a staged deletion.
- sync/mod.rs:3012, 3020, 3035, read_verified_object: a `cat-file -t`, `cat-file` or `hash-object` spawn error.
- sync/mod.rs:3022, 3037, read_verified_object: `cat-file <kind>` or `hash-object` exiting non-zero.

None in run_bounded_with_input, run_local_with_input, read_blob_by_id, listed_tree_ids, verify_ancestor_trees or require_validated_content. Report: target/llvm-cov-02-23.txt, log target/llvm-cov-02-23.log.

## Files Created/Modified

- `docs/ARCHITECTURE.md`: DEF-11 and DEF-12 replaced by `## The two rows`.
- `.planning/phases/02-sanitizer-boundaries-security-hardening/02-20-PLAN.md`: the same two rows, plus the NOT-10 against DEF-11 and DEF-12 sentence in `How the rows were kept apart`.
- `.planning/todos/pending/2026-11-01-hand-edits-owed-for-phase-02-after-re-verification.md`: a new section, `## Added by plan 02-23`. Every earlier line is kept and the todo stays in pending.

## Decisions Made

- The control edits were applied by a script of exact single-match string replacements. A replacement that matched zero or two places would have stopped the run; each was dry-run once on a copy first.
- Rows 3 and 4 also delete `let mut verified = HashSet::new();`. Only the removed call used that set, so without the call it no longer infers a type (E0282). Row 3's first run exited 101 on that error. The plan defines exit 101 as a wrong edit to redo, not a restart, so the row was redone. The log and line were overwritten, and rows 4 to 10 continued. Removing a declaration that only the removed call used is still "remove the call". The plan's `#[allow(dead_code)]` provision covers the same kind of fallout for functions.
- Row 1's edit is `if kind != "blob" && printed != oid.as_bytes()` and row 2's is the same with `"tree"`. hash-object still runs and its result is ignored for that kind, which is "return the bytes read without the comparison".
- Row 8 returns `Some(HashMap::new())` after the loop, so the parse-failure `None` paths stay as they were.

## Deviations from Plan

**1. [Rule 3 - Blocking] Row 3's first edit did not compile and was redone**
- **Found during:** Task 2 (row 3)
- **Issue:** removing the verify_ancestor_trees call left `let mut verified = HashSet::new();` with no type (E0282), and the run exited 101.
- **Fix:** rows 3 and 4 also remove that declaration. Exit 101 is a redo under the plan, not a restart.
- **Files modified:** none tracked; the edit script is in the session scratch directory.
- **Verification:** the redo of row 3 exited 100 with exactly its set. Task 2 verify 1 exited 0.

**2. [Minor] `PROPTEST_DISABLE_FAILURE_PERSISTENCE=1` on the coverage run**
- The plan's coverage command lacks it. It was added so that a failing property could not write proptest-regressions/ into the checkout. It does not change what is measured.

**Total deviations:** 1 auto-fixed (blocking), 1 minor addition. **Impact:** none on the results. No code change, no table change.

## Issues Encountered

- The auto-mode safety classifier twice returned no verdict on a read-only command. It was retried and succeeded; this had no effect on the run.

## Threat Flags

| Flag | File | Description |
|------|------|-------------|
| threat_flag: T-02-94 | src/sync/mod.rs | mitigate. Mitigated: after each row the files were restored from copies and `git diff --exit-code -- src` confirmed; the hash of the code before `mod tests {` matches the control-start record (Task 2 verify 1 and Task 3 verify 6) |
| threat_flag: T-02-95 | .git (index, config) | mitigate. Mitigated: `git diff --cached --quiet` and the local config equal to target/mutants-tree-02-23-config.before after the run (22 entries throughout) |
| threat_flag: T-02-97 | docs/ARCHITECTURE.md | mitigate. Mitigated: Task 1 verify 1 and 2 (rows equal in both files, only those lines changed, all 230 DEF names listed once and run, 232 passed) |
| threat_flag: T-02-SC | Cargo.toml, Cargo.lock | mitigate. Mitigated: unchanged since 4f9f043 |

T-02-96 is unused: no row is corrected and no name was dropped. No new security surface: the plan adds no production code.

## Known Stubs

None.

## Scope of these results

- The closed list is closed by decision and is not proven exhaustive.
- These results do not claim:
  - re-verification's verdict;
  - any hosted remote, or Windows;
  - that an edit outside the control table would fail a test (for example dropping `--literally` or `--no-filters`);
  - mutation of validate_committed_tree beyond 02-21's run, or of src/main.rs;
  - the base-side channel.
- The hand edits owed after re-verification are in `.planning/todos/pending/2026-11-01-hand-edits-owed-for-phase-02-after-re-verification.md`: CR-400 to `fixed`, and what to tell the verifier.
- One item is still open with the operator, recorded in the same todo: the base-side channel from 02-22-PLAN.md's `Not in this plan` bullet. The choice is a further gap plan or a NOT row.

## Kept and removed paths

- **Kept** (gitignored, cited above or read by a verify command):
  - target/controls-02-23.tsv, target/controls-02-23-rows/, target/controls-02-23.nontest.sha256, target/controls-02-23.table.sha256
  - target/mutants-tree-02-23/, target/mutants-tree-02-23-run.log, target/mutants-tree-02-23-config.before
  - target/llvm-cov-02-23.txt, target/llvm-cov-02-23.log
- **Removed:** target/hermetic-gate, target/llvm-cov-target, /var/home/denniyahh/.cache/ude-mutants-02-23-tmp, and the controls' source copies under the session scratch directory.

## User Setup Required

None.

## Next Phase Readiness

- Phase 02 is ready for re-verification. The SEC-04 flip in REQUIREMENTS.md is to be reverted if re-verification finds gaps.

## Self-Check: PASSED

- FOUND: docs/ARCHITECTURE.md (`not_matching_its_id` on 2 lines), the todo section, and every kept target/ path listed above.
- FOUND: 733a58a and 6b32f57, both ancestors of HEAD. `check evaluation-scope --plan 02-23 --commits-only` resolved both.

---
*Phase: 02-sanitizer-boundaries-security-hardening*
*Completed: 2026-11-04*
