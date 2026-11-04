---
phase: 02-sanitizer-boundaries-security-hardening
plan: 20
subsystem: sync
tags: [docs, publication-gate, closed-list, security]

requires:
  - phase: 02-19
    provides: require_fetched_head_holds, first_path_not_held, the parent-free continuity refusal, and the tests DEF-12 and DEF-14 name
provides:
  - "docs/ARCHITECTURE.md section 3 subsection `### What the publication gate promises`: scope, what was run, three promises, DEF-01 to DEF-15 (byte-identical to the plan), NOT-01 to NOT-15 (plan rows with the Record column dropped), and the rule for classifying a later finding"
  - "section 3's opening sentence no longer says `To guarantee`; it says changes pass through the gate and points at the last subsection for what is and is not promised"
  - "doc comments on ValidatedBlobs, validate_committed_tree and push_to_remote point at docs/ARCHITECTURE.md section 3; ValidatedBlobs no longer says `whatever git filter or commit hook changed it`"
affects: [02-21]

actuals:
  tokens: 6826    # chars/4 of `git diff c7d8e86 93b9392 -- docs src` (diff text, context lines included)
  tasks: 2
  commits: 2      # git rev-list --count plan_head_before..plan_head_after
plan_head_before: c7d8e86dcb23f1513684bc9782ad7c8d7edcbff1
plan_head_after: 93b93926eb1a4efc5af41021d5b388968e976e1e

tech-stack:
  added: []
  patterns:
    - "The gate's promise is a closed list in docs/ARCHITECTURE.md; code comments defer to it instead of making open-ended claims"
    - "The doc copy is generated from the plan's rows (extraction script), not retyped, so the byte comparison in the verify holds by construction"

key-files:
  created: []
  modified:
    - docs/ARCHITECTURE.md
    - src/sync/mod.rs

key-decisions:
  - "02-20: the gate's promise is the closed list in docs/ARCHITECTURE.md section 3 (DEF-01..DEF-15 defended, NOT-01..NOT-15 not); it is closed by operator decision, not proven exhaustive"
  - "02-20: section 3's opening points at the subsection by position (`the last subsection of this section`), not by its title, so the title appears once in the file as the acceptance criterion requires"

patterns-established:
  - "A later finding is classified by the doc's rule: a defect against a DEF row, an accepted limit on a NOT row, or new scope for the operator"

requirements-completed: []   # SEC-04, SEC-02 declared; requirements.ready-ids reports both blocked by sibling plan 02-21

coverage:
  - id: D1
    description: "docs/ARCHITECTURE.md holds the closed list: 15 DEF rows identical to the plan's, 15 NOT rows identical once the Record column is dropped, NOT-15 present, and no threat, review, decision or requirement ID, PLAN file or .planning reference in the subsection"
    requirement: SEC-04
    verification:
      - kind: other
        ref: "02-20-PLAN.md Task 1 verify 1 (row counts, four cells per row, DEF/NOT byte comparison, ID-free subsection, ID pattern sees the plan's Record column)"
        status: pass
    human_judgment: false
  - id: D2
    description: "Every test named in a DEF row exists exactly once (bin ude or tests/e2e_lifecycle) and passes: 214 distinct names, 214 run, 214 passed"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "cargo nextest run --bin ude --test e2e_lifecycle <the 214 DEF-row names> (Task 1 verify 1)"
        status: pass
    human_judgment: false
  - id: D3
    description: "02-CONTEXT.md and plans 02-01 to 02-15 are unchanged from b8c54e9, all sixteen present there"
    requirement: SEC-04
    verification:
      - kind: other
        ref: "02-20-PLAN.md Task 1 verify 2 (git ls-tree count 16, git diff --exit-code b8c54e9)"
        status: pass
    human_judgment: false
  - id: D4
    description: "Three doc comments in src/sync/mod.rs point at docs/ARCHITECTURE.md; nothing but comment lines changed; fmt and clippy clean"
    requirement: SEC-04
    verification:
      - kind: other
        ref: "02-20-PLAN.md Task 2 verify 1 (rg count >= 3, comment-stripped prefix before `mod tests {` equal to HEAD) and verify 2 (cargo fmt --check, cargo clippy --workspace --all-targets -- -D warnings)"
        status: pass
    human_judgment: false
  - id: D5
    description: "Whether the list's wording reads correctly to a later reviewer (that a finding can be classified from the doc alone) is a reading, not a mechanical check"
    requirement: SEC-04
    verification: []
    human_judgment: true
    rationale: "The plan's success criterion is that a later review can classify a finding from the doc; no test can assert comprehension. The pairwise separation of rows was a reading at plan revision, bounded to the pairs the plan lists"

duration: 7min
completed: 2026-11-04
status: complete
---

# Phase 02 Plan 20: The publication gate's closed list Summary

**docs/ARCHITECTURE.md section 3 now states what the publication gate promises as a closed list (15 defended channels, each with the tests that pin it, and 15 recorded non-promises), and the gate's doc comments point at it instead of claiming to stop "whatever git filter or commit hook" changed a blob. The list is closed by operator decision; it is not proven exhaustive.**

## Performance

- **Duration:** 7 min
- **Started:** 2026-11-04T03:29:37Z
- **Completed:** 2026-11-04T03:36:34Z
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments

- Subsection `### What the publication gate promises` added at the end of section 3, before the `---` that precedes section 4. It holds the scope, what was run, the three promises, the DEF table, the NOT table without the plan-only `Record (this plan only)` column, and the classification rule. The `How the rows were kept apart` paragraph stays in the plan only.
- The doc copy was produced by a script that extracts the rows from the plan, so the DEF rows are byte-identical and each NOT row is the plan row cut after its third cell.
- Section 3's opening sentence was rewritten: changes pass through the gate, and the last subsection states what it does and does not promise. Nothing else in the file changed.
- `ValidatedBlobs` now says what its record is compared with: each staged index blob (the staged gate), each blob the commit changes read by object id (the committed gate), and each recorded path in the commit's tree, or in the fetched head's tree when there is nothing to push (`first_path_not_held`, from `require_validated_content` and `require_fetched_head_holds`). Each of these was checked against the code. It then points at the closed list for which channels that defends.
- `validate_committed_tree` and `push_to_remote` each gained one sentence: what they defend is the closed list in docs/ARCHITECTURE.md section 3, and nothing beyond it.

## Task Commits

1. **Task 1: Publish the closed list in docs/ARCHITECTURE.md and prove every named test exists and passes** - `333dcde` (docs)
2. **Task 2: Make the code's comments defer to the list** - `93b9392` (docs)

## Verification

- Task 1 verify 1 (the plan's exact command): exit 0. 214 tests run, 214 passed. Counted a second way: the DEF rows hold 217 name tokens and 214 distinct names (three names appear in two rows: shared between DEF-02 and DEF-12, and DEF-02 and DEF-14), which matches the 214 nextest ran. Re-run after the final edit to the doc: exit 0 again.
- Task 1 verify 2: exit 0 against b8c54e9.
- Acceptance: `rg -c 'What the publication gate promises' docs/ARCHITECTURE.md` prints 1. At HEAD before the plan it found nothing.
- Task 2 verify 1: exit 0 before the commit. `docs/ARCHITECTURE.md` occurs 3 times in src/sync/mod.rs (0 at HEAD). Second, independent check: all 13 changed lines in the diff are `//` comment lines, so nothing changed after `mod tests {` either. The strip comparison does not cover that part of the file.
- Task 2 verify 2: `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings` both exit 0. The commit was gated on them directly, with no pipe.
- Full suite: `PROPTEST_DISABLE_FAILURE_PERSISTENCE=1 cargo nextest run --workspace --no-fail-fast` gave 529 passed and 1 skipped. The skipped test is the pre-existing `#[ignore]` `test_dump_parity_corpus` in src/sanitizer/gitleaks.rs. After the run, the repo's `git config --local -l` and `git status --porcelain` hashes were unchanged.

### Negative controls

Each control below must fail, and each did. All ran on scratch copies outside the repo.

- The doc checks of verify 1 ran on four inputs:
  - HEAD's doc fails, because its DEF rows differ.
  - A doc with one DEF byte changed (`The commit` to `the commit` in DEF-07) fails.
  - A doc with `(see T-02-84)` inserted in the classification paragraph fails, on the ID check.
  - A doc with `(see 02-20-PLAN.md)` inserted there fails, on the ID check.
  - The real doc passes.
- The exactly-one-test check: the bogus name `test_no_such_test_exists` matches 0 tests and the bare prefix `test_direct_push_refuses` matches 0, so both would fail the check. The real name `test_confirm_published` matches 1.
- Verify 2: run against 85898aa~1, it exits 1, as the plan states.
- Task 2's comment-only check: a copy with `const NEGATIVE_CONTROL: u8 = 0;` inserted before `mod tests {` exits 1.

### What these checks do not establish

- That the list is complete. The list is closed by decision.
- That each DEF test proves its row's whole class. A passing test proves the case it runs, on a local path or file URL with git 2.56.0 on Linux, which is what the doc's `What was run` paragraph says.
- That no NOT row overlaps a DEF row. That is the plan's pairwise reading, bounded to the pairs the plan lists, and it was not re-checked mechanically here.
- Names in NOT rows are not checked by the verify, which extracts only DEF-row names. The one test a NOT row names, `test_index_hook_exec_flip_reaches_the_remote` (NOT-10), was confirmed to exist (src/sync/mod.rs). It was not run separately, but the full suite ran it and it passed.
- The scope's function names (`ensure_remote_repo_for_push`, `push_to_remote`, `push_updates`, `with_excerpt`) were confirmed to exist with rg.

## Files Created/Modified

- `docs/ARCHITECTURE.md`: section 3 opening sentence reworded; subsection `### What the publication gate promises` added
- `src/sync/mod.rs`: doc comments on `ValidatedBlobs`, `push_to_remote` and `validate_committed_tree` (comments only)

## Decisions Made

- The opening sentence refers to "the last subsection of this section" rather than quoting its title. Quoting the title made the acceptance count 2, and the criterion requires 1.
- No test renamed since the rows were written: none of the 02-16 to 02-19 SUMMARYs records a rename, and every DEF name resolved to exactly one test.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Opening sentence repeated the subsection title**
- **Found during:** Task 1 (acceptance check)
- **Issue:** My first wording of the new opening sentence named the subsection `What the publication gate promises`, so the acceptance count was 2, not 1.
- **Fix:** The sentence now points at "the last subsection of this section".
- **Files modified:** docs/ARCHITECTURE.md
- **Verification:** the count prints 1, and verify 1 was re-run on the final doc (exit 0, 214 of 214 passed)
- **Committed in:** 333dcde (before the commit)

---

**Total deviations:** 1 auto-fixed (Rule 1). **Impact on plan:** a wording fix inside the task, with no change in scope.

## Issues Encountered

- My first run of the negative controls printed `exit=0` for every case, because `$?` was read after a `$(basename)` substitution. The failure messages showed that the failing path had run. I re-ran the controls with the exit code captured first, and the results above come from that run.

## User Setup Required

None.

## Next Phase Readiness

- Ready for 02-21. Its `Who updates which document` section tells the orchestrator to point the re-verification at this list. Plan 02-21 re-runs Task 1's verify at the end.
- Still open, as recorded in the list itself: a failed git command's stderr excerpt is still printed in refusals (NOT-09, measured); the items in NOT-10 are pending the operator.

## Self-Check: PASSED

- FOUND: docs/ARCHITECTURE.md, src/sync/mod.rs
- FOUND: 333dcde, 93b9392 (ancestors of HEAD; `check evaluation-scope --plan 02-20 --commits-only` lists both)
- Commits measured from the ledger: c7d8e86..93b9392 = 2

---
*Phase: 02-sanitizer-boundaries-security-hardening*
*Completed: 2026-11-04*
