---
phase: 02-sanitizer-boundaries-security-hardening
plan: 28
subsystem: sync-publication-gate
tags: [git, closed-list, controls, mutation-testing, coverage, test-split, rust]

requires:
  - phase: 02-sanitizer-boundaries-security-hardening
    provides: "02-27's ListingBudget (record limit, byte limit, one deadline per gate call) and the repeated-path check before any blob read"
provides:
  - "Review IN-600 closed in code: test_verified_tree_listing_matches_ls_tree split into four tests and test_listing_diff into twelve, one scenario each"
  - "DEF-09 and DEF-10 in docs/ARCHITECTURE.md and 02-20-PLAN.md naming the walk bound, the earlier repeated-path check and the split tests"
  - "20 control rows on the final code, all `same`; a scoped mutation run with 0 missed"
  - "The phase's final gates after the gate's last change, and a coverage report over src/sync/mod.rs and src/process.rs"
  - "The hand-edits todo carried forward for plan 02-28"
affects: [phase-02-re-verification]

actuals:
  tokens: 17528
  tasks: 3
  commits: 3
plan_head_before: f0ec87780d44f7cad71e2ad7f73c84d57353c26c
plan_head_after: 8af921fec3031b47088478cd09eae1c083d6092e

tech-stack:
  added: []
  patterns:
    - "One scenario per test with shared fixture builders (ls_tree_fixture, diff_ids, listing_blob, diff_base), so a failure names its case"

key-files:
  created: []
  modified:
    - src/sync/mod.rs
    - docs/ARCHITECTURE.md
    - .planning/phases/02-sanitizer-boundaries-security-hardening/02-20-PLAN.md
    - .planning/phases/02-sanitizer-boundaries-security-hardening/02-VALIDATION.md
    - .planning/todos/pending/2026-11-01-hand-edits-owed-for-phase-02-after-re-verification.md

key-decisions:
  - "All 20 control rows on the final code came out `same` (no corrected row, T-02-119 unused); nothing of 02-28 to re-run at re-verification"
  - "Scoped mutation run: 44 listed, 43 caught, 0 missed, 1 unviable, 0 timeouts, no test added"
  - "The split's listing_diff cases call a shared listing_blob helper where the old test used a local closure named blob; the assert_eq! arguments are otherwise unchanged and every label is verbatim"

patterns-established: []

requirements-completed: [SEC-04]

coverage:
  - id: D1
    description: "The sixteen tests of `## The split` exist once each and pass; test_listing_diff is gone; the kept name holds only the SHA-1 scenario; the code before `mod tests {` equals 02-27's last code commit"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "02-28-PLAN.md Task 1 verify 1 (19 named tests run, 19 passed)"
        status: pass
    human_judgment: false
  - id: D2
    description: "DEF-09 and DEF-10 equal the plan's rows byte for byte in both files; every DEF name is exactly one test and passes; 02-CONTEXT.md and plans 02-01 to 02-15 unchanged"
    requirement: SEC-04
    verification:
      - kind: other
        ref: "02-28-PLAN.md Task 1 verify 2 and 4"
        status: pass
      - kind: integration
        ref: "02-28-PLAN.md Task 1 verify 3 (closed-list run: 277 tests run, 277 passed)"
        status: pass
    human_judgment: false
  - id: D3
    description: "Each part of the record limit, the byte limit, the deadline, their texts, the reorder and the split is load-bearing on the final code: 20 control rows, each failing exactly its named tests"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "target/controls-02-28.tsv and target/controls-02-28-rows/*.1.log; 02-28-PLAN.md Task 2 verify 1 (same=20 corrected=0)"
        status: pass
    human_judgment: false
  - id: D4
    description: "No mutant of the lines plans 02-27 and 02-28 changed survives the scoped tests"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "target/mutants-tree-02-28/mutants.out (43 caught, 0 missed, 1 unviable); 02-28-PLAN.md Task 2 verify 2 and 3"
        status: pass
    human_judgment: false
  - id: D5
    description: "Phase gates on the final state: hermeticity with decoy, coverage, fmt, clippy, full cargo test, cargo deny, cargo machete, Cargo files and sanitizer/template unchanged, working tree clean"
    requirement: SEC-04
    verification:
      - kind: other
        ref: "02-28-PLAN.md Task 3 verify 1 to 6, all exit 0"
        status: pass
    human_judgment: false

duration: 96min
completed: 2026-11-05
status: complete
---

# Phase 2 Plan 28: Split tests, DEF-09/DEF-10, controls and final gates for the bounded walk Summary

**The two chained tests of review IN-600 are now sixteen atomic tests, and DEF-09 and DEF-10 name the walk bound and the earlier repeated-path check. All 20 controls on the final code fail exactly their named tests, the scoped mutation run missed nothing (43 caught, 1 unviable), and the phase's final gates pass after the gate's last change; the hand edits are owed after re-verification, listed in the todo.**

## Performance

- **Duration:** about 96 min, 2026-11-05T00:38Z to 02:14Z
- **Tasks:** 3 of 3
- **Commits:** 3, measured with `git rev-list --count f0ec877..HEAD` before this SUMMARY's commit
- **Files modified:** src/sync/mod.rs (tests only), docs/ARCHITECTURE.md, 02-20-PLAN.md, 02-VALIDATION.md, the todo

## What this establishes, and what it does not

- **Established:** each of the 20 edits in Task 2's table, made alone on the final code, makes exactly the named tests fail (exit 100), and every other test in `sync::tests::` and `process::tests::` (386 run per row) still passes. Every failure is a panic inside a test module: 45 distinct panic sites, all in src/sync/mod.rs below `mod tests {` (line 4201). A classifier control with a production line and a src/process.rs line flagged both.
- **Not established:**
  - That each panic is the test's own refusal assertion rather than an earlier assertion in the same test; the check rules out production-code panics only.
  - That the closed list is exhaustive: it is closed by decision, not proven.
  - Any edit outside the table; mutation outside the lines plans 02-27 and 02-28 changed, and of src/main.rs.
  - The change loop under the deadline: it is not (orchestrator decision 2026-11-04).
  - Hosted remotes and Windows.
- **Against the result:** validate_committed_tree's `the changes of the commit to publish {LISTING_PAST_DEADLINE}` arm (lines 2871-2875) is not covered by any test. listing_diff's own deadline is pinned (rows 5 and 6), but no test makes the deadline pass after the fetched head's listing and before listing_diff's first check, and no hook point lies there. Mutation did not flag it because cargo-mutants generates no mutant for a refusal string. See Issues Encountered.

## Task Commits

| Task | Commit | Subject |
|------|--------|---------|
| 1 | `20d9a90` | test(02-28): split the listing and listing-diff tests into atomic tests |
| 1 | `10659a3` | docs(02-28): name the walk bound and the split tests in DEF-09 and DEF-10 |
| 2 | none | The controls and the mutation run change no tracked file; no mutant survived, so no test was added. Outputs are gitignored under target/ |
| 3 | `8af921f` | docs(02-28): carry the closed-list change into the hand-edits todo |

Each commit was gated by `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && PROPTEST_DISABLE_FAILURE_PERSISTENCE=1 timeout -k 30 1800 cargo test && git commit`, after the branch and toplevel check. Each gated run passed 482 + 114 tests (1 ignored, pre-existing).

## Task 1: the split

The code before `mod tests {` equals 02-27's last code commit (1125c68), checked by verify 1. Shared setup: `ls_tree_fixture(git, dir) -> LsTreeFixture` (ids full, generic, other_dir, empty_commit, plain, empty, link), built exactly as the old test built the SHA-1 fixture; `DiffIds`/`diff_ids()` (x, y, z, zero), `listing_blob(id, path)` and `diff_base(&ids)` for the listing_diff cases.

| New test | Scenario it holds |
|----------|-------------------|
| test_verified_tree_listing_matches_ls_tree (kept name) | SHA-1 fixture: the claims loop over git's listing (`git's listing lacks`) and the built listing equal to it |
| test_verified_tree_listing_matches_ls_tree_on_the_empty_tree | the fixture's commit of the empty tree: both listings empty and equal |
| test_verified_tree_listing_matches_ls_tree_in_sha256_repository | the `--object-format=sha256` repository: 64-digit head, executable mode, built listing equal to git's |
| test_verified_tree_listing_refuses_subtree_not_matching_its_id | negative control: forge_loose_object on `generic`, ls-tree still succeeds, built listing gives IdMismatch naming `templates/projects/generic` |
| test_listing_diff_of_equal_listings_is_empty | `"equal"` |
| test_listing_diff_adds_every_record_over_an_empty_base | `"an empty base"` |
| test_listing_diff_deletes_every_record_for_an_empty_commit | `"an empty commit"` |
| test_listing_diff_lists_content_change_and_executable_flip_as_modified | `"a content change and an executable flip"` |
| test_listing_diff_lists_symlink_and_gitlink_retype_as_type_change | `"a blob that becomes a symlink, and one that becomes a gitlink"` |
| test_listing_diff_lists_directory_replaced_by_file_as_added_and_deleted | `"a directory replaced by a file"` |
| test_listing_diff_orders_a_deletion_before_a_later_addition | `"records in path order, a deletion before a later addition"` |
| test_listing_diff_ignores_tree_records | `"tree records are ignored"` |
| test_listing_diff_matches_a_repeated_path_as_a_multiset | `"a repeated path"` |
| test_listing_diff_writes_64_digit_zero_ids_for_64_digit_ids | `"64-hex ids"` |
| test_listing_diff_refuses_a_commit_record_without_a_tab | `"a record without a tab"` |
| test_listing_diff_refuses_a_base_record_without_a_tab | `"a base record without a tab"` |

- Verify 1 exit 0: every label or marker inside its own test's body, the kept test free of the SHA-256 and IdMismatch scenarios, each name exactly one test, 19 named tests run and passed. Negative control on the body extraction: the kept body at HEAD has 0 matches for `object-format|IdMismatch`, the old body at f0ec877 has 2.
- Unit-test count went from 468 to 482 (+16 new, -2 removed).

## Task 1: the documents

- A script copied the DEF-09 and DEF-10 lines from `## The DEF-09 and DEF-10 rows` into docs/ARCHITECTURE.md and 02-20-PLAN.md. It asserted one match per row per file, that no old DEF-09 name was lost (test_listing_diff replaced by all twelve split names), and that DEF-10's test list was unchanged. It inserted the NOT-14 against DEF-09 sentence after `one overwritten after the gate's last read is NOT.` (one match) and made the two single-match replacements in 02-VALIDATION.md.
- Verify 2 exit 0. Since e9165f0: the doc changed 2 lines in and 2 out, 02-20-PLAN.md 3 and 3, 02-VALIDATION.md 2 and 2. Negative control: the DEF-09 and DEF-10 lines at HEAD before the edit both differed from the plan's.
- Verify 3 exit 0, the closed-list run: every DEF name exactly one test; 277 tests run, 277 passed in 135.7 s.
- Verify 4 exit 0: 02-CONTEXT.md and plans 02-01 to 02-15 equal b8c54e9.
- Acceptance: `rg -c record_limit docs/ARCHITECTURE.md` prints 1, `rg -c "before any of its blobs is read" docs/ARCHITECTURE.md` prints 1.

## Control table

Start-of-table steps: the tree outside .planning was clean; `target/controls-02-28.table.sha256` written once (`00c4548b…`); the code before each `mod tests {` hashed into `target/controls-02-28.nontest.sha256`; copies of the three files kept in the session scratch directory; 02-25's order check (the commit's listing before the fetched head's read and listing) exited 0.

Each row: one edit by exact single-match replacement from a script (all 20 dry-run first on scratch copies, each matching once, every match before `mod tests {`); `PROPTEST_DISABLE_FAILURE_PERSISTENCE=1 timeout -k 30 900 cargo nextest run --bin ude --no-fail-fast --color never sync::tests:: process::tests::`; restore from the copies, then `git diff --quiet -- src` and the sha256 of src/sync/mod.rs compared with the start value, both after every row. No edit needed `#[allow(dead_code)]` (warnings do not fail the nextest build). No edit makes production code panic: row 1 returns Ok(()) without touching either field.

**Counts: 20 `same`, 0 `corrected`.**

| # | Edit | State | Exit | Failing tests (observed = table) |
|---|------|-------|------|----------------------------------|
| 1 | ListingBudget::take returns Ok(()) at once | same | 100 | 8: the table's set (both push record-limit rows, fetched-head-holds and committed fetched-head record rows, counts-both-listings, commit byte-limit row, both production over-limit rows) |
| 2 | take refuses when records >= records_left | same | 100 | test_committed_gate_counts_both_listings_against_one_record_limit, test_committed_gate_refuses_fetched_head_tree_over_the_record_limit, test_fetched_head_holds_refuses_tree_over_the_record_limit |
| 3 | fetched head listed with a fresh self.listing_budget() | same | 100 | test_committed_gate_carries_the_deadline_into_the_fetched_head_listing, test_committed_gate_counts_both_listings_against_one_record_limit, test_committed_gate_refuses_commit_tree_over_the_byte_limit, test_committed_gate_refuses_fetched_head_tree_over_the_record_limit |
| 4 | push_tree_entries without budget.check_time() | same | 100 | test_committed_gate_carries_the_deadline_into_the_fetched_head_listing, test_committed_gate_refuses_commit_listing_past_the_deadline, test_fetched_head_holds_refuses_listing_past_the_deadline |
| 5 | listing_diff: no deadline check in the commit-record loop | same | 100 | test_listing_diff_refuses_changes_past_its_deadline |
| 6 | listing_diff: no deadline check in the deletion loop | same | 100 | test_listing_diff_refuses_deletions_past_its_deadline |
| 7 | listing_budget takes self.limits.network | same | 100 | test_listing_budget_deadline_follows_the_local_time_limit |
| 8 | listing_budget: usize::MAX for MAX_LISTING_RECORDS | same | 100 | test_verified_tree_listing_refuses_tree_over_the_production_record_limit |
| 9 | require_fetched_head_holds: TooLarge with the cannot-be-listed text | same | 100 | test_fetched_head_holds_refuses_tree_over_the_record_limit |
| 10 | validate_committed_tree: commit TooLarge with `failed to list the tree of` | same | 100 | test_committed_gate_refuses_commit_tree_over_the_byte_limit, test_direct_push_refuses_tree_over_the_record_limit, test_pr_push_refuses_tree_over_the_record_limit |
| 11 | validate_committed_tree: fetched head TooLarge with BASE_UNLISTED | same | 100 | test_committed_gate_counts_both_listings_against_one_record_limit, test_committed_gate_refuses_commit_tree_over_the_byte_limit, test_committed_gate_refuses_fetched_head_tree_over_the_record_limit |
| 12 | require_fetched_head_holds: PastDeadline with the cannot-be-listed text | same | 100 | test_fetched_head_holds_refuses_listing_past_the_deadline |
| 13 | validate_committed_tree: commit PastDeadline with `failed to list the tree of` | same | 100 | test_committed_gate_refuses_commit_listing_past_the_deadline |
| 14 | validate_committed_tree: fetched head PastDeadline with BASE_UNLISTED | same | 100 | test_committed_gate_carries_the_deadline_into_the_fetched_head_listing |
| 15 | listing_diff writes `M` where the type bits differ | same | 100 | test_listing_diff_lists_symlink_and_gitlink_retype_as_type_change, test_listing_diff_matches_diff_tree |
| 16 | listing_diff pushes no `D` record | same | 100 | 7: the table's set (test_committed_gate_refuses_base_tree_not_matching_its_id, test_committed_gate_refuses_fetched_head_tree_over_the_record_limit, test_listing_diff_refuses_deletions_past_its_deadline, and the empty-commit, directory-replaced, deletion-order and diff-tree listing_diff tests) |
| 17 | read_verified_object: hash-object with self.git(None) | same | 100 | test_listing_diff_matches_diff_tree, test_read_verified_object_in_sha256_repository, test_verified_tree_listing_matches_ls_tree_in_sha256_repository |
| 18 | tree_listing_blobs moved back after the change loop | same | 100 | test_committed_gate_refuses_bad_path_components, test_committed_gate_refuses_repeated_path_before_reading_its_blobs, test_direct_push_refuses_index_hook_hidden_blob, test_direct_push_refuses_index_hook_hidden_token, test_pr_push_refuses_index_hook_hidden_blob, test_pr_push_refuses_index_hook_hidden_token |
| 19 | take compares and decreases records only | same | 100 | test_committed_gate_refuses_commit_tree_over_the_byte_limit, test_verified_tree_listing_refuses_tree_over_the_production_byte_limit |
| 20 | listing_budget: usize::MAX for MAX_LISTING_BYTES | same | 100 | test_verified_tree_listing_refuses_tree_over_the_production_byte_limit |

The full comma-joined names of every row are in `target/controls-02-28.tsv`; rows 1 and 16 are shortened above. Task 2 verify 1 checks each row's names against its log and against the table: `same=20 corrected=0`, exit 0.

- The module run took 109.9 s to 181.3 s per row (386 tests run). No row hung and no row failed to build.
- Rows 15, 16 and 17 re-establish 02-26's rows 10, 11 and 13 under the split names: each fails only the split test of its case (plus the tests 02-26 already named).
- Row 18 is the control for 02-27's reorder: moved back, the five updated tests fail on their new texts and the repeated-path row fails on its zero-reads assertion.

Re-run at re-verification: No corrected rows.

## Mutation run

`cargo mutants` 27.1.0, `--in-diff target/mutants-02-28.diff` (git diff from e818692^ to HEAD over src/sync/mod.rs, 1854 lines) `--file src/sync/mod.rs`, with the plan's libtest filters. Output in `target/mutants-tree-02-28`, log in `target/mutants-tree-02-28-run.log`, TMPDIR `/var/home/denniyahh/.cache/ude-mutants-02-28-tmp` (removed after the run).

| Listed | Caught | Missed | Unviable | Timeouts | Exit | Wall |
|--------|--------|--------|----------|----------|------|------|
| 44 | 43 | 0 | 1 | 0 | 0 | 1491 s (cargo-mutants: "44 mutants tested in 25m") |

- Unmutated baseline: 66 s build, 28 s test. Per-test timeout auto-set to 143 s.
- Unviable: `replace SyncEngine::listing_budget -> ListingBudget with Default::default()` (ListingBudget has no Default).
- is_past's two mutants (`true`, `false`) were caught. No mutant compared two Instant readings with `>=` against `>`; the stop rule did not fire.
- No test was added. There is no `.cargo/mutants.toml`.
- Verify 3 exit 0: missed.txt empty, caught.txt non-empty, temp directory removed, nothing staged, local config equal to the saved 22 lines.

## Gates (Task 3), in order

| Gate | Exit | Result |
|------|------|--------|
| Hermeticity with decoy GIT_DIR (`cargo test --bin ude sync::`) | 0 | 398 passed, 85 filtered out. The decoy held 0 object files and 0 ref files, and no packed-refs |
| Coverage `cargo +nightly llvm-cov --bin ude --branch --summary-only --show-missing-lines` | 0 | 482 passed, 1 ignored; report in target/llvm-cov-02-28.txt (nightly built; no fallback needed) |
| Coverage report check (verify 2) | 0 | rows for process.rs and sync/mod.rs present |
| `cargo fmt --check` | 0 | |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 | |
| `timeout -k 30 1800 cargo test` | 0 | 482 passed, 1 ignored (pre-existing), and 114 passed |
| `timeout -k 30 600 cargo deny check advisories bans sources` | 0 | advisories ok, bans ok, sources ok; one non-failing `duplicate` warning for `syn` |
| `git diff --exit-code e9165f0 -- Cargo.toml Cargo.lock` | 0 | no dependency change |
| `git diff --quiet b8c54e9 HEAD -- src/sanitizer src/template` | 0 | SEC-01 and SEC-03 regression-only, unchanged |
| `cargo machete` (verify 4) | 0 | no unused dependency |
| Todo check (verify 5) | 0 | every named item in the 02-28 section, every line from e9165f0 kept, the 02-27 section and STATE.md's concern line present |
| Final state (verify 6) | 0 | no generated output in the tree, code before `mod tests {` equal to Task 2's hashes, src/tests/docs committed, nothing outside .planning in `git status`; run after the todo commit 8af921f |

The fmt-to-src/template chain ran as the plan's single `&&` command, so its one exit 0 means every link returned 0. The todo commit came after the chain. Its own commit gate re-ran fmt, clippy and the full cargo test (exit 0). Verify 6 then ran on the final state. The gates ran with `TMPDIR` in the session scratch directory and `PROPTEST_DISABLE_FAILURE_PERSISTENCE=1`; these are environment additions, no command changed.

### Coverage (no threshold)

| File | Regions missed | Lines | Missed lines | Line cover | Branches | Missed branches | Branch cover |
|------|----------------|-------|--------------|------------|----------|-----------------|--------------|
| process.rs | 23 of 549 | 454 | 15 | 96.70% | 26 | 6 | 76.92% |
| sync/mod.rs | 534 of 16483 | 13025 | 492 | 96.22% | 724 | 162 | 77.62% |

`Uncovered Lines` lists 466 lines for sync/mod.rs against a summary count of 492, the same kind of gap 02-26 recorded and left unexplained. The report covers the `ude` binary's unit tests only.

Uncovered lines inside the functions 02-27 changed:
- ListingBudget's methods, is_past, listing_budget, listing_kind, listing_diff, count_empty_directories: none.
- verified_tree_listing 3241: a root id that is neither 40 nor 64 characters.
- push_tree_entries 3288: a tree read failing as Unreadable (not as an id mismatch).
- require_fetched_head_holds:
  - 3071-3075 and 3089: the `repeats a path, has a bad path component or cannot be parsed` closure for an unparseable listing.
  - 3078: an unreadable listing.
  - The new TooLarge and PastDeadline arms (3079-3088) are covered.
- validate_committed_tree:
  - 2744-2746: an unreadable commit listing.
  - 2769-2770 and 2829-2830: an id mismatch with no named path.
  - 2784-2786 and 2789-2791: the fetched head's commit unreadable or not matching its id.
  - 2797 and 2806: the fetched head with no tree line, or an unreadable listing.
  - 2818-2821: an unparseable fetched-head listing.
  - 2844-2846: tree_listing_blobs reporting Unparseable, now that it runs before listing_diff.
  - **2870 and 2872-2874: listing_diff unparseable, and listing_diff past the deadline (`the changes of the commit to publish`).** The second is a new 02-27 arm.
  - 2878: raw_diff_entries refusing listing_diff's output.
  - The four new TooLarge and PastDeadline listing arms (2748-2757, 2807-2816) are covered.

## Hand edits

The edits owed after re-verification are in `.planning/todos/pending/2026-11-01-hand-edits-owed-for-phase-02-after-re-verification.md`, under `## Added by plan 02-28`:
- IN-600 to `fixed` with 20d9a90; WR-600 as the 02-27 section says.
- The 02-VALIDATION.md walk-bound row for the next strategy update.
- The renames for older commands (02-26 rows 1, 10, 11 and 13 and its mutation filters).
- What to tell the verifier.

The todo stays in pending. STATE.md's concern line stands. 02-REVIEW-DISPOSITION.md and 02-SECURITY.md were not edited.

The closed list is closed by decision and is not proven exhaustive.

## Decisions Made

- Task 2 produced no commit. Its outputs are gitignored evidence files, and no test was needed.
- Row 2's edit: take holds no `>` (it uses checked_sub), so the greater-or-equal edit is an inserted `if records >= self.records_left { return Err(ListingDefect::TooLarge); }` at the top of take. That is the plan's meaning: refuse when the records equal what is left.
- Row 18 moved the statement only. Its comment stayed above the change loop.
- Rows 8 and 20 changed the cfg(test) line of listing_budget, the one a test build compiles. The cfg(not(test)) line still names the constants.

## Deviations from Plan

**1. [Rule 3 - naming] The listing_diff split calls a shared helper `listing_blob` where the old test used a local closure `blob`.**
- **Found during:** Task 1.
- **Issue:** the plan asks for shared helpers (`the old test's x, y, z, zero and blob`). A module-level `fn blob` would be shadowed by the `let blob = |bytes| write_blob(...)` closures other tests already use.
- **Fix:** the helper is named `listing_blob`. The arguments are unchanged, ids come from `diff_ids()` by reference, and every label is verbatim.
- **Commit:** 20d9a90.

**2. The empty-tree test uses the fixture's `empty_commit`.** It does not build its own repository; the plan allowed either. Its assertions are unchanged.

No 02-27 name, timing or line number differed from what the plan assumed in a way that changed a row, a filter or a test name. The 15 s / 16 s deadline-carry timing affects no row set. No test name was dropped.

## Issues Encountered

- **An uncovered refusal arm (new in 02-27, not fixed here).** validate_committed_tree's `the changes of the commit to publish {LISTING_PAST_DEADLINE}` arm (src/sync/mod.rs 2871-2875) is reached by no test.
  - To reach it, the deadline must pass after the fetched head's listing finishes and before listing_diff's first loop check. No test seam lies between those two points.
  - listing_diff's own deadline is pinned by rows 5 and 6 and the direct tests.
  - Changing that arm's text, or mapping it to another refusal, would pass the suite.
  - A test would need a new cfg(test) seam in production code. This plan adds no production code, so it is left for the orchestrator.
- **A misparse caught before any number was recorded.** The first panic-site classifier read the wrong awk field and flagged every site. It was rewritten, and a control was added: a production line and a src/process.rs line must be flagged, and both were.

## Threat Flags

| Flag | File | Status |
|------|------|--------|
| threat_flag: T-02-116 | src/sync/mod.rs | mitigate: after every restore, `git diff --quiet -- src` and the sha256 of src/sync/mod.rs matched; after the last restore and as Task 3's last command, the hashes of the code before `mod tests {` matched the record and `git status` was clean outside .planning |
| threat_flag: T-02-117 | src/sync/mod.rs | mitigate: after the mutation run `git diff --cached --quiet` exits 0 and the local config equals the saved 22 lines |
| threat_flag: T-02-118 | docs/ARCHITECTURE.md | mitigate: Task 1's four verifies exit 0 (rows equal the plan in both files, only those lines changed, no old name lost, every DEF name exactly one test and passing, each case's label in its own test); rows 15 to 17 fail the split tests of their case |
| threat_flag: T-02-SC | Cargo.toml, Cargo.lock | mitigate: `git diff --exit-code e9165f0 -- Cargo.toml Cargo.lock` exits 0 |

T-02-119 is unused: no row is `corrected`, so no name was dropped.

## Cleanup

- Removed:
  - the control copies, the dry-run copies and the session TMPDIR under the session scratch directory;
  - `/var/home/denniyahh/.cache/ude-mutants-02-28-tmp`;
  - `target/hermetic-gate`, before and after its gate;
  - `target/llvm-cov-target`.
- Kept, gitignored and cited here or read by a verify command:
  - target/controls-02-28.tsv and target/controls-02-28-rows/;
  - target/controls-02-28.nontest.sha256 and target/controls-02-28.table.sha256;
  - target/mutants-02-28.diff;
  - target/mutants-tree-02-28/, with target/mutants-tree-02-28-run.log and target/mutants-tree-02-28-config.before;
  - target/llvm-cov-02-28.txt and target/llvm-cov-02-28.log.
- `git config --local --list | wc -l` was 22 after the mutation run and at the end.

## Known Stubs

None.

## User Setup Required

None.

## Next Phase Readiness

Phase 02 is ready for re-verification. The verifier gets the todo's `## Added by plan 02-28` list. There are no 02-28 control rows to re-run; the lists of 02-21, 02-23 and 02-26 still stand.

## Self-Check: PASSED

- FOUND: src/sync/mod.rs, docs/ARCHITECTURE.md, 02-20-PLAN.md, 02-VALIDATION.md and the todo, each with its change committed.
- FOUND: commits 20d9a90, 10659a3 and 8af921f, all ancestors of HEAD. `git rev-list --count f0ec877..HEAD` = 3 before this SUMMARY's commit.
- FOUND: target/controls-02-28.tsv (20 lines), target/controls-02-28-rows (20 logs), target/mutants-tree-02-28/mutants.out/caught.txt (43 lines), missed.txt (empty) and target/llvm-cov-02-28.txt.
