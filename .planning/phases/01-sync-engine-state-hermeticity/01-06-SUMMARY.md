---
phase: 01-sync-engine-state-hermeticity
plan: 06
subsystem: sync
tags: [rust, tempfile, staging, transaction, file-modes, symlink, cargo-mutants, tdd]

requires:
  - phase: 01-04
    provides: "ProjectState::stage_to_dir / StagedState::commit (validated, staged state.json write)"
  - phase: 01-05
    provides: "EOL-preserving pull/push content and fingerprint + LF ancestor records (kept unchanged by this plan)"
provides:
  - "sync::staging::StagedTransaction (new, stage_write, commit, Drop cleanup): stage and validate every write, then rename; temps and created dirs removed on abort"
  - "sync::staging::file_mode_for, EXECUTABLE_MODE (0o755), REGULAR_MODE (0o644)"
  - "pull_updates: all files and state.json staged before any rename; files committed, state.json last"
  - "push_updates: templates staged under upstream_root; state staged in persist-immediately mode and committed after the templates"
affects: [01-07, 01-09, 01-10, sync]

actuals:
  tokens: 8779
  tasks: 3
  commits: 8
plan_head_before: f4b56cbc631e0bab9258dd5661191181840344c0
plan_head_after: 09c041e1ba5f67c40edf77896ff2f3ad4b45e483

tech-stack:
  added: []
  patterns:
    - "Stage -> validate -> commit: every workspace or template write goes through StagedTransaction; state.json is staged before and committed after the files"
    - "Explicit modes on staged temps (never the 0o600 tempfile default); existing modes kept, execute added on hook/script paths only where read is granted"

key-files:
  created: [src/sync/staging.rs]
  modified: [src/sync/mod.rs]

key-decisions:
  - "A rename-phase failure (after all staging succeeded) is reported as IoFailed on the transaction root (project dir for pull, upstream root for push); the reason names the file and how many files were already replaced. StagedTransaction::commit keeps the planned Result<Vec<PathBuf>, String> signature"
  - "Executable paths are matched by path components (Path::starts_with \"scripts/hooks\" / \"scratch\"), not string prefixes"
  - "create_missing_dirs treats any stat failure as missing and lets create_dir report the real error (removes an equivalent mutant)"
  - "Existing modes are taken with the plan's 0o7777 mask, so setuid/setgid/sticky bits on an existing managed file are kept"

patterns-established:
  - "Plans 01-07 and 01-09 add stage_delete and sidecars to the same StagedTransaction; state stays staged before and committed after it"

requirements-completed: [SYNC-02]

coverage:
  - id: D1
    description: "Pull stages every file and state.json before replacing anything; a staging failure leaves .gitconfig, .gitignore and state.json byte-identical and no .tmp* file anywhere; without the failure all three files and the state apply"
    requirement: SYNC-02
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_pull_staging_failure_no_partial_writes"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_pull_updates_materializes_files_and_updates_state, test_pull_updates_conflict_blocked_no_partial_writes, test_pull_updates_refuses_to_write_through_symlink"
        status: pass
    human_judgment: false
  - id: D2
    description: "Staged files get explicit modes: new hook 0o755, new config 0o644, existing 0o600 / 0o640 kept, existing 0o644 hook -> 0o755, existing 0o700 hook stays 0o700; never the 0o600 temp default"
    requirement: SYNC-02
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_staged_write_modes"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_file_mode_for"
        status: pass
      - kind: other
        ref: "cargo mutants --file src/sync/staging.rs -- --bin ude -- sync:: : 28 mutants, 28 caught, 0 missed"
        status: pass
    human_judgment: false
  - id: D3
    description: "An aborted transaction removes only the directories it created (a pre-existing empty dir stays); a symlink swapped in after staging is refused at commit and not replaced"
    requirement: SYNC-02
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_staging_abort_removes_created_dirs"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_commit_revalidates_symlink_before_rename"
        status: pass
      - kind: other
        ref: "4 hand mutants (commit skips re-validation; Drop removes dirs before temps; pre-existing parent recorded as created; directory-target check skipped) each fail the named test"
        status: pass
    human_judgment: false
  - id: D4
    description: "Push writes upstream templates through the same transaction: a staging failure on the hook leaves gitconfig.repo and state.json unchanged; no production fs::write remains in src/sync/mod.rs"
    requirement: SYNC-02
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_push_staging_failure_no_partial_writes"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_push_updates_* (6 existing push tests)"
        status: pass
      - kind: other
        ref: "plan Task 3 automated check: production fs::write count 0, file total 77 (pre-plan source: 2 production calls)"
        status: pass
      - kind: integration
        ref: "hermeticity gate: GIT_DIR=<decoy bare repo> GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 cargo test --bin ude sync:: (66 passed)"
        status: pass
    human_judgment: false
  - id: D5
    description: "Rename-phase behaviour: files are committed before state.json, and a failure between renames reports how many files were replaced (D-01 partial outcome)"
    requirement: SYNC-02
    verification:
      - kind: other
        ref: "code reading: in pull_updates and push_updates transaction.commit() precedes the state commit (rg order check)"
        status: pass
    human_judgment: true
    rationale: "No test injects a failure between two renames, and no test fails if the state commit is moved before the file commit. The count text is exercised only for the '0 of 1' case (symlink re-validation)."
  - id: D6
    description: "Remote-mode cache lock serializes two ude sync commands for the whole command (backstop truth; unchanged by this plan)"
    verification: []
    human_judgment: true
    rationale: "Rests on existing src/main.rs (_remote_lock binding) and the 01-03 lock tests; this plan neither changed nor re-tested it."

duration: 15min
completed: 2026-10-18
status: complete
---

# Phase 01 Plan 06: Staged Multi-File Writes with Explicit Modes Summary

**Pull and push now stage every file (and `state.json`) as same-directory temp files, validate all of them, and only then rename them over their targets, with `state.json` committed last. A staging failure changes nothing on disk. Staged files get explicit modes: existing modes are kept, hooks and scripts never end up non-executable, and permissions are never widened.**

## Performance

- **Duration:** about 15 min
- **Started:** 2026-10-18T19:46:36Z
- **Completed:** 2026-10-18T20:01:57Z
- **Tasks:** 3 (plus one coverage-test commit and one refactor)
- **Files modified:** 2

## Accomplishments
- New `src/sync/staging.rs` with `StagedTransaction`. `stage_write` runs `validate_safe_write_target`, refuses a target that is a directory, creates missing parents one level at a time (recording each), writes a `NamedTempFile` in the target's directory and sets its mode. `commit` re-validates each target right before its rename and stops at the first failure, reporting how many files were already replaced. `Drop` deletes the temps first, then removes the directories it created, deepest first.
- `pull_updates`: Pass 1 is unchanged (01-05 EOL rules intact). Pass 2 stages every candidate, records the ancestors in memory, stages `state.json` with `stage_to_dir`, then commits the files and then the state.
- `file_mode_for`: an existing file keeps its mode (0o600 stays 0o600). An existing hook or script gets an execute bit for each class that can read it, plus the owner bit (0o644 -> 0o755, 0o700 stays 0o700, 0o640 -> 0o750). New files get 0o755 (hook/script) or 0o644.
- `push_updates`: templates are staged under `upstream_root`. In persist-immediately mode `state.json` is staged before the templates are committed and committed after them. No production code in `src/sync/mod.rs` calls `fs::write` any more.

## Task Commits

1. **Task 1 (tracer) RED:** `041d4df` test(01-06): add failing test for staged pull with no partial writes
2. **Task 1 GREEN:** `6131480` feat(01-06): stage pull writes and state.json before replacing anything
3. **Task 2 RED:** `142945b` test(01-06): add failing tests for staged modes, dir cleanup and symlink re-check
4. **Task 2 GREEN:** `a45d5f4` feat(01-06): set explicit staged file modes with read-gated execute bits
5. **Coverage (deviation, see below):** `e498582` test(01-06): cover file_mode_for for every path family and existing mode
6. **REFACTOR:** `ff81a0a` refactor(01-06): let create_dir report uninspectable staging parents
7. **Task 3 RED:** `9cfdeab` test(01-06): add failing test for staged push with no partial writes
8. **Task 3 GREEN:** `09c041e` feat(01-06): write upstream templates through the staged transaction

Tracer gate (interactive, `end-of-phase`, automated-only verify): Task 1's verify command was re-run on the committed tree at `6131480` (4 passed) before Task 2 started.

## TDD Gate Compliance

Each task has a `test` commit before its `feat` commit. One `refactor` commit follows Task 2. `e498582` is a coverage test added after GREEN, not a RED commit.

RED evidence: nextest JUnit reports, classified by `gsd_run check tdd-red-evidence`. All three returned `RED_EVIDENCE_OK` / `target_test_failed`.

| Target | Failed on | Semantic assessment |
|---|---|---|
| test_pull_staging_failure_no_partial_writes | `.gitconfig` bytes changed (upstream "input" written before the hook failed) | intended |
| test_staged_write_modes | `scripts/hooks/commit-msg: 700 became 755` | intended (Task 1's copied rule forced 0o755) |
| test_push_staging_failure_no_partial_writes | `gitconfig.repo` bytes changed before the hook failed | intended |

**Characterization tests (not RED):** `test_staging_abort_removes_created_dirs` and `test_commit_revalidates_symlink_before_rename` passed in the Task 2 RED run, because Task 1 already built that behaviour (the plan expected this). To show they still detect breakage, four hand mutants were run against them, and each mutant made its test fail (see Verification).

## Verification

| Check | Result |
|---|---|
| Task 1 verify (`test_pull_staging_failure_no_partial_writes`, `test_pull_updates*`) | 4 passed |
| Task 2 verify (4 named tests) | 4 passed |
| Task 3 verify (`test_push_staging_failure_no_partial_writes`, `test_push_updates*`) | 7 passed |
| No production `fs::write(` in `src/sync/mod.rs` | exit 0. Production count 0, file total 77. Control: the pre-plan source (`f4b56cb`) has 2 production calls |
| Task 2 acceptance grep (inline 0o755 rule gone from pull_updates) | exit 0. Control: the pre-plan source matches it twice |
| Hermeticity gate | 66 passed, exit 0. `git rev-parse` confirmed the decoy is a valid bare repo |
| `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test` | exit 0 before every commit. Final: 93 unit + 11 e2e (before this plan: 88 + 11) |
| Mutation (`cargo mutants --file src/sync/staging.rs`, `sync::` tests) | First run: 31 mutants, 3 missed. After `e498582` and `ff81a0a`: 28 mutants, 28 caught, 0 missed. caught.txt (28) and outcomes.json (28 CaughtMutant) agree |
| Hand mutants (cargo-mutants cannot generate these) | 4/4 caught. Source restored by rewriting the original bytes and bumping mtime; empty `git diff` and 65 `sync::` tests green afterwards |

## What the verification does NOT establish
- **Rename-phase failures:** no test injects a failure between two renames. If the state commit were moved before the file commit, no test would fail; the order was checked only by reading the code. The "N of M files were already replaced" text is exercised only for "0 of 1".
- **Push with `persist_ancestor_immediately = false`:** every push test passes `true`. The `None` state branch is covered by the compiler only, not by a test.
- **Special mode bits:** setuid/setgid/sticky bits on an existing managed file are kept (0o7777 mask, as planned). No test covers this.
- **Windows:** nothing here was compiled or run on Windows. Mode handling is `cfg(unix)`.
- **Directories created by staging** get the process umask mode (`fs::create_dir`), the same as the old `create_dir_all`. Not asserted.
- **Remote lock serialization** (backstop truth) was not touched or re-tested here.

## Files Created/Modified
- `src/sync/staging.rs` (new) - StagedTransaction, file_mode_for, EXECUTABLE_MODE, REGULAR_MODE
- `src/sync/mod.rs` - `pub mod staging;`; pull and push Pass 2 rewritten on the transaction; 6 new tests (`test_pull_staging_failure_no_partial_writes`, `test_staged_write_modes`, `test_file_mode_for`, `test_staging_abort_removes_created_dirs`, `test_commit_revalidates_symlink_before_rename`, `test_push_staging_failure_no_partial_writes`) plus fixtures `staging_failure_fixture`, `push_staging_fixture`, `tmp_leftovers`, `file_mode`

## Decisions Made
- **Error path for a rename-phase failure:** the plan did not say which path `IoFailed` should name. It names the transaction root (project dir for pull, upstream root for push), and the reason names the file. `commit` keeps the planned `Result<Vec<PathBuf>, String>` signature, which plans 01-07 and 01-09 build on.
- **Executable-path matching** uses path components, so `scripts/hooksX` would not match. All managed paths match as before.
- **`create_missing_dirs`** treats any stat failure as "missing" and lets `create_dir` report the real error (ENOTDIR under a regular file). The result is the same, and an equivalent mutant goes away.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Test strength] Two mode mutants survived the planned tests**
- **Found during:** mutation run after Task 2 GREEN
- **Issue:** no test covered `scratch/*` or `scripts/check-in-container.sh` modes, or an existing hook that is already executable (`|` -> `^` survived).
- **Fix:** `test_file_mode_for` covers each executable path family (new and existing), 0o755 -> 0o755, 0o640 -> 0o750, 0o000 -> 0o100, non-script files at 0o600 and 0o755, and a symlinked target.
- **Verification:** the re-run reports 28/28 caught.
- **Committed in:** `e498582`

**2. [Rule 2 - Test strength] Extra controls in the planned tests**
- `test_pull_staging_failure_no_partial_writes`: a positive control for the `.tmp` walker (a planted nested `.tmpPROBE` must be found).
- `test_staging_abort_removes_created_dirs`: a pre-existing empty `scratch/` must survive the abort. This catches recording a parent the transaction did not create.
- `test_commit_revalidates_symlink_before_rename`: also asserts that the planted symlink is still a symlink and the error says "0 of 1". The victim file stays unchanged even without re-validation (a rename replaces the link), so checking the victim alone would not catch a missing re-check.
- `test_push_staging_failure_no_partial_writes`: also asserts that `state.json` is unchanged and that there are no `.tmp*` files in the local project.

---

**Total deviations:** 2 auto-fixed (both Rule 2, test strength). **Impact:** tests only, plus the behaviour-neutral refactor `ff81a0a`. No production scope change.

## Issues Encountered
- **One gate failure with no recorded cause:** one pre-commit gate run for Task 3 GREEN failed while the background mutation run was loading the CPU. Its `cargo test` output went to `/dev/null`, so the cause is unknown. fmt and clippy passed when re-run alone. Three full-suite runs in a row and the logged commit gate then passed. A load-sensitive timing test (for example the 01-03 lock-timeout tests) is the likely cause, but this is unverified.

## Known Stubs
None.

## Threat Flags
None. T-01-15 (re-validation at stage time and before rename), T-01-16 (modes) and T-01-17 (no partial writes on a staging failure) are mitigated and tested. T-01-18 (partial rename phase) is accepted under D-01. No new trust-boundary surface: every write still goes through `validate_safe_write_target`.

## User Setup Required
None.

## Next Phase Readiness
- Plan 01-07 can add `stage_delete` to `StagedTransaction` (deletions committed after writes) and use `file_mode_for`.
- Plan 01-09 can stage `.conflict` sidecars with `stage_write` and `file_mode_for(...) & 0o666`.
- `PullResult::Success { applied_files }` still counts applied writes; 01-07 replaces it with `PullReport`.

## Self-Check: PASSED
- FOUND: src/sync/staging.rs, src/sync/mod.rs, .planning/phases/01-sync-engine-state-hermeticity/01-06-SUMMARY.md
- FOUND on this branch: 041d4df, 6131480, 142945b, a45d5f4, e498582, ff81a0a, 9cfdeab, 09c041e

---
*Phase: 01-sync-engine-state-hermeticity*
*Completed: 2026-10-18*
