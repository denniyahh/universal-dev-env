---
phase: 01-sync-engine-state-hermeticity
plan: 05
subsystem: sync
tags: [rust, sha2, crlf, line-endings, state-migration, proptest, cargo-mutants, tdd]

requires:
  - phase: 01-04
    provides: "CURRENT_STATE_VERSION with forward guard, Result-returning load_from_dir, stage_to_dir/commit, scan_project -> Result"
provides:
  - "SyncEngine::fingerprint (SHA-256 of LF-normalized UTF-8, raw for non-UTF-8) and SyncEngine::normalize_eol (CRLF -> LF only)"
  - "private SyncEngine::classify_file: status from fingerprints only; merge only when fingerprints conflict and all three versions are text"
  - "pub enum EolStyle { Lf, Crlf }, pub fn detect_eol_style, pub fn apply_eol_style (module-level in src/sync/mod.rs)"
  - "CURRENT_STATE_VERSION = 2 with in-memory migration of version 1 state files on load"
  - "pull writes upstream and merged text in the local file's line-ending style; push writes into an existing template in the template's style"
affects: [01-06, 01-07, 01-08, 01-09, sync]

actuals:
  tokens: 11376
  tasks: 3
  commits: 7
plan_head_before: 497f1aac6f2b2372e330c6bf4afd696204b146ec
plan_head_after: 92204826ec2a2684e65e31a6b02ed8daf81a8adf

tech-stack:
  added: []
  patterns:
    - "Classify by fingerprint, merge by content: compare_3way_hashes on fingerprints decides the status; text is touched only to merge"
    - "Ancestors are stored as fingerprint + LF-normalized content; writes re-apply the destination's line-ending style"
    - "State schema migrations run in memory in load_from_dir; the next save persists them"

key-files:
  created: []
  modified: [src/sync/mod.rs, src/sync/state.rs, src/main.rs, tests/e2e_lifecycle.rs]

key-decisions:
  - "fingerprint normalizes every valid UTF-8 input (no contains(\"\\r\\n\") guard): same result, and it avoids an equivalent mutant on the guard"
  - "The v1 migration reads hash-only records through validate_safe_write_target itself, which is stricter than a component check: it also refuses symlinked targets and escaping parents"
  - "classify_file takes Option<FileVersion> (fingerprint + optional text) per side, so an absent file cannot be passed to merge as empty text"
  - "detect_eol_style/apply_eol_style are module-level functions; fingerprint/normalize_eol stay SyncEngine associated functions as planned"
  - "No .cargo/mutants.toml: no mutant needed an exclusion"

patterns-established:
  - "Pull/push writes go through existing_text_eol_style(path): Some(style) re-applies it to LF text, None (absent, unreadable, binary) writes the source as is"

requirements-completed: [SYNC-04]

coverage:
  - id: D1
    description: "Fingerprint: CRLF and LF text share a fingerprint; lone CR, non-UTF-8 bytes and real edits stay distinct; CR-free text hashes raw"
    requirement: SYNC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_fingerprint"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_fingerprint_crlf_lf_equivalence_prop (proptest, default 256 cases)"
        status: pass
    human_judgment: false
  - id: D2
    description: "Classification by fingerprint: a CRLF-only local copy reads In Sync; a real edit in a CRLF file reads Locally Modified (unit scan and `ude init` + `ude sync status`)"
    requirement: SYNC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_scan_crlf_only_difference_in_sync"
        status: pass
      - kind: e2e
        ref: "tests/e2e_lifecycle.rs#test_sync_status_crlf_only_change_in_sync"
        status: pass
    human_judgment: false
  - id: D3
    description: "Ancestors written by pull (applied, merged, converged) and push are fingerprint + LF-normalized content"
    requirement: SYNC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_ancestor_writers_record_fingerprint_and_lf_content"
        status: pass
      - kind: other
        ref: "six hand mutants (raw hash or unnormalized content per writer) each fail that test"
        status: pass
    human_judgment: true
    rationale: "The `ude init` writer is changed but not distinguishable by any test: init writes LF files, for which fingerprint equals the raw hash and normalization is a no-op. It was checked by reading the code (SyncEngine::fingerprint + normalize_eol in src/main.rs) only."
  - id: D4
    description: "State version 2: v1 content records re-hash from stored content; hash-only records adopt the fingerprint only when the raw hash matches the local file; mismatched, `..` and absolute keys stay unchanged; load never writes; next save writes version 2"
    requirement: SYNC-04
    verification:
      - kind: unit
        ref: "src/sync/state.rs#sync::state::tests::test_migrate_v1_content_record, test_migrate_v1_hash_only_record, test_migrate_v1_mismatch_unchanged, test_migrate_v1_unsafe_key_ignored"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_scan_after_v1_migration_in_sync"
        status: pass
      - kind: e2e
        ref: "tests/e2e_lifecycle.rs#test_cli_e2e_init_and_sync_lifecycle (asserts \"version\": 2)"
        status: pass
      - kind: other
        ref: "hand mutants: skipping the safe-key check fails test_migrate_v1_unsafe_key_ignored; dropping the raw-hash equality fails test_migrate_v1_mismatch_unchanged"
        status: pass
    human_judgment: false
  - id: D5
    description: "Line-ending preservation: pull into CRLF writes CRLF (applied and merged), an LF file never gains a CR, an absent local file is written as upstream renders it; push keeps an existing template's style and publishes a new one as is"
    requirement: SYNC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_eol_detection"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_eol_style_roundtrip_prop"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_pull_preserves_eol_style"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_pull_merged_crlf_keeps_crlf"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_push_preserves_upstream_eol_style"
        status: pass
      - kind: other
        ref: "cargo mutants --file src/sync/mod.rs --re fingerprint|normalize_eol|detect_eol_style|apply_eol_style|classify_file: 18 mutants, 14 caught, 4 unviable, 0 missed, exit 0"
        status: pass
    human_judgment: false

duration: 15min
completed: 2026-10-18
status: complete
---

# Phase 01 Plan 05: CRLF-Insensitive Fingerprints, State v2 Migration and Line-Ending-Preserving Writes Summary

**Managed files are now classified by a SHA-256 fingerprint of LF-normalized text, so a CRLF-only copy reads In Sync. `state.json` moves to version 2: version 1 files are migrated in memory on load and persisted on the next save. Pull and push write each file in its destination's existing line-ending style.**

## Performance

- **Duration:** about 15 min
- **Started:** 2026-10-18T19:27:27Z
- **Completed:** 2026-10-18T19:42:24Z
- **Tasks:** 3 (plus one coverage-test commit)
- **Files modified:** 4

## Accomplishments
- `SyncEngine::fingerprint` / `normalize_eol`. `compute_hash` is unchanged and still keys the remote cache directory and the PR branch name.
- `classify_file`: the status comes from `compare_3way_hashes` over fingerprints. Merge runs only when the status is Conflicted and the ancestor, local and upstream versions are all text, and it runs on normalized text. The old fallback that fed an absent side to merge as empty text is gone. `scan_project` and `compare_3way_with_merge` both go through it.
- Ancestors are recorded as fingerprint + LF content at init, pull (applied, merged, converged) and push.
- `CURRENT_STATE_VERSION = 2`. A version 1 file is migrated on load and never written by the load. The forward guard now refuses versions above 2.
- `EolStyle`, `detect_eol_style` (CRLF only when CRLF pairs outnumber bare LF), `apply_eol_style`. Pull and push re-apply the destination's style. Absent or binary destinations get the source bytes as they are.

## Task Commits

1. **Task 1 (tracer) RED:** `0cddfcc` test(01-05): add failing tests for CRLF-insensitive fingerprints
2. **Task 1 GREEN:** `cbeff98` feat(01-05): classify managed files by CRLF-insensitive fingerprint
3. **Task 2 RED:** `32bc78f` test(01-05): add failing tests for v1 -> v2 state migration (D-07)
4. **Task 2 GREEN:** `4d76556` feat(01-05): migrate v1 state.json to fingerprint semantics (D-07)
5. **Task 3 RED:** `3eb1a30` test(01-05): add failing tests for line-ending style preservation
6. **Task 3 GREEN:** `5f41926` feat(01-05): preserve each file's line-ending style on pull and push
7. **Coverage (deviation, see below):** `9220482` test(01-05): assert ancestor writers record fingerprint and LF content

Tracer gate (interactive, `end-of-phase`, automated-only verify): the Task 1 verify commands were re-run on the committed tree at `cbeff98` and passed before Task 2 started.

## TDD Gate Compliance

RED then GREEN commits are present for all three tasks, in order (3 `test`, 3 `feat`, no `refactor`). `9220482` is a post-GREEN coverage test, not a RED commit.

RED evidence: nextest JUnit reports passed to `gsd_run check tdd-red-evidence`. All 15 records returned `RED_EVIDENCE_OK` / `target_test_failed`.

| Target | Failed on | Semantic assessment |
|---|---|---|
| test_fingerprint | CRLF == LF equality (stub hashed raw bytes) | intended |
| test_fingerprint_crlf_lf_equivalence_prop | first `prop_assert_eq` (CRLF vs LF) | intended |
| test_scan_crlf_only_difference_in_sync | `LocallyModified` != `InSync` | intended (research Pitfall 1: raw string equality) |
| e2e test_sync_status_crlf_only_change_in_sync | no `.gitconfig: In Sync` after the CRLF rewrite | intended |
| test_migrate_v1_content_record | ancestor hash still raw | intended |
| test_migrate_v1_hash_only_record | ancestor hash still raw | intended |
| test_migrate_v1_mismatch_unchanged | `version` 1 != 2 | negative-control test. Its hash assertion already holds before the change, so it fails only on load-as-v2 |
| test_migrate_v1_unsafe_key_ignored | `version` 1 != 2 | negative-control test, as above. Proven discriminating by the hand mutant below |
| test_scan_after_v1_migration_in_sync | `Merged` != `UpstreamModified` | intended (the unmigrated raw CRLF ancestor reads as a local edit) |
| e2e test_cli_e2e_init_and_sync_lifecycle | `"version": 2` assertion | intended |
| test_eol_detection | `Lf` != `Crlf` (stub) | intended |
| test_eol_style_roundtrip_prop | CRLF detection of the CRLF rendering | intended |
| test_pull_preserves_eol_style | LF written into a CRLF file | intended |
| test_pull_merged_crlf_keeps_crlf | LF merge written into a CRLF file | intended |
| test_push_preserves_upstream_eol_style | CR published into an LF template | intended |

Classifier negative control: a record that names a test absent from the report as its target returned `INVALID_RED` / `no_target_test_failure`.

## Verification

| Check | Result |
|---|---|
| Task 1 verify: 3 unit tests (`test_fingerprint` filter matches 2), e2e CRLF test | pass |
| Empty-string-fallback check (plan's second automated command) | exit 0. Negative control: the same pipeline on the RED commit's source exits 1 |
| Task 2 verify: `test_migrate_v1` (4 passed), `test_scan_after_v1_migration_in_sync` | pass |
| Task 3 verify: 5 named tests | pass |
| `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test` | exit 0 before every commit. Final: 87 unit + 11 e2e (baseline before this plan: 73 + 10) |
| Mutation gate (`--list` first: 18 mutants) | exit 0 in 75 s (about 90 s wall clock). 14 caught, 4 unviable, 0 missed, 0 timeout. caught.txt (14) and outcomes.json `caught` (14) agree. The baseline ran 55 tests, all `sync::`, none of them wall-clock tests. The plan's check command exits 0 |
| Hermeticity gate from 01-03 (`GIT_DIR=<decoy bare repo>`, `GIT_CONFIG_GLOBAL=/dev/null`, `GIT_CONFIG_NOSYSTEM=1`, `cargo test --bin ude sync::`) | 59/59, exit 0. Control: `git rev-parse` with that `GIT_DIR` confirms the decoy is a valid bare repo |
| Acceptance greps | one each for `pub fn fingerprint(bytes: &[u8]) -> String`, `pub fn normalize_eol`, `compute_hash(key`, `compute_hash(commit_msg`, `SyncEngine::fingerprint` in main.rs, `CURRENT_STATE_VERSION: u32 = 2;`, `pub enum EolStyle`, `pub fn detect_eol_style`, `pub fn apply_eol_style`. `"version\": 1` no longer appears in tests/e2e_lifecycle.rs |

The 4 unviable mutants replace `detect_eol_style` and `classify_file` results with `Default::default()`. They do not compile, because `EolStyle` and `SyncStatus` have no `Default`. They are not equivalent mutants, so nothing was excluded.

## What the verification does NOT establish
- **`ude init` ancestor writer:** it is changed (fingerprint + `normalize_eol`) but not distinguishable by any test. Init writes LF files, so the fingerprint equals the raw hash. Coverage D3 is marked human_judgment for this reason.
- **The pull/push write wiring is not mutation-tested.** The gate covers only the five helpers named in the plan. `existing_text_eol_style` and the three write sites are covered by the pull/push tests and, for ancestor recording, by the six hand mutants. cargo-mutants does not cover them.
- **Windows:** CRLF behavior was tested only with CRLF bytes on Linux. Nothing here has been compiled or run on Windows.
- **Mixed-EOL files:** a file is written in its *dominant* style (research A6). The minority line endings of a mixed file are converted on the next write. No test covers a mixed file being rewritten.
- **Proptest strength:** two properties at the default 256 random cases each. That is a sample over CR-free lines up to 16 characters, not an exhaustive check.
- **Migration with symlinked managed files:** a hash-only v1 record whose file is a symlink is not migrated, because `validate_safe_write_target` refuses it. It keeps its raw hash and reads as modified until the next sync records it. This is conservative by design and untested.

## Files Created/Modified
- `src/sync/mod.rs` - fingerprint, normalize_eol, FileVersion, classify_file, EolStyle, detect/apply_eol_style, existing_text_eol_style; scan/pull/push rewired; 12 new tests (2 proptest)
- `src/sync/state.rs` - version 2, RAW_HASH_STATE_VERSION, `migrated_from_raw_hashes`; 4 migration tests
- `src/main.rs` - init records fingerprint + LF content
- `tests/e2e_lifecycle.rs` - `test_sync_status_crlf_only_change_in_sync`; lifecycle asserts version 2

## Decisions Made
- `fingerprint` normalizes every valid UTF-8 input rather than only inputs that contain CRLF. The output is identical (the replace is the identity on CR-free text), and it removes a match-guard mutant that no test could kill.
- The migration reuses `validate_safe_write_target` for hash-only keys. The plan asked for "as validate_safe_write_target does". Calling it directly adds its symlink and escaping-parent refusals at no cost.
- `classify_file` takes `Option<FileVersion>` per side (a fingerprint and optional text), so text cannot exist without a fingerprint and absence cannot become empty text.
- No `.cargo/mutants.toml` was created, because no mutant needed an exclusion.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing test coverage] Ancestor writers were not asserted by any test**
- **Found during:** pre-SUMMARY review against must-have truth 3
- **Issue:** every planned post-pull check (InSync) passes whatever the recorded ancestor is, because the local and upstream fingerprints match. Reverting a pull or push writer to a raw hash or to unnormalized content would have stayed green.
- **Fix:** `test_ancestor_writers_record_fingerprint_and_lf_content` covers pull (applied, merged, converged) and push. Each case writes CRLF bytes.
- **Verification:** six hand mutants (raw hash or unnormalized content, one per writer) each fail it at the writer they target. Sources were restored with plain `cp` + `touch`, then rebuilt green.
- **Committed in:** `9220482`

**2. [Rule 2 - Test strength] Planned tests strengthened where they could not discriminate**
- `test_scan_after_v1_migration_in_sync`: the planned InSync assertion passes without any migration (the local and upstream fingerprints are equal). Added an upstream-edited case that must read UpstreamModified, which fails without migration (RED showed `Merged`). Committed in `32bc78f`.
- `test_migrate_v1_unsafe_key_ignored`: added an absolute-path key, and a safe key in the same state that must migrate (a positive control proving the migration ran). Committed in `32bc78f`.
- `test_pull_preserves_eol_style` / `test_push_preserves_upstream_eol_style`: added the absent-local and absent-template cases from must-have truth 5. Committed in `3eb1a30`.
- `test_eol_detection`: added `\r\n`, `\n`, empty input and lone-CR cases. These are the inputs that kill the subtraction, division and comparison mutants. Committed in `3eb1a30`.

---

**Total deviations:** 2 auto-fixed (both Rule 2: test coverage/strength). **Impact:** tests only. No production scope change.

## Issues Encountered
- proptest wrote `proptest-regressions/sync/mod.txt` during each failing RED run. The file was deleted each time and never committed. The GREEN runs produce none.

## Known Stubs
None. The RED stubs (`fingerprint` hashing raw bytes, `detect_eol_style` always LF, `apply_eol_style` identity) were replaced in their GREEN commits.

## Threat Flags
None. T-01-13 (only CRLF pairs normalized, binary and lone CR stay distinct) and T-01-14 (exact re-hash; adoption only on a raw-hash match; unsafe keys never read) are mitigated and tested. There is no new surface: the migration reads only paths that pass `validate_safe_write_target`.

## User Setup Required
None.

## Next Phase Readiness
- Plans 01-06 and later can rely on fingerprint semantics, `classify_file` and state version 2. A schema change for deletion or conflict records would need version 3 plus a 2 -> 3 step in `load_from_dir`.
- Plan 01-07's mutation gate can select `classify_file` by name, as planned.
- Plan 01-08 swaps the merge engine. `classify_file` already passes normalized LF text to `merge::merge_3way_text`, and pull re-applies the local style to the merged result.
- Pre-existing behavior, unchanged here: a cleanly merged file records the *upstream* version as its ancestor, so it reads Locally Modified after pull (D-17 territory).

## Self-Check: PASSED
- FOUND: src/sync/mod.rs, src/sync/state.rs, src/main.rs, tests/e2e_lifecycle.rs, .planning/phases/01-sync-engine-state-hermeticity/01-05-SUMMARY.md
- FOUND on this branch: 0cddfcc, cbeff98, 32bc78f, 4d76556, 3eb1a30, 5f41926, 9220482

---
*Phase: 01-sync-engine-state-hermeticity*
*Completed: 2026-10-18*
