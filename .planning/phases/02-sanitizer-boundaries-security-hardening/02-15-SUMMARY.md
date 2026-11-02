---
phase: 02-sanitizer-boundaries-security-hardening
plan: 15
subsystem: sync
tags: [rust, sync, push, publication-gate, controls, mutation-testing, coverage, gap-closure]

requires:
  - phase: 02-sanitizer-boundaries-security-hardening (02-13)
    provides: "the committed gate bound to the staged tree, its raw listing checks and read_blob_by_id's type check"
  - phase: 02-sanitizer-boundaries-security-hardening (02-14)
    provides: "run_push, run_bounded_ignoring_grafts, the grafts hint option and remote_branch_is_at"
provides:
  - "A control table: each of the 23 checks, reads and push options removed one at a time on the final code, with the failing tests of each"
  - "A scoped mutation result over 16 gate functions: 96 mutants, 83 caught, 0 missed, 0 timeouts, 13 unviable"
  - "A test pinning read_blob_by_id's type check inside the mutation run's name list"
  - "The phase's final gates and a coverage record on the final state"
  - "A pending todo, a STATE.md concern line and this one-liner for the hand edits owed after re-verification"
affects: [02-re-verification, 02-SECURITY, 02-VALIDATION, 02-REVIEW-DISPOSITION]

actuals:
  tokens: 2007    # chars/4 over the test diff, the todo and the STATE.md diff; this SUMMARY is not counted
  tasks: 3
  commits: 1
plan_head_before: 1a5c88fa1d8f0619be36942b3a118c7fd51a7a67
plan_head_after: 3aeb1d2245b3e906990fe39cfcc5f126d6acf9a3

tech-stack:
  added: []
  patterns:
    - "A control row is edit, run, compare, restore, with a nextest log for each run"
    - "A mutant missed under a name-listed filter is killed by a test the filter selects, so the plan's command stays as written"

key-files:
  created:
    - .planning/todos/pending/2026-11-01-hand-edits-owed-for-phase-02-after-re-verification.md
  modified:
    - src/sync/mod.rs
    - .planning/STATE.md

key-decisions:
  - "The mutant that survived the first run was killed by a new test whose name the plan's filter selects as a substring, not by changing the plan's command"
  - "A supplementary mutation pass covered require_validated_content, split_at_nul and spawn_prepared, which the plan's regex leaves out; it is reported beside the plan's result, not merged into it"

patterns-established:
  - "A function extracted during execution falls outside a name-based mutation regex: re-list the scope against the SUMMARYs before trusting a clean run"

requirements-completed: [SEC-04, SEC-02]

coverage:
  - id: D1
    description: "Every one of the 23 control rows fails its expected tests when its check is removed from the final code"
    requirement: SEC-04
    verification:
      - kind: other
        ref: "Task 1's first verify over target/controls-02-15.tsv and target/controls-02-15-rows"
        status: pass
    human_judgment: false
  - id: D2
    description: "No mutant survives in the plan's 16 gate functions under the plan's test list"
    requirement: SEC-04
    verification:
      - kind: other
        ref: "cargo mutants -o target/mutants-tree (Task 2's first verify), caught.txt and missed.txt in target/mutants-tree/mutants.out"
        status: pass
    human_judgment: false
  - id: D3
    description: "The phase's final gates pass on the final state, with the code before each mod tests line byte-identical to the controls' start"
    requirement: SEC-02
    verification:
      - kind: other
        ref: "Task 2's verify commands 3 to 11"
        status: pass
      - kind: integration
        ref: "tests/e2e_lifecycle.rs#test_sync_push_pr_does_not_advance_ancestor_state, test_sync_push_remote_failure_does_not_advance_ancestor_state, test_sync_ignores_inherited_git_dir"
        status: pass
    human_judgment: false
  - id: D4
    description: "After a context reset the reader is told the hand edits are owed"
    verification:
      - kind: other
        ref: "Task 3's verify over the todo and STATE.md, and the plan's summary-extract check on this one-liner"
        status: pass
    human_judgment: true
    rationale: "What a later /gsd-resume-work or /gsd-progress prints was read from those workflows and their query commands, not observed in a later session"

duration: 76min
completed: 2026-11-02
status: complete
---

# Phase 02 Plan 15: Controls, mutation run and final gates Summary

**All 23 per-check controls failed exactly their expected tests on the final gate code, the scoped mutation run ended with none missed after one surviving mutant forced a new test, the final gates pass, and hand edits are owed after re-verification (see the todo).**

## Performance

- **Duration:** 76 min
- **Started:** 2026-11-02T17:59:30Z
- **Completed:** 2026-11-02T19:15Z
- **Tasks:** 3
- **Files modified:** 3, plus this SUMMARY

## Accomplishments

- 23 of 23 control rows came out `same`: each removal failed exactly the row's tests, on the tests' own assertions, with exit 100.
- The first mutation run missed one mutant, in `read_blob_by_id`'s type check. A new test kills it. The re-run has 0 missed.
- `cargo fmt`, `cargo clippy`, full `cargo test`, `cargo deny`, `cargo machete`, the e2e remote pushes and the hermeticity gate all exit 0 after the last commit.
- Plans 02-13, 02-14 and 02-15 close the five gaps of 02-VERIFICATION.md pending re-verification. T-02-52 is ready to move to mitigated by the hand edit named in 02-15-PLAN.md. The channel list is the channels probed, not a proven complete list.
- The hand edits owed are in the todo `.planning/todos/pending/2026-11-01-hand-edits-owed-for-phase-02-after-re-verification.md`.

## Control table

**Counts: 23 `same`, 0 `corrected`, 0 `different`.** Task 1's first verify printed `same=23 corrected=0` and exited 0. The rows ran on commit `1a5c88f`. The later test-only commit `3aeb1d2` left the code before each file's `mod tests {` byte-identical, which the hash comparison confirms. An unedited run of the same command beforehand exited 0 with 236 tests passed and no `FAIL` line, so the pattern can read no failures.

| # | Edit | State | Failing tests |
|---|------|-------|---------------|
| 1 | Remove the comparison with the recorded tree id | same | test_committed_gate_requires_the_staged_tree, test_direct_push_refuses_hook_exec_flip, test_pr_push_refuses_hook_exec_flip |
| 2 | Read changed blobs by `<commit>:<path>` instead of the entry's id | same | test_direct_push_refuses_index_hook_hidden_blob, test_direct_push_refuses_index_hook_hidden_token, test_pr_push_refuses_index_hook_hidden_blob, test_pr_push_refuses_index_hook_hidden_token |
| 3 | Remove the held-content check | same | test_committed_gate_requires_validated_content, test_direct_push_refuses_index_hook_removed_file, test_pr_push_refuses_index_hook_removed_file, test_push_refuses_index_hook_symlink |
| 4 | tree_listing_blobs: remove the repeated-path refusal | same | prop_tree_listing_blobs_round_trips, test_direct_push_refuses_index_hook_repeated_same_blob, test_pr_push_refuses_index_hook_repeated_same_blob, test_tree_listing_blobs |
| 5 | tree_listing_blobs: remove the path-component refusal | same | prop_tree_listing_blobs_round_trips, test_committed_gate_refuses_bad_path_components, test_tree_listing_blobs |
| 6 | tree_listing_blobs: put every record in the map | same | test_committed_gate_requires_validated_content, test_push_refuses_index_hook_symlink, test_tree_listing_blobs |
| 7 | Treat count_empty_directories' result as zero | same | test_push_refuses_hook_empty_directory, test_push_refuses_hook_replaced_tree_hiding_empty_directory, test_push_refuses_index_hook_empty_directory |
| 8 | read_blob_by_id: drop `--no-replace-objects` from `cat-file blob` | same | test_committed_gate_reads_blob_past_replace_ref, test_push_refuses_hook_replaced_blob |
| 9 | `cat-file commit` read: drop `--no-replace-objects` | same | test_direct_push_replaced_commit_refusal_does_not_say_upstream_moved, test_push_refuses_hook_merge_replaced_by_validated_commit, test_push_refuses_hook_replaced_message |
| 10 | `diff-tree` read: drop `--no-replace-objects` | same | test_push_refuses_hook_replaced_tree |
| 11 | `ls-tree` read: drop `--no-replace-objects` | same | test_push_refuses_hook_replaced_tree_hiding_empty_directory |
| 12 | push_direct: drop `core.commitGraph=false` | same | test_direct_push_ignores_inherited_git_test_commit_graph, test_direct_push_sends_no_object_from_forged_commit_graph, test_push_sends_no_object_from_forged_alternate_commit_graph, test_push_sends_no_object_from_forged_split_commit_graph |
| 13 | push_pr_branch: drop `core.commitGraph=false` | same | test_pr_push_ignores_inherited_git_test_commit_graph, test_pr_push_sends_no_object_from_forged_commit_graph |
| 14 | push_direct: drop `pack.useBitmaps=false` | same | test_direct_push_sends_no_object_from_graft_built_bitmap, test_push_sends_no_object_from_graft_built_multi_pack_index_bitmap |
| 15 | push_pr_branch: drop `pack.useBitmaps=false` | same | test_pr_push_sends_no_object_from_graft_built_bitmap |
| 16 | push_direct: drop the remove_git_test_env call | same | test_direct_push_ignores_inherited_git_test_commit_graph |
| 17 | push_pr_branch: drop the remove_git_test_env call | same | test_pr_push_ignores_inherited_git_test_commit_graph |
| 18 | push_direct: push through run_network | same | test_cli_direct_push_alone_gets_the_null_grafts_file, test_direct_push_sends_no_object_from_grafts_written_during_push, test_direct_push_sends_no_object_from_pre_push_grafts |
| 19 | push_pr_branch: push through run_network | same | test_cli_pr_push_alone_gets_the_null_grafts_file, test_pr_push_sends_no_object_from_grafts_written_during_push, test_pr_push_sends_no_object_from_pre_push_grafts |
| 20 | push_direct: drop `advice.graftFileDeprecated=false` | same | test_direct_push_failure_excerpt_has_no_grafts_hint |
| 21 | push_pr_branch: drop `advice.graftFileDeprecated=false` | same | test_pr_push_failure_excerpt_has_no_grafts_hint |
| 22 | run_bounded_ignoring_grafts: drop the line that sets the variable | same | test_cli_direct_push_alone_gets_the_null_grafts_file, test_cli_pr_push_alone_gets_the_null_grafts_file, test_direct_push_sends_no_object_from_grafts_written_during_push, test_direct_push_sends_no_object_from_pre_push_grafts, test_pr_push_sends_no_object_from_grafts_written_during_push, test_pr_push_sends_no_object_from_pre_push_grafts, test_run_bounded_ignoring_grafts_sets_the_null_device |
| 23 | push_direct's non-fast-forward arm: remove the remote_branch_is_at branch | same | test_direct_push_replaced_commit_refusal_does_not_say_upstream_moved, test_direct_push_shallow_file_refusal_does_not_say_upstream_moved |

**How each row was read.** I read each failing test's panic site and first message line. All are inside the tests, on the row's behaviour: `the remote changed; push result: Ok(...)`, `the remote holds the injected blob`, a missing refusal text, or `expected a hard push failure, got Err(UpstreamMoved ...)`. No log holds a compile error.

**What this does not establish.** A row shows that the listed tests fail when that one edit is made. It does not show the set is the only test pinning the check, or that the check is sufficient for the threat. Rows 12 to 21 have no second check, because cargo mutants does not touch those arguments and statements.

No corrected rows

## Mutation result

| Run | Scope | Listed | Caught | Missed | Timeouts | Unviable | Wall time | Exit |
|-----|-------|--------|--------|--------|----------|----------|-----------|------|
| First | Plan's command, at `1a5c88f` | 96 | 82 | 1 | 0 | 13 | 13 min | 2 |
| **Final** | **Plan's command, at `3aeb1d2`** | **96** | **83** | **0** | **0** | **13** | **20 min** | **0** |
| Supplementary | `require_validated_content`, `split_at_nul`, `spawn_prepared` | 22 | 20 | 0 | 0 | 2 | 3 min | 0 |

Output files of the final run: `target/mutants-tree/mutants.out/{caught,missed,timeout,unviable}.txt`. `missed.txt` is empty. Plan 02-12's run for comparison was 49 listed, 49 caught.

**The missed mutant.** `src/sync/mod.rs:2605:42: replace || with && in SyncEngine::read_blob_by_id`. That line is the type check plan 02-13 added during execution. Under the mutant a tag object at a recorded path reads as the bytes it points at. Plan 02-13's three tag tests do catch it (run with the mutant applied), but none of them is in the plan's name list, which was written before they existed. So this was a gap in the filter and not a gap in the suite.

**The test added.** `test_committed_gate_requires_validated_content_refuses_tag_object` (commit `3aeb1d2`). The plan's filter `test_committed_gate_requires_validated_content` selects it as a substring, so the plan's command runs unchanged. With the mutant applied it fails on `a tag object must not count as the recorded content`. Without the mutant it passes, and its first half shows the same record is satisfied by the blob the tag points at. Both directions were run.

**Unviable, so the mutation run says nothing about these:** `run_push` and `run_bounded_ignoring_grafts` returning `Ok(Default::default())` (rows 18, 19 and 22 cover those two), `raw_diff_entries` returning `Some(vec![Default::default()])`, nine `tree_listing_blobs` returns with a made-up map (rows 4 to 6 and its unit test cover the function), and deleting one `tree_listing_blobs` match arm. The supplementary pass's two unviable are not itemised here.

**Scope gaps found.** The plan's regex does not name `require_validated_content` or `split_at_nul`, which plan 02-13 added, or `spawn_prepared`, which holds `run_bounded`'s moved code. The supplementary pass covers them with a different test filter (`process::tests::`, `test_committed_gate`, the raw-diff tests and the listed hook tests). It is extra evidence and does not change the plan's result. Its log and output were removed after these figures were recorded.

**What this does not establish.** Mutation testing checks the operators cargo mutants generates in these functions, under a name-listed filter of about 75 tests. It does not exercise string arguments or deleted statements, which is what the control table is for.

## Gates

Each code captured directly, in the plan's order, after the last commit `3aeb1d2`:

| Gate | Exit | Result |
|------|------|--------|
| Mutation command (final) | 0 | see above |
| caught.txt non-empty, missed.txt empty | 0 | |
| /var/home/denniyahh/.cache/ude-mutants-02-15-tmp absent | 0 | |
| Index unchanged, local git config unchanged | 0 | |
| E2E remote pushes: direct, failing remote, PR | 0 | 3 passed |
| Hermeticity, GIT_DIR inherited from a decoy | 0 | 252 passed; decoy 0 object files, 0 ref files, no packed-refs |
| Coverage report has both rows | 0 | see below |
| `cargo fmt --check` | 0 | |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 | |
| `cargo test` | 0 | 333 unit passed, 90 e2e passed, 1 ignored |
| `cargo deny check advisories bans sources` | 0 | |
| `git diff --exit-code -- Cargo.toml Cargo.lock` | 0 | |
| `cargo machete` | 0 | |
| Generated dirs absent, nothing outside `.planning` in `git status` | 0 | |
| Code before `mod tests {` matches the controls' start | 0 | run last in Task 2 and again in Task 3 |
| Task 1's first verify | 0 | same=23 corrected=0 |
| Task 3's verify | 0 | |

**What this does not establish.** The hermeticity gate shows no sync test wrote to the decoy, not that none read it. The e2e pushes use local bare remotes, not a hosted remote. Nothing here was run on Windows. A single run of each gate is a weak bound on test flakiness: it shows these runs passed, not that the suite is stable.

## Coverage

`cargo +nightly llvm-cov --bin ude --branch --summary-only --show-missing-lines`, exit 0, branches measured. This is `--bin ude` only, so nothing through the e2e suite counts. No threshold.

| File | Lines at HEAD 9c84904 | Lines now | Branches at HEAD 9c84904 | Branches now |
|------|-----------------------|-----------|--------------------------|--------------|
| src/process.rs | 347, 96.54% | 372, 96.77% (12 missed) | 22, 81.82% | 22, 77.27% (5 missed) |
| src/sync/mod.rs | 7483, 94.15% | 8903, 94.99% (446 missed) | 518, 74.90% | 568, 75.70% (138 missed) |

**src/process.rs branch coverage fell by one branch** on the same count of 22. I did not determine why. The report gives uncovered lines per file and no per-branch detail, and one run cannot separate a real change from a timing-dependent path.

Uncovered lines in `push_direct`, `push_pr_branch` and the functions in the mutation regex:

| Function | Lines | What they are |
|----------|-------|---------------|
| record_staged_tree | 1957 | spawn failure of `git write-tree` |
| push_pr_branch | 1989, 1992-1995 | `git checkout -b` failing |
| push_pr_branch | 2003, 2006-2010 | `git commit` failing |
| push_pr_branch | 2016-2018 | resolve_commit failing after the commit |
| push_pr_branch | 2071 | spawn failure of the push |
| push_pr_branch | 2100-2101 | `gh pr create` succeeding: no test has a working `gh` |
| push_direct | 2133, 2136-2139 | `git commit` failing |
| push_direct | 2145-2146 | resolve_commit failing after the commit |
| resolve_commit | 2242, 2244 | `git rev-parse` failing to spawn or exiting non-zero |
| refuse_grafts_file | 2306 | spawn failure of `git rev-parse` |
| validate_committed_tree | 2366-2367, 2369-2372 | `cat-file commit` failing |
| validate_committed_tree | 2390-2392 | a commit object with no empty line |
| validate_committed_tree | 2428-2429 | spawn failure of `diff-tree` |
| validate_committed_tree | 2440-2442 | an unparseable `diff-tree` entry |
| validate_committed_tree | 2478-2481 | a listed path with no validated record |
| validate_committed_tree | 2506-2509, 2511-2516 | `ls-tree` failing |
| validate_committed_tree | 2524-2526, 2543-2545 | an unparseable `ls-tree` entry |
| raw_commit_tree | 2643 | `.next()?` on a split that always has a first item |

No uncovered lines in `run_push`, `remote_branch_is_at`, `read_blob_by_id`, `raw_commit_parents`, `raw_commit_parts`, `raw_diff_entries`, `remove_git_test_env`, `count_empty_directories`, `tree_listing_blobs`, `git_path_bytes` or `run_bounded_ignoring_grafts`. Nearly all the lines above are refusal paths for a git command failing or printing what git does not print. They are refusals that were not driven by a test, so that a refusal message or a clean rollback on those paths has not been run.

## Task Commits

1. **Task 1: Control table.** No commit: the checkout ended byte-identical.
2. **Task 2: Mutation run and gates.** `3aeb1d2` (test), forced by the missed mutant.
3. **Task 3: Todo and concern line.** Committed with this SUMMARY.

## Files Created/Modified

- `src/sync/mod.rs` - one test after `mod tests {`, no other change
- `.planning/todos/pending/2026-11-01-hand-edits-owed-for-phase-02-after-re-verification.md` - the hand edits owed
- `.planning/STATE.md` - the concern line under `### Blockers/Concerns`, and `### Pending Todos` re-rendered from `init.todos` (now 15 items; the section held 11 before)

## Decisions Made

- **How the missed mutant was killed.** The plan allows a new test row or assertion and forbids an exclusion. Adding the existing tag tests to the name list would also have worked, but then the plan's command as written would still exit 2 for anyone re-running it. So the new test sits under a name the filter already selects.
- **Row 3's edit.** Plan 02-13 moved the held-content loop into `require_validated_content`, so the edit removes that one call from `validate_committed_tree`. The table was not changed, because no name in it is wrong.
- **Row 8's edit.** Plan 02-13's SUMMARY says the row must edit the `cat-file blob` line, and it did. The `cat-file -t` read keeps its option. So that option on the type read has no control row, and it was not probed separately.
- **No added control row for the type check.** 02-13's SUMMARY left this to this plan. The table stays at 23 because the verify requires it, and the mutation run now pins the type check.

## Deviations from Plan

1. **Supplementary mutation pass, not in the plan.** It closes the scope gap above without changing the plan's command. Nothing depended on it passing. Temporary directory and output removed.
2. **Each row and each gate ran through a scratch script, with a `timeout` prefix on the long commands.** The arguments are the plan's. The plan asks for bounded runs and gives no form for the bound. All 23 row logs end in `exit=100`.
3. **Timing differs from the plan's interfaces.** An unedited unit run took 120 s here against the plan's 40 s. The whole table took about 29 min, which is in line with the plan's figure. The mutation re-run took 20 min against 13 min for the first run, with the same 96 mutants; I did not determine why.

**Total deviations:** 3, none of them to production code. **Impact:** no change to what the plan's verifies check.

## Issues Encountered

- **Safety check refusal.** One compound command holding `rm -rf "$S/copies"` was refused by Claude Code's removal check and did not run. I re-ran the verify on its own and removed the copies with a separate command naming the literal path, as the plan already does for its own removals. It passed the same check.
- `.agents/GEMINI.md`, named as the project instructions in the dispatch prompt, does not exist in this checkout. Nothing depended on it.

## Threat Flags

| Flag | File | Description |
|------|------|-------------|
| threat_flag: T-02-60 | src/sync/mod.rs | pending the operator, not accepted |
| threat_flag: T-02-61 | src/sync/mod.rs | pending the operator, not accepted |
| threat_flag: T-02-64 | src/sync/mod.rs | pending the operator, not accepted |
| threat_flag: T-02-65 | src/sync/mod.rs | pending the operator, not accepted |
| threat_flag: T-02-67 | src/process.rs | pending the operator, not accepted |

Status when this SUMMARY was written; ## Operator answers in 02-15-PLAN.md holds the current one.

## Cleanup

Removed and confirmed absent: target/hermetic-gate, target/llvm-cov-target, /var/home/denniyahh/.cache/ude-mutants-02-15-tmp, the controls' copies under the session scratch directory. Kept as the plan says: target/mutants-tree, target/mutants-tree-run.log, target/mutants-tree-config.before, target/controls-02-15.tsv, target/controls-02-15-rows, target/controls-02-15.nontest.sha256, target/controls-02-15.table.sha256, target/llvm-cov-02-15.txt and its log. No proptest-regressions directory exists.

## User Setup Required

None.

## Next Phase Readiness

Re-verification of phase 02 is next, and `requirements-completed` above copies the plan's frontmatter and is not a verdict. No control row needs re-running at re-verification beyond what the verifier chooses, since none is `corrected`. `phase.complete` must wait for re-verification. Not claimed here: re-verification's verdict, that the channel list is complete, any hosted remote, Windows, or the operator's answers on the five risks above.

## Self-Check: PASSED

The key files exist, `3aeb1d2` is an ancestor of HEAD, and `git rev-list --count 1a5c88f..3aeb1d2` is 1, all checked by the command that commits this file.
