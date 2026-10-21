---
phase: 01-sync-engine-state-hermeticity
verified: 2026-10-21T18:15:30Z
status: passed
score: 15/15 must-haves verified
covered_files: [".config/nextest.toml", ".planning/phases/01-sync-engine-state-hermeticity/01-01-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-01-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-02-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-02-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-03-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-03-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-04-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-04-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-05-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-05-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-06-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-06-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-07-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-07-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-08-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-08-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-09-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-09-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-10-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-10-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-11-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-11-SUMMARY.md", "Cargo.lock", "Cargo.toml", "scripts/hooks/pre-push", "src/audit/mod.rs", "src/cli/mod.rs", "src/main.rs", "src/process.rs", "src/sanitizer/mod.rs", "src/semantic/mod.rs", "src/sync/merge.rs", "src/sync/mod.rs", "src/sync/staging.rs", "src/sync/state.rs", "src/template/mod.rs", "templates/projects/generic/gitignore.repo", "tests/e2e_lifecycle.rs"]
covered_digest: "v3:sha256:de06f9bebc9095eeb239374b0599fc6053e56c9e70ec13d9866b49f21eeabb72"
behavior_unverified: 0
overrides_applied: 1
overrides:
  - must_have: "01-01 backstop: deadline boundary is inclusive (elapsed >= timeout kills the group; a child that exits before the poll observes the deadline returns its real status; POLL_INTERVAL = 10 ms)"
    reason: "Accepted on the evidence: run_bounded checks child.try_wait() before the `Instant::now() >= deadline` kill and returns the real status on Ok(Some), POLL_INTERVAL is Duration::from_millis(10), and the timeout tests pass. A test pinned to the exact millisecond boundary would itself be flaky."
    accepted_by: "orchestrator ruling (not the operator), on the implementation read plus passing timeout tests"
    accepted_at: "2026-10-20T00:33:36Z"
re_verification:
  previous_status: passed
  previous_score: 12/12
  previous_head: 539a3d7
  gaps_closed: []
  gaps_remaining: []
  regressions: []
deferred:
  - truth: "Windows runtime behaviour of the bounded runner (JobObject kill), the remote cache lock (release on process death) and staged state.json replace"
    addressed_in: "Phase 999.1 (backlog)"
    evidence: "Phase 999.1 goal: 'On a Windows host, run the three checks waived for phase 01 on 2026-10-20 (operator) ...'. 01-UAT.md tests 1-3 skipped under the operator waiver. clippy -D warnings for x86_64-pc-windows-msvc re-run at HEAD 3fa02f4 on Linux: exit 0 (nothing linked or run)"
  - truth: "Push must not leak the pusher's home path when a {{ user.home }} / {{ user.xdg_config }} placeholder is followed by , ; . ) ] } or = (re-review W2; latent)"
    addressed_in: "Phase 2"
    evidence: "Phase 2 goal: 'Screen and redact all confidential tokens, raw credentials, and user-specific paths before any remote git staging or push.'"
  - truth: "A file that ude init kept must not be publishable by a plain push. Re-probed at HEAD: an uncommitted own .gitignore with no gitignore template still reads Locally Modified and push publishes it as a new gitignore.repo (probe A). The new unmanaged-marker path (b5c11a3) covers every other managed file but not this one, because a pre-existing .gitignore takes the kept branch, which records nothing when the template is absent"
    addressed_in: "Phase 4"
    evidence: "Phase 4 goal: '... safe re-initialization protections.' Operator accepted for phase 01 on 2026-10-19; pending todo 2026-10-19-never-publish-files-that-init-kept-over-the-shared-template"
  - truth: "Re-init side effects (01-REVIEW-REINIT WR-02, WR-03, IN-01): explicit --type change not applied to mise.toml; update-index stages an uncommitted hook edit; lost exec bit not restored"
    addressed_in: "Phase 4"
    evidence: "deferred-items.md ('candidate for Phase 4, safe re-initialization'); Phase 4 SC2 covers re-init on an existing workspace"
  - truth: "Init judges 'no shared template' against the local templates, not the remote that sync uses (01-REVIEW IN-04, unverified)"
    addressed_in: "Phase 4"
    evidence: "01-REVIEW-DISPOSITION.md IN-04 deferred: 'init vs remote template root belongs with .ude.toml / central_repo work (Phase 4)'; Phase 4 SC4 '.ude.toml is parsed and establishes deterministic precedence'"
---

# Phase 1: Sync Engine & State Hermeticity Verification Report

**Phase Goal:** Ensure bidirectional synchronization is hermetic, crash-resilient, and atomic without phantom drift or zombie file resurrections.
**Verified:** 2026-10-21T18:15:30Z, at HEAD 3fa02f4 (Linux).
**Status:** passed. Windows runtime is deferred to backlog Phase 999.1 under the operator's waiver.
**Re-verification:** Yes. The previous report (passed 12/12 at 539a3d7) went stale when four files changed: src/main.rs, src/cli/mod.rs, tests/e2e_lifecycle.rs and scripts/hooks/pre-push. Nothing under src/sync/, src/process.rs, src/template/, src/audit/, src/sanitizer/, Cargo.toml or templates/ changed since 539a3d7 (`git diff --stat` is empty for those paths). I re-checked every truth against HEAD. I did not copy the old verdicts.

## Read this first: what argues against "passed"

1. **A `--remote` that is not a ude template repository deletes every unedited managed file, and pull exits 0** (probe F). This is a new finding, not covered by this phase's tests. It is the remote-mode twin of review finding CR-02, which was fixed only for local mode. When the cache checkout has no `templates/`, `ensure_remote_repo` uses the checkout root as the template root (src/sync/mod.rs, `templates_dir = ... else target_checkout.clone()`), so every managed file reads Deleted upstream.
   - Edited files are kept: `.gitconfig` became a conflict with a sidecar.
   - A later pull from the correct remote restores the deleted files (probe F2).
   - Push to the wrong remote published nothing.
   - I rate it a warning, not a blocker. The user has to name the wrong remote, no edited content is lost, and the deletion follows the phase's own rule that a missing upstream template is a deletion.
2. **An uncommitted own `.gitignore` with no template is still published by push** (probe A). The new unmanaged path does not cover this one file. The operator already deferred this case to Phase 4, so it stays in `deferred`.
3. **The CR-01/CR-02 "exit 2 when no templates directory is found" path has no automated test.** The dev binary always falls back to the `CARGO_MANIFEST_DIR` templates. I proved the path with a scratch build of src/ that had no templates directory, against a control run (probe D). A regression would not turn the suite red.

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | SC1: An upstream template deletion propagates locally on pull via tombstones and does not come back on later pushes | ✓ VERIFIED | `test_pull_upstream_deletion`, `test_push_local_deletion`, `test_both_deleted`, `test_sync_status_deletions`, `test_unreadable_upstream_is_not_a_deletion`, `test_fresh_clone_does_not_resurrect_template_deleted_upstream` pass. Probe F2: tombstoned paths come back only when upstream re-adds them |
| 2 | SC2: Writes are staged in a shadow step before the atomic update, with no partial writes on error | ✓ VERIFIED | `test_pull_staging_failure_no_partial_writes`, `test_push_staging_failure_no_partial_writes`, `test_state_save_atomic`, `test_commit_revalidates_symlink_before_rename` pass. staging.rs and state.rs are unchanged |
| 3 | SC3: git/gh calls have bounded timeouts and a hung child is killed | ✓ VERIFIED (Linux) | `test_timeout_kills_group`, `test_pipe_holder_outside_group_*`, `test_push_timeout_hanging_hook`, `test_drains_large_output`, `test_env_scrub` pass. The two new production git spawns (`committed_paths`, `recognised_repository`, src/main.rs) go through `process::run_bounded` with `LOCAL_TIMEOUT`. Every other production `Command::new("git")` site is unchanged. Windows is deferred to 999.1 |
| 4 | SC4: CRLF-normalized fingerprints give no false drift | ✓ VERIFIED | 5 crlf, 4 eol and 5 migration tests pass, including `test_sync_status_crlf_only_change_in_sync`. Probe B: a template that reappears equal except for CRLF reads In Sync |
| 5 | SC5: Divergent edits produce `.conflict` files and keep local changes | ✓ VERIFIED | `test_sync_pull_conflict_exits_1_and_writes_sidecar`, `test_sync_resolve_end_to_end`, `test_pull_deletion_conflicts_write_sidecars`, `test_sidecar_mode_follows_local` pass. Probe B: a template that reappears with different content gives Conflicted; pull exits 1, writes a sidecar and leaves the local file intact; push exits 1 and leaves upstream unchanged |
| 6 | SC6: Push verifies branch continuity and rejects divergent candidates | ✓ VERIFIED | `test_direct_push_aborts_when_upstream_moved`, `test_push_continuity_violation_is_hard_failure`, `test_force_does_not_bypass_upstream_moved`, `test_classify_push_porcelain(_prop)`, `test_push_skips_conflicts_and_merged` pass |
| 7 | 01-06 backstop: in remote mode the cache lock serializes commands for the whole command | ✓ VERIFIED (Linux, observed) | Probe 4: while `ude sync push --remote` sat in a 4 s pre-receive hook (its last step), `flock -n <lock>` exited 1. After push exited 0, `flock -n` exited 0 (the control). The status and pull arms bind the guard to the named `_remote_cache`, not `_`. Lock unit tests pass |
| 8 | 01-01 backstop: the deadline boundary is inclusive, with a 10 ms poll | PASSED (override) | Override carried forward: accepted by an orchestrator ruling, 2026-10-20. Re-read at HEAD (process.rs unchanged): `try_wait()` comes before the `Instant::now() >= deadline` kill; `POLL_INTERVAL = 10 ms` |
| 9 | Re-init keeps every prior record (files, tombstones, open conflicts) | ✓ VERIFIED | The `test_reinit_*` tests pass. The init loop still runs `continue` for every path with a prior record |
| 10 | A committed file that init keeps with no template is never deleted by pull or published by push | ✓ VERIFIED | `test_init_keeps_committed_gitignore_when_template_is_absent`, `test_fresh_clone_does_not_resurrect_template_deleted_upstream` pass |
| 11 | A user's own uncommitted file with no template is left Unmanaged, and status says "no shared template" (b5c11a3, c868d1b) | ✓ VERIFIED (except `.gitignore`, deferred) | `test_init_leaves_uncommitted_file_without_template_unmanaged` passes, with `test_init_writes_template_over_uncommitted_file_when_template_exists` as its control. Probe B (`.gitconfig`): Unmanaged with the new wording; pull keeps the file; push publishes nothing. Probe A: `.gitignore` is the exception (see the deferred list) |
| 12 | A missing or foreign local `templates/` is never treated as upstream; init and sync exit 2 (464bfc4) | ✓ VERIFIED | `test_sync_ignores_a_project_templates_dir_that_is_not_ude` passes. Probe D, with a templates-less build: init exit 2 with nothing written; status exit 2; pull exit 2 with managed files present; a foreign `templates/foo` also gives exit 2. Control (same build, real tree in cwd): init 0 and 11 In Sync |
| 13 | Init fails closed: an unreadable committed file with no template, a state.json that cannot be saved, or a parent repo git recognises but cannot read gives exit 2; a parent `.git` git does not recognise is ignored (bd08717, 6b0a2d3, 3249d70, 7e44cf1) | ✓ VERIFIED | `test_init_refuses_unreadable_committed_file_without_template`, `test_init_fails_when_state_cannot_be_saved`, `test_init_fails_when_parent_repo_index_is_unreadable`, `test_init_ignores_unusable_git_dir_above_project`, `test_init_fails_on_unusable_git_dir_in_project` pass. The unreadable and unsaveable tests check that the fixture really is unreadable or unwritable. Probes C2 (corrupt parent index): exit 2. Probe C1 (broken parent gitfile): warns, then proceeds |
| 14 | Init records the rendered project type; `--type` is validated; `--mempalace=false` and `--graphify=false` work (db2e1a8, 8b41fcb) | ✓ VERIFIED | `test_init_records_the_detected_type_for_generic`, `test_init_rejects_an_unknown_type` (with the control `node` → 0), `test_init_can_skip_mempalace_and_graphify` pass |
| 15 | Hermetic tests and runtime: no `set_var`/`remove_var`; fixtures go through scrubbed builders; an inherited `GIT_DIR` is ignored | ✓ VERIFIED | `rg set_var\|remove_var src tests` finds nothing. `test_sync_ignores_inherited_git_dir` and `test_env_scrub` pass. The e2e `sync_cli` helper now sets `GIT_CEILING_DIRECTORIES` (9bd2777). Its limits are in the Anti-Patterns table |

**Score:** 15/15 truths verified (1 by override; 0 present-but-behavior-unverified)

#### Probes against the HEAD 3fa02f4 binary

Setup: scratch dirs under the session scratchpad (outside this checkout). Fake HOME, XDG and global git config; `GIT_CEILING_DIRECTORIES` set to the scratch root; repo-local git vars and `TYPESAFE_API_KEY` unset. A copy of `templates/` sat in each project's cwd, so a local push wrote only to the copy. After every probe, `git status --short` showed no changes beyond the pre-existing untracked `.planning` entries.

| # | Scenario | Result | Verdict |
|---|----------|--------|---------|
| A | No git; uncommitted own `.gitignore`; `gitignore.repo` removed | Locally Modified; push exit 0, "1 file(s)", template created | deferred (Phase 4, operator-accepted) |
| B | No git; uncommitted own `.gitconfig`; `gitconfig.repo` removed | Init: "Kept existing .gitconfig ... unmanaged until one is published". Status Unmanaged (no shared template). Pull 0, kept. Push 0, no template created | ✓ |
| B2 | ...the template reappears equal except for CRLF | In Sync | ✓ no phantom drift |
| B3 | ...the template reappears with different content | Conflicted; pull 1, local kept, sidecar written; push 1, upstream unchanged | ✓ |
| C1 | Parent dir holds a `.git` gitfile pointing at a missing gitdir | Warning "not treating the project as a git checkout"; init 0 | ✓ as designed (see Anti-Patterns) |
| C2 | Parent repo with a corrupt index | init 2 | ✓ |
| D | Templates-less build: init, status, pull; foreign `templates/` | 2, 2, 2 (files intact), 2. Control: init 0, status 11 In Sync | ✓ |
| F | `--remote` = a git repo with only README | Status: every file Deleted upstream. Pull removes every unedited managed file | ⚠️ warning (see top) |
| F2 | Edited `.gitconfig`, then pull from the foreign remote, then pull from the right remote | Edited file kept (conflict). Deleted files restored by the right pull. Push to the foreign remote: "No changes" | ✓ recoverable |
| 4 | Lock held during push | `flock -n` 1 during, 0 after | ✓ |

### Deferred Items

| # | Item | Addressed In | Evidence |
|---|------|-------------|----------|
| 1 | Windows runtime: JobObject kill, cache-lock release, state.json replace | Phase 999.1 (backlog) | Operator waiver 2026-10-20; Windows-target clippy passes at HEAD (check only) |
| 2 | Home-path placeholder followed by `, ; . ) ] } =` (W2, latent) | Phase 2 | Phase 2 goal |
| 3 | Kept files publishable by a plain push (probe A: `.gitignore` without a template) | Phase 4 | Operator-accepted 2026-10-19; pending todo |
| 4 | Re-init side effects (REINIT WR-02, WR-03, IN-01) | Phase 4 | deferred-items.md |
| 5 | "No shared template" judged against local, not remote, templates (IN-04) | Phase 4 | 01-REVIEW-DISPOSITION.md |

### Required Artifacts

`verify.artifacts`: 32/32 across the 11 plans, all with exit 0.

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/process.rs` | Bounded runner, env scrub, redaction | ✓ VERIFIED | unchanged since 539a3d7 |
| `src/sync/staging.rs` | Stage, validate, commit | ✓ VERIFIED | unchanged |
| `src/sync/state.rs` | v2 schema, tombstones, conflicts, staged save | ✓ VERIFIED | unchanged |
| `src/sync/merge.rs` | diffy diff3, sidecar renderer | ✓ VERIFIED | unchanged |
| `src/sync/mod.rs` | Deletion states, conflicts, resolve, push skips, continuity, lock | ✓ VERIFIED | unchanged |
| `src/main.rs` | Init state recording, exit codes, template-root guard | ✓ VERIFIED | +126/-52 since 539a3d7; read in full for the changed hunks |
| `src/cli/mod.rs` | `SyncAction::Resolve`, `--type` value parser, bool flags | ✓ VERIFIED | |
| `templates/projects/generic/gitignore.repo` | `*.conflict`, `.ude/state.json` | ✓ VERIFIED | unchanged |
| `tests/e2e_lifecycle.rs` | Hermetic e2e coverage | ✓ VERIFIED | 64 e2e tests pass (53 before, 11 new) |

### Key Link Verification

`verify.key-links` returned exit 1 for plans 01-02 through 01-11. Every unverified link failed with "Source file not found": the plans write `from:` as a description (for example "src/sync/mod.rs scan_project"), not a bare path. That is a parser mismatch, not missing wiring. I checked each link's `pattern` by hand with `rg` in the named file. All are present:

| From | To | Pattern | Count |
|------|----|---------|-------|
| src/sync/mod.rs | process.rs | `process::run_bounded` | 2 |
| src/sync/mod.rs tests | SyncEngine::with_extra_env | `with_extra_env` | 13 |
| scan_project | load_from_dir | `load_from_dir\(.*\)\?` | 2 |
| main.rs dispatch | exit codes | `std::process::exit\(EXIT_` | 38 |
| scan_project | fingerprint | `Self::fingerprint\(` | 7 |
| state.rs migration | fingerprint | `fingerprint\(` | 6 |
| main.rs init | fingerprint | `SyncEngine::fingerprint` | 3 |
| pull_updates | staging | `stage_to_dir` / `stage_delete` | 3 / 8 |
| publication gate | git diff | `name-status` | 3 |
| classifier | merge | `merge::merge_texts` | 1 |
| pull_updates | sidecar renderer | `render_conflict_sidecar` | 1 |
| main.rs | resolve / EXIT_DRIFT / fetched_sha / remove_file | each pattern | 1 / 4 / 3 / 1 |
| sync | clear_conflict / skipped_merged / UpstreamMoved | each pattern | 2 / 6 / 5 |
| staging.rs | validate_safe_write_target | pattern | 4 |

Behaviour behind these links is covered by the named passing tests in the truths table, not by the pattern counts.

### Data-Flow Trace (Level 4)

Not applicable: CLI with no rendered UI.

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Full suite (one run) | `env -u TYPESAFE_API_KEY TMPDIR=<scratchpad> cargo test --workspace` | 136 unit + 64 e2e passed, 0 failed, 0 ignored, exit 0 | ✓ PASS |
| Lint and format | `cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --check` | exit 0, exit 0 | ✓ PASS |
| Windows target | `cargo clippy --target x86_64-pc-windows-msvc --workspace --all-targets -- -D warnings` | exit 0 (cached check; nothing linked or run) | ✓ PASS |
| Test-name lookups in the saved log | `rg` for 16 named tests | all ok; a made-up name reads MISSING (control) | ✓ PASS |
| Debt markers in lines added since 539a3d7 | `rg TBD\|FIXME\|XXX\|TODO\|HACK\|todo!(\|unimplemented!(` | none | ✓ PASS |
| Disabled tests | `rg '#\[ignore\]'` | none | ✓ PASS |

### Probe Execution

No `scripts/*/tests/probe-*.sh` exist and no plan declares one. Step 7c does not apply. My ad hoc probes are listed above.

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| SYNC-01 | 01-07 | Tombstones; deletions propagate without resurrection | ✓ SATISFIED | Truths 1, 10, 11 |
| SYNC-02 | 01-04, 01-06 | Two-phase staging | ✓ SATISFIED | Truths 2, 7 |
| SYNC-03 | 01-01, 01-02, 01-03 | Bounded git/gh timeouts | ✓ SATISFIED (Linux) | Truths 3, 8, 15; Windows deferred to 999.1 |
| SYNC-04 | 01-05 | CRLF-normalized fingerprints | ✓ SATISFIED | Truth 4 |
| SYNC-05 | 01-08, 01-09, 01-10 | `.conflict` files; local edits preserved | ✓ SATISFIED | Truth 5 |
| SYNC-06 | 01-10, 01-11 | Safe push candidates; continuity | ✓ SATISFIED | Truth 6 |

All six IDs are claimed in plan `requirements:` fields, and REQUIREMENTS.md maps exactly these six to Phase 1. None is orphaned.

**Prohibitions.** All five are test-tier and marked `status: resolved`. Each has passing enforcement tests, so none is flagged:
- **Never widen a synced file's mode:** `test_staged_write_modes`, `test_state_save_mode`, `test_file_mode_for`.
- **Never destroy local edits without resolve:** `test_pull_deletion_conflicts_write_sidecars`, plus probe F2.
- **A sidecar is never wider than its file:** `test_sidecar_mode_follows_local`.
- **Never publish unresolved or merged-unpulled content:** `test_push_skips_conflicts_and_merged`, `test_sync_push_skips_unresolved_conflict_exit_1`.
- **Never force-push:** `test_force_does_not_bypass_upstream_moved`.

### Decision Coverage

All 18 trackable CONTEXT.md decisions are honored by shipped artifacts (`check.decision-coverage-verify`, exit 0).

### Test Quality Audit

| Test File | Linked Req | Active | Skipped | Circular | Assertion Level | Verdict |
|-----------|-----------|--------|---------|----------|-----------------|---------|
| src/process.rs, src/sync/*.rs (unit) | SYNC-01..06 | 136 | 0 | none found | Value and behavioral | OK |
| tests/e2e_lifecycle.rs | SYNC-01..06 | 64 | 0 | none found | Behavioral (exit code, file bytes, state.json) | OK |

- **Disabled tests on requirements:** 0.
- **Circular patterns:** 0.
- **Insufficient assertions:** 1 warning. No automated test covers the no-templates exit-2 path (see "Read this first", item 3).

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| src/sync/mod.rs | `ensure_remote_repo`, `templates_dir` fallback | A remote with no `templates/` counts as a template root, so a foreign remote reads as "everything deleted upstream" | ⚠️ Warning | Probe F. Unedited managed files are removed with exit 0. Recoverable; edited files conflict instead. Not tracked anywhere yet |
| tests/e2e_lifecycle.rs | `sync_cli_with_env` vs about 25 direct `ude_command(bin)` calls and `scrubbed_git` | `GIT_CEILING_DIRECTORIES` is set only in the `sync_cli` helper | ⚠️ Warning | IN-06's fence covers only `sync_cli` callers. With a TMPDIR inside a checkout, direct callers' init still walks up to the enclosing repo. Its upward git calls are read-only (`ls-files`, `rev-parse`; `update-index` runs only when the project has its own `.git`). I did not run this, following the operator's instruction |
| src/main.rs | `find_templates_dir` | Falls back to the compile-time `CARGO_MANIFEST_DIR/templates` | ℹ️ Info | The e2e suite cannot reach the "no templates" exit path; see probe D |
| src/main.rs | `committed_paths` / `recognised_repository` | A parent `.git` gitfile pointing at a missing gitdir counts as "no repo" (probe C1) | ℹ️ Info | Init would then overwrite uncommitted copies of templated files there. This is init's existing behaviour for non-git projects, and init/re-init safety is Phase 4 scope |
| src/sync/mod.rs | `scan_project` `?` on symlink refusal | One symlinked managed file fails every sync command (older 01-REVIEW WR-01) | ⚠️ Warning | Fails closed. Carried from the previous report; operator deferred it to a todo |
| src/semantic/mod.rs | curl spawn | The TypeSafe API key is passed in curl's argv (`-H Authorization: Bearer ...`) | ℹ️ Info | Visible to other local users via the process list while curl runs. Outside SYNC scope; I found no tracking for it in this phase's docs |
| scripts/hooks/pre-push | - | Blocks every push when the advisory database cannot be fetched (01-REVIEW WR-02) | ℹ️ Info | Still `open` in 01-REVIEW-DISPOSITION.md, operator decision pending. Not a sync-engine requirement. The only change since 539a3d7 is a comment |
| (plans) | files_modified | `.cargo/mutants.toml` is listed by plans 01-05/07/08/11 but is not tracked in git | ℹ️ Info | No must-have depends on it |

### Human Verification Required

None. This is an infrastructure phase with no behavior-unverified or abstained truths. The Windows checks are operator-waived and backlogged as Phase 999.1.

### Gaps Summary

No blocking gaps. All six roadmap criteria and the plan and review must-haves hold at HEAD 3fa02f4 on Linux. The post-verification init changes do what their commits claim. I checked each one with tests and probes, with negative controls where a control was cheap. Nothing in them regresses the sync engine, whose code is unchanged since 539a3d7.

What this does not establish:
- **Windows:** compile and clippy only.
- **A crash during the rename phase:** there is no fsync and no journal (D-01, AR-01-02).
- **A foreign or misconfigured `--remote`:** unedited files are deleted (warning above).
- **Kept `.gitignore` publication:** deferred to Phase 4.
- **Dubious-ownership parent repos:** untested (needs a second uid).
- **The lock observation (probe 4):** a single run on one host, during the publish step of push only. Status and pull rest on the named-guard binding plus the lock unit tests.

---

_Verified: 2026-10-21T18:15:30Z_
_Verifier: Claude (gsd-verifier)_
