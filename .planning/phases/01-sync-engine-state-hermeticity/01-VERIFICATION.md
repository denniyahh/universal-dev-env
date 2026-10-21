---
phase: 01-sync-engine-state-hermeticity
verified: 2026-10-21T23:09:19Z
status: passed
score: 18/18 must-haves verified
covered_files: [".config/nextest.toml", ".planning/phases/01-sync-engine-state-hermeticity/01-01-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-01-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-02-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-02-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-03-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-03-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-04-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-04-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-05-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-05-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-06-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-06-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-07-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-07-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-08-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-08-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-09-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-09-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-10-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-10-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-11-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-11-SUMMARY.md", "Cargo.lock", "Cargo.toml", "deny.toml", "scripts/hooks/pre-push", "src/audit/mod.rs", "src/cli/mod.rs", "src/main.rs", "src/process.rs", "src/sanitizer/mod.rs", "src/semantic/mod.rs", "src/sync/merge.rs", "src/sync/mod.rs", "src/sync/staging.rs", "src/sync/state.rs", "src/template/mod.rs", "templates/projects/generic/gitignore.repo", "tests/e2e_lifecycle.rs"]
covered_digest: "v3:sha256:326631ce6bcae00c62a1a18948fc917d5191d74d77b358d9bd9bb4965c72fed8"
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
  previous_head: 1e058a9
  gaps_closed: []
  gaps_remaining: []
  regressions: []
  previous_items_closed:
    - "Info 1 (push's edit-site template-logic message not pinned in remote mode; revert control D left the suite green): closed by 8853a50. Control D now fails test_push_names_the_shared_template_in_remote_mode at :1888 (the new block); control D2 (the remedy on that line too) fails at the same line; restored source passes"
deferred:
  - truth: "Windows runtime behaviour of the bounded runner (JobObject kill), the remote cache lock (release on process death) and staged state.json replace"
    addressed_in: "Phase 999.1 (backlog)"
    evidence: "ROADMAP.md 'Phase 999.1: Windows runtime checks for the phase 01 sync engine (BACKLOG)'; operator waiver 2026-10-20. clippy -D warnings for x86_64-pc-windows-msvc at 1e058a9: exit 0 (check only; nothing linked or run; not re-run at 8853a50)"
  - truth: "Push must not leak the pusher's home path when a {{ user.home }} / {{ user.xdg_config }} placeholder is followed by , ; . ) ] } or = (re-review W2; latent)"
    addressed_in: "Phase 2"
    evidence: "Phase 2 goal: 'Screen and redact all confidential tokens, raw credentials, and user-specific paths before any remote git staging or push.'"
  - truth: "A file that ude init kept must not be publishable by a plain push (an uncommitted own .gitignore with no gitignore template reads Locally Modified and push publishes it)"
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
**Verified:** 2026-10-21T23:09:19Z, at HEAD 8853a50 (Linux).
**Status:** passed. Windows runtime stays deferred to backlog Phase 999.1 under the operator's waiver.
**Re-verification:** Yes. Previous report: passed 18/18 at 1e058a9 (commit 7571031), stale only because 8853a50 changed tests/e2e_lifecycle.rs (+14/-0). `git diff --stat 1e058a9 HEAD -- src`: empty. `git diff --stat 1e058a9 HEAD` over the whole tree: only `01-VERIFICATION.md` and `tests/e2e_lifecycle.rs`. No product code, manifest, deny.toml, hook, template or plan changed, so the previous probes (P1, P1b, P7r, P8) and revert controls A, B, C and E are carried forward rather than re-run. Re-run this round: one full suite, two revert controls on the edit-site message, a restore run, clippy and fmt.

## Read this first: what argues against "passed"

1. **The push scan-failure test still runs in local mode** (carried forward). `test_scan_failure_is_not_reported_as_a_state_file_error` loops over push without `--remote`, so a failed history lookup (unshallow or `git log`) is not triggered at run time. By reading the code at 1e058a9: `never_deleted_upstream` is called with `?` inside `scan_project`, so its error reaches push's `ScanFailed` arm. Probe P7r (1e058a9) confirmed the remote-mode wording for a symlink scan failure.
2. **One symlinked managed file still stops every sync command** (01-REVIEW WR-01, carried forward, operator deferred to a todo). Unchanged; fails closed with exit 2.
3. **Most evidence below is carried forward from 1e058a9.** It still applies because no `src/` file changed, but the probes and controls A, B, C and E were not re-run against the 8853a50 build.

None of these breaks the phase goal: nothing is lost, published or reported as drift that does not exist.

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | SC1: an upstream template deletion propagates locally on pull via tombstones and does not come back on later pushes | ✓ VERIFIED | `test_pull_upstream_deletion`, `test_push_local_deletion`, `test_both_deleted`, `test_fresh_clone_does_not_resurrect_template_deleted_upstream`, `test_pull_deletes_only_templates_the_remote_history_deleted`, `test_pull_confirms_a_deletion_made_while_resolving_a_merge` pass at 8853a50 |
| 2 | SC2: writes are staged in a shadow step before the atomic update, with no partial writes on error | ✓ VERIFIED | `test_pull_staging_failure_no_partial_writes`, `test_push_staging_failure_no_partial_writes`, `test_state_save_atomic` pass |
| 3 | SC3: git/gh calls have bounded timeouts and a hung child is killed | ✓ VERIFIED (Linux) | `test_timeout_kills_group`, `test_push_timeout_hanging_hook`, `test_drains_large_output`, `test_env_scrub` pass. Windows deferred to 999.1 |
| 4 | SC4: CRLF-normalized fingerprints give no false drift | ✓ VERIFIED | `test_sync_status_crlf_only_change_in_sync` and the CRLF/EOL unit tests pass |
| 5 | SC5: divergent edits produce `.conflict` files and keep local changes | ✓ VERIFIED | `test_sync_pull_conflict_exits_1_and_writes_sidecar`, `test_sync_resolve_end_to_end`, `test_pull_deletion_conflicts_write_sidecars`, `test_sidecar_mode_follows_local` pass |
| 6 | SC6: push verifies branch continuity and rejects divergent candidates | ✓ VERIFIED | `test_direct_push_aborts_when_upstream_moved`, `test_push_continuity_violation_is_hard_failure`, `test_force_does_not_bypass_upstream_moved`, `test_push_skips_conflicts_and_merged`, `test_sync_push_skips_unresolved_conflict_exit_1` pass |
| 7 | 01-06 backstop: in remote mode the cache lock serializes commands for the whole command | ✓ VERIFIED (Linux) | `test_remote_lock_concurrency_serialization_and_timeout`, `test_remote_lock_released_when_holder_killed`, `test_remote_lock_not_inherited_by_children` pass |
| 8 | 01-01 backstop: the deadline boundary is inclusive, with a 10 ms poll | PASSED (override) | Operator-accepted 2026-10-21 (frontmatter). process.rs unchanged |
| 9 | Re-init keeps every prior record (files, tombstones, open conflicts) | ✓ VERIFIED | All 10 `test_reinit_*` e2e tests pass |
| 10 | A committed file that init keeps with no template is never deleted by pull or published by push | ✓ VERIFIED | `test_init_keeps_committed_gitignore_when_template_is_absent`, `test_fresh_clone_does_not_resurrect_template_deleted_upstream` pass |
| 11 | A user's own uncommitted file with no template is left Unmanaged, and status says "no shared template" | ✓ VERIFIED (except `.gitignore`, deferred to Phase 4) | `test_init_leaves_uncommitted_file_without_template_unmanaged` passes |
| 12 | A missing or foreign template root is never treated as upstream | ✓ VERIFIED | `test_sync_refuses_a_remote_that_is_not_a_template_repo`, `test_sync_ignores_a_project_templates_dir_that_is_not_ude`, the three `test_first_ude_templates_*` unit tests pass |
| 13 | Init fails closed on unreadable or unsaveable state and on a parent repo git recognises but cannot read | ✓ VERIFIED | `test_init_fails_when_parent_repo_index_is_unreadable` and related tests pass |
| 14 | Init records the rendered project type; `--type` is validated; `--mempalace=false` and `--graphify=false` work | ✓ VERIFIED | `test_init_rejects_an_unknown_type` and related tests pass |
| 15 | Hermetic tests and runtime: no `set_var`/`remove_var`; fixtures use scrubbed builders; inherited `GIT_DIR` ignored; git discovery fenced | ✓ VERIFIED | `rg -c 'set_var\|remove_var' src tests`: no match, exit 1 (control: `fn map_to_upstream` matches 1 in src/sync/mod.rs). `test_sync_ignores_inherited_git_dir` passes. Full suite in a throwaway clone under /var/tmp with TMPDIR outside the repo: 140 unit + 73 e2e passed, exit 0; clone `git status --short` empty and HEAD unchanged afterwards; the run's TMPDIR held 0 entries afterwards |
| 16 | D-14 amended: push never deletes a `.jinja` template that has template logic; status, pull and init agree; the remedy names the template in the shared templates | ✓ VERIFIED | `test_push_never_deletes_a_template_with_logic` and `test_push_names_the_shared_template_in_remote_mode` pass. Both push message sites are now pinned in remote mode: control C (deletion site, 1e058a9) fails at :1874; **control D** (edit-site skip line prints the raw template path, src/main.rs:1232) now fails at :1888, the new block's "not under the temp dir" assertion, after the deletion block passed; the control binary printed `mise.toml: upstream template <tmp>/cache/ude/remote-…-main/templates/projects/agents/mise.toml.jinja`. **Control D2** (also the remedy on that line, :1233) fails at :1888. Restored source: both tests pass after a rebuild. Probe P8 (1e058a9, clean binary) showed the shared path `projects/agents/mise.toml.jinja` |
| 17 | D-12 amended: with a remote, status and pull treat a template as deleted only when the cache checkout's history shows its deletion, including one made in a merge; one the remote never had reads Missing upstream and pull keeps the file and its record, edited or not; failure to confirm fails closed | ✓ VERIFIED | `test_pull_deletes_only_templates_the_remote_history_deleted`, `test_pull_confirms_a_deletion_made_while_resolving_a_merge`, `test_edited_file_with_template_the_remote_never_had_is_missing_upstream` pass. Probe P1 (1e058a9) carried forward. A failed lookup returns `PushResult::ScanFailed` from push (code read; see item 1 above) |
| 18 | An edited file whose template the remote never had is handled the same way by status, pull, push and resolve, and each command says what happens to it | ✓ VERIFIED | `test_edited_file_with_template_the_remote_never_had_is_missing_upstream` passes at 8853a50. Probes P1/P1b and controls A (fails at :1845) and E (fails at :1824) carried forward from 1e058a9; the code they exercise is unchanged |

**Score:** 18/18 truths verified (1 by override; 0 present-but-behavior-unverified; 0 failed)

#### Re-check of 8853a50's claim

| Claim | Result |
|-------|--------|
| "An edited mise.toml under a template with logic now asserts the shared path too; reverting that one message fails it" | Holds. Control D (only `shared_template_path(...)` on the edit-site skip line replaced by `template.display()`) fails the test at :1888, which is inside the new block, so the deletion-block assertions before it passed. The positive half of the new block (`contains("projects/agents/mise.toml.jinja")`) cannot catch this regression on its own, because the cache path also ends with that suffix; the negative assertion (`!contains(temp)`) is the one that bites, and it relies on the test's `XDG_CACHE_HOME` sitting under the test's temp dir (`sync_cli` sets it to `<temp>/cache`). `line_with` returns "" when no line matches, so a missing skip line also fails the positive assertion rather than passing silently |

#### Probes

Not re-run this round (test-only change; no `src/` file changed since 1e058a9). Results from the 1e058a9 report, on that clean binary:

| Probe | Setup | Result | Verdict |
|-------|-------|--------|---------|
| P1 | Thin remote (only `mise.toml.jinja`); `.gitconfig` edited | status "Missing upstream (… pull keeps it, push never publishes it)"; pull keeps it; push names it, exit 0; resolve exit 2; remote ref, file and state.json unchanged | ✓ |
| P1b | Same remote, `.gitconfig` unedited | status Missing upstream; push exit 0, no `.gitconfig` line | ✓ |
| P7r | Symlinked `.gitconfig`, remote mode | push exit 2, "could not compare the project with upstream: Refusing to follow symlink" | ✓ |
| P8 | Full remote, mise.toml edited | push names `projects/agents/mise.toml.jinja`, exit 1 | ✓ |

### Deferred Items

| # | Item | Addressed In | Evidence |
|---|------|-------------|----------|
| 1 | Windows runtime: JobObject kill, cache-lock release, state.json replace | Phase 999.1 (backlog) | Operator waiver 2026-10-20; Windows-target clippy passed at 1e058a9 (check only; not re-run at 8853a50) |
| 2 | Home-path placeholder followed by `, ; . ) ] } =` (W2, latent) | Phase 2 | Phase 2 goal |
| 3 | Kept files publishable by a plain push (`.gitignore` without a template) | Phase 4 | Operator-accepted 2026-10-19; pending todo |
| 4 | Re-init side effects (REINIT WR-02, WR-03, IN-01) | Phase 4 | deferred-items.md |
| 5 | "No shared template" judged against local, not remote, templates (IN-04) | Phase 4 | 01-REVIEW-DISPOSITION.md |

### Advisory (New Scope, Unevidenced)

None. No new finding this round.

### Required Artifacts

`verify.artifacts` at 1e058a9: 32/32 across the 11 plans, `all_passed: true`. No plan or listed artifact path changed since; carried forward.

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/process.rs` | Bounded runner, env scrub, redaction | ✓ VERIFIED | unchanged since 1e058a9 |
| `src/sync/staging.rs` | Stage, validate, commit | ✓ VERIFIED | unchanged |
| `src/sync/state.rs` | v2 schema, tombstones, conflicts, staged save | ✓ VERIFIED | unchanged |
| `src/sync/merge.rs` | diffy diff3, sidecar renderer | ✓ VERIFIED | unchanged |
| `src/sync/mod.rs` | Deletion states, conflicts, resolve, push skips, continuity, lock, history-confirmed deletion | ✓ VERIFIED | unchanged |
| `src/main.rs` | Dispatch, exit codes, status/pull/push wording | ✓ VERIFIED | unchanged |
| `tests/e2e_lifecycle.rs` | Hermetic e2e coverage | ✓ VERIFIED | 73 e2e tests; one extended by 8853a50 (+14 lines) |

### Key Link Verification

Carried forward from 1e058a9 (no source file changed): 01-01 `verify.key-links` 3/3; 01-02 to 01-11 fail only on the tool's "from must be a relative file path" parser mismatch; manual count of all 20 declared patterns in their declared files non-zero, made-up pattern 0 (control). Presence only; behaviour is carried by the tests and controls.

### Data-Flow Trace (Level 4)

Not applicable: a CLI with no rendered UI.

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Full suite, throwaway clone of 8853a50 under /var/tmp, TMPDIR outside the repo (run once) | `env -u TYPESAFE_API_KEY -u GIT_DIR … TMPDIR=<scratch>/tmp cargo test --workspace --no-fail-fast` | 140 + 73 passed, 0 failed, 0 ignored, exit 0; clone clean, HEAD unchanged | ✓ PASS |
| Named-test lookups in the saved log | `rg` for 28 named tests plus 10 `test_reinit_*` | all `ok`; a made-up name reads MISSING (control). Tests named in the truths but not looked up individually are covered by the run's 0 failed, 0 ignored | ✓ PASS |
| Control D: edit-site skip line prints the raw template path (main.rs:1232 only) | the remote-mode test and the logic-template test, forced rebuild (`Compiling ude` in log) | remote-mode test FAILED at :1888; logic-template test passed | ✓ bites |
| Control D2: D plus the remedy on that line (main.rs:1233) | same | FAILED at :1888; passed | ✓ bites |
| Restore after controls | `git checkout -- src`; `git diff --quiet`; same two tests, rebuilt | clean; both pass | ✓ |
| Controls A, B, C, E | (1e058a9) | fail at :1845, :3387, :1874, :1824 | carried forward |
| Lint and format | `cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --check` on the clean clone | exit 0 (log shows `Checking ude`), exit 0 | ✓ PASS |
| Debt markers, disabled tests, panicking calls in the 14 added lines | `rg 'TBD\|FIXME\|XXX\|TODO\|HACK\|#\[ignore\|unwrap\('` on the 1e058a9..8853a50 additions | none | ✓ PASS |

### Probe Execution

No `scripts/*/tests/probe-*.sh` exist and no plan declares one, so Step 7c does not apply.

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| SYNC-01 | 01-07 | Tombstones; deletions propagate without resurrection | ✓ SATISFIED | Truths 1, 10, 11, 12, 16, 17, 18 |
| SYNC-02 | 01-04, 01-06 | Two-phase staging | ✓ SATISFIED | Truths 2, 7 |
| SYNC-03 | 01-01, 01-02, 01-03 | Bounded git/gh timeouts | ✓ SATISFIED (Linux) | Truths 3, 8, 15; Windows deferred to 999.1 |
| SYNC-04 | 01-05 | CRLF-normalized fingerprints | ✓ SATISFIED | Truth 4 |
| SYNC-05 | 01-08, 01-09, 01-10 | `.conflict` files; local edits preserved | ✓ SATISFIED | Truth 5 |
| SYNC-06 | 01-10, 01-11 | Safe push candidates; continuity | ✓ SATISFIED | Truths 6, 18 |

All six IDs are claimed in plan `requirements:` fields (re-extracted per plan at HEAD). REQUIREMENTS.md maps exactly these six to Phase 1 (traceability rows 70-75). None is orphaned.

**Prohibitions.** The five test-tier items are all `status: resolved` with passing enforcement tests at 8853a50, so none is flagged. Nothing was found broken. (The two `backstop` items are truths 7 and 8.)
- **Never widen a synced file's mode:** `test_staged_write_modes`, `test_state_save_mode`.
- **Never destroy local edits without resolve:** `test_pull_deletion_conflicts_write_sidecars`.
- **A sidecar is never wider than its file:** `test_sidecar_mode_follows_local`.
- **Never publish unresolved or merged-unpulled content:** `test_push_skips_conflicts_and_merged`, `test_sync_push_skips_unresolved_conflict_exit_1`.
- **Never force-push:** `test_force_does_not_bypass_upstream_moved`.

### Decision Coverage

Carried forward from 1e058a9 (CONTEXT and artifacts unchanged): `check.decision-coverage-verify` 18/18 honored. The tool matches decision text to artifacts; it does not test behaviour.

### Test Quality Audit

| Test File | Linked Req | Active | Skipped | Circular | Assertion Level | Verdict |
|-----------|-----------|--------|---------|----------|-----------------|---------|
| src/**/*.rs (unit) | SYNC-01..06 | 140 | 0 | none found | Value and behavioral | OK |
| tests/e2e_lifecycle.rs | SYNC-01..06 | 73 | 0 | none found | Behavioral (exit code, output lines, file bytes, state.json, remote ref) | OK. Thin spots: push scan failure exercised only in local mode; the Missing-upstream test does not run resolve (checked by probe at 1e058a9) |

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| src/sync/mod.rs | `scan_project` `?` on symlink refusal | One symlinked managed file fails every sync command (01-REVIEW WR-01) | ⚠️ Warning | Fails closed, exit 2. Carried forward; operator deferred to a todo |
| src/sync/mod.rs | `never_deleted_upstream` | A template added and deleted only on a side branch reads Deleted upstream | ℹ️ Info | Carried forward; matches "history shows its deletion" |
| src/sync/mod.rs | `never_deleted_upstream` | One full-history `git log` per missing template, serial | ℹ️ Info | Carried forward; not re-timed |
| (plans) | files_modified | `.cargo/mutants.toml` is listed by plans 01-05/07/08/11 but not tracked | ℹ️ Info | Carried forward; no must-have depends on it |

Closed since the previous report: push's edit-site template-logic message unpinned in remote mode (Info), by 8853a50.

### Human Verification Required

None. No truth is behavior-unverified or abstained. The Windows checks are operator-waived and backlogged as Phase 999.1.

### Gaps Summary

No gaps. 8853a50 does what it claims: the remote-mode test now covers push's edited-file message as well as the deletion message, and reverting the edited-file message alone fails it. Every truth holds at 8853a50 on Linux, with a fresh full-suite run.

What this verification does not establish:
- **Probes and controls A, B, C, E were not re-run at 8853a50.** They stand on "no `src/` change since 1e058a9".
- **Windows:** compile and clippy only, last run at 1e058a9.
- **A crash during the rename phase:** there is no fsync and no journal (D-01).
- **A failed history lookup inside push:** read in the code (routes to `ScanFailed`, exit 2); not triggered at run time.
- **`push --pr` on a Missing-upstream file:** not run (needs `gh`).
- **Push as the first command on a fresh shallow cache, the lock observation and the pre-push hook:** not re-run; their code, scripts/hooks/pre-push and deny.toml are unchanged.
- **The new assertion's bite depends on the fixture's cache living under the test temp dir.** If a later change moves `XDG_CACHE_HOME` out of the temp dir, control D would pass again.
- **Dubious-ownership parent repos:** untested (needs a second uid).

---

_Verified: 2026-10-21T23:09:19Z_
_Verifier: Claude (gsd-verifier)_
