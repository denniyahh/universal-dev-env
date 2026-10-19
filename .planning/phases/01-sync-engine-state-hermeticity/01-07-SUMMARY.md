---
phase: 01-sync-engine-state-hermeticity
plan: 07
subsystem: sync
tags: [rust, sync, deletion, tombstone, git, publication-gate, tdd, cargo-mutants]

requires:
  - phase: 01-05
    provides: fingerprint classifier (compare_3way_hashes / classify_file), state v2
  - phase: 01-06
    provides: StagedTransaction (stage_write / commit), staged state.json committed last
provides:
  - SyncStatus::{LocalDeleted, UpstreamDeleted, BothDeleted, Unmanaged, Absent}
  - compare_3way_hashes implementing the C-less rows 1-11 of the PHASE SYNC STATE TABLE (tombstone flag added)
  - Tombstone, ProjectState.tombstones, record_tombstone / tombstone / clear_tombstone; record_file clears the tombstone (I1)
  - StagedTransaction::stage_delete (regular files only, removed after all writes)
  - PullReport {applied, deleted, locally_deleted_pending}; PullResult::Success(PullReport)
  - PushResult::Success.deleted; push deletes upstream templates of LocalDeleted files (D-14)
  - name-status --no-renames publication gates (staged and committed) that accept deletions and still scan every non-D blob
affects: [01-09, 01-10, 01-11]

actuals:
  tokens: 17305
  tasks: 3
  commits: 6
plan_head_before: d2cb25fb111fd843ef796dede5582cf2d1b8e8fd
plan_head_after: 7e56044fe526f9245393be77f8abb504d89a7828

tech-stack:
  added: []
  patterns:
    - "Absence is Option::None end to end; only NotFound / NotADirectory reads count as absent"
    - "Deletions are staged in the same StagedTransaction as writes and applied after them; state.json still last"
    - "Exhaustive enumeration test against a row-by-row oracle of the state table"

key-files:
  created: []
  modified:
    - src/sync/mod.rs
    - src/sync/state.rs
    - src/sync/staging.rs
    - src/main.rs

key-decisions:
  - "scan_project fails loudly on any local or upstream read error other than NotFound / NotADirectory, so an unreadable file is never classified as deleted (deletions now propagate)"
  - "A local deletion of a publish-boundary (local-only) path is BoundaryBlocked, the same as a modification of one"
  - "A tombstone is consulted only when the path has no files record (invariant I1 makes the combination unreachable through the API)"

patterns-established:
  - "Pull and push report deletions per path (PullReport.deleted / locally_deleted_pending, PushResult::Success.deleted)"
  - "Publication gates parse status<TAB>path and fail closed on an unparseable line"

requirements-completed: [SYNC-01]

coverage:
  - id: D1
    description: "Deletion-aware classifier: every C-less state-table row (1-11), empty file is present, overrides only affect present-local rows"
    requirement: SYNC-01
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_compare_3way_hashes_exhaustive (256 cases)"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_sync_status_deletions"
        status: pass
      - kind: other
        ref: "cargo mutants --file src/sync/mod.rs --re 'compare_3way_hashes|classify_file' (25 mutants: 21 caught, 4 unviable, 0 missed)"
        status: pass
    human_judgment: false
  - id: D2
    description: "Pull deletes an unchanged local copy of an upstream-deleted file, drops its record, writes a tombstone and reports it (D-12); an edited copy blocks the pull untouched (D-13)"
    requirement: SYNC-01
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_pull_upstream_deletion"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_unreadable_upstream_is_not_a_deletion"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_stage_delete_only_removes_regular_files_after_writes"
        status: pass
    human_judgment: false
  - id: D3
    description: "Pull keeps a local deletion and reports it as pending (D-15); a double deletion drops tracking with a tombstone and never creates a 0-byte file (D-16); Unmanaged and tombstone clearing"
    requirement: SYNC-01
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_pull_keeps_local_deletion"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_both_deleted"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_unmanaged_and_tombstone_clear"
        status: pass
    human_judgment: false
  - id: D4
    description: "Push deletes the upstream template of a locally deleted file and drops its record in persist-immediately mode (D-14); boundary paths are blocked"
    requirement: SYNC-01
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_push_local_deletion"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_push_local_deletion_respects_publish_boundary"
        status: pass
    human_judgment: false
  - id: D5
    description: "Publication gates (staged index and committed tree) accept deletions via --name-status --no-renames, allowlist every entry, scan every non-deleted blob, fail closed on malformed lines"
    requirement: SYNC-01
    verification:
      - kind: integration
        ref: "src/sync/mod.rs#sync::tests::test_push_to_remote_deletion (GitFixture bare remote)"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_parse_name_status_fails_closed"
        status: pass
      - kind: integration
        ref: "existing gate tests: test_publication_gate_scans_staged_blob_rejecting_clean_filter_secrets, test_post_commit_verification_catches_hook_injected_secrets_and_rolls_back, test_defect2_publication_gate_allowlist_staging_blocks_unvalidated_files"
        status: pass
    human_judgment: false
  - id: D6
    description: "A path deleted upstream and recreated locally is never republished (success criterion 1)"
    requirement: SYNC-01
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_tombstone_prevents_republish"
        status: pass
    human_judgment: false
  - id: D7
    description: "CLI: direct remote push drops records of deleted files only after a successful publish; status/pull/push print the new lines"
    verification:
      - kind: manual_procedural
        ref: "smoke: ude init + rm .gitconfig + ude sync status / ude sync pull in a scratch project (status and pull lines seen, pull exit 0)"
        status: pass
    human_judgment: true
    rationale: "The main.rs record update after a successful direct remote push and the push print line are not exercised by any test (no e2e test deletes a file); only compilation and code reading cover them. Status and pull lines were smoke-tested by hand."

duration: 16min
completed: 2026-10-19
status: complete
---

# Phase 01 Plan 07: Deletion Lifecycle Summary

**Deletions now propagate both ways: pull removes unchanged copies of upstream-deleted files and tombstones them, pull keeps local deletions pending, push deletes upstream templates through a name-status gate that still scans every published blob, and a tombstoned path is never republished.**

## Performance

- **Duration:** 16 min
- **Started:** 2026-10-19T10:56:52Z
- **Completed:** 2026-10-19T11:13:16Z
- **Tasks:** 3
- **Files modified:** 4

## Accomplishments
- `compare_3way_hashes` implements state-table rows 1-11 (tombstone flag added). A file that exists nowhere is `Absent`, never `In Sync`. An empty file counts as present.
- Pull: an `UpstreamDeleted` file is deleted through `stage_delete` in the same transaction as the writes, its record is dropped and a tombstone is written (D-12). `BothDeleted` gets the same record handling with no file operation (D-16). `LocalDeleted` is reported as pending and not recreated (D-15). The no-op early return now returns the report built so far.
- Push: the template of a `LocalDeleted` file is staged for deletion, and the removal is listed in `staged_files` and `deleted` (D-14). Both publication gates parse `--name-status --no-renames`, fail closed on an unparseable line, allowlist every entry, and scan every entry that is not a deletion.
- `record_file` clears the path's tombstone (I1), so pull writes, reconciles and push records all clear it with no extra code.

## Task Commits

1. **Task 1 (tracer) RED:** `9aca425` test(01-07): add failing tests for deletion statuses and upstream delete
2. **Task 1 GREEN:** `59ab808` feat(01-07): propagate upstream deletions on pull with tombstones
3. **Task 2 RED:** `9e5868e` test(01-07): add failing tests for local, double and tombstoned deletions
4. **Task 2 GREEN:** `65da37e` feat(01-07): keep local deletions, drop double deletions, mark Unmanaged
5. **Task 3 RED:** `ed8117e` test(01-07): add failing tests for pushing local deletions upstream
6. **Task 3 GREEN:** `7e56044` feat(01-07): push local deletions through a name-status publication gate

Tracer gate (interactive, `end-of-phase`, verify is automated only): Task 1's verify was re-run on the committed tree at `59ab808` (2 passed) before Task 2 started.

## TDD Gate Compliance

Every task has a `test` commit before its `feat` commit. No refactor commits.

RED evidence: nextest JUnit reports (`target/nextest/default/junit.xml`), classified by `gsd_run check tdd-red-evidence`. All 8 targets returned `RED_EVIDENCE_OK` / `target_test_failed`.

| Target | Failed on | Semantic assessment |
|---|---|---|
| test_sync_status_deletions | `(base, None, base)` gave `LocallyModified`, expected `LocalDeleted` | intended |
| test_pull_upstream_deletion | `IoFailed` reading the missing gitconfig.repo (ENOENT), expected `Success` with `deleted: [.gitconfig]` | intended: the remediation 1.1 ENOENT defect |
| test_pull_keeps_local_deletion | `locally_deleted_pending` empty | intended |
| test_both_deleted | `deleted` empty, record kept | intended |
| test_unmanaged_and_tombstone_clear | scan gave `LocallyModified`, expected `Unmanaged` | intended |
| test_compare_3way_hashes_exhaustive | `(None, a, None, tombstone)` gave `LocallyModified`, expected `Unmanaged`; the oracle's 3 hand-row controls passed first | intended |
| test_push_local_deletion | `deleted` empty, template kept | intended |
| test_push_to_remote_deletion | `Publication gate violation: failed to inspect staged git blob` (`git show` on a deleted path) | intended: research Pitfall 3 |

To make the targets fail on assertions and not on compile errors, the RED commits carry scaffolding: the new types and fields, a classifier parameter that is ignored, and CLI arms. Their commit messages list it.

**Characterization test (not RED):** `test_tombstone_prevents_republish` already passed at Task 3 RED, because Task 2 built `Unmanaged`. A hand mutant (scan ignores the tombstone) makes it fail.

## Verification

| Check | Result |
|---|---|
| Task 1 verify (2 named tests) | 2 passed (each filter has an `ok` line) |
| Task 2 verify (4 named tests) | 4 passed |
| Task 3 verify (3 new tests + 3 existing gate tests) | 3 + 3 passed |
| Mutation gate, `compare_3way_hashes` + `classify_file` | `--list`: 25 mutants. Run: 21 caught, 4 unviable (`Default::default()` for `SyncStatus`, which has no `Default`), 0 missed, 0 timeout. Wall time 102 s (45 s baseline build). Plan check command exit 0: baseline ran 69 tests, all `sync::`, no timing test; caught.txt (21) = outcomes.json `caught` (21). No `.cargo/mutants.toml` exclusion. Negative controls on the check: exit file `2` fails it, and a baseline line `test_remote_lock_x` fails it |
| Hand mutants (paths cargo-mutants was not scoped to) | 11/11 caught. Task 1: read errors treated as absent, `stage_delete` accepting non-files, commit not re-validating a deletion. Task 3: staged / committed gate never skipping `D`, staged / committed gate skipping every entry, scan ignoring the tombstone, push ignoring `LocalDeleted`, push keeping deleted records, deletion skipping the boundary check. Sources restored byte-identical (`cmp`) and `touch`ed |
| Acceptance greps | 5 variants in the enum; 5 `SyncStatus::` arms in main.rs; both D-15 strings present; `--name-status` and `--no-renames` 3 each; no `--name-only` before `mod tests` (control: the previous HEAD has 2) |
| Hermeticity gate (decoy `GIT_DIR`, `/dev/null` global config) | exit 0, 78 `sync::` tests |
| `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test` | exit 0 before every commit. Final: 105 unit + 11 e2e (before this plan: 93 + 11) |
| CLI smoke (scratch project, local templates, `.gitconfig` deleted) | status: `.gitconfig: Deleted locally (push to propagate)`. pull: `.gitconfig: deleted locally — push to propagate`, exit 0, file not recreated, repo `templates/` untouched. Push was not run, because local-templates mode would edit this repo's `templates/` |

## What the verification does NOT establish
- **Recording after a direct remote push** (`src/main.rs`: drop deleted records after a successful publish; PR mode leaves them) is covered only by compiling and reading. No test runs `ude sync push --remote` with a deletion.
- **Scan-to-commit race:** if the local file is edited between the scan and the deletion commit, the edit is deleted. Writes have the same window (pre-existing class). Not tested.
- **Unusual paths in git output:** git quotes non-ASCII and special-character paths by default (`core.quotePath`), so the gate rejects them as unvalidated (fails closed). All managed paths are ASCII. The fail-closed parse is unit-tested, but no integration test sends a malformed line through the gate.
- **The mutation gate covers only the classifier.** Pull and push deletion handling is covered by the named tests plus 11 hand mutants, not by a cargo-mutants run.
- **Windows:** not compiled or run.
- **C-open rows 12-16 and resolve R1-R4:** not implemented here (plans 01-09 / 01-10). The exhaustive oracle covers only rows 1-11.

## Files Created/Modified
- `src/sync/mod.rs`: new statuses; classifier rows 1-11 with a tombstone flag; `read_if_present`; `PullReport`; pull deletion handling and the early-return change; push `LocalDeleted` handling; `parse_name_status`; both gates on name-status; 13 new tests and the fixtures `upstream_deletion_fixture`, `deletion_fixture`, `pull_dirs`, `push_dirs`, `applied_only`, `state_table_oracle`
- `src/sync/state.rs`: `Tombstone`, `tombstones` map (additive under v2), `record_tombstone` / `tombstone` / `clear_tombstone`; `record_file` clears the tombstone
- `src/sync/staging.rs`: `stage_delete`; `commit` removes staged deletions after all writes
- `src/main.rs`: status arms for the 5 statuses; pull prints removed and pending files; push prints deleted templates and drops their records after a successful direct push

## Decisions Made
- **Unreadable is not absent.** Now that absence drives deletions, a template that cannot be read (for example a directory in its place, or no permission) would otherwise delete the user's local file on pull. So would an unreadable local file deleting the upstream template on push. Only NotFound and NotADirectory count as absent. Any other read error fails the scan, which surfaces as `IoFailed` on `.ude/state.json` (the existing scan-error mapping) with the file named in the reason.
- **Publish boundary applies to deletions.** A local deletion of a local-only path (for example `scratch/*`) is `BoundaryBlocked`, the same as a modification of one. The upstream template stays.
- **Tombstone with a files record:** the classifier ignores the tombstone when an ancestor exists. I1 keeps this combination unreachable through `record_file`.
- **Pull message:** "No pending upstream updates to apply." prints only when nothing was applied and nothing was removed.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing critical] Unreadable files were classified as absent**
- **Found during:** Task 1
- **Issue:** `scan_project` used `fs::read(..).ok()`, so any read error looked like a deletion. Once deletions propagate, that becomes a destructive outcome.
- **Fix:** `read_if_present` treats only NotFound and NotADirectory as absent. Other errors fail the scan.
- **Files modified:** src/sync/mod.rs
- **Verification:** `test_unreadable_upstream_is_not_a_deletion`, with a negative control (missing template gives `UpstreamDeleted`), plus a hand mutant
- **Committed in:** 59ab808

**2. [Rule 2 - Missing critical] Deletions bypassed the publish boundary**
- **Found during:** Task 3
- **Issue:** the plan's push handling had no boundary check for deletions, so deleting `scratch/*` locally would delete it upstream although that path family is local-only.
- **Fix:** the same `is_local_only_path` gate as modifications, giving `BoundaryBlocked`.
- **Files modified:** src/sync/mod.rs
- **Verification:** `test_push_local_deletion_respects_publish_boundary` plus a hand mutant
- **Committed in:** 7e56044

**3. [Rule 2 - Missing critical] Tests for the T-01-21 and T-01-20 mitigations**
- **Found during:** Tasks 1 and 3
- **Issue:** no planned test covered `stage_delete` refusing directories, symlinks, missing or escaping paths, or a symlink swapped in before commit. No planned test covered the gate failing closed on a line without a tab.
- **Fix:** `test_stage_delete_only_removes_regular_files_after_writes` and `test_parse_name_status_fails_closed`
- **Committed in:** 59ab808, 7e56044

**4. [Ordering] `ProjectState::tombstone` and `PullReport.locally_deleted_pending` were added in Task 2, not Task 1**
- Task 1 had no reader for either. Both exist now with the planned shapes.

**5. [Test migration] `PullResult::Success { applied_files: N }` assertions**
- Replaced by `applied_only(result) == N`. The helper also asserts that `deleted` and `locally_deleted_pending` are empty, so the old whole-value strength is kept.

---

**Total deviations:** 3 auto-fixed (all Rule 2), 2 minor ordering or test-migration notes.
**Impact on plan:** the two behaviour additions (fail on unreadable, boundary for deletions) only make deletion handling fail closed in cases the plan did not cover. Neither changes a planned outcome.

## Issues Encountered
None.

## Known Stubs
None. The RED-commit scaffolding (the ignored classifier parameter, the always-empty `PushResult::Success.deleted`, and `#[allow(dead_code)]` on the new variants) was removed in each GREEN commit.

## User Setup Required
None.

## Next Phase Readiness
- Plan 01-09 can replace pull's first-conflict gate with sidecars against the same state table. `test_pull_upstream_deletion`, `test_pull_keeps_local_deletion` and `test_both_deleted` assert `ConflictBlocked` in their negative controls; 01-09 migrates them.
- Plan 01-10 can rely on `PushResult::Success.deleted`, `remove_file` and the tombstone accessors for resolve outcomes R2-R4.

---
*Phase: 01-sync-engine-state-hermeticity*
*Completed: 2026-10-19*

## Self-Check: PASSED

- Files: src/sync/mod.rs, src/sync/state.rs, src/sync/staging.rs, src/main.rs and this SUMMARY exist.
- Commits: 9aca425, 59ab808, 9e5868e, 65da37e, ed8117e, 7e56044, 095e427 are ancestors of HEAD; `check evaluation-scope --plan 01-07 --commits-only` resolves them on this branch.
- TDD gate log: 3 `test(01-07)` commits each precede their `feat(01-07)` commit.
