---
phase: 02-sanitizer-boundaries-security-hardening
plan: 26
subsystem: sync-publication-gate
tags: [git, object-integrity, closed-list, controls, mutation-testing, coverage, rust]

requires:
  - phase: 02-sanitizer-boundaries-security-hardening
    provides: "02-24's verified_tree_listing, first_path_under and the commit read through read_verified_object; 02-25's listing_diff and the fetched head's verified listing on the committed route"
provides:
  - "DEF-09, DEF-11 and DEF-12 in docs/ARCHITECTURE.md and 02-20-PLAN.md naming the swap, commit, root and base-side channels with their tests"
  - "13 control rows on the final code, all `same`; a scoped mutation run with 0 missed"
  - "The phase's final gates after the gate's last change, and a coverage report over src/sync/mod.rs and src/process.rs"
  - "The hand-edits todo carried forward for plans 02-24 to 02-26"
affects: [phase-02-re-verification]

actuals:
  tokens: 12320
  tasks: 3
  commits: 2
plan_head_before: f6c3cf30f341024d6ee2528532582ddc8556d68c
plan_head_after: a5cd7ae

tech-stack:
  added: []
  patterns:
    - "Control edits applied by exact single-match replacement from a script, restored from copies, with `git diff --exit-code -- src` after every restore"

key-files:
  created: []
  modified:
    - docs/ARCHITECTURE.md
    - .planning/phases/02-sanitizer-boundaries-security-hardening/02-20-PLAN.md
    - .planning/todos/pending/2026-11-01-hand-edits-owed-for-phase-02-after-re-verification.md

key-decisions:
  - "All 13 control rows on the final code came out `same` (no corrected row, T-02-109 unused); nothing to re-run at re-verification beyond 02-21's list"
  - "Scoped mutation run: 69 listed, 62 caught, 0 missed, 7 unviable, 0 timeouts, no test added"
  - "The staged gate's git diff --cached interpretation is recorded in the todo as answered by the operator (2026-11-04), no longer open"

patterns-established: []

requirements-completed: [SEC-04]

coverage:
  - id: D1
    description: "DEF-09, DEF-11 and DEF-12 in docs/ARCHITECTURE.md and 02-20-PLAN.md equal 02-26-PLAN.md's three rows byte for byte; every named test exists exactly once and passes"
    requirement: SEC-04
    verification:
      - kind: other
        ref: "02-26-PLAN.md Task 1 verify 1 (rows equal, only 3+3 and 4+4 lines changed since e39e860)"
        status: pass
      - kind: integration
        ref: "02-26-PLAN.md Task 1 verify 2 (closed-list run: 247 tests run, 247 passed)"
        status: pass
      - kind: other
        ref: "02-26-PLAN.md Task 1 verify 3 (02-CONTEXT.md and plans 02-01 to 02-15 equal b8c54e9)"
        status: pass
    human_judgment: false
  - id: D2
    description: "Each part of the id check, the verified listing, the in-process diff and the stdin deadline is load-bearing on the final code: 13 control rows, each failing exactly its named tests"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "target/controls-02-26.tsv and target/controls-02-26-rows/*.1.log; 02-26-PLAN.md Task 2 verify 1 (same=13 corrected=0)"
        status: pass
    human_judgment: false
  - id: D3
    description: "No mutant of the scoped functions survives the scoped tests"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "target/mutants-tree-02-26/mutants.out (62 caught, 0 missed, 7 unviable); 02-26-PLAN.md Task 2 verify 2 and 3"
        status: pass
    human_judgment: false
  - id: D4
    description: "Phase gates on the final state: hermeticity with decoy, fmt, clippy, full cargo test, cargo deny, cargo machete, Cargo files and sanitizer/template unchanged, working tree clean"
    requirement: SEC-04
    verification:
      - kind: other
        ref: "02-26-PLAN.md Task 3 verify 1 to 6, all exit 0"
        status: pass
    human_judgment: false

duration: 60min
completed: 2026-11-04
status: complete
---

# Phase 2 Plan 26: Closed-list rows, controls and final gates for the hashed-bytes gate Summary

**DEF-09, DEF-11 and DEF-12 now name the channels plans 02-24 and 02-25 closed, with their tests. All 13 controls on the final code fail exactly their named tests, and the scoped mutation run missed nothing (62 caught, 7 unviable). The phase's final gates pass after the gate's last change. The hand edits are owed after re-verification, and they are listed in the todo.**

## Performance

- **Duration:** about 60 min
- **Started:** 2026-11-04T18:44:25Z
- **Completed:** 2026-11-04T19:44Z
- **Tasks:** 3
- **Files modified:** 3 (docs/ARCHITECTURE.md, 02-20-PLAN.md, the todo). No source file was changed: every control edit was restored.

## What this establishes, and what it does not

- **Established:** each of the 13 edits in Task 2's table, made alone on the final code, makes exactly the named tests fail (exit 100), and every other test in `sync::tests::` and `process::tests::` still passes. Every failure is a panic inside a test module: 40 distinct panic sites, all below `mod tests {`. A classifier control showed that a production line is flagged. No row hung and no row failed to build.
- **Not established:** that each panic is the test's refusal assertion rather than an earlier assertion in the same test. The check above only rules out production-code panics. That the closed list is exhaustive: it is closed by decision, not proven. Any edit outside the table. Two named in the plan: the empty-status route's verified commit read, which git's `rev-parse --verify` shadows, and dropping `--literally` or `--no-filters`. validate_committed_tree beyond rows 2, 6 and 7. Hosted remotes and Windows.

## Task Commits

1. **Task 1: name the closed channels in DEF-09, DEF-11, DEF-12.** `24dfc0a` (docs)
2. **Task 2: controls and the scoped mutation run.** No commit. The controls and the mutation run change no tracked file, no mutant survived, and so no test was added. The outputs are gitignored under target/.
3. **Task 3: final gates and the hand edits carried forward.** `a5cd7ae` (docs)

Each commit was gated: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && timeout -k 30 1800 cargo test && git commit`. Each gated run passed 452 + 114 tests.

## Task 1: the three rows

- docs/ARCHITECTURE.md and 02-20-PLAN.md: the DEF-09, DEF-11 and DEF-12 lines were replaced with `## The three rows` of 02-26-PLAN.md. The replacement was a script that copied the lines from the plan. It asserted that each row matched exactly once and that no test name of the old row was lost. 02-20-PLAN.md's `How the rows were kept apart` gained the plan's NOT-10 against DEF-09, DEF-11 and DEF-12 sentence, right after plan 02-23's. No NOT row changed.
- Verify 1 exit 0. Since e39e860 the doc changed 3 lines in and 3 out, and 02-20-PLAN.md changed 4 and 4. Negative control: the doc and 02-20-PLAN.md at HEAD before the edit did not equal the plan's rows (exit 1 for both).
- Verify 2 exit 0, the closed-list run. Every DEF name is exactly one test. 247 tests ran and 247 passed in 125.0 s.
- Verify 3 exit 0: 02-CONTEXT.md and plans 02-01 to 02-15 equal b8c54e9.
- Acceptance: `rewritten_only_while_listed` appears in docs/ARCHITECTURE.md on 2 lines, and `hidden_by_base_tree` on 1.

## Control table

Procedure: 02-23's. The start-of-table steps were:
- The tree outside .planning was clean.
- The table hash was recorded in `target/controls-02-26.table.sha256`, and the code before each `mod tests {` in `target/controls-02-26.nontest.sha256`.
- Copies of the three files were kept in the session scratch directory.
- 02-25's order check (the commit's listing before the fetched head's read and listing) exited 0.

Each row was then run as follows:
1. One edit, by an exact single-match replacement. All 13 replacements were dry-run first on scratch copies, and each matched once.
2. `PROPTEST_DISABLE_FAILURE_PERSISTENCE=1 timeout -k 30 900 cargo nextest run --bin ude --no-fail-fast --color never sync::tests:: process::tests::`, with TMPDIR in the session scratch directory.
3. The three files were restored from the copies (plain `cp`, so the mtime is new and cargo rebuilds), followed by `git diff --exit-code -- src`.

No edit needed `#[allow(dead_code)]`. Warnings do not fail the nextest build.

**Counts: 13 `same`, 0 `corrected`.**

| # | Edit | State | Exit | Failing tests (observed = table) |
|---|------|-------|------|----------------------------------|
| 1 | read_verified_object: `tree` returns the bytes without the comparison | same | 100 | 18: test_committed_gate_refuses_base_tree_not_matching_its_id, test_direct_push_no_changes_refuses_deletion_hidden_only_while_listed, test_direct_push_no_changes_refuses_subtree_hiding_a_deleted_path, test_direct_push_no_changes_refuses_subtree_not_matching_its_id, test_direct_push_no_changes_refuses_subtree_rewritten_only_while_listed, test_direct_push_refuses_added_file_hidden_by_base_tree_not_matching_its_id, test_direct_push_refuses_unchanged_subtree_not_matching_its_id, test_direct_push_refuses_unchanged_subtree_rewritten_only_while_listed, test_fetched_head_holds_refuses_root_tree_not_matching_its_id, test_pr_push_no_changes_refuses_deletion_hidden_only_while_listed, test_pr_push_no_changes_refuses_subtree_hiding_a_deleted_path, test_pr_push_no_changes_refuses_subtree_not_matching_its_id, test_pr_push_no_changes_refuses_subtree_rewritten_only_while_listed, test_pr_push_refuses_added_file_hidden_by_base_tree_not_matching_its_id, test_pr_push_refuses_unchanged_subtree_not_matching_its_id, test_pr_push_refuses_unchanged_subtree_rewritten_only_while_listed, test_read_verified_object_checks_bytes_against_the_id, test_verified_tree_listing_matches_ls_tree |
| 2 | read_verified_object: `commit` without the comparison | same | 100 | test_committed_gate_refuses_commit_not_matching_its_id |
| 3 | read_verified_object: `blob` without the comparison | same | 100 | 8: test_committed_gate_refuses_changed_blob_not_matching_its_id, test_direct_push_no_changes_refuses_blob_not_matching_its_id, test_direct_push_refuses_unchanged_blob_not_matching_its_id, test_pr_push_no_changes_refuses_blob_not_matching_its_id, test_pr_push_refuses_unchanged_blob_not_matching_its_id, test_read_verified_object_checks_bytes_against_the_id, test_read_verified_object_in_sha256_repository, test_read_verified_object_refuses_an_abbreviated_id |
| 4 | verified_tree_listing: root read by a plain `--no-replace-objects cat-file tree` | same | 100 | test_fetched_head_holds_refuses_root_tree_not_matching_its_id |
| 5 | require_fetched_head_holds: IdMismatch refused with the `cannot be listed` text | same | 100 | 9: the four `_no_changes_refuses_subtree_not_matching_its_id` / `_subtree_hiding_a_deleted_path` rows in both modes, the four `_no_changes_refuses_subtree_rewritten_only_while_listed` / `_deletion_hidden_only_while_listed` rows in both modes, test_fetched_head_holds_refuses_root_tree_not_matching_its_id |
| 6 | validate_committed_tree: commit listing's IdMismatch refused with `failed to list the tree of` | same | 100 | test_direct_push_refuses_unchanged_subtree_not_matching_its_id, test_pr_push_refuses_unchanged_subtree_not_matching_its_id, test_direct_push_refuses_unchanged_subtree_rewritten_only_while_listed, test_pr_push_refuses_unchanged_subtree_rewritten_only_while_listed |
| 7 | validate_committed_tree: fetched head listing's IdMismatch refused with `failed to list the tree of the fetched head` | same | 100 | test_direct_push_refuses_added_file_hidden_by_base_tree_not_matching_its_id, test_pr_push_refuses_added_file_hidden_by_base_tree_not_matching_its_id, test_committed_gate_refuses_base_tree_not_matching_its_id |
| 8 | first_path_under: always None | same | 100 | 16: the table's set exactly (row 1's set without the two unit tests of read_verified_object and the listing) |
| 9 | parse_tree_object: mode kept as read | same | 100 | test_parse_tree_object, test_verified_tree_listing_matches_ls_tree |
| 10 | listing_diff: `M` where the type bits differ | same | 100 | test_listing_diff, test_listing_diff_matches_diff_tree |
| 11 | listing_diff: no `D` record | same | 100 | test_committed_gate_refuses_base_tree_not_matching_its_id, test_listing_diff, test_listing_diff_matches_diff_tree |
| 12 | spawn_prepared: input written on the calling thread, then `drop(stdin)` | same | 100 | test_run_bounded_with_input_times_out_when_the_child_neither_reads_nor_exits |
| 13 | read_verified_object: `hash-object` run with `self.git(None)` | same | 100 | test_listing_diff_matches_diff_tree, test_read_verified_object_in_sha256_repository, test_verified_tree_listing_matches_ls_tree |

The full comma-joined names of every row are in `target/controls-02-26.tsv`. Rows 5 and 8 are shortened above. Task 2 verify 1 checks each row's names against its log and against the table: `same=13 corrected=0`, exit 0.

The module run took 85 s to 164 s per row (356 tests run, 97 skipped). Each run's variation is in the build and in the run itself, not in a timeout.

Among the rows, two back review findings with a test that can fail:
- Row 4: with the root tree read without the check, only test_fetched_head_holds_refuses_root_tree_not_matching_its_id fails (WR-501).
- Row 12: the synchronous stdin write fails test_run_bounded_with_input_times_out_when_the_child_neither_reads_nor_exits (WR-502).

Re-run at re-verification: none. No corrected rows.

## Mutation run

`cargo mutants` 27.1.0 over src/sync/mod.rs and src/process.rs, with the plan's regex and test list. Output in `target/mutants-tree-02-26`, log in `target/mutants-tree-02-26-run.log`, TMPDIR `/var/home/denniyahh/.cache/ude-mutants-02-26-tmp`.

| Listed | Caught | Missed | Unviable | Timeouts | Exit | Wall |
|--------|--------|--------|----------|----------|------|------|
| 69 | 62 | 0 | 7 | 0 | 0 | 958 s (cargo-mutants: "69 mutants tested in 16m") |

- Unmutated baseline: 60 s build, 12 s test. Per-test timeout auto-set to 65 s.
- Unviable, from mutants.out/unviable.txt: 4 `Ok(Default::default())` / `Some(...Default...)` returns whose types have no Default (run_bounded_with_input, spawn_prepared, run_local_with_input, first_path_not_held). Also `Some(vec![Default::default()])` in parse_tree_object, `Some(Box::leak(...))` in first_path_under, and `+` to `*` on spawn_prepared's deadline (`Instant * Duration` does not compile).
- No test was added. There is no `.cargo/mutants.toml`.
- Verify 3 exit 0: missed.txt empty, caught.txt non-empty, temp directory removed, nothing staged, local config equal to the saved 22 lines.
- Not covered: validate_committed_tree (outside the regex by the plan's stated exclusion) and src/main.rs.

## Gates (Task 3), in order

| Gate | Exit | Result |
|------|------|--------|
| Hermeticity with decoy GIT_DIR (`cargo test --bin ude sync::`) | 0 | 368 passed, 85 filtered out. The decoy held 0 object files and 0 ref files, and no packed-refs |
| Coverage `cargo +nightly llvm-cov --bin ude --branch --summary-only --show-missing-lines` | 0 | 452 passed, 1 ignored; report in target/llvm-cov-02-26.txt |
| Coverage report check (verify 2) | 0 | rows for process.rs and sync/mod.rs present |
| `cargo fmt --check` | 0 | |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 | |
| `timeout -k 30 1800 cargo test` | 0 | 452 passed, 1 ignored (pre-existing), and 114 passed |
| `timeout -k 30 600 cargo deny check advisories bans sources` | 0 | advisories ok, bans ok, sources ok. One non-failing `duplicate` warning for `syn` |
| `git diff --exit-code e39e860 -- Cargo.toml Cargo.lock` | 0 | no dependency change |
| `git diff --quiet b8c54e9 HEAD -- src/sanitizer src/template` | 0 | SEC-01 and SEC-03 regression-only, unchanged |
| `cargo machete` (verify 4) | 0 | no unused dependency |
| Todo check (verify 5) | 0 | all named items in the 02-26 section, every line from e39e860 kept, STATE.md's concern line present. Negative control: a copy with one old line deleted reports 1 lost line |
| Final state (verify 6) | 0 | no generated output in the tree, code before `mod tests {` equal to Task 2's hashes, src/tests/docs committed, nothing outside .planning in `git status`. Re-run after the Task 3 commit: exit 0 |

The chain above ran as the plan's single `&&` command, so its one exit 0 means every link returned 0. The gates ran with `TMPDIR` in the session scratch directory and `PROPTEST_DISABLE_FAILURE_PERSISTENCE=1` exported. These are environment additions only; no command was changed.

### Coverage (no threshold)

| File | Regions missed | Lines | Missed lines | Line cover | Branches | Missed branches | Branch cover |
|------|----------------|-------|--------------|------------|----------|-----------------|--------------|
| process.rs | 23 of 549 | 454 | 15 | 96.70% | 26 | 5 | 80.77% |
| sync/mod.rs | 514 of 15483 | 12244 | 474 | 96.13% | 700 | 157 | 77.57% |

`Uncovered Lines` lists 15 lines for process.rs, which matches its summary. For sync/mod.rs it lists 450, against a summary count of 474. The difference is in llvm-cov's own counting and is not explained here. The listed set ends at line 18460-18462, so it is not truncated. This report covers the `ude` binary's unit tests only, not tests/e2e_lifecycle.rs.

Uncovered lines inside the functions of Task 2's regex:
- process.rs spawn_prepared 187-188: refusal when the child has no stdin pipe. run_bounded_with_input always pipes stdin, so its callers cannot reach it.
- process.rs spawn_prepared 210-211: `try_wait` itself failing.
- sync/mod.rs require_fetched_head_holds:
  - 2940 and 2943: the `cannot be listed` closure and an unreadable fetched-head commit.
  - 2945-2947: the fetched head's commit not matching its id. This is the read the plan notes git's `rev-parse --verify` shadows.
  - 2952: no tree line.
  - 2956-2960 and 2964: the unparseable refusal for the listing.
  - 2963: an unreadable listing.
  - 2979: an id mismatch with no recorded or staged path under the tree.
  - 2985: tree_listing_blobs refusing the built listing.
- sync/mod.rs read_verified_object:
  - 3056 and 3064: spawn errors of `cat-file -t` and `cat-file`.
  - 3066: `cat-file` failing after `-t` succeeded.
  - 3079 and 3081: `hash-object` failing to spawn or exiting non-zero.
- sync/mod.rs verified_tree_listing 3107: a root id that is neither 40 nor 64 characters.
- No uncovered line in read_blob_by_id, parse_tree_object, first_path_under, listing_diff, first_path_not_held, require_validated_content, run_bounded_with_input or run_local_with_input.

## Hand edits

The edits owed after re-verification are in `.planning/todos/pending/2026-11-01-hand-edits-owed-for-phase-02-after-re-verification.md`, under `## Added by plan 02-26`. They cover:
- WR-500, WR-501 and WR-502 to `fixed`, with 02-24's commits.
- IN-501 not taken; IN-500 out of scope.
- The base-side channel, answered by the operator (`Plan 02-24: both`) and closed by 02-25 (d936992, 7c9d86f).
- The staged-diff interpretation, recorded as answered by the operator on 2026-11-04.
- Dropping test_listed_tree_ids from 02-VALIDATION.md's SEC-04 row.
- What to tell the verifier.

The todo stays in pending. STATE.md's concern line stands. 02-REVIEW-DISPOSITION.md, 02-SECURITY.md and 02-VALIDATION.md were not edited.

The closed list is closed by decision and is not proven exhaustive.

## Decisions Made

- Task 2 produced no commit. Its outputs are the gitignored evidence files, and no test was needed.
- The coverage parse first read the summary rows. They carry no line list, so every function showed 0 missed lines. That was a proxy reading. A cross-check against process.rs's 15 missed lines exposed it, and the lines were then taken from the `Uncovered Lines` section.

## Deviations from Plan

None. The plan was executed as written. Environment additions, not deviations: `TMPDIR` in the session scratch directory and `PROPTEST_DISABLE_FAILURE_PERSISTENCE=1` on the hermeticity gate and the gate chain, as in 02-24 and 02-25.

## Issues Encountered

- The coverage misparse described under Decisions Made. It was caught before any number was recorded.

## Threat Flags

| Flag | File | Status |
|------|------|--------|
| threat_flag: T-02-106 | src/sync/mod.rs | mitigate: the hashes of the code before `mod tests {` match after the last restore and as Task 3's last command; `git diff --exit-code -- src` after each restore; `git status` clean outside .planning |
| threat_flag: T-02-107 | src/sync/mod.rs | mitigate: after the mutation run `git diff --cached --quiet` exits 0 and the local config equals the saved 22 lines |
| threat_flag: T-02-108 | docs/ARCHITECTURE.md | mitigate: Task 1's verifies exit 0 (rows equal in both files, only those lines changed, every DEF name exactly once and passing); rows 4 and 12 fail their root-tree and stdin-deadline tests |
| threat_flag: T-02-SC | Cargo.toml, Cargo.lock | mitigate: `git diff --exit-code e39e860 -- Cargo.toml Cargo.lock` exits 0 |

T-02-109 is unused: no row is `corrected`, so no name was dropped.

## Cleanup

- Removed: the control copies in the session scratch directory, `/var/home/denniyahh/.cache/ude-mutants-02-26-tmp`, `target/hermetic-gate` (before and after its gate), and `target/llvm-cov-target`.
- Kept, gitignored and cited here or read by a verify command:
  - target/controls-02-26.tsv and target/controls-02-26-rows/;
  - target/controls-02-26.nontest.sha256 and target/controls-02-26.table.sha256;
  - target/mutants-tree-02-26/, with target/mutants-tree-02-26-run.log and target/mutants-tree-02-26-config.before;
  - target/llvm-cov-02-26.txt and target/llvm-cov-02-26.log.
- `git config --local --list | wc -l` was 22 after every run.

## Known Stubs

None.

## User Setup Required

None.

## Next Phase Readiness

Phase 02 is ready for re-verification. The verifier gets the todo's `## Added by plan 02-26` list. There are no 02-26 control rows to re-run; 02-21's list still stands.

## Self-Check: PASSED

- FOUND: docs/ARCHITECTURE.md, 02-20-PLAN.md and the todo, each with its change committed.
- FOUND: commits 24dfc0a and a5cd7ae, both ancestors of HEAD. `git rev-list --count f6c3cf3..HEAD` = 2 before this SUMMARY's commit.
- FOUND: target/controls-02-26.tsv (13 lines), target/mutants-tree-02-26/mutants.out/caught.txt (62 lines) and target/llvm-cov-02-26.txt.
- Every verify of Tasks 1, 2 and 3 exited 0. Task 3's last verify was re-run after the last task commit.
