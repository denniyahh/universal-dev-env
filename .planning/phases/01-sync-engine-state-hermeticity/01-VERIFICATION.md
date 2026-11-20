---
phase: 01-sync-engine-state-hermeticity
verified: 2026-11-20T15:51:44Z
status: passed
score: 18/18 must-haves verified
covered_files: [".config/nextest.toml", ".planning/phases/01-sync-engine-state-hermeticity/01-01-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-01-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-02-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-02-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-03-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-03-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-04-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-04-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-05-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-05-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-06-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-06-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-07-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-07-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-08-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-08-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-09-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-09-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-10-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-10-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-11-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-11-SUMMARY.md", "Cargo.lock", "Cargo.toml", "deny.toml", "scripts/hooks/pre-push", "src/audit/mod.rs", "src/cli/mod.rs", "src/main.rs", "src/process.rs", "src/sanitizer/mod.rs", "src/semantic/mod.rs", "src/sync/merge.rs", "src/sync/mod.rs", "src/sync/staging.rs", "src/sync/state.rs", "src/template/mod.rs", "templates/projects/generic/gitignore.repo", "tests/e2e_lifecycle.rs"]
covered_digest: "v4:sha256:e89204a5883b76fd7147f2bf069a66283aade8ad6e322564197bd6c178612166"
behavior_unverified: 0
overrides_applied: 1
overrides:
  - must_have: "01-01 backstop: deadline boundary is inclusive (elapsed >= timeout kills the group; a child that exits before the poll observes the deadline returns its real status; POLL_INTERVAL = 10 ms)"
    reason: "Accepted on the evidence: run_bounded checks child.try_wait() before the `Instant::now() >= deadline` kill and returns the real status on Ok(Some), POLL_INTERVAL is Duration::from_millis(10), and the timeout tests pass. A test pinned to the exact millisecond boundary would itself be flaky."
    accepted_by: "operator (2026-10-21), after being told the exact boundary is untested and the worst case is one 10 ms poll later; first ruled by the orchestrator on 2026-10-20"
    accepted_at: "2026-10-21T22:00:00Z"
re_verification:
  previous_status: passed
  previous_score: 18/18
  previous_head: 8853a50
  current_head: 6797e2c
  gaps_closed: []
  gaps_remaining: []
  regressions: []
  previous_items_closed:
    - "Deferred W2 (home-path placeholder followed by , ; . ) ] } or =): closed by Phase 2. test_path_boundary_table (src/sanitizer/mod.rs:517), test_push_parameterizes_punctuation_delimited_home_paths (src/sync/mod.rs:7666) and test_cli_push_parameterizes_trailing_separator_home (tests/e2e_lifecycle.rs:5486) pass at 6797e2c"
deferred:
  - truth: "Windows runtime behaviour of the bounded runner (JobObject kill), the remote cache lock (release on process death) and staged state.json replace"
    addressed_in: "Phase 999.1 (backlog)"
    evidence: "ROADMAP.md:207 'Phase 999.1: Windows runtime checks for the phase 01 sync engine (BACKLOG)'; operator waiver 2026-10-20. Windows-target clippy last run at 1e058a9 (check only); not re-run at 6797e2c"
  - truth: "A file that ude init kept must not be publishable by a plain push (an uncommitted own .gitignore with no gitignore template reads Locally Modified and push publishes it)"
    addressed_in: "Phase 4"
    evidence: "Phase 4 goal (ROADMAP.md:177): '... safe re-initialization protections.' Operator accepted for phase 01 on 2026-10-19"
  - truth: "Re-init side effects (01-REVIEW-REINIT WR-02, WR-03, IN-01): explicit --type change not applied to mise.toml; update-index stages an uncommitted hook edit; lost exec bit not restored"
    addressed_in: "Phase 4"
    evidence: "deferred-items.md ('candidate for Phase 4, safe re-initialization'); Phase 4 SC2 covers re-init on an existing workspace"
  - truth: "Init judges 'no shared template' against the local templates, not the remote that sync uses (01-REVIEW IN-04, unverified)"
    addressed_in: "Phase 4"
    evidence: "01-REVIEW-DISPOSITION.md IN-04 deferred; Phase 4 SC4 (ROADMAP.md:184) '.ude.toml is parsed and establishes deterministic precedence'"
---

# Phase 1: Sync Engine & State Hermeticity Verification Report

**Phase Goal:** Ensure bidirectional synchronization is hermetic, crash-resilient, and atomic without phantom drift or zombie file resurrections.
**Verified:** 2026-11-20T15:51:44Z, at HEAD 6797e2c on branch gsd/phase-02-sanitizer-boundaries-security-hardening (Linux).
**Status:** passed. Windows runtime stays deferred to backlog Phase 999.1 under the operator's waiver.
**Re-verification:** Yes. Previous report: passed 18/18 at 8853a50. Phase 2 (386 commits since) changed src/main.rs, src/process.rs, src/sanitizer/mod.rs, src/sync/mod.rs, src/template/mod.rs, tests/e2e_lifecycle.rs and scripts/hooks/pre-push. src/sync/staging.rs, src/sync/state.rs, src/sync/merge.rs and src/semantic/mod.rs are unchanged since 8853a50. Every truth below was re-checked by re-running its named tests at 6797e2c; source lines were re-located.

## Read this first: what argues against "passed"

1. **The revert controls (A to E) and probes (P1, P1b, P7r, P8) were not re-run at 6797e2c.** They were run at 1e058a9/8853a50. The tests they proved bite all still exist and pass, and the code sites they targeted are still in place (src/main.rs:1235 and :1245 use `shared_template_path`; src/sync/mod.rs:979 calls `never_deleted_upstream` with `?`), but whether each test would still fail under the same revert on Phase 2's rewritten code is not re-established.
2. **The push scan-failure test still runs in local mode** (carried forward). `test_scan_failure_is_not_reported_as_a_state_file_error` loops over `status`, `pull`, `push` without `--remote`. By reading: src/sync/mod.rs:979 propagates a history-lookup error with `?` from `scan_project`, and src/main.rs:1342 has push's `ScanFailed` arm.
3. **One symlinked managed file still stops every sync command** (01-REVIEW WR-01, carried forward, operator deferred). Fails closed with exit 2.

None of these breaks the phase goal: nothing is lost, published or reported as drift that does not exist.

## Goal Achievement

### Observable Truths

All named tests below were run at 6797e2c in one filtered `cargo test --workspace` invocation (exit 0; 38 unit + 51 e2e passed, 0 failed), and each name was matched individually as `... ok` in the saved log; a made-up name read MISSING (control). The regression gate's full `cargo test` at this branch: 598 passed, 0 failed, 1 ignored (the ignored one is `test_dump_parity_corpus`, src/sanitizer/gitleaks.rs:1083, a Phase 2 corpus writer). `cargo test -- --list` enumerates 599 tests.

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | SC1: an upstream template deletion propagates locally on pull via tombstones and does not come back on later pushes | ✓ VERIFIED | `test_pull_upstream_deletion`, `test_push_local_deletion`, `test_both_deleted`, `test_fresh_clone_does_not_resurrect_template_deleted_upstream`, `test_pull_deletes_only_templates_the_remote_history_deleted`, `test_pull_confirms_a_deletion_made_while_resolving_a_merge` pass |
| 2 | SC2: writes are staged in a shadow step before the atomic update, with no partial writes on error | ✓ VERIFIED | `test_pull_staging_failure_no_partial_writes`, `test_push_staging_failure_no_partial_writes`, `test_state_save_atomic` pass; staging.rs and state.rs unchanged since 8853a50 |
| 3 | SC3: git/gh calls have bounded timeouts and a hung child is killed | ✓ VERIFIED (Linux) | `test_timeout_kills_group`, `test_push_timeout_hanging_hook`, `test_drains_large_output`, `test_env_scrub` pass. src/process.rs:12 `LOCAL_TIMEOUT` 30 s, :15 `NETWORK_TIMEOUT` 60 s. No `.output()`/`.status()`/`.spawn()` outside process.rs in production code except src/semantic/mod.rs:152 (curl to the semantic API, not git/gh; unchanged). Windows deferred to 999.1 |
| 4 | SC4: CRLF-normalized fingerprints give no false drift | ✓ VERIFIED | `test_sync_status_crlf_only_change_in_sync` and 11 CRLF/EOL unit tests pass |
| 5 | SC5: divergent edits produce `.conflict` files and keep local changes | ✓ VERIFIED | `test_sync_pull_conflict_exits_1_and_writes_sidecar`, `test_sync_resolve_end_to_end`, `test_pull_deletion_conflicts_write_sidecars`, `test_sidecar_mode_follows_local` pass |
| 6 | SC6: push verifies branch continuity and rejects divergent candidates | ✓ VERIFIED | `test_direct_push_aborts_when_upstream_moved`, `test_push_continuity_violation_is_hard_failure`, `test_force_does_not_bypass_upstream_moved`, `test_push_skips_conflicts_and_merged`, `test_sync_push_skips_unresolved_conflict_exit_1` pass |
| 7 | 01-06 backstop: in remote mode the cache lock serializes commands for the whole command | ✓ VERIFIED (Linux) | `test_remote_lock_concurrency_serialization_and_timeout`, `test_remote_lock_released_when_holder_killed`, `test_remote_lock_not_inherited_by_children` pass |
| 8 | 01-01 backstop: the deadline boundary is inclusive, with a 10 ms poll | PASSED (override) | Override conditions re-read at 6797e2c: src/process.rs:18 `POLL_INTERVAL = Duration::from_millis(10)`; :199-208 `try_wait()` `Ok(Some)` returns the real status before the `Instant::now() >= deadline` kill arm. Phase 2's process.rs change is additive (+177/-1, the -1 an import) |
| 9 | Re-init keeps every prior record (files, tombstones, open conflicts) | ✓ VERIFIED | All 10 `test_reinit_*` e2e tests pass |
| 10 | A committed file that init keeps with no template is never deleted by pull or published by push | ✓ VERIFIED | `test_init_keeps_committed_gitignore_when_template_is_absent`, `test_fresh_clone_does_not_resurrect_template_deleted_upstream` pass |
| 11 | A user's own uncommitted file with no template is left Unmanaged, and status says "no shared template" | ✓ VERIFIED (except `.gitignore`, deferred to Phase 4) | `test_init_leaves_uncommitted_file_without_template_unmanaged` passes |
| 12 | A missing or foreign template root is never treated as upstream | ✓ VERIFIED | `test_sync_refuses_a_remote_that_is_not_a_template_repo`, `test_sync_ignores_a_project_templates_dir_that_is_not_ude`, the three `test_first_ude_templates_*` unit tests pass |
| 13 | Init fails closed on unreadable or unsaveable state and on a parent repo git recognises but cannot read | ✓ VERIFIED | `test_init_fails_when_parent_repo_index_is_unreadable` and the other `test_init_*` tests pass |
| 14 | Init records the rendered project type; `--type` is validated; `--mempalace=false` and `--graphify=false` work | ✓ VERIFIED | `test_init_rejects_an_unknown_type` and the other `test_init_*` tests pass |
| 15 | Hermetic tests and runtime: no `set_var`/`remove_var`; fixtures use scrubbed builders; inherited `GIT_DIR` ignored; git discovery fenced | ✓ VERIFIED | `rg 'set_var\|remove_var' src tests`: no match, exit 1 (control: `fn map_to_upstream` matches 1 in src/sync/mod.rs). `test_sync_ignores_inherited_git_dir` passes. Production pushes strip inherited git test env (`remove_git_test_env`, src/sync/mod.rs:2316, :2470) |
| 16 | D-14 amended: push never deletes a `.jinja` template that has template logic; status, pull and init agree; the remedy names the template in the shared templates | ✓ VERIFIED | `test_push_never_deletes_a_template_with_logic` and `test_push_names_the_shared_template_in_remote_mode` pass. Push's edit-site and deletion-site messages use `shared_template_path` (src/main.rs:1235, :1245; fn at :1434). Revert controls C/D/D2 not re-run (see "Read this first" 1) |
| 17 | D-12 amended: with a remote, status and pull treat a template as deleted only when the cache checkout's history shows its deletion, including one made in a merge; one the remote never had reads Missing upstream and pull keeps the file and its record; failure to confirm fails closed | ✓ VERIFIED | `test_pull_deletes_only_templates_the_remote_history_deleted`, `test_pull_confirms_a_deletion_made_while_resolving_a_merge`, `test_edited_file_with_template_the_remote_never_had_is_missing_upstream` pass. src/sync/mod.rs:979 reclassifies to `MissingUpstream` and propagates lookup failure with `?` |
| 18 | An edited file whose template the remote never had is handled the same way by status, pull, push and resolve, and each command says what happens to it | ✓ VERIFIED | `test_edited_file_with_template_the_remote_never_had_is_missing_upstream` passes. The resolve leg was shown by probe P1 at 1e058a9, not re-run |

**Score:** 18/18 truths verified (1 by override; 0 present-but-behavior-unverified; 0 failed)

### Deferred Items

| # | Item | Addressed In | Evidence |
|---|------|-------------|----------|
| 1 | Windows runtime: JobObject kill, cache-lock release, state.json replace | Phase 999.1 (backlog) | ROADMAP.md:207; operator waiver 2026-10-20 |
| 2 | Kept files publishable by a plain push (`.gitignore` without a template) | Phase 4 | Operator-accepted 2026-10-19 |
| 3 | Re-init side effects (REINIT WR-02, WR-03, IN-01) | Phase 4 | deferred-items.md |
| 4 | "No shared template" judged against local, not remote, templates (IN-04) | Phase 4 | 01-REVIEW-DISPOSITION.md |

Closed since the previous report: W2 (home-path placeholder followed by punctuation), by Phase 2; three tests pass at 6797e2c (frontmatter `previous_items_closed`).

### Advisory (New Scope, Unevidenced)

None.

### Required Artifacts

`verify.artifacts` at 6797e2c: 32/32 across the 11 plans, every plan exit 0, `all_passed: true`.

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/process.rs` | Bounded runner, env scrub, redaction | ✓ VERIFIED | Changed by Phase 2 (additive); deadline loop :197-214 |
| `src/sync/staging.rs` | Stage, validate, commit | ✓ VERIFIED | Unchanged since 8853a50 |
| `src/sync/state.rs` | v2 schema, tombstones, conflicts, staged save | ✓ VERIFIED | Unchanged since 8853a50 |
| `src/sync/merge.rs` | diffy diff3, sidecar renderer | ✓ VERIFIED | Unchanged since 8853a50 |
| `src/sync/mod.rs` | Deletion states, conflicts, resolve, push skips, continuity, lock, history-confirmed deletion | ✓ VERIFIED | Changed by Phase 2; production code ends at :4217 (`#[cfg(test)]` at :4218) |
| `src/main.rs` | Dispatch, exit codes, status/pull/push wording | ✓ VERIFIED | Changed by Phase 2 |
| `tests/e2e_lifecycle.rs` | Hermetic e2e coverage | ✓ VERIFIED | 114 e2e tests |

### Key Link Verification

`verify.key-links`: 01-01 3/3; 01-02 to 01-11 fail only on the tool's "from must be a relative file path" parser mismatch (plans write `from: "src/sync/mod.rs scan_project"`), as at the previous verification. Manual check of all 26 declared patterns at 6797e2c: 23 found by `rg -c` in their declared file; the 3 whose `from` names only a symbol (`clear_conflict`, `skipped_merged`, `UpstreamMoved`) are present in production code (src/sync/state.rs:311, src/sync/mod.rs:173, :427). A made-up pattern returns no match (control). Presence only; behaviour is carried by the tests.

### Data-Flow Trace (Level 4)

Not applicable: a CLI with no rendered UI.

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Test enumeration | `cargo test --workspace -- --list` | 599 tests; every test named above present once; made-up name 0 | ✓ PASS |
| Phase 1 named tests | one `cargo test --workspace -- <filters>` run, each name matched as `ok` in the log | exit 0; 38 + 51 passed, 0 failed | ✓ PASS |
| W2 closure | `cargo nextest run --no-tests=fail -E 'test(=…) \| …'` | 3 run, 3 passed | ✓ PASS |
| Full suite (regression gate, not this agent) | `cargo test` on this branch | 598 passed, 0 failed, 1 ignored | cited |
| No force push in production | `rg '"--force\|"-f"\|force-with-lease\|\+refs' src` | only test-module hits (src/sync/mod.rs:14434, :18019, :18035, all after :4218); production push refspecs at :2300 and :2455 carry no `+` | ✓ PASS |
| pre-push hook syntax | `bash -n scripts/hooks/pre-push` | exit 0; Phase 2's change adds a personal-artifact check before the unchanged cargo-deny steps | ✓ PASS |
| Revert controls A-E, probes P1/P1b/P7r/P8 | not re-run | from 1e058a9/8853a50 | carried forward |

### Probe Execution

No `scripts/*/tests/probe-*.sh` exist and no plan declares one, so Step 7c does not apply.

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| SYNC-01 | 01-07 | Tombstones; deletions propagate without resurrection | ✓ SATISFIED | Truths 1, 10, 11, 12, 16, 17, 18 |
| SYNC-02 | 01-04, 01-06 | Two-phase staging | ✓ SATISFIED | Truths 2, 7 |
| SYNC-03 | 01-01, 01-02, 01-03 | Bounded git/gh timeouts (30-60 s) | ✓ SATISFIED (Linux) | Truths 3, 8, 15; Windows deferred to 999.1 |
| SYNC-04 | 01-05 | CRLF-normalized fingerprints | ✓ SATISFIED | Truth 4 |
| SYNC-05 | 01-08, 01-09, 01-10 | `.conflict` files; local edits preserved | ✓ SATISFIED | Truth 5 |
| SYNC-06 | 01-10, 01-11 | Safe push candidates; continuity | ✓ SATISFIED | Truths 6, 18 |

All six IDs are claimed in plan `requirements:` fields (re-extracted per plan at 6797e2c). REQUIREMENTS.md maps exactly these six to Phase 1 (lines 71-76). None is orphaned.

**Prohibitions.** The five test-tier items are `status: resolved` and their enforcement tests pass at 6797e2c, so none is flagged.
- **Never widen a synced file's mode:** `test_staged_write_modes`, `test_state_save_mode`.
- **Never destroy local edits without resolve:** `test_pull_deletion_conflicts_write_sidecars`.
- **A sidecar is never wider than its file:** `test_sidecar_mode_follows_local`.
- **Never publish unresolved or merged-unpulled content:** `test_push_skips_conflicts_and_merged`, `test_sync_push_skips_unresolved_conflict_exit_1`.
- **Never force-push:** `test_force_does_not_bypass_upstream_moved`; production refspecs carry no `+` (src/sync/mod.rs:2300, :2455).

### Anti-Patterns Found

Debt markers: `rg '\b(TBD|FIXME|XXX)\b'` over the covered source files and the hook: no match (exit 1). `TODO`/`HACK`: no match.

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| src/sync/mod.rs | 882 `scan_project` | One symlinked managed file fails every sync command (01-REVIEW WR-01) | ⚠️ Warning | Fails closed, exit 2. Carried forward; operator deferred |
| src/sync/mod.rs | 992 `never_deleted_upstream` | A template added and deleted only on a side branch reads Deleted upstream | ℹ️ Info | Carried forward |
| src/sync/mod.rs | 992 `never_deleted_upstream` | One full-history `git log` per missing template, serial | ℹ️ Info | Carried forward; not timed |
| (plans) | files_modified | `.cargo/mutants.toml` is listed by plans 01-05/07/08/11 but not tracked | ℹ️ Info | Carried forward; no must-have depends on it |

### Human Verification Required

None. No truth is behavior-unverified or abstained. The Windows checks are operator-waived and backlogged as Phase 999.1.

### Gaps Summary

No gaps. Phase 2's rewrites of src/main.rs, src/process.rs, src/sync/mod.rs and tests/e2e_lifecycle.rs did not regress any Phase 1 truth: every named test still exists and passes at 6797e2c, the override's code conditions still hold, and all six SYNC requirements stay satisfied. Phase 2 also closed the W2 deferral. The revert controls and probes stand on earlier runs.

---

_Verified: 2026-11-20T15:51:44Z_
_Verifier: Claude (gsd-verifier)_
