---
phase: 01-sync-engine-state-hermeticity
plan: 09
subsystem: sync
tags: [rust, sync, conflicts, sidecar, diff3, state-json, gitignore]

requires:
  - phase: 01-06
    provides: StagedTransaction (stage_write / stage_delete / commit), file_mode_for, state staged before and committed after files
  - phase: 01-07
    provides: PullReport, deletion statuses, tombstones, PHASE SYNC STATE TABLE rows 1-16
  - phase: 01-08
    provides: render_conflict_sidecar and ConflictSide (pure renderer)
provides:
  - ConflictRecord and the persisted ProjectState.conflicts map (record_conflict / conflict / clear_conflict)
  - SyncStatus::ConflictPending, FileSyncState.conflict_open and FileSyncState.local_content
  - pull writes a `<file>.conflict` sidecar per conflicted file in the same staged transaction as the clean files
  - PullReport.conflicts and PullReport.converged; PullResult::ConflictBlocked removed
  - convergence auto-clear (state table rows 13-16) and open-record precedence (P1, row 12)
  - CLI pull exits 1 on conflicts; status shows "Unresolved conflict"
  - "*.conflict" rule in templates/projects/generic/gitignore.repo (D-11)
affects: [01-10 push skip reporting and ude sync resolve, 01-11]

actuals:
  tokens: 14935
  tasks: 3
  commits: 6
plan_head_before: 0e730d2ebd86a305fc775a5f5aa1ba5d02c0c305
plan_head_after: d82893beb49142a0abfab7a0ff755464b929083f

tech-stack:
  added: []
  patterns:
    - "Sidecar writes and deletes go through StagedTransaction like any other project file; sidecars are filtered out of report.applied"
    - "An open conflict record overrides the record-less status until local and upstream fingerprints are equal or both absent; conflict_open stays true after convergence so pull knows to clear it"

key-files:
  created: []
  modified:
    - src/sync/state.rs
    - src/sync/mod.rs
    - src/main.rs
    - templates/projects/generic/gitignore.repo
    - tests/e2e_lifecycle.rs

key-decisions:
  - "01-09: pull writes <file>.conflict sidecars in the same staged transaction as clean files (mode = local mode & 0o666) and records ProjectState.conflicts (upstream fingerprint + LF content, None when upstream deleted); PullResult::ConflictBlocked is gone and pull exits 1 when any conflict is reported"
  - "01-09: an open conflict record reports ConflictPending until local and upstream fingerprints are equal or both absent; pull then clears the record and sidecar and, when both are present, records upstream as the ancestor (rows 13-16); re-pulls regenerate the sidecar and overwrite detected_at"

patterns-established:
  - "Conflict sidecar path: append `.conflict` to the full relative path (sidecar_rel)"
  - "Converged = conflict_open && status != ConflictPending"

requirements-completed: [SYNC-05]

coverage:
  - id: D1
    description: "A mixed pull applies clean files, writes one sidecar per conflicted file, records the detected upstream version, leaves local files and ancestors untouched, and lists every conflict; a clean pull leaves no sidecar"
    requirement: SYNC-05
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_pull_multiple_conflicts_sidecars"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_pull_upstream_deletion (D-13 control)"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_pull_keeps_local_deletion (D-15 control)"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_both_deleted (empty-file control)"
        status: pass
      - kind: unit
        ref: "src/sync/state.rs#sync::state::tests::test_conflict_record_roundtrip"
        status: pass
    human_judgment: false
  - id: D2
    description: "Open conflicts stay ConflictPending (including over UpstreamDeleted), sidecars regenerate against the latest upstream, hand-deleting a sidecar does not resolve, and converged conflicts (equal content, or both absent with and without a files record) are cleared"
    requirement: SYNC-05
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_pull_conflict_regenerates_sidecar"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_conflict_auto_clears_on_convergence"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_conflict_both_absent_converges"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_pull_deletion_conflicts_write_sidecars"
        status: pass
    human_judgment: false
  - id: D3
    description: "Sidecar safety: deletion and binary variants render correctly, a symlinked sidecar path is refused with no partial apply, and a sidecar is never wider than its local file"
    requirement: SYNC-05
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_pull_binary_conflict_sidecar"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_pull_refuses_symlinked_conflict_sidecar"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_sidecar_mode_follows_local"
        status: pass
    human_judgment: false
  - id: D4
    description: "CLI: a conflicted pull exits 1, writes mise.toml.conflict, keeps local bytes, points at `ude sync resolve`, removes the unchanged .gitconfig (D-12); status prints Unresolved conflict; a clean pull exits 0 with no sidecar"
    requirement: SYNC-05
    verification:
      - kind: e2e
        ref: "tests/e2e_lifecycle.rs#test_sync_pull_conflict_exits_1_and_writes_sidecar"
        status: pass
    human_judgment: false
  - id: D5
    description: "The managed gitignore template ignores *.conflict (D-11) and does not ignore the managed files themselves"
    requirement: SYNC-05
    verification:
      - kind: e2e
        ref: "tests/e2e_lifecycle.rs#test_sync_pull_conflict_exits_1_and_writes_sidecar (git check-ignore section)"
        status: pass
      - kind: other
        ref: "bash -c 'rg -qx \"\\*\\.conflict\" templates/projects/generic/gitignore.repo && ! rg -q \"\\{\\{\" templates/projects/generic/gitignore.repo'"
        status: pass
    human_judgment: false

duration: 11min
completed: 2026-10-19
status: complete
---

# Phase 01 Plan 09: Conflict Sidecars, Pending Conflicts and Convergence Summary

**A conflicted pull no longer aborts the whole update. It applies the clean files, writes a `<file>.conflict` diff3 sidecar beside each conflicted file through the staged transaction (at the local file's mode minus execute bits), records the upstream version each conflict was detected against, and exits 1 listing every conflict. Open conflicts stay `ConflictPending`, and their sidecars are regenerated on every pull until the user runs `resolve` or local and upstream converge. Converged conflicts are cleared automatically. The managed gitignore now ignores `*.conflict`.**

## Performance

- **Duration:** 11 min
- **Started:** 2026-10-19T11:31:45Z
- **Completed:** 2026-10-19T11:42:45Z
- **Tasks:** 3 (all TDD: RED then GREEN, no refactor commits)
- **Files modified:** 5

## Accomplishments
- `ProjectState.conflicts` (additive under schema v2, omitted from JSON when empty) holds a `ConflictRecord { upstream_hash, upstream_content, detected_at }` per open conflict. It also covers paths that have no files record.
- `pull_updates`: the first-conflict gate is gone. Conflicted and ConflictPending files get a sidecar rendered by `render_conflict_sidecar`. The sidecar's sides are text, fingerprint or absent. It is staged with `stage_write` in the same transaction as the clean writes and deletions, with state committed last. Sidecars are left out of `report.applied`.
- `scan_project` sets `conflict_open` from the conflict map. An unconverged open record reports `ConflictPending` over every record-less row (P1, row 12). That covers the case where the user restored the ancestor and upstream deleted the file: without the record that would be UpstreamDeleted and the file would be deleted.
- Convergence (D-09 amendment, rows 13-16): the record is cleared and the sidecar removed through `stage_delete`, and the path is listed in `report.converged`. When both sides are present, upstream becomes the ancestor. When both are absent, the row 14 BothDeleted path runs (tombstone with the recorded ancestor) or row 16 applies (nothing recorded). The early return counts conflicts and convergence clears as work.
- CLI: the pull prints `<file>: conflicting local and upstream changes; wrote <file>.conflict` and `resolve with: ude sync resolve <file>` to stderr and exits 1, and prints `conflict resolved by convergence`. Status prints `Unresolved conflict (see <file>.conflict; run ude sync resolve <file>)`.
- `templates/projects/generic/gitignore.repo` has a `# UDE sync conflict sidecars (...)` section with `*.conflict`.

## Task Commits

1. **Task 1 (tracer) RED:** `51ec4cb` test(01-09): add failing tests for conflict sidecars on pull
2. **Task 1 GREEN:** `6baca06` feat(01-09): write .conflict sidecars and keep applying clean files on pull
3. **Task 2 RED:** `bc84da2` test(01-09): add failing tests for pending, regenerated and converged conflicts
4. **Task 2 GREEN:** `a95eef4` feat(01-09): keep open conflicts pending and clear them on convergence
5. **Task 3 RED:** `9ecfa53` test(01-09): add failing e2e test for the CLI conflict flow and D-11 rule
6. **Task 3 GREEN:** `d82893b` feat(01-09): ignore .conflict sidecars in the managed gitignore (D-11)

Tracer gate (interactive, `end-of-phase`, automated-only verify): Task 1's verify was re-run on the committed tree at `6baca06` (4 passed, clean tree) before Task 2 started.

## TDD Gate Compliance

Every task has a `test(01-09)` commit before its `feat(01-09)` commit. No refactor commits.

RED evidence came from nextest JUnit reports (`target/nextest/default/junit.xml`), classified with `gsd_run check tdd-red-evidence`. Negative control on the classifier: a green run of `test_both_deleted` was classified `INVALID_RED / unexpected_green`. The classifier's exit code is 0 for both verdicts, so the verdict field was read, not the exit code.

| Target | Verdict | Failed on | Semantic assessment |
|---|---|---|---|
| test_pull_multiple_conflicts_sidecars | RED_EVIDENCE_OK | `ConflictBlocked { .gitconfig }`, expected `Success` with two conflicts and the hook applied | intended (D-08 all-or-nothing behaviour) |
| test_pull_upstream_deletion | RED_EVIDENCE_OK | `ConflictBlocked`, expected `conflicts: [.gitconfig]` | intended (migrated D-13 control) |
| test_pull_keeps_local_deletion | RED_EVIDENCE_OK | same | intended (migrated D-15 control) |
| test_both_deleted | RED_EVIDENCE_OK | same | intended (migrated empty-file control) |
| test_pull_conflict_regenerates_sidecar | RED_EVIDENCE_OK | status `Conflicted`, expected `ConflictPending` | intended |
| test_conflict_auto_clears_on_convergence | RED_EVIDENCE_OK | `(Conflicted, false)`, expected `(ConflictPending, true)` | intended |
| test_conflict_both_absent_converges | RED_EVIDENCE_OK | `Conflicted`, expected `ConflictPending` | intended |
| test_pull_deletion_conflicts_write_sidecars | RED_EVIDENCE_OK | open-record case `UpstreamDeleted`, expected `ConflictPending` (the D-13 and D-15 parts passed before it) | intended (P1) |
| test_sync_pull_conflict_exits_1_and_writes_sidecar | RED_EVIDENCE_OK | `gitignore.repo lacks a *.conflict line`; every CLI assertion before it passed | intended (D-11). It also shows that Tasks 1-2 left no CLI wiring gap |

The RED commits carry scaffolding so the targets fail on assertions, not on compile errors. Each commit message lists it: `ConflictRecord` and its accessors plus `PullReport.conflicts` (Task 1), and `ConflictPending`, `conflict_open`, `PullReport.converged` and the status arm (Task 2).

**Characterization tests (not RED):** `test_pull_binary_conflict_sidecar`, `test_pull_refuses_symlinked_conflict_sidecar` and `test_sidecar_mode_follows_local` already passed at Task 2 RED (`INVALID_RED / no_target_test_failure`), because Task 1 built that behaviour, as the plan expected. Hand mutants show they detect breakage (below). The e2e CLI assertions were also never seen failing against old code; a hand mutant covers the exit-1 assertion.

## Verification

| Check | Result | What it does not establish |
|---|---|---|
| Task 1 verify (4 named tests) | 4 passed, each filter has an `ok` line; re-run at the tracer gate | — |
| ConflictBlocked removal check (plan command) | exit 0. Control: the RED commit's `PullResult` enum still matches `ConflictBlocked` (count 1) | — |
| Task 2 verify (7 named tests) | 7 passed | — |
| Task 3 e2e target | 1 passed | Convergence and its CLI line are engine-tested only, not run through the binary |
| Template check (plan command) | exit 0. The e2e also runs `git check-ignore`: `.gitconfig.conflict` and `scripts/hooks/pre-commit.conflict` ignored, `.gitconfig` and `mise.toml` not ignored | — |
| Hermeticity gate (decoy `GIT_DIR`, `/dev/null` global config) | exit 0, 96 `sync::` tests | — |
| `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test` | exit 0 before each GREEN commit; RED commits passed fmt and clippy. Final: 123 unit + 12 e2e (before this plan: 115 + 11) | — |
| Hand mutants, src/sync/mod.rs | 13/13 caught (each failed on an assertion, none was a compile error): mask keeps exec bits, mode ignores local, binary shown as absent, sidecar staging error ignored, open record always pending, both-absent not converged, sidecar kept on convergence, no ancestor on convergence, early return ignores converged, early return ignores conflicts, sidecars reported as applied, pending not regenerated, conflict not recorded. The source was restored byte-identical (`cmp`), touched, and the `sync::` tests re-run green (96) | No cargo-mutants run; mutants were picked by hand. One equivalent mutant was skipped: the InSync reconcile loop's new `!conflict_open` guard writes the same record convergence writes |
| Hand mutant, src/main.rs (pull no longer exits 1) | caught by the e2e ("a conflicted pull must exit 1"); restored byte-identical and rebuilt | — |
| Acceptance greps | `ConflictRecord` / `conflicts` map: 1 each; `render_conflict_sidecar` in pull_updates; `ConflictPending` in the enum, scan_project and pull_updates; `SyncStatus::ConflictPending` in main.rs: 1; `pub converged` / `pub conflict_open`: 1 each; `*.conflict` line: 1 | — |

## What the verification does NOT establish
- **Push and pending conflicts:** `push_updates` still blocks only on `Conflicted`. A `ConflictPending` file is not LocallyModified, so it is never published. But push exits 0 and does not list it. D-10 skip reporting is plan 01-10's job.
- **Rename-phase failure:** if the commit fails after a sidecar is renamed into place but before `state.json` is committed, a sidecar can exist with no record. If the file still conflicts, the next pull regenerates both. If it does not, the stale sidecar stays. This is the accepted D-01 class; it is not tested.
- **A non-regular `.conflict` path at convergence:** convergence only deletes a sidecar that is a regular file. A symlink or directory at that path is left alone (neither followed nor removed). Not tested.
- **Scan-to-commit race:** a local edit made between the scan and the commit is not detected. This is the pre-existing class. Not tested.
- **Windows:** not compiled or run. Mode handling is `cfg(unix)`.

## Files Created/Modified
- `src/sync/state.rs` - `ConflictRecord`, the `conflicts` map (serde default, omitted when empty), `record_conflict` / `conflict` / `clear_conflict`; `test_conflict_record_roundtrip`
- `src/sync/mod.rs` - `ConflictPending`; `FileSyncState.local_content` / `conflict_open`; `PullReport.conflicts` / `converged`; `ConflictBlocked` removed from `PullResult`; `SIDECAR_EXEC_MASK`, `sidecar_rel`, `conflict_side`; sidecar staging, conflict recording and convergence clearing in `pull_updates`; open-record classification in `scan_project`; 8 new tests plus 3 migrated controls and the helpers `write_upstream`, `conflict_files`, `assert_text_sidecar`, `scan_state`, `conflicts_only`, `conflicted_gitconfig`, `upstream_gitconfig_path`
- `src/main.rs` - pull conflict and convergence lines, exit 1 on conflicts, ConflictPending status arm; the "No pending upstream updates" line is suppressed when conflicts or convergences were reported
- `templates/projects/generic/gitignore.repo` - `*.conflict` section (D-11)
- `tests/e2e_lifecycle.rs` - `test_sync_pull_conflict_exits_1_and_writes_sidecar` plus the helpers `init_rust_project`, `seed_mise_remote`, `conflict_sidecars`

## Decisions Made
- **Sidecar mode** is `file_mode_for(rel, local) & 0o666` in every case. An absent local file gives REGULAR_MODE or EXECUTABLE_MODE, and both mask to 0o644 (REGULAR_MODE), so this matches the plan's "REGULAR_MODE when absent" without a separate branch. Special bits (setuid and similar) are cleared too.
- **`detected_at`** is overwritten every time a pull re-records a pending conflict. It is the latest detection time, not the first.
- **The InSync reconcile loop skips open-record files**, so convergence is the only place their ancestor is written.
- **An ancestor recorded without content** (for example a migrated v1 hash-only record) is shown in the sidecar as a fingerprint, as the plan specified, even if the file was text.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Test strength] Persistence test for the new state map**
- **Found during:** Task 1 GREEN
- **Issue:** no planned test covered serde for `conflicts`: an empty map omitted, an old state file without the key, a `None` upstream round-trip.
- **Fix:** `sync::state::tests::test_conflict_record_roundtrip`, with a negative control (an open conflict does serialize the key).
- **Files modified:** src/sync/state.rs
- **Committed in:** 6baca06

**2. [Rule 2 - Test strength] D-11 checked by behaviour, not only by grep**
- **Found during:** Task 3 RED
- **Fix:** the e2e runs `git check-ignore --no-index` against the template, with must-not-ignore controls (`.gitconfig`, `mise.toml`).
- **Committed in:** 9ecfa53

**3. [Rule 2 - Test strength] Extra controls in the planned tests**
- A positive control for each `.conflict` walker (a planted sidecar must be found). The convergence test converges with a CRLF local file, which shows convergence compares fingerprints and not raw bytes. The symlink test also checks that `state.json` is unchanged and that the symlink is still a symlink. The mode test checks that the hook's own mode stays 0o755.

---

**Total deviations:** 3 auto-fixed (all Rule 2, test strength). **Impact on plan:** tests only. No production scope change. Task 3 needed no src/main.rs fix (no CLI gap).

## Issues Encountered
- The first hand-mutant script aborted on a pattern that rustfmt had reflowed, with mutant M11 still applied to `src/sync/mod.rs`. The original bytes were restored at once (`cmp` identical) before anything else ran. The script was then rewritten to check every pattern up front and restore in a `finally`, and all 13 were re-run. Nothing was committed while a mutant was applied.

## Known Stubs
None.

## Threat Flags
None. T-01-24 (sidecar mode and `*.conflict` ignore), T-01-25 (staged, symlink-refused sidecar writes; deletes through `stage_delete`) and T-01-26 (conflicted local files and ancestors never written) are mitigated and tested. No new trust-boundary surface beyond the plan's threat model.

## User Setup Required
None.

## Next Phase Readiness
- Plan 01-10 can read `ProjectState::conflict(rel)` (recorded `upstream_hash` / `upstream_content`) to implement `ude sync resolve` (R1-R4), delete the sidecar with `sidecar_rel` and `stage_delete`, and add push skip reporting for `ConflictPending` (D-10).
- SYNC-05 (structured `.conflict` files with markers, local changes preserved) is delivered.

---
*Phase: 01-sync-engine-state-hermeticity*
*Completed: 2026-10-19*
