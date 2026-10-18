---
phase: 01-sync-engine-state-hermeticity
plan: 04
subsystem: sync
tags: [rust, state, tempfile, atomic-write, exit-codes, symlink, tdd]

requires:
  - phase: 01-03
    provides: "bounded, hermetic sync engine; hermeticity gate; validate_safe_write_target unchanged"
provides:
  - "pub const CURRENT_STATE_VERSION: u32 = 1 (plan 01-05 raises it to 2)"
  - "ProjectState::load_from_dir -> Result<Self, String>: Err on unreadable, corrupt, version 0 or newer-than-supported state; missing file -> Ok(default)"
  - "ProjectState::stage_to_dir -> Result<StagedState, String> and StagedState::commit (validate at stage and before persist); save_to_dir = stage + commit"
  - "pub(crate) const STATE_FILE_REL (\".ude/state.json\"), STATE_FILE_MODE 0o644"
  - "SyncEngine::scan_project -> Result<Vec<FileSyncState>, String>; pull/push map state errors to IoFailed { path: .ude/state.json }"
  - "src/main.rs EXIT_DRIFT = 1, EXIT_HARD_FAILURE = 2 in the sync dispatch"
affects: [01-05, 01-06, 01-07, 01-08, 01-09, 01-10, 01-11, sync, cli]

actuals:
  tokens: 8298
  tasks: 2
  commits: 4
plan_head_before: 5834125662a52bd51463f7bc54041c6b9de2b5a0
plan_head_after: 59797eb67d03534620c53ce02ba941752e4ed69d

tech-stack:
  added: []
  patterns:
    - "State errors are Result<_, String> and surface as exit 2; the state file is never reset by a load"
    - "Single-file atomic write: validate target -> NamedTempFile::new_in(parent) -> write -> set mode -> re-validate -> persist"

key-files:
  created: []
  modified: [src/sync/state.rs, src/sync/mod.rs, src/main.rs, tests/e2e_lifecycle.rs]

key-decisions:
  - "RED commits carry type-correct stubs that reproduce today's behaviour (load_from_dir returns Ok(old fallback), callers unwrap_or_default; stage_to_dir writes directly, commit is a no-op) so the RED tests fail on assertions, not compile errors"
  - "Post-push ancestor save failure in main.rs stays a warning (plan scoped only the load failure to exit 2)"
  - "SecretBlocked, BoundaryBlocked, the pre-API secret scan and the TypeSafe policy-shift exit keep exit 1 until CLI-04 (Phase 3), as planned"
  - "STATE_FILE_REL lives in state.rs (pub(crate)) and mod.rs imports it, so the ledger path is defined once"
  - "An existing state.json keeps its full permission bits (mode & 0o7777); a new one gets 0o644 regardless of umask"

patterns-established:
  - "Plan 01-06 stages state with stage_to_dir before replacing workspace files and commits it last"

requirements-completed: [SYNC-02]

coverage:
  - id: D1
    description: "Corrupt or newer state.json makes `ude sync status` and `ude sync pull` exit 2, name the file, and leave it byte-identical; the original state still gives exit 0"
    requirement: SYNC-02
    verification:
      - kind: e2e
        ref: "tests/e2e_lifecycle.rs#test_sync_corrupt_state_fails_loudly"
        status: pass
    human_judgment: false
  - id: D2
    description: "load_from_dir: corrupt -> Err 'corrupt', unreadable -> Err 'Failed to read state file', version 0 -> Err 'unsupported', version CURRENT+1 and 99 -> Err 'newer'; missing file and current version load"
    requirement: SYNC-02
    verification:
      - kind: unit
        ref: "src/sync/state.rs#sync::state::tests::test_load_corrupt_state_errors"
        status: pass
      - kind: unit
        ref: "src/sync/state.rs#sync::state::tests::test_load_unreadable_state_errors"
        status: pass
      - kind: unit
        ref: "src/sync/state.rs#sync::state::tests::test_load_future_version_errors"
        status: pass
      - kind: unit
        ref: "src/sync/state.rs#sync::state::tests::test_project_state_load_nonexistent_returns_default"
        status: pass
    human_judgment: false
  - id: D3
    description: "A staged but uncommitted state leaves state.json byte-identical with no .tmp* file; commit installs the new state"
    requirement: SYNC-02
    verification:
      - kind: unit
        ref: "src/sync/state.rs#sync::state::tests::test_state_save_atomic"
        status: pass
    human_judgment: false
  - id: D4
    description: "New state.json is 0o644 (not the 0o600 tempfile default); an existing 0o640 file keeps 0o640"
    requirement: SYNC-02
    verification:
      - kind: unit
        ref: "src/sync/state.rs#sync::state::tests::test_state_save_mode"
        status: pass
    human_judgment: false
  - id: D5
    description: "Symlinked .ude is refused by save_to_dir and stage_to_dir with nothing written outside; a state.json swapped for a symlink after staging is refused at commit and the outside file is untouched"
    requirement: SYNC-02
    verification:
      - kind: unit
        ref: "src/sync/state.rs#sync::state::tests::test_state_save_refuses_symlinked_ude_dir"
        status: pass
      - kind: other
        ref: "mutation check: removing the commit-time validate fails case B; removing the stage-time validate fails case A"
        status: pass
    human_judgment: false
  - id: D6
    description: "Sync dispatch exit codes: conflicts exit 1, state/remote-cache/I/O/remote-push failures exit 2"
    requirement: SYNC-02
    verification:
      - kind: e2e
        ref: "tests/e2e_lifecycle.rs#test_sync_corrupt_state_fails_loudly (status and pull state-error paths only)"
        status: pass
    human_judgment: true
    rationale: "Only the state-error paths of status and pull are exercised end to end. The remote-ensure, IoFailed, GitRemoteFailed, push_to_remote and ConflictBlocked exit mappings were checked by reading the code and by rg counts (7 x EXIT_HARD_FAILURE, 2 x EXIT_DRIFT), not by a test that asserts their exit code."

duration: 8min
completed: 2026-10-18
status: complete
---

# Phase 01 Plan 04: Fail-Loud State Ledger, Atomic state.json and Sync Exit Codes Summary

**A corrupt, unreadable or newer `.ude/state.json` now stops `ude sync` with exit 2 and an actionable message instead of being silently replaced by empty state. state.json is written through a validated temp file in `.ude/` and renamed into place, with mode 0o644 or the existing mode. The sync dispatch uses named exit codes: 1 for conflicts, 2 for hard failures.**

## Performance

- **Duration:** about 8 min
- **Started:** 2026-10-18T19:15:59Z
- **Completed:** 2026-10-18T19:23:48Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments
- `load_from_dir` returns `Result`. Errors: `Failed to read state file ...`, `State file ... is corrupt (...); refusing to reset it. Fix or remove the file, then re-run.`, `... has unsupported version 0`, `... has version N, written by a newer ude (this build supports up to 1); upgrade ude`. A missing file still loads as default.
- `scan_project` returns `Result` and propagates with `?`. `pull_updates` and `push_updates` turn a scan or load error into `IoFailed { path: .ude/state.json }`. Push's persist-immediately branch now fails when the save fails instead of discarding the result.
- `src/main.rs`: `EXIT_DRIFT` / `EXIT_HARD_FAILURE`. Status prints `Failed to read sync state: ...` and exits 2. Remote-ensure failure, pull/push `IoFailed`, `GitRemoteFailed`, `Failed to push to remote` and the post-push state load exit 2. Pull and push `ConflictBlocked` exit 1.
- `stage_to_dir` / `StagedState::commit`: validate `.ude/state.json`, create `.ude`, write a `NamedTempFile` in `.ude/`, set the mode, re-validate, `persist`. `save_to_dir` is stage + commit. No `fs::write` remains in production state code.

## Task Commits

1. **Task 1 (tracer) RED:** `047419a` test(01-04): add failing tests for corrupt and newer state.json
2. **Task 1 GREEN:** `1729528` feat(01-04): fail loudly on corrupt or newer state.json (D-06)
3. **Task 2 RED:** `7f28639` test(01-04): add failing tests for staged, atomic state.json writes
4. **Task 2 GREEN:** `59797eb` feat(01-04): stage state.json through a temp file and persist (D-05)

Tracer gate (interactive, `end-of-phase`, automated-only verify): all three Task 1 verify commands ran green on the committed tree before Task 2 started.

## TDD Gate Compliance

RED then GREEN commits are present for both tasks, in order. No refactor commits.

RED evidence (nextest JUnit, `gsd_run check tdd-red-evidence`). Every record returned `RED_EVIDENCE_OK` / `target_test_failed`:

| Target | Failed on | Semantic assessment |
|---|---|---|
| sync::state::tests::test_load_corrupt_state_errors | `expect_err`: got `Ok(ProjectState { version: 1, files: {} })` | intended reason (silent reset) |
| sync::state::tests::test_load_unreadable_state_errors | `expect_err`: got `Ok(default)` | intended reason (read error swallowed) |
| sync::state::tests::test_load_future_version_errors | `expect_err`: got `Ok(ProjectState { version: 2 })` | intended reason (no forward guard) |
| test_sync_corrupt_state_fails_loudly | first exit-code assertion: `exit Some(0)`, every file printed "In Sync" | intended reason |
| sync::state::tests::test_state_save_atomic | byte-identity assertion after drop: file already held the new state | intended reason (stub wrote the target directly) |
| sync::state::tests::test_state_save_refuses_symlinked_ude_dir | case A `expect_err`: `save_to_dir` returned `Ok(())` through the symlink | intended reason. Case B was not reached in RED; it is proven by the mutation check below |
| sync::state::tests::test_state_save_mode | `assert_ne!(mode, 0o600)`: mode was 0o600 | on the planned assertion, but environment-dependent: this host's umask is 0077, so `fs::write` produced 0o600. Under umask 022 this test would pass on the old code |

Classifier negative control: a record targeting the passing `test_project_state_persistence_roundtrip` returned `INVALID_RED` / `no_target_test_failure`.

Mutation negative controls on the GREEN code (each must turn the suite red, and did):
- Commit-time `validate_safe_write_target` removed: `test_state_save_refuses_symlinked_ude_dir` fails at case B ("commit replaced a symlinked state.json").
- Stage-time validation removed: the same test fails at case A ("stage_to_dir accepted a symlinked .ude").
- Explicit mode removed (forced 0o600): `test_state_save_mode` fails.

## Verification

| Check | Result |
|---|---|
| `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 before each commit |
| `cargo test` | 73 unit + 10 e2e pass (baseline before this plan: 67 + 9) |
| `cargo test --bin ude sync::state::tests` | 8 passed (plan requires at least 7) |
| Hermeticity gate from 01-03 (decoy `GIT_DIR`, `GIT_CONFIG_GLOBAL=/dev/null`, `GIT_CONFIG_NOSYSTEM=1`) | 46/46, exit 0 |
| Acceptance greps | load_from_dir signature, scan_project `Result`, both exit constants, 7 x `exit(EXIT_HARD_FAILURE)` (needs 6+), 2 x `exit(EXIT_DRIFT)` (needs 2+), `stage_to_dir` / `StagedState`, no `fs::write` before `mod tests {` (control: the RED commit has 1), 2 non-comment `validate_safe_write_target(` calls |

## Files Created/Modified
- `src/sync/state.rs` - version constant, fail-loud load with forward guard, staged atomic save, 6 new tests
- `src/sync/mod.rs` - `scan_project` returns `Result`; pull/push state-error mapping via `state_pull_failure` / `state_push_failure`; test call sites unwrap
- `src/main.rs` - exit-code constants and their use in the sync dispatch
- `tests/e2e_lifecycle.rs` - `test_sync_corrupt_state_fails_loudly`

## Decisions Made
- Added `test_load_unreadable_state_errors` (a directory in place of state.json, which fails as non-NotFound regardless of uid) and the `CURRENT_STATE_VERSION + 1` boundary in the future-version test. The plan's behaviour list only named version 99.
- Two small helpers, `state_pull_failure` and `state_push_failure`, build the `IoFailed { path: .ude/state.json }` outcomes so the four call sites do not repeat the struct literal.
- The e2e test removes inherited repo-local `GIT_*` variables from its `ude` spawns. That is cheap hermeticity, even though this path runs no git.

## Deviations from Plan

None in behaviour or scope. One process note:

**[Process] A clippy gate was piped through `tail` once**
- **Found during:** the Task 1 RED commit
- **Issue:** `cargo clippy ... | tail -2 && git add ... && git commit` gates on `tail`'s exit code, not clippy's. This is the 2026-10-09 incident pattern.
- **Impact:** none here. The output shows clippy finished with no diagnostics, and every later gate ran clippy unpiped.
- **Commit:** 047419a

## Issues Encountered
- **Stale build after the mutation checks:** restoring `state.rs` with `cp -p` reset its mtime to before the last mutation's build. Cargo then reused the mutant binary, and the first hermeticity-gate run failed `test_state_save_mode` (mode 0o600). The restored file's sha256 matched the GREEN copy. After `touch` and a rebuild the gate passed 46/46, and the full suite was re-run before the GREEN commit. Lesson: restore mutated files without `-p` (or touch them) so cargo sees the change.

## Known Stubs
None. Both RED stubs (Result-wrapped silent reset, direct-write `stage_to_dir`) were replaced in their GREEN commits.

## Threat Flags
None. T-01-10, T-01-11 and T-01-12 are mitigated as planned. No new surface.

## What the verification does NOT establish
- **Exit-code mapping beyond the state path:** only `status` and `pull` on a bad state file are asserted end to end. The other remapped exits (remote-ensure, `IoFailed`, `GitRemoteFailed`, `push_to_remote` Err, both `ConflictBlocked`) are checked by reading the code and by counting, not by tests (coverage D6, human_judgment).
- **`ude sync push` with a corrupt state.json** is not tested end to end. It goes through the same `scan_project` error path, mapped to `PushResult::IoFailed` -> exit 2.
- **Crash safety:** "atomic" means a rename in the same directory. Readers see the old or the new file. There is no fsync (D-01), so a power loss can still lose the new contents. The tests simulate a crash by dropping the staged state, not by killing a process mid-write.
- **TOCTOU:** the commit-time re-validation narrows the symlink race but does not close it. A swap between the check and `persist` is still possible.
- **Windows:** the staged write (no mode handling there) has never been compiled or run, and both the mode and symlink tests are Unix-only. Recorded in `.planning/WINDOWS.md` (entry 3).
- **Older binaries:** binaries built before this phase still reset on a corrupt file. That is the accepted limitation recorded with D-07.

## User Setup Required
None.

## Next Phase Readiness
- Plan 01-05 can raise `CURRENT_STATE_VERSION` to 2 and add migration. The forward guard and its tests use the constant, so they follow it.
- Plan 01-06 can stage state with `stage_to_dir` before replacing workspace files and commit it last.
- `ude sync push` still treats a failed post-push ancestor save as a warning (exit 0). That is outside this plan's scope.

## Self-Check: PASSED
- FOUND: src/sync/state.rs, src/sync/mod.rs, src/main.rs, tests/e2e_lifecycle.rs, 01-04-SUMMARY.md
- FOUND commits on this branch: 047419a, 1729528, 7f28639, 59797eb, and the SUMMARY commit 8451db9

---
*Phase: 01-sync-engine-state-hermeticity*
*Completed: 2026-10-18*
