---
phase: 02-sanitizer-boundaries-security-hardening
plan: 27
subsystem: sync-publication-gate
tags: [git, denial-of-service, bounds, deadline, tdd, rust]

requires:
  - phase: 02-sanitizer-boundaries-security-hardening
    provides: "02-24's verified_tree_listing and push_tree_entries; 02-25's listing_diff; 02-13's tree_listing_blobs repeated-path check"
provides:
  - "ListingBudget: one budget per gate call of MAX_LISTING_RECORDS (1 << 18) records, MAX_LISTING_BYTES (1 << 26) listing bytes and one deadline (the local time limit from the start of the walk), shared by every listing of the call and by listing_diff"
  - "Fixed refusals past either bound, naming the route and never tree text (LISTING_TOO_LARGE, LISTING_PAST_DEADLINE)"
  - "count_empty_directories linear in listing bytes (parent paths only)"
  - "validate_committed_tree refuses a repeated path or bad component before listing_diff and the change loop read any blob"
  - "cfg(test) seams listing_record_limit, listing_byte_limit, listing_time and blob_reads with their builders"
affects: [02-28]

actuals:
  tokens: 22267
  tasks: 3
  commits: 5
plan_head_before: d5146c3600ca39d9663774b6dfed96fa1debec8b
plan_head_after: 9594b4754744f22a1de5a953457d88d670feff6e

tech-stack:
  added: []
  patterns:
    - "Charge a tree's records and bytes against the budget before pushing its entries, so the listing plus the pending stack never pass the limit"
    - "Deadline checks through is_past (checked_duration_since), with no comparison operator"
    - "Test seams that shorten only the walk's bound, never the git reads, so a deadline refusal can be told from a timed-out read"

key-files:
  created: []
  modified:
    - src/sync/mod.rs
    - .planning/todos/pending/2026-11-01-hand-edits-owed-for-phase-02-after-re-verification.md

key-decisions:
  - "The per-task RED-commit gate tripped before each GREEN (Rust gap #4379: missing RED commit for 27/1 and 27/2); continued on the operator-approved substitute evidence (RED commits e818692 and 5452316 by exact subject, ancestors of HEAD, 6 and 4 markers, all 10 records RED_EVIDENCE_OK); last_gate_trip not written because STATE.md has no such field"
  - "Orchestrator 2026-11-04: test_committed_gate_carries_the_deadline_into_the_fetched_head_listing walks 15 s with a 16 s hook (control bound WALK / 3 = 5 s, hooked bound 16 s + 5 s); the planned 5 s / 6 s left the control's 3x margin failing under parallel load (1.861 s against 1.667 s)"
  - "Accepted deviations: no #[expect(dead_code)] on the seam fields (the lint never fires, so the expectation was unfulfilled); #[expect(clippy::too_many_arguments)] on push_tree_entries; the byte-limit RED row's literal 1 << 18 replaced by MAX_LISTING_RECORDS in GREEN"

patterns-established:
  - "One budget object per gate call carries every bound of the in-process walk"

requirements-completed: [SEC-04]

coverage:
  - id: D1
    description: "A commit tree whose subtrees repeat is refused at the record limit in direct and PR mode, the remote untouched, naming no tree text; control under the production limit refused as unvalidated"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_direct_push_refuses_tree_over_the_record_limit"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_pr_push_refuses_tree_over_the_record_limit"
        status: pass
    human_judgment: false
  - id: D2
    description: "The fetched head's listing, the empty-status listing and both listings of one call are bounded by one record budget; the byte limit bounds long names; each with a just-under control"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_committed_gate_refuses_fetched_head_tree_over_the_record_limit"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_committed_gate_counts_both_listings_against_one_record_limit"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_fetched_head_holds_refuses_tree_over_the_record_limit"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_committed_gate_refuses_commit_tree_over_the_byte_limit"
        status: pass
    human_judgment: false
  - id: D3
    description: "The production limits are wired: over the record and byte limits refused, just under the record limit byte-identical to ls-tree"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_verified_tree_listing_refuses_tree_over_the_production_record_limit"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_verified_tree_listing_lists_tree_under_the_production_record_limit"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_verified_tree_listing_refuses_tree_over_the_production_byte_limit"
        status: pass
    human_judgment: false
  - id: D4
    description: "One deadline, the local time limit, bounds the commit listing, the fetched head's listing (reused, not renewed), the empty-status listing and both loops of listing_diff"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_committed_gate_refuses_commit_listing_past_the_deadline"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_committed_gate_carries_the_deadline_into_the_fetched_head_listing"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_fetched_head_holds_refuses_listing_past_the_deadline"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_listing_diff_refuses_changes_past_its_deadline"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_listing_diff_refuses_deletions_past_its_deadline"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_listing_budget_deadline_follows_the_local_time_limit"
        status: pass
    human_judgment: false
  - id: D5
    description: "A commit repeating an allowlisted path 64 times is refused after zero blob reads; control reads 2"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_committed_gate_refuses_repeated_path_before_reading_its_blobs"
        status: pass
      - kind: unit
        ref: "cargo test (full suite, 468 + 114 passed, 1 ignored pre-existing)"
        status: pass
    human_judgment: false

duration: 51min
completed: 2026-11-04
status: complete
---

# Phase 2 Plan 27: Bounded publication-gate tree walk Summary

**One budget per gate call (2^18 records, 64 MiB of listing bytes, one 30 s deadline) bounds every in-process listing and listing_diff with fixed refusals. The repeated-path check now runs before any blob is read.**

## Performance

- **Duration:** about 51 min, 2026-11-04T23:43Z to 2026-11-05T00:34Z. This includes one stop for an orchestrator decision.
- **Tasks:** 3 of 3.
- **Files modified:** 2.
- **Commits:** 5 task commits, measured from d5146c3.

## What changed and what it means

- **The walk is bounded again (review WR-600).** A post-commit hook could make the gate list 2^d paths from d + 3 tree objects, or very long names, without limit. Now every listing of one gate call draws on one `ListingBudget`:
  - at most 262 144 records and 64 MiB of listing bytes in total, taken before a tree's entries are pushed;
  - one deadline, the local time limit (30 s) from the start of the walk;
  - past either bound, a fixed refusal that names the route and never prints tree text.
- **listing_diff stops at the same deadline,** checked at each pass of both loops.
- **count_empty_directories is linear in the listing's bytes.**
- **A repeated path is refused before any blob read.** validate_committed_tree runs `tree_listing_blobs` before listing_diff and the change loop. A commit repeating an allowlisted path 64 times is refused after 0 blob reads; before the fix it took 64. With unique paths, the change loop reads at most one blob per allowlisted path.
- **No legitimate push was refused.** This repository lists 288 records. Its walk took 1.9 s with the production bounds, and the full suite passes.

## Task Commits

| Task | Commit | Subject |
|------|--------|---------|
| 1 RED | `e818692` | test(02-27): add RED rows for a tree over the record limit |
| 1 GREEN | `e914a41` | feat(02-27): cap the tree records one publication gate call lists |
| 2 RED | `5452316` | test(02-27): add RED rows for a walk past its deadline and a repeated path |
| 2 GREEN | `1125c68` | feat(02-27): bound the walk by one deadline and refuse repeats before blob reads |
| 3 | `9594b47` | docs(02-27): carry the walk bounds into the hand-edits todo |

Each commit passed `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and the full `cargo test` (timeout 1800). It was gated on their exit codes and on the branch check.

## Files Created/Modified

- `src/sync/mod.rs`, production:
  - new: MAX_LISTING_RECORDS, MAX_LISTING_BYTES, LISTING_TOO_LARGE, LISTING_PAST_DEADLINE, ListingBudget (`new`, `take`, `check_time`), `is_past`, `listing_kind`, `SyncEngine::listing_budget`, DiffDefect, ListingDefect::TooLarge and ::PastDeadline;
  - four cfg(test) seams;
  - both routes and listing_diff bounded;
  - the repeated-path check moved before the change loop;
  - count_empty_directories linear;
  - read_blob_by_id counts its reads under cfg(test).
- `src/sync/mod.rs`, tests:
  - helpers TOO_LARGE, PAST_DEADLINE, shared_subtree_tree, ls_tree_counts, ls_tree_record_count, ls_tree_byte_count, hook_shared_subtree, root_with_shared_subtree, assert_tree_over_the_record_limit_refused, validate_templates_commit, fetched_head_with_shared_subtree, root_with_bomb, production_listing, depth_over_the_production_record_limit, commit_changing_f, one_changed_record and diff_unbounded;
  - 16 new tests;
  - the five reorder updates.
- `.planning/todos/pending/2026-11-01-hand-edits-owed-for-phase-02-after-re-verification.md`: `## Added by plan 02-27`.

## New refusal format strings

- `LISTING_TOO_LARGE` = `is larger than the publication gate lists for one push; a tree in the cache checkout most likely repeats a subtree or holds very long names`
- `LISTING_PAST_DEADLINE` = `could not be listed within the publication gate's time limit for one push`
- The committed route:
  - `Post-commit publication gate violation: the tree of the commit to publish {LISTING_TOO_LARGE}`
  - `Post-commit publication gate violation: the tree of the commit to publish {LISTING_PAST_DEADLINE}`
  - `Post-commit publication gate violation: the tree of the fetched head {LISTING_TOO_LARGE}`
  - `Post-commit publication gate violation: the tree of the fetched head {LISTING_PAST_DEADLINE}`
  - `Post-commit publication gate violation: the changes of the commit to publish {LISTING_PAST_DEADLINE}`
- The empty-status route:
  - `{PREFIX} the fetched head's tree {LISTING_TOO_LARGE}`
  - `{PREFIX} the fetched head's tree {LISTING_PAST_DEADLINE}`
  - PREFIX = `Publication gate violation: git status reports nothing to push, but`.

The same text covers both limits, and no number is printed, so a test's lower limit prints the production text. The push rows and the direct rows assert that no refusal holds `bomb`, and the byte row that none holds the 4096-byte name.

## RED Evidence

Records are in `target/gsd-red/02-27-t<n>-<row>.json`, with junit copies in `target/gsd-red/02-27-t1-red.junit.xml` and `02-27-t2-red.junit.xml`. All 10 records classify `RED_EVIDENCE_OK`.

Negative control on the checker: a copy of one record forged to exit 0 with no failures classified `INVALID_RED`, and was deleted.

**Task 1** (at e9165f0 plus the seams; run exit 100, 6 of 6 failed):

| Row | Failure line | Assessment |
|-----|--------------|------------|
| test_direct_push_refuses_tree_over_the_record_limit | `mod.rs:18619`: `Post-commit publication gate violation: unvalidated file committed in HEAD` (TOO_LARGE assertion) | The hook ran, the remote was unchanged and git counted 195 records before it failed; the limit was ignored |
| test_pr_push_refuses_tree_over_the_record_limit | `mod.rs:18619`: same text | Same, PR mode |
| test_fetched_head_holds_refuses_tree_over_the_record_limit | `mod.rs:18684`: `a limit one under git's record count must refuse, got Ok(())` | 51 records and the just-under control (Ok) passed first |
| test_committed_gate_refuses_fetched_head_tree_over_the_record_limit | `mod.rs:18767`: unvalidated where TOO_LARGE was asserted | Counts (2, 49) and the control passed first |
| test_committed_gate_counts_both_listings_against_one_record_limit | `mod.rs:18807`: `both listings together are over 97, got Ok(())` | Counts (49, 49) and the limit-98 control (Ok) passed first |
| test_committed_gate_refuses_commit_tree_over_the_byte_limit | `mod.rs:18855`: unvalidated where TOO_LARGE was asserted (first refused_as_too_large call) | git accepted the 4096-byte name; the m control passed first |

**Task 2** (at e914a41 plus the seams; run exit 100, 4 of 4 failed):

| Row | Failure line | Assessment |
|-----|--------------|------------|
| test_committed_gate_refuses_commit_listing_past_the_deadline | `mod.rs:19199`: `a 1 ms walk must refuse, got Ok(())` | The default-time control passed first |
| test_fetched_head_holds_refuses_listing_past_the_deadline | `mod.rs:19242`: `a 1 ms walk must refuse, got Ok(())` | The control passed first |
| test_committed_gate_carries_the_deadline_into_the_fetched_head_listing | `mod.rs:19286`: `the fetched head's listing starts past the deadline, got Ok(())` | RED ran at WALK 5 s / HOOK 6 s; the control took 1.189 s, under 5/3 s |
| test_committed_gate_refuses_repeated_path_before_reading_its_blobs | `mod.rs:19332`: `blobs read before the repeat was refused`, left 64, right 0 | The control read 2; the refusal text was already `repeats a path`, after one read per repeat |

## Why each deadline row proves the deadline fired

- **test_committed_gate_refuses_commit_listing_past_the_deadline:**
  - The refusal holds PAST_DEADLINE and `the tree of the commit to publish`.
  - It holds neither TOO_LARGE nor the read-failure text `failed to list the tree of`.
  - The same call with the default walk time gives Ok(()); this is the negative control.
  - `with_listing_time` shortens only the walk, so each git read keeps its 30 s `limits.local`.
  - The call returned within 5 s.
- **test_fetched_head_holds_refuses_listing_past_the_deadline:**
  - The refusal holds PAST_DEADLINE, EMPTY_STATUS_REFUSAL and `the fetched head's tree`, and not `cannot be listed` (that route's read-failure text).
  - The default-time control gives Ok(()).
  - The reads keep their own limit.
- **test_committed_gate_carries_the_deadline_into_the_fetched_head_listing:**
  - The refusal holds `the tree of the fetched head` and PAST_DEADLINE, and not `the tree of the commit to publish`. So the commit listing finished inside the deadline and the fetched head's listing started past it.
  - A budget made afresh for that listing would have had a new 15 s and passed.
  - The no-hook control gives Ok(()) within WALK / 3.
- **test_listing_diff_refuses_changes_past_its_deadline and test_listing_diff_refuses_deletions_past_its_deadline:**
  - Each pairs `Instant::now()` (PastDeadline) with a deadline an hour away (Ok, the `M` or `D` record).
  - The deletion row has an empty commit listing, so only the deletion loop can refuse.

## Tests that passed on first run, and why

They need the fix's own items, which do not exist before it:
- test_verified_tree_listing_refuses_tree_over_the_production_record_limit, test_verified_tree_listing_lists_tree_under_the_production_record_limit and test_verified_tree_listing_refuses_tree_over_the_production_byte_limit need MAX_LISTING_RECORDS, MAX_LISTING_BYTES and `listing_budget`;
- test_listing_diff_refuses_changes_past_its_deadline and test_listing_diff_refuses_deletions_past_its_deadline need the deadline parameter and DiffDefect;
- test_listing_budget_deadline_follows_the_local_time_limit needs `listing_budget` and its deadline.

They are recorded as first-run passes, not as RED. The depth and size of each production-limit fixture is derived from the constants: d = 17 is over and d = 16 under the record limit. For the byte test, depth was added until git's byte count passed 64 MiB.

## The five tests whose expected refusal the reorder changed

| Test (assertion function) | Old expected text | New expected text |
|------|------|------|
| test_direct_push_refuses_index_hook_hidden_blob, test_pr_push_refuses_index_hook_hidden_blob (assert_index_hook_hidden_blob_refused) | `differs from the validated content` | `repeats a path` |
| test_direct_push_refuses_index_hook_hidden_token, test_pr_push_refuses_index_hook_hidden_token (assert_index_hook_hidden_token_refused) | `secret detected in committed HEAD blob` | `repeats a path` (the no-token-body assertion stays) |
| test_committed_gate_refuses_bad_path_components, row `..` | `unvalidated file committed` | `path component` |

- The doc comments of the two assertion functions now say the repeat is refused before the blob is read.
- No other assertion changed.
- The listing_diff calls in test_listing_diff and assert_listing_diff_matches now go through `diff_unbounded` (a deadline an hour away), with no other change.
- The rows that cross the moved check without asserting its text pass unchanged, in the Task 2 named run and the module run.

## Cost (module run `sync::tests:: process::tests::`)

| Run | Tests | nextest time | Wall |
|-----|-------|--------------|------|
| Before Task 1 (d5146c3 code) | 356 | 129.2 s | 135.3 s (includes a 5.7 s compile) |
| Task 1 verify, after GREEN | 365 | 149.3 s | not taken |
| Task 2 verify (5 s / 6 s code) | 372 | 141.3 s | 141.6 s |
| Task 2 verify after the 15 s / 16 s change | 372 | 152.7 s | 153.1 s |

What this does not show: earlier plans measured about 25% swing between runs. The rise is inside that noise, apart from the deadline-carry row's own 16 s sleep, which now adds about 16 s to that one test.

## Task 3 measurements (debug build, one host, one run each)

Each measured run was preceded by `cargo nextest list`, and no measured log holds a `Compiling` line.

| Measurement | Number | Bound | Within |
|-------------|--------|-------|--------|
| M1, this repository at 1125c68 (`git ls-tree -r -t HEAD`) | 288 records, 56 trees; 29 711 bytes `-z --full-tree` | recorded, no bound | n/a |
| M2, walk of this repository, production bounds | 288 records (git's own count agrees), 56 trees read, 1902 ms; test 1.924 s; peak RSS 40 124 kB | walk under 10 s; test under 15 s; RSS under 512 MiB | yes |
| M3 over, test_verified_tree_listing_refuses_tree_over_the_production_record_limit | 3.371 s; peak RSS 40 332 kB | under 10 s and 512 MiB | yes |
| M3 under, test_verified_tree_listing_lists_tree_under_the_production_record_limit | 2.856 s; peak RSS 49 588 kB | under 10 s and 512 MiB | yes |
| M4 gate at the record limit (196 610 records over both listings; 32 768 changed leaves) | gate call 6325 ms; test 6.960 s; peak RSS 64 764 kB | under 15 s and 512 MiB | yes |
| M4 gate at the byte limit (63 793 444 bytes = 95.1% of 64 MiB; 6146 records; 1066-byte names at depth 10) | gate call 4117 ms; test 6.222 s; peak RSS 111 756 kB | under 15 s and 512 MiB; bytes in 90-100%; records under 65 536 | yes |

**Implied tree ceiling** of the deadline for a legitimate push:
- per-tree cost = 1902 ms / 56 trees = 34.0 ms;
- LOCAL_TIMEOUT / per-tree cost = 30 000 ms / 34.0 ms, about **880 distinct trees**.

The operator has said template repositories are far below 1000 directories. This repository has 56 trees.

**What these numbers do not establish:**
- **Single samples.** Each is one run on one host with 4 CPUs, in a debug build; none is a release build.
- **The RSS floor.** Peak RSS comes from GNU time over `cargo nextest`, so it includes cargo's and nextest's own processes. The roughly 40 MB of the walk and the over-limit runs is mostly that floor, not the gate. Only the byte-limit probe (111.8 MB) clearly rises above it.
- **No derived memory bound.** Memory is not bounded by a derived number (02-27-PLAN.md `Not claimed`). The probes show one peak near each limit, not the worst case of the copies one gate call holds.
- **Fixture time.** The test times include building the fixture. The gate-call times are the `elapsed_ms` the probes printed.
- **Per-tree cost.** M2's per-tree cost is dominated by three git process spawns per tree object on this host. The ceiling moves with spawn cost.

The probes existed only in a copy of the source and were removed: the restore was confirmed with `git diff --exit-code -- src/sync/mod.rs`, and the copy was deleted. Logs: `target/measure-02-27-{repo.txt,over,under,walk,gate,bytes}.log`.

## Decisions Made

- **Orchestrator decision, 2026-11-04 (within its remit; the 5 s / 6 s values were its own plan revision):**
  - test_committed_gate_carries_the_deadline_into_the_fetched_head_listing now walks 15 s, with a 16 s hook.
  - The control bound is WALK / 3 = 5 s. The hooked call's bound is 16 s + 5 s.
  - The stop rule is kept.
- **The control timings that led to it, at 5 s / 6 s:**

  | Run | Control time | Result |
  |---|---|---|
  | Exact Task 2 verify 2 (43 tests in parallel) | 1.861 s | **fail** against 1.667 s |
  | Same command, repeat 1 | 1.354 s | pass |
  | Same command, repeat 2 | 1.431 s | pass |
  | Same command, repeat 3 | 1.662 s | pass |
  | RED run, for reference | 1.189 s | — |
  | Serial run, for reference | 0.903 s | — |

- **After the change:** the exact verify 2 passed (the row took 18.578 s in total). A timing run of the same names measured the control at 2.339 s, under the 5 s bound. The module run and the commit gate's full suite passed.
- **Where the deadline is checked.** It is checked first thing in push_tree_entries, before the `verified` lookup, so the root and every popped tree check it. listing_diff checks it at each pass of both loops.
- **`listing_budget` reads `self.limits.local` once,** and the cfg(test) override is applied on top of it. So production code reads it on exactly three lines.

## Deviations from Plan

All three were accepted by the orchestrator on 2026-11-04.

**1. [Rule 3 - Blocking] No `#[expect(dead_code)]` on the seam fields**
- **Found during:** Task 1 RED, with the same result in Task 2.
- **Issue:** clippy `-D warnings` failed with `this lint expectation is unfulfilled` on `listing_record_limit` and `listing_byte_limit`. Without the attribute, clippy is clean: the fields never warn, because the cfg(test) builders write them and tests use the builders. So the attribute had no place to move to. `listing_time` behaved the same.
- **Fix:** the RED commits carry no expectation. The final check for no dead-code expectation holds trivially.
- **Commits:** e818692, 5452316.

**2. [Rule 3 - Blocking] `#[expect(clippy::too_many_arguments)]` on push_tree_entries**
- **Issue:** the budget parameter makes 8 of 7.
- **Fix:** `expect`, with a reason, not `allow`, so it errors once it is no longer needed.
- **Commit:** e914a41.

**3. [Rule 3 - Blocking] The byte-limit RED row's record assertion**
- **Issue:** the row asserts its record count is under MAX_LISTING_RECORDS, a constant that does not exist before the fix.
- **Fix:** the RED commit used the literal `1 << 18`, and GREEN replaced it with the constant. This is the only edit to a RED row between its RED and GREEN commits.
- **Commit:** e914a41.

**Also recorded:**
- The plan named the hooked call's bound "within 6 s + 5 s". After the timing change it is HOOK_SLEEP + 5 s, as the orchestrator set it: 16 s + 5 s, held in a named constant AFTER_SLEEP.
- During the stop I also ran the Task 2 named verify once with `--no-capture`, to read the control time. That run is serial and is not the plan's command.

## TDD Gate Compliance

| Task | RED commit | GREEN commit | RED markers in the RED commit |
|------|------------|--------------|-------------------------------|
| 1 | `e818692` test(02-27): add RED rows for a tree over the record limit | `e914a41` feat(02-27): cap the tree records one publication gate call lists | 6 × `#[ignore = "RED: fails until 02-27 Task 1 fix"]` |
| 2 | `5452316` test(02-27): add RED rows for a walk past its deadline and a repeated path | `1125c68` feat(02-27): bound the walk by one deadline and refuse repeats before blob reads | 4 × `#[ignore = "RED: fails until 02-27 Task 2 fix"]` |
| 3 | no tdd attribute; the per-task gate does not run | `9594b47` docs(02-27) | none |

Each GREEN commit descends from its RED commit (`git merge-base --is-ancestor`), and no RED marker of this plan survives in src.

**The per-task RED-commit gate, run unchanged before each GREEN.**
- **How it was bound:** a script copying execute-phase.md's `TDD gate` block verbatim, with `TDD_MODE=true`, `PHASE_NUMBER=02`, `PLAN_ID=27`, `TASK_ID` 1 or 2, and the plan file as `TASK_FILE`. `task.is-behavior-adding` returned true both times.
- **Trip lines, verbatim:** `TDD GATE TRIPPED: missing RED commit for 27/1` and `TDD GATE TRIPPED: missing RED commit for 27/2`.
- **`last_gate_trip`: none written.** `state.update` replied `{"updated": false, "reason": "Field \"last_gate_trip\" not found in STATE.md", "preserved": []}`. STATE.md's sha256 was the same before and after each run.
- **Substitute evidence, Task 1:**
  - `e8186928cd0ad6aadaa2c29e1dc8792c6e4c3d58`, found by its exact subject (1 match);
  - an ancestor of HEAD;
  - its only file is src/sync/mod.rs, with 6 markers;
  - all 6 records are `RED_EVIDENCE_OK`.
- **Substitute evidence, Task 2:**
  - `5452316deaea9d72c49c699396ace91da533389c`, found by its exact subject (1 match);
  - an ancestor of HEAD;
  - its only file is src/sync/mod.rs, with 4 markers;
  - all 4 records are `RED_EVIDENCE_OK`.
- **Control:** the same `evaluation-scope` query without the test-file pathspecs returned the RED commit each time (`e8186928cd …`, `5452316dea …`). So each trip comes from the pathspec (Rust gap #4379), not from a missing commit.
- GREEN continued under the operator's approval of the substitute evidence (STATE.md). No pathspec was widened and no file under tests/ was touched.

## Threat Flags

| Flag | File | Description |
|------|------|-------------|
| T-02-110 (mitigate) | src/sync/mod.rs | Unbounded walk memory and time from repeated subtrees, long names or deep chains. Mitigated by one ListingBudget per gate call (records, bytes, one deadline), count_empty_directories made linear, and fixed refusals. RED rows on every caller; production-limit tests. **Supersedes T-02-102 (02-24),** whose claim that each bounded read bounds the walk was wrong |
| T-02-111 (mitigate) | src/sync/mod.rs | The bounds refusing a legitimate push. This repository: 288 records, 56 trees, a 1.9 s walk. The tree just under the production limit lists as ls-tree does. Every Task 3 measurement is within its bound. The no-change, deletion, benign-hook and gc'd-remote publish rows and the full suite pass. Tree ceiling: about 880 |
| T-02-112 (mitigate) | src/sync/mod.rs | Test seams reaching a non-test build. All four fields and builders are cfg(test), checked by Task 1's and Task 2's structural verifies. listing_budget and read_blob_by_id read them only under cfg(test) |
| T-02-113 (mitigate) | src/sync/mod.rs | A refusal printing a tree-chosen name. The texts are fixed; the rows assert no `bomb` and no long name |
| T-02-114 (mitigate) | src/sync/mod.rs | listing_diff's quadratic multiset match. The deadline is checked at each pass of both loops, pinned by two rows with far-deadline controls |
| T-02-115 (mitigate) | src/sync/mod.rs | The change loop reading a repeated path's blob once per repeat. tree_listing_blobs now runs before the loop: 0 reads, against a control of 2; the five tests updated |
| T-02-SC (mitigate) | Cargo.toml, Cargo.lock | No crate added. `git diff --exit-code e9165f0 -- Cargo.toml Cargo.lock` exits 0 |

No security surface beyond the threat model.

## Not claimed

- **The change loop is not under the deadline** (orchestrator decision of 2026-11-04). With the repeated-path check first, it reads at most one blob (three git runs) per allowlisted path.
- **A single tree object of many megabytes is not bounded by the byte limit.** It is bounded by its read's time limit, and its memory is linear in what the hook wrote.
- **No memory bound is derived.** Task 3 measured one peak near each limit.
- **No release-build numbers.** Every number above is from a debug build.
- **Left to plan 02-28:** docs/ARCHITECTURE.md and 02-20-PLAN.md (DEF-09), the split of review IN-600, the controls on the final code and the phase gates. Neither docs/ARCHITECTURE.md nor any NOT row was edited here; NOT-14 is unchanged.

## Known Stubs

None.

## Issues Encountered

- The deadline-carry row's control margin failed once under parallel load at 5 s / 6 s. The plan stopped and reported, and the orchestrator decided on 15 s / 16 s (see Decisions Made).
- The plan-time counts of this repository (286 records at e9165f0) moved to 288 at 1125c68, because plan files were added. The walk and git agree on 288.

## User Setup Required

None.

## Next Phase Readiness

- Plan 02-28 can run on the final code: the IN-600 split, DEF-09 in docs/ARCHITECTURE.md and 02-20-PLAN.md, the controls, the scoped mutation run and the phase gates.
- The hand edits this plan owes are in the todo's `## Added by plan 02-27`.

## Self-Check: PASSED

- src/sync/mod.rs and the todo exist with this plan's changes, and every file named under Task 3 exists.
- All 5 commits are ancestors of HEAD.
- `git status --porcelain` was empty before this SUMMARY was written.
