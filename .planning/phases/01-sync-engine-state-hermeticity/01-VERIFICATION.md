---
phase: 01-sync-engine-state-hermeticity
verified: 2026-10-21T20:18:38Z
status: passed
score: 15/15 must-haves verified
covered_files: [".config/nextest.toml", ".planning/phases/01-sync-engine-state-hermeticity/01-01-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-01-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-02-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-02-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-03-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-03-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-04-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-04-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-05-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-05-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-06-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-06-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-07-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-07-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-08-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-08-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-09-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-09-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-10-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-10-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-11-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-11-SUMMARY.md", "Cargo.lock", "Cargo.toml", "deny.toml", "scripts/hooks/pre-push", "src/audit/mod.rs", "src/cli/mod.rs", "src/main.rs", "src/process.rs", "src/sanitizer/mod.rs", "src/semantic/mod.rs", "src/sync/merge.rs", "src/sync/mod.rs", "src/sync/staging.rs", "src/sync/state.rs", "src/template/mod.rs", "templates/projects/generic/gitignore.repo", "tests/e2e_lifecycle.rs"]
covered_digest: "v3:sha256:36a4ef2a9473b9ad42b80963de9a2186408aa2e50bdf2f414d4457e9e47a11b4"
behavior_unverified: 0
overrides_applied: 1
overrides:
  - must_have: "01-01 backstop: deadline boundary is inclusive (elapsed >= timeout kills the group; a child that exits before the poll observes the deadline returns its real status; POLL_INTERVAL = 10 ms)"
    reason: "Accepted on the evidence: run_bounded checks child.try_wait() before the `Instant::now() >= deadline` kill and returns the real status on Ok(Some), POLL_INTERVAL is Duration::from_millis(10), and the timeout tests pass. A test pinned to the exact millisecond boundary would itself be flaky."
    accepted_by: "operator (2026-10-21), after being told the exact boundary is untested and the worst case is one 10 ms poll later; first ruled by the orchestrator on 2026-10-20"
    accepted_at: "2026-10-21T22:00:00Z"
re_verification:
  previous_status: passed
  previous_score: 15/15
  previous_head: 5f5e1f7
  gaps_closed: []
  gaps_remaining: []
  regressions: []
  warnings_closed:
    - "Foreign --remote with its own projects/ deleted unedited managed files (577c520)"
    - "Unit GitFixture not fenced against a TMPDIR inside a checkout (2a14361)"
deferred:
  - truth: "Windows runtime behaviour of the bounded runner (JobObject kill), the remote cache lock (release on process death) and staged state.json replace"
    addressed_in: "Phase 999.1 (backlog)"
    evidence: "Phase 999.1 goal: 'On a Windows host, run the three checks waived for phase 01 on 2026-10-20 (operator) ...'. 01-UAT.md tests 1-3 skipped under the operator waiver. clippy -D warnings for x86_64-pc-windows-msvc at HEAD 3cbbe4f on Linux: exit 0 (check only; nothing linked or run)"
  - truth: "Push must not leak the pusher's home path when a {{ user.home }} / {{ user.xdg_config }} placeholder is followed by , ; . ) ] } or = (re-review W2; latent)"
    addressed_in: "Phase 2"
    evidence: "Phase 2 goal: 'Screen and redact all confidential tokens, raw credentials, and user-specific paths before any remote git staging or push.'"
  - truth: "A file that ude init kept must not be publishable by a plain push (an uncommitted own .gitignore with no gitignore template reads Locally Modified and push publishes it)"
    addressed_in: "Phase 4"
    evidence: "Phase 4 goal: '... safe re-initialization protections.' Operator accepted for phase 01 on 2026-10-19; pending todo 2026-10-19-never-publish-files-that-init-kept-over-the-shared-template. Init code unchanged since 5f5e1f7"
  - truth: "Re-init side effects (01-REVIEW-REINIT WR-02, WR-03, IN-01): explicit --type change not applied to mise.toml; update-index stages an uncommitted hook edit; lost exec bit not restored"
    addressed_in: "Phase 4"
    evidence: "deferred-items.md ('candidate for Phase 4, safe re-initialization'); Phase 4 SC2 covers re-init on an existing workspace"
  - truth: "Init judges 'no shared template' against the local templates, not the remote that sync uses (01-REVIEW IN-04, unverified)"
    addressed_in: "Phase 4"
    evidence: "01-REVIEW-DISPOSITION.md IN-04 deferred: 'init vs remote template root belongs with .ude.toml / central_repo work (Phase 4)'; Phase 4 SC4 '.ude.toml is parsed and establishes deterministic precedence'"
---

# Phase 1: Sync Engine & State Hermeticity Verification Report

**Phase Goal:** Ensure bidirectional synchronization is hermetic, crash-resilient, and atomic without phantom drift or zombie file resurrections.
**Verified:** 2026-10-21T20:18:38Z, at HEAD 3cbbe4f (Linux).
**Status:** passed. Windows runtime stays deferred to backlog Phase 999.1 under the operator's waiver.
**Re-verification:** Yes. The previous report (passed 15/15 at 5f5e1f7) went stale because src/sync/mod.rs and tests/e2e_lifecycle.rs changed. `git diff --stat 5f5e1f7 HEAD` lists only those two code files plus 01-SECURITY.md and 01-VALIDATION.md. I re-checked every truth against HEAD and did not copy the old verdicts.

## Read this first: what argues against "passed"

1. **The new remote check proves "holds at least one managed template", not "is a complete ude template repo".** A remote that holds exactly one managed template (probe P2: only `projects/generic/hooks/pre-commit`) is accepted. Status then reads 10 Deleted upstream, and pull deletes those 10 unedited files with exit 0. This is the phase's own rule (a missing upstream template is a deletion), and the e2e suite relies on single-template remotes (`seed_mise_remote`). A foreign repository would need a file at an exact ude template path to reach this. I rate it Info, not a warning.
2. **The "no templates directory found" exit-2 path still has no automated test.** The dev binary always falls back to `CARGO_MANIFEST_DIR/templates`. src/main.rs is unchanged since 3fa02f4, where that path was proven with a templates-less scratch build. I did not repeat that build.
3. **A symlinked managed file in the project still fails every sync command** (01-REVIEW WR-01, operator deferred to a todo). Fails closed. Unchanged.

The two warnings in the previous report are closed. Both have negative controls (below): reverting each fix in a throwaway clone makes its test fail.

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | SC1: An upstream template deletion propagates locally on pull via tombstones and does not come back on later pushes | ✓ VERIFIED | `test_pull_upstream_deletion`, `test_push_local_deletion`, `test_both_deleted`, `test_fresh_clone_does_not_resurrect_template_deleted_upstream` pass in the full run at HEAD |
| 2 | SC2: Writes are staged in a shadow step before the atomic update, with no partial writes on error | ✓ VERIFIED | `test_pull_staging_failure_no_partial_writes`, `test_push_staging_failure_no_partial_writes`, `test_state_save_atomic` pass. staging.rs and state.rs unchanged since 5f5e1f7 |
| 3 | SC3: git/gh calls have bounded timeouts and a hung child is killed | ✓ VERIFIED (Linux) | `test_timeout_kills_group`, `test_push_timeout_hanging_hook`, `test_drains_large_output`, `test_env_scrub` pass. The new remote check adds no spawn (it is `is_file` over `map_to_upstream` paths). Windows deferred to 999.1 |
| 4 | SC4: CRLF-normalized fingerprints give no false drift | ✓ VERIFIED | `test_sync_status_crlf_only_change_in_sync` and the CRLF/EOL/migration unit tests pass. Fingerprint code unchanged |
| 5 | SC5: Divergent edits produce `.conflict` files and keep local changes | ✓ VERIFIED | `test_sync_pull_conflict_exits_1_and_writes_sidecar`, `test_sync_resolve_end_to_end`, `test_pull_deletion_conflicts_write_sidecars`, `test_sidecar_mode_follows_local` pass |
| 6 | SC6: Push verifies branch continuity and rejects divergent candidates | ✓ VERIFIED | `test_direct_push_aborts_when_upstream_moved`, `test_push_continuity_violation_is_hard_failure`, `test_force_does_not_bypass_upstream_moved`, `test_push_skips_conflicts_and_merged`, `test_sync_push_skips_unresolved_conflict_exit_1` pass |
| 7 | 01-06 backstop: in remote mode the cache lock serializes commands for the whole command | ✓ VERIFIED (Linux, re-observed) | Re-observed against the HEAD binary: during `ude sync push --remote` with a 4 s pre-receive hook, `flock -n -E 77 <lock>` returned 77 and `/proc/locks` had 1 entry on the lock inode. After push exited 0, `flock -n` returned 0 (control). The new refusal returns `Err` before `Ok(RemoteCache)`, so the guard drops on that path. Lock unit tests pass |
| 8 | 01-01 backstop: the deadline boundary is inclusive, with a 10 ms poll | PASSED (override) | Override carried forward (orchestrator ruling, 2026-10-20). process.rs unchanged since 5f5e1f7 |
| 9 | Re-init keeps every prior record (files, tombstones, open conflicts) | ✓ VERIFIED | The `test_reinit_*` e2e tests pass. Init code unchanged |
| 10 | A committed file that init keeps with no template is never deleted by pull or published by push | ✓ VERIFIED | `test_init_keeps_committed_gitignore_when_template_is_absent`, `test_fresh_clone_does_not_resurrect_template_deleted_upstream` pass |
| 11 | A user's own uncommitted file with no template is left Unmanaged, and status says "no shared template" | ✓ VERIFIED (except `.gitignore`, deferred to Phase 4) | `test_init_leaves_uncommitted_file_without_template_unmanaged` passes. Code unchanged since 5f5e1f7, where probes A/B established the `.gitignore` exception |
| 12 | A missing or foreign template root is never treated as upstream: local mode via `is_ude_templates`, remote mode via `ensure_remote_repo` requiring a template for a managed file (464bfc4, 8349e70, 577c520) | ✓ VERIFIED (residual: one-template remote, Info) | `test_sync_refuses_a_remote_that_is_not_a_template_repo` (README-only and `projects/website/` cases) and `test_sync_ignores_a_project_templates_dir_that_is_not_ude` pass. **Negative control:** with the check reverted to `templates_dir.join("projects").is_dir()` in a throwaway clone, the e2e test fails: `own-projects status: exit Some(0)`. **Probes against the HEAD binary:** a directory at `projects/generic/gitconfig.repo` (P1), and `templates/README` plus a root `projects/generic/gitconfig.repo` (P4) are both refused: exit 2 on status, pull and push, "no template for any managed file", project and remote ref unchanged. A symlink at `projects/generic/gitconfig.repo` pointing to /etc/hostname (P3) passes the check, then scan refuses the symlink: exit 2, project unchanged. Control V (full `templates/` copy): 11 In Sync, pull 0, push "No changes". Residual P2 is in "Read this first", item 1 |
| 13 | Init fails closed on unreadable or unsaveable state and on a parent repo git recognises but cannot read | ✓ VERIFIED | `test_init_fails_when_parent_repo_index_is_unreadable` and related init failure tests pass. main.rs unchanged |
| 14 | Init records the rendered project type; `--type` is validated; `--mempalace=false` and `--graphify=false` work | ✓ VERIFIED | `test_init_rejects_an_unknown_type` and related tests pass. cli/mod.rs unchanged |
| 15 | Hermetic tests and runtime: no `set_var`/`remove_var`; fixtures go through scrubbed builders; inherited `GIT_DIR` ignored; unit and e2e fixtures fence git discovery at the temp dir (3d1a544, 2a14361) | ✓ VERIFIED | `rg 'set_var\|remove_var' src tests`: no matches (control: `rg -c 'fn map_to_upstream' src` finds 1). The full suite with TMPDIR inside a throwaway clone of HEAD passed 137 unit + 67 e2e, and clone status, index and HEAD were unchanged afterwards. **Negative control:** with the two `GIT_CEILING_DIRECTORIES` lines removed from `GitFixture`, the same configuration fails `test_stage_publication_fails_when_git_status_fails` ("fixture dir must not be inside a git repository") and `test_git_fixture_scrubs_local_env` (the new assertion). The failing run's tempdir path was inside the clone, so TMPDIR was honoured |

**Score:** 15/15 truths verified (1 by override; 0 present-but-behavior-unverified)

#### Re-checks of the two code commits since 5f5e1f7

| Commit | Claim | Result |
|--------|-------|--------|
| 577c520 | `ensure_remote_repo` refuses a remote with no template for any managed file (`map_to_upstream` over `MANAGED_FILES`, `is_file`); e2e covers README-only and `projects/website/` | Holds. Code read in full: the check runs after fetch and before `Ok(RemoteCache)`. Test, revert control and probes P1-P4 are in truth 12. `is_file` follows symlinks, but scan refuses a symlinked template (P3), so it fails closed |
| 2a14361 | `GitFixture::cmd` and `gitconfig_env` set `GIT_CEILING_DIRECTORIES` to the temp dir | Holds. The full suite passes with TMPDIR inside a clone, and the revert control fails (truth 15). The ceiling is `std::env::temp_dir()`, so discovery stops below the clone root |

### Deferred Items

| # | Item | Addressed In | Evidence |
|---|------|-------------|----------|
| 1 | Windows runtime: JobObject kill, cache-lock release, state.json replace | Phase 999.1 (backlog) | Operator waiver 2026-10-20; Windows-target clippy passes at HEAD (check only) |
| 2 | Home-path placeholder followed by `, ; . ) ] } =` (W2, latent) | Phase 2 | Phase 2 goal |
| 3 | Kept files publishable by a plain push (`.gitignore` without a template) | Phase 4 | Operator-accepted 2026-10-19; pending todo |
| 4 | Re-init side effects (REINIT WR-02, WR-03, IN-01) | Phase 4 | deferred-items.md |
| 5 | "No shared template" judged against local, not remote, templates (IN-04) | Phase 4 | 01-REVIEW-DISPOSITION.md |

### Required Artifacts

`verify.artifacts`: 32/32 across the 11 plans, every plan exit 0.

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/process.rs` | Bounded runner, env scrub, redaction | ✓ VERIFIED | unchanged |
| `src/sync/staging.rs` | Stage, validate, commit | ✓ VERIFIED | unchanged |
| `src/sync/state.rs` | v2 schema, tombstones, conflicts, staged save | ✓ VERIFIED | unchanged |
| `src/sync/merge.rs` | diffy diff3, sidecar renderer | ✓ VERIFIED | unchanged |
| `src/sync/mod.rs` | Deletion states, conflicts, resolve, push skips, continuity, lock, remote-root guard | ✓ VERIFIED | +29/-3 since 5f5e1f7 (template check; fixture fence); diff read in full |
| `src/main.rs`, `src/cli/mod.rs` | Dispatch, exit codes, template-root guard, Resolve | ✓ VERIFIED | unchanged |
| `tests/e2e_lifecycle.rs` | Hermetic e2e coverage | ✓ VERIFIED | 67 e2e tests; the remote-refusal test now loops over two foreign repos |

### Key Link Verification

`verify.key-links` returned exit 0 for 01-01 (3/3) and exit 1 for 01-02 through 01-11. As before, every unverified link fails with "Source file not found (from: must be a relative file path)", because the plans write `from:` as a description such as "src/sync/mod.rs scan_project". That is a parser mismatch, not missing wiring. I recounted the link patterns at HEAD with `rg`, and all are present: in src/sync/mod.rs `process::run_bounded` 2, `GitFixture` 17, `with_extra_env` 13, `Self::fingerprint\(` 7, `stage_to_dir` 3, `validate_safe_write_target` 5, `stage_delete` 8, `name-status` 3, `merge::merge_texts` 1, `render_conflict_sidecar` 1, `ConflictPending` 14, `resolve_conflict` 3, `clear_conflict` 2, `skipped_merged` 6, `fetched_sha` 25, `UpstreamMoved` 5; in src/main.rs `std::process::exit\(EXIT_` 38. These counts show only that the symbols are present. The behaviour is carried by the passing tests named in the truths table.

### Data-Flow Trace (Level 4)

Not applicable: a CLI with no rendered UI.

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Full suite, TMPDIR inside a throwaway clone of HEAD (run once) | `env -u TYPESAFE_API_KEY -u GIT_DIR ... TMPDIR=<clone>/tmp cargo test --workspace` (cwd = clone) | 137 + 67 passed, exit 0; clone status/index/HEAD unchanged | ✓ PASS |
| Revert control, fixture fence | the 2 ceiling lines removed; the 2 named unit tests run | both FAILED, exit 101 | ✓ control bites |
| Revert control, remote check | old `projects/` check restored; the e2e refusal test run | FAILED: `own-projects status: exit Some(0)` | ✓ control bites |
| Lint and format | `cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --check` | exit 0, exit 0 (log shows `Checking ude`) | ✓ PASS |
| Windows target | `cargo clippy --target x86_64-pc-windows-msvc --workspace --all-targets -- -D warnings` | exit 0 (check only; nothing linked or run) | ✓ PASS |
| Named-test lookups in the saved log | `rg` for the named tests | all `ok`; a made-up name reads MISSING (control) | ✓ PASS |
| Debt markers in lines added since 5f5e1f7 | `rg 'TBD\|FIXME\|XXX\|TODO\|HACK\|todo!(\|unimplemented!(\|placeholder'` | none | ✓ PASS |
| Disabled tests | `rg '#\[ignore' src tests` | none | ✓ PASS |

### Probe Execution

No `scripts/*/tests/probe-*.sh` exist and no plan declares one, so Step 7c does not apply. My ad hoc probes ran the HEAD binary built from the throwaway clone, in scratch dirs outside this checkout. They used a fake HOME, XDG dirs and global git config, set `GIT_CEILING_DIRECTORIES` to the probe root, and unset the repo-local git vars and `TYPESAFE_API_KEY`.

| # | Remote | Result | Verdict |
|---|--------|--------|---------|
| V | full `templates/` copy (control) | status 11 In Sync; pull 0; push "No changes"; project and remote unchanged | ✓ |
| P1 | `projects/generic/gitconfig.repo` is a directory | status/pull/push exit 2, "no template for any managed file"; unchanged | ✓ |
| P2 | only `projects/generic/hooks/pre-commit` | status 0: 10 Deleted upstream, 1 In Sync; pull 0 deletes 10 unedited files | ℹ️ by design (Read this first, item 1) |
| P3 | `projects/generic/gitconfig.repo` symlink to /etc/hostname | exit 2 "Refusing to follow symlink"; project `.gitconfig` still the template content | ✓ fails closed |
| P4 | `templates/README` plus root `projects/generic/gitconfig.repo` | exit 2 on all three; unchanged | ✓ (message is accurate for the `templates/` root it checks) |
| L | push with 4 s pre-receive | `flock -n` 77 during, 0 after; 1 `/proc/locks` entry; push 0 | ✓ |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| SYNC-01 | 01-07 | Tombstones; deletions propagate without resurrection | ✓ SATISFIED | Truths 1, 10, 11, 12 |
| SYNC-02 | 01-04, 01-06 | Two-phase staging | ✓ SATISFIED | Truths 2, 7 |
| SYNC-03 | 01-01, 01-02, 01-03 | Bounded git/gh timeouts | ✓ SATISFIED (Linux) | Truths 3, 8, 15; Windows deferred to 999.1 |
| SYNC-04 | 01-05 | CRLF-normalized fingerprints | ✓ SATISFIED | Truth 4 |
| SYNC-05 | 01-08, 01-09, 01-10 | `.conflict` files; local edits preserved | ✓ SATISFIED | Truth 5 |
| SYNC-06 | 01-10, 01-11 | Safe push candidates; continuity | ✓ SATISFIED | Truth 6 |

All six IDs are claimed in plan `requirements:` fields. REQUIREMENTS.md maps exactly these six to Phase 1 (traceability rows 70-75). None is orphaned.

**Prohibitions.** All five are test-tier and marked `status: resolved`. Each has enforcement tests that pass at HEAD, so none is flagged:
- **Never widen a synced file's mode:** `test_staged_write_modes`, `test_state_save_mode`.
- **Never destroy local edits without resolve:** `test_pull_deletion_conflicts_write_sidecars`.
- **A sidecar is never wider than its file:** `test_sidecar_mode_follows_local`.
- **Never publish unresolved or merged-unpulled content:** `test_push_skips_conflicts_and_merged`, `test_sync_push_skips_unresolved_conflict_exit_1`.
- **Never force-push:** `test_force_does_not_bypass_upstream_moved`.

### Decision Coverage

`check decision-coverage-verify <phase_dir> <phase_dir>/01-CONTEXT.md --raw`: 18/18 honored, `not_honored: []`. Control: the one-argument form returns `skipped: true, reason: "CONTEXT.md missing"`. So the previous report's "missing" came from the invocation, not from the phase.

### Test Quality Audit

| Test File | Linked Req | Active | Skipped | Circular | Assertion Level | Verdict |
|-----------|-----------|--------|---------|----------|-----------------|---------|
| src/**/*.rs (unit) | SYNC-01..06 | 137 | 0 | none found | Value and behavioral | OK |
| tests/e2e_lifecycle.rs | SYNC-01..06 | 67 | 0 | none found | Behavioral (exit code, file bytes, state.json, remote ref) | OK |

The remote-refusal test checks `.gitconfig` bytes and the remote ref, not every managed file. That is adequate, because the refusal happens before any scan. The revert control shows the test catches the old behaviour.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| src/sync/mod.rs | `ensure_remote_repo` `has_template` | Accepts a remote that holds any one managed template | ℹ️ Info | Probe P2: the other 10 files read Deleted upstream and pull deletes the unedited ones. Matches the phase's deletion rule and the single-template e2e fixtures. A foreign repo needs an exact ude template path to reach it |
| src/main.rs | `find_templates_dir` | Falls back to the compile-time `CARGO_MANIFEST_DIR/templates` | ℹ️ Info | The e2e suite cannot reach the "no templates" exit path |
| src/sync/mod.rs | `scan_project` `?` on symlink refusal | One symlinked managed file fails every sync command (01-REVIEW WR-01) | ⚠️ Warning | Fails closed. Carried forward; operator deferred it to a todo |
| (plans) | files_modified | `.cargo/mutants.toml` is listed by plans 01-05/07/08/11 but is not tracked | ℹ️ Info | No must-have depends on it |

### Human Verification Required

None. No truth is behavior-unverified or abstained. The Windows checks are operator-waived and backlogged as Phase 999.1.

### Gaps Summary

No blocking gaps. All six roadmap criteria and the plan and review must-haves hold at HEAD 3cbbe4f on Linux. Both commits since 5f5e1f7 do what their messages claim, and each fix has a revert control that turns its test red. Both warnings in the previous report are closed.

What this verification does not establish:
- **Windows:** compile and clippy only.
- **A crash during the rename phase:** there is no fsync and no journal (D-01).
- **A remote that holds only part of the templates:** files it lacks are deleted on pull (probe P2, by design).
- **Kept `.gitignore` publication:** deferred to Phase 4.
- **Dubious-ownership parent repos:** untested (needs a second uid).
- **The lock observation:** one run on one host, 2 s into a push only.
- **The pre-push hook:** not re-run. scripts/hooks/pre-push is unchanged since 5f5e1f7, where it was exercised offline.

---

_Verified: 2026-10-21T20:18:38Z_
_Verifier: Claude (gsd-verifier)_
