---
phase: "01"
slug: "sync-engine-state-hermeticity"
# status lifecycle: draft (seeded by plan-phase) → validated (set by validate-phase §6)
# audit-milestone §5.5 distinguishes NOT-VALIDATED (draft) from PARTIAL (validated + nyquist_compliant: false) (#2117)
status: validated
nyquist_compliant: true
wave_0_complete: true
created: "2026-10-16"
---

# Phase 01 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.
> Source: `01-RESEARCH.md` § Validation Architecture.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in test harness (`cargo test`); unit tests in `#[cfg(test)]` modules of the `ude` bin target, plus `tests/e2e_lifecycle.rs` |
| **Config file** | none — no `src/lib.rs`, so `cargo test --lib` errors; use `cargo test --bin ude` |
| **Quick run command** | `cargo test --bin ude sync:: && cargo test --bin ude process::` |
| **Full suite command** | `cargo test && cargo clippy --workspace --all-targets -- -D warnings` |
| **Hermeticity gate** | `GIT_DIR=<decoy bare repo> cargo test --bin ude sync::` must pass (6 failures at research baseline) |
| **Estimated runtime** | ~5 seconds after build (quick); full suite longer |

---

## Sampling Rate

- **After every task commit:** Run `cargo test --bin ude sync:: && cargo test --bin ude process::` plus `cargo clippy --workspace --all-targets -- -D warnings`
- **After every plan wave:** Run `cargo test`, then the hermeticity gate with a freshly initialized bare decoy repo as `GIT_DIR`
- **Before `/gsd-verify-work`:** Full suite + clippy + hermeticity gate must be green
- **Max feedback latency:** 30 seconds (post-build)

---

## Per-Task Verification Map

Seeded at requirement level from RESEARCH.md; task IDs bound from the planner's mapping (2026-10-17). Test names/commands here are the research seed — each PLAN task's `<automated>` command is authoritative where they differ. Every row carries a negative control (NC) that must produce the opposite result.

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 01-07-T1 | 07 | 6 | SYNC-01 | — | Upstream delete + unchanged local → removed + tombstone. NC: local edit → Conflicted, bytes unchanged | unit | `cargo test --bin ude sync::tests::test_pull_upstream_deletion` | ✅ | ✅ green |
| 01-07-T3 | 07 | 6 | SYNC-01 | — | Tombstone blocks republish. NC: fresh state → LocallyModified | unit | `cargo test --bin ude sync::tests::test_tombstone_prevents_republish` | ✅ | ✅ green |
| 01-07-T3 | 07 | 6 | SYNC-01 | — | Publication gate accepts `D` entries, still scans blobs. NC: secret-gate tests stay green | unit | `cargo test --bin ude sync::tests::test_push_to_remote_deletion` | ✅ | ✅ green |
| 01-06-T1 | 06 | 5 | SYNC-02 | — | Staging failure → targets + state.json byte-identical, no `.tmp*`. NC: no collision → all applied | unit | `cargo test --bin ude sync::tests::test_pull_staging_failure_no_partial_writes` | ✅ | ✅ green |
| 01-04-T1 | 04 | 3 | SYNC-02 | — | Corrupt/future-version state.json → Err. NC: missing file → Ok(default) | unit | `cargo test --bin ude sync::state::tests::test_load_` | ✅ | ✅ green |
| 01-06-T2 | 06 | 5 | SYNC-02 | — | Staged writes keep modes (0755 hooks, 0644 existing). NC: not 0600 | unit (unix) | `cargo test --bin ude sync::tests::test_staged_write_modes` | ✅ | ✅ green |
| 01-06-T2 (+01-09-T2 sidecar) | 06, 09 | 5, 8 | SYNC-02 | — | Symlinked target/sidecar refused. NC: regular path succeeds | unit | `cargo test --bin ude sync::tests::test_pull_updates_refuses_to_write_through_symlink` | ✅ | ✅ green |
| 01-01-T1 | 01 | 1 | SYNC-03 | — | Timeout kills the whole process group (grandchild dead, < 3s; plan 01-01 T1 is authoritative). NC: `echo ok` → Ok | unit | `cargo test --bin ude process::tests::test_timeout_kills_group` | ✅ | ✅ green |
| 01-01-T1 | 01 | 1 | SYNC-03 | — | No pipe deadlock on 1 MiB stdout+stderr. NC: 0-byte case | unit | `cargo test --bin ude process::tests::test_drains_large_output` | ✅ | ✅ green |
| 01-01-T1 | 01 | 1 | SYNC-03 | — | Git env scrubbed, `GIT_TERMINAL_PROMPT=0`. NC: raw Command lacks entries | unit | `cargo test --bin ude process::tests::test_env_scrub` | ✅ | ✅ green |
| 01-03-T2 | 03 | 2 | SYNC-03 | — | Hanging pre-receive hook → timeout, ref unchanged. NC: hook exit 0 → success | unit | `cargo test --bin ude sync::tests::test_push_timeout_hanging_hook` | ✅ | ✅ green |
| 01-03-T1/T2 | 03 | 2 | SYNC-03 | — | Inherited GIT_DIR ignored by `ude`. NC: plain git resolves to decoy | e2e | `cargo test --test e2e_lifecycle test_sync_ignores_inherited_git_dir` | ✅ | ✅ green |
| 01-05-T1 | 05 | 4 | SYNC-04 | — | Fingerprint CRLF == LF. NC: different text differs; invalid UTF-8 untouched | unit | `cargo test --bin ude sync::tests::test_fingerprint` | ✅ | ✅ green |
| 01-05-T1 | 05 | 4 | SYNC-04 | — | CRLF-only difference → InSync. NC: real edit → LocallyModified | unit | `cargo test --bin ude sync::tests::test_scan_crlf_only_difference_in_sync` | ✅ | ✅ green |
| 01-05-T2 | 05 | 4 | SYNC-04 | — | v1 → v2 migration. NC: mismatched old hash stays modified | unit | `cargo test --bin ude sync::state::tests::test_migrate_v1` | ✅ | ✅ green |
| 01-09-T1 | 09 | 8 | SYNC-05 | — | Multiple conflicts → LOCAL/ANCESTOR/UPSTREAM sidecars, clean file applied. NC: clean pull → zero `*.conflict` | unit | `cargo test --bin ude sync::tests::test_pull_multiple_conflicts_sidecars` | ✅ | ✅ green |
| 01-08-T2 | 08 | 7 | SYNC-05 | — | Adjacent edits conflict. NC: one-line separation merges | unit | `cargo test --bin ude sync::merge::tests::test_adjacent_edits_conflict` | ✅ | ✅ green |
| 01-10-T3 | 10 | 9 | SYNC-05 | — | `resolve` clears conflict. NC: hand-deleted sidecar → push still skips | unit | `cargo test --bin ude sync::tests::test_resolve` | ✅ | ✅ green |
| 01-11-T1 | 11 | 10 | SYNC-06 | — | Upstream moved → UpstreamMoved, remote unchanged. NC: no race → success | unit | `cargo test --bin ude sync::tests::test_direct_push_aborts_when_upstream_moved` | ✅ | ✅ green |
| 01-11-T3 | 11 | 10 | SYNC-06 | — | `--force` does not bypass upstream-moved abort. NC: no race → succeeds | e2e | `cargo test --test e2e_lifecycle test_force_does_not_bypass_upstream_moved` | ✅ | ✅ green |
| post-verify 539a3d7 | — | — | SYNC-01 | — | Fresh clone does not resurrect a template deleted upstream (kept file Unmanaged, never republished). NC: pre-commit hook with a template stays In Sync | e2e | `cargo test --test e2e_lifecycle test_fresh_clone_does_not_resurrect_template_deleted_upstream` | ✅ | ✅ green |
| post-verify 539a3d7 | — | — | SYNC-01 | — | Committed file with no template: pull keeps it, push publishes nothing. NC: uncommitted .gitignore records nothing (`test_init_keeps_uncommitted_gitignore_when_template_is_absent`) | e2e | `cargo test --test e2e_lifecycle test_init_keeps_committed_gitignore_when_template_is_absent` | ✅ | ✅ green |
| post-review b5c11a3 | — | — | SYNC-01 | — | User's own file with no template left Unmanaged, not deleted by pull. NC: template present → init writes it, In Sync | e2e | `cargo test --test e2e_lifecycle test_init_leaves_uncommitted_file_without_template_unmanaged` | ✅ | ✅ green |
| post-review b5c11a3 | — | — | SYNC-01 | — | Unreadable committed file with no template stops init (exit 2), no state recorded. NC: readable file → Unmanaged | e2e (unix) | `cargo test --test e2e_lifecycle test_init_refuses_unreadable_committed_file_without_template` | ✅ | ✅ green |
| post-review 464bfc4 | — | — | SYNC-01 | — | A project's own templates/ is not taken as upstream; pull deletes nothing. NC: no templates/ → 11 In Sync | e2e | `cargo test --test e2e_lifecycle test_sync_ignores_a_project_templates_dir_that_is_not_ude` | ✅ | ✅ green |
| post-verify 8349e70, 577c520 | — | — | SYNC-01 | — | A remote with no template for any managed file (README-only, or its own projects/) is refused (status/pull/push exit 2), files and remote unchanged. NC: template remotes in the root-layout test still publish | e2e | `cargo test --test e2e_lifecycle test_sync_refuses_a_remote_that_is_not_a_template_repo` | ✅ | ✅ green |
| post-verify 20cf077 | — | — | SYNC-01 | — | Pull deletes only templates the remote's history deleted; a template the remote never had reads Missing upstream and is kept. NC: a deletion commit behind a later commit (shallow file:// clone) is applied | e2e | `cargo test --test e2e_lifecycle test_pull_deletes_only_templates_the_remote_history_deleted` | ✅ | ✅ green |
| post-review 65b9931 | — | — | SYNC-01 | — | Push never deletes a template that has logic (mise.toml); status and init agree. NC: .gitconfig's template is still deleted | e2e | `cargo test --test e2e_lifecycle test_push_never_deletes_a_template_with_logic` | ✅ | ✅ green |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [x] `src/sync/mod.rs` tests + `tests/e2e_lifecycle.rs` — `git_fixture(dir)` helper scrubbing the 15 local git env vars, `GIT_CONFIG_GLOBAL=<tempdir>/gitconfig`, `GIT_CONFIG_NOSYSTEM=1`; all existing git-fixture tests use it
- [x] `SyncEngine` extra-env injection so in-process tests avoid `std::env::set_var`
- [x] `src/process.rs` — new file with `#[cfg(test)]` module
- [x] Rewrite the 5 tests encoding replaced behavior (`test_merge_orthogonal_changes_clean`, `test_multi_pull_merge_idempotence`, `test_pull_updates_conflict_blocked_no_partial_writes`, `test_push_updates_blocked_on_conflict`, `test_sync_status_deletions`) as red tests before implementation
- [x] No framework install needed

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Windows `JobObject` kill branch (01-01-T2) | SYNC-03 | Compiles and passes clippy for x86_64-pc-windows-msvc on this host (check only); the kill/cleanup behavior needs a Windows host to run | Run WINDOWS.md checks on a Windows host (backlog Phase 999.1) |

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers all MISSING references
- [x] No watch-mode flags
- [x] Feedback latency < 30s
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** re-validated 2026-10-21 at c868d1b+ (20/20 original rows green, 8 post-verify/post-review rows added and green; hermeticity gate 109/109 with decoy GIT_DIR, decoy untouched, control: plain git resolves the decoy). Previously validated 2026-10-19 (validate-phase audit: 20/20 rows matched to passing tests; hermeticity gate 104/104 with decoy GIT_DIR, decoy untouched; Windows JobObject branch remains manual-only)

## Validation Audit 2026-10-19

| Metric | Count |
|---|---|
| Gaps found | 0 |
| Resolved | 0 |
| Escalated | 0 |
