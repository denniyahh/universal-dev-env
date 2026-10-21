---
phase: "01"
slug: "sync-engine-state-hermeticity"
status: verified
# threats_open = count of OPEN threats at or above workflow.security_block_on severity (the blocking gate)
threats_open: 0
asvs_level: 1
created: "2026-10-19"
---

# Phase 01 — Security

> Per-phase security contract: threat register, accepted risks, and audit trail.
> Register authored at plan time (all 11 PLAN files carry a `<threat_model>`). Verified by gsd-security-auditor at HEAD e8c8e95, ASVS L1, block_on: high.

---

## Trust Boundaries

| Boundary | Description | Data Crossing |
|----------|-------------|---------------|
| Caller / test environment -> git spawns | Inherited GIT_DIR, GIT_INDEX_FILE, GIT_CONFIG etc. could redirect git into another repository | repository paths, commits |
| UDE -> git/gh children and remote | Children inherit env; network peers, hooks and credential prompts can stall | process lifetime, credentials |
| git stderr / remote URL -> terminal and agent logs | Remote URLs may embed tokens | credentials |
| .ude/state.json on disk -> sync engine | Untrusted: corrupt, truncated, hand-edited or newer-version files | sync ledger |
| Project / upstream working trees -> engine writes and deletions | Planted symlinks, directories where files are expected, restricted modes; deletions must never destroy local work | managed file content |
| Local file content -> `.conflict` sidecars and git index | Sidecars copy local content, which may contain secrets | file content |
| Local working tree -> upstream publication | Push decides what leaves the machine; the publication gate is the last check | file content, secrets |
| Cache checkout -> remote branch / push output | Shared branch pushed concurrently; porcelain output decides race vs failure | branch history |
| CLI argument `<file>` -> filesystem and state | User- or agent-supplied path for `ude sync resolve` | paths |

---

## Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation | Status |
|-----------|----------|-----------|----------|-------------|------------|--------|
| T-01-01 | D | src/process.rs run_bounded | high | mitigate | Deadline poll with ProcessGroup::leader / JobObject group kill on timeout; test_timeout_kills_group asserts < 3 s against a 5 s grandchild and that the grandchild is dead afterwards (the liveness check is what distinguishes a group kill from a leader-only kill); reader threads are joined only within KILL_GRACE after the kill and are otherwise detached, so a descendant that escaped the group with setsid cannot hold the call open (test_pipe_holder_outside_group_timeout_path and _post_exit_path assert < 5 s (about 1.3 s when correct) against a live 10 s outside holder) | closed |
| T-01-02 | I | run_bounded error messages | medium | mitigate | Messages built from the caller's static label only; test_spawn_failure_names_label asserts a `user:secret@` argument never appears | closed |
| T-01-03 | T | prepare_command | high | mitigate | env_remove of all 15 `git rev-parse --local-env-vars` names on every spawn; test_env_scrub structural and behavioural checks with negative control | closed |
| T-01-04 | D | run_bounded on Windows (JobObject) | low | accept | Compile-checked only (cargo check / clippy for x86_64-pc-windows-msvc pass on this host since 2026-10-20); never run; runtime checks waived by the operator and backlogged as Phase 999.1; Linux is the operator's platform | closed (accepted) |
| T-01-05 | T | sync and e2e test fixtures | medium | mitigate | GitFixture env_removes the 15 local variables and pins GIT_CONFIG_GLOBAL / GIT_CONFIG_NOSYSTEM, so a fixture can never write into the host checkout or read the host identity; test_git_fixture_scrubs_local_env asserts it with a negative control | closed |
| T-01-06 | T | SyncEngine git spawns | high | mitigate | Every spawn through run_bounded's 15-variable scrub; e2e test_sync_ignores_inherited_git_dir proves a decoy is byte-identical after status and push; hermeticity gate on the unit suite | closed |
| T-01-07 | I | clone progress line, git stderr excerpts, main.rs remote URL lines | medium | mitigate | process::redact_userinfo on every printed URL and on stderr_excerpt; labels never contain argv; test_redact_userinfo with negative control | closed |
| T-01-08 | D | git clone / fetch / push / gh in sync | high | mitigate | Network tier 60 s with process-group kill; test_push_timeout_hanging_hook proves cancellation of a real hook process tree | closed |
| T-01-09 | D | RemoteLockGuard | medium | mitigate | std File::try_lock (flock(2)); kernel releases on holder death and children do not inherit it, proven by test_remote_lock_released_when_holder_killed and test_remote_lock_not_inherited_by_children | closed |
| T-01-10 | T | ProjectState::load_from_dir | high | mitigate | Corrupt or unreadable state returns Err and exits 2 (D-06); never silently reset; e2e test_sync_corrupt_state_fails_loudly asserts exit 2 and byte-identical file | closed |
| T-01-11 | T | state.json version handling (downgrade/upgrade) | medium | mitigate | Forward guard refuses version 0 and versions above CURRENT_STATE_VERSION; operator confirmed (2026-10-17) no older ude builds are in use, so old-binary behaviour is an accepted limitation, not a task | closed |
| T-01-12 | T | ProjectState::save_to_dir / stage_to_dir | medium | mitigate | NamedTempFile in .ude/ + persist (rename) so readers see the old or the new file, never a partial one; validate_safe_write_target on `.ude/state.json` at stage time and before persist, so a symlinked `.ude` or state.json cannot redirect the write outside the project; test_state_save_atomic, test_state_save_refuses_symlinked_ude_dir | closed |
| T-01-13 | T | SyncEngine::fingerprint | low | mitigate | Only CRLF pairs are normalized and only for valid UTF-8; binary and lone CR stay distinct (test_fingerprint negative controls), so two genuinely different files never share a fingerprint by normalization | closed |
| T-01-14 | T | version 1 -> 2 migration | medium | mitigate | Exact re-hash from stored content; hash-only records adopt the new fingerprint only when the old raw hash matches the local file; keys with non-normal components are never read (test_migrate_v1_unsafe_key_ignored) | closed |
| T-01-15 | T/E | StagedTransaction::stage_write / commit | high | mitigate | validate_safe_write_target at stage time and again immediately before persist; rename replaces a symlink instead of following it; test_commit_revalidates_symlink_before_rename and test_pull_updates_refuses_to_write_through_symlink | closed |
| T-01-16 | I | staged file modes | medium | mitigate | file_mode_for keeps the existing mode (so 0o600 stays 0o600), adds execute only where read is already granted on hook/script paths (0o644 hook becomes 0o755, 0o700 hook stays 0o700), and never leaves the 0o600 tempfile default on new files; test_staged_write_modes | closed |
| T-01-17 | T | pull_updates / push_updates write phase | high | mitigate | All candidates and state.json staged and validated before any rename; staging failure changes nothing; test_pull_staging_failure_no_partial_writes, test_push_staging_failure_no_partial_writes | closed |
| T-01-18 | T | rename phase partial failure | low | accept | D-01 excludes a journal and fsync; the error reports how many files were replaced; staging-time checks push predictable failures before any rename | closed (accepted) |
| T-01-19 | T | pull UpstreamDeleted handling | high | mitigate | Delete only when local fingerprint equals the ancestor (D-12); edited copies are Conflicted (D-13) and untouched; test_pull_upstream_deletion negative control | closed |
| T-01-20 | I/T | publication gate (staged + committed) | high | mitigate | --name-status --no-renames; allowlist every entry; secret scan every non-D blob; malformed lines fail closed; test_push_to_remote_deletion plus the existing secret-gate tests | closed |
| T-01-21 | T | StagedTransaction::stage_delete | high | mitigate | validate_safe_write_target before staging and before removal; only regular files are deletable (no directories, no symlinks) | closed |
| T-01-22 | T | push of tombstoned paths | medium | mitigate | Unmanaged status is never published; test_tombstone_prevents_republish | closed |
| T-01-23 | T | relabel_conflict_markers | low | mitigate | Rewrites only exact full marker lines, so content resembling a marker is never altered (test_relabel_conflict_markers negative control) | closed |
| T-01-24 | I | `.conflict` sidecar | medium | mitigate | `*.conflict` in the managed gitignore (D-11); sidecar mode follows the local file with execute bits cleared, so a 0o600 file never yields a world-readable sidecar (test_sidecar_mode_follows_local) | closed |
| T-01-25 | T/E | sidecar staging | high | mitigate | Sidecars go through stage_write, i.e. validate_safe_write_target at stage time and before persist; test_pull_refuses_symlinked_conflict_sidecar | closed |
| T-01-26 | T | conflicted local files | high | mitigate | Pull never writes a conflicted local file or advances its ancestor; tests assert byte-identical local files in every conflict case | closed |
| T-01-27 | T/E | SyncEngine::resolve_conflict path handling | high | mitigate | Only Component::Normal paths in MANAGED_FILES with an open conflict; sidecar validated with validate_safe_write_target and removed before state is committed, so a symlinked or non-regular sidecar is refused with the conflict still open; test_resolve_rejects_invalid_targets and test_resolve_sidecar_failure_keeps_conflict assert state unchanged | closed |
| T-01-28 | T/I | push_updates candidate selection | high | mitigate | Conflicted, ConflictPending and Merged files are skipped (D-10, D-17); test_push_skips_conflicts_and_merged asserts upstream untouched for them | closed |
| T-01-29 | E | `--force` flag | medium | mitigate | force only feeds allow_agent_rules; no force parameter reaches push_updates or push_to_remote (automated source check); e2e asserts --force still exits 1 and does not publish the conflicted file | closed |
| T-01-30 | T | push_to_remote direct mode | high | mitigate | Plain push only (no force or lease option, source-checked); the server's fast-forward check is the atomic compare-and-swap; test_direct_push_aborts_when_upstream_moved and the --force e2e test | closed |
| T-01-31 | R | cache commit after a failed publish | medium | mitigate | rollback_commit on UpstreamMoved and on continuity violation (tests assert cache HEAD). Any other direct-push failure (remote hook rejection, timeout, network error) leaves the unpublished cache commit in place until the next `ude sync` command: every remote-mode sync action calls ensure_remote_repo before touching the cache (src/main.rs Sync arm, plan-time lines 370-384), and its fetch path runs `git checkout -B <branch> origin/<branch>` then `git reset --hard origin/<branch>` (plan-time src/sync/mod.rs:458-476, verified at revision 2026-10-18), which discards it before any template is read or published. Accepted residual: the commit can sit in the cache between commands; it is never read, published, or recorded as an ancestor | closed |
| T-01-32 | T | classify_push_porcelain | low | mitigate | Classify only on the `!` flag field plus `[rejected]` prefix; `[remote rejected]` and anything else are hard failures; test_classify_push_porcelain negative control | closed |
| T-01-SC (01-01) | T | cargo install of diffy =0.5.2 | high | mitigate | Package Legitimacy Audit in 01-RESEARCH.md rates diffy OK (bmwill/diffy, ~375k downloads/week, first published 2020); exact pin `=0.5.2` because tests assert diffy's label text; similar is removed; `cargo deny check advisories bans sources` and a cargo-machete check gate Task 1; no [ASSUMED]/[SUS] packages, so no blocking checkpoint Audit note: the audit found a caret requirement; pinned to `=10.0.1` in a53aecd. | closed |
| T-01-SC (01-08) | T | cargo install of process-wrap 10.0.1 | high | mitigate | Package Legitimacy Audit in 01-RESEARCH.md rates process-wrap OK (watchexec, ~831k downloads/week); exact version pinned; `cargo deny check advisories bans sources` and a cargo-machete unused-dependency check gate Task 1; no [ASSUMED]/[SUS] packages, so no blocking checkpoint is required | closed |
| T-01-SC (01-02..01-07, 01-09..01-11) | T | package installs | low | accept | These 9 plans install no packages | closed (accepted) |

*Status: open · closed · open — below high threshold (non-blocking)*
*Severity: critical > high > medium > low — only open threats at or above workflow.security_block_on count toward threats_open*
*Disposition: mitigate (implementation required) · accept (documented risk) · transfer (third-party)*

Per-threat evidence (file:line and pinning test for each closed row) is in the auditor's verdict summarized in the audit trail below.

---

## Accepted Risks Log

| Risk ID | Threat Ref | Rationale | Accepted By | Date |
|---------|------------|-----------|-------------|------|
| AR-01-01 | T-01-04 | Windows Job Object kill branch (`src/process.rs` JobObject path) cannot be compiled or run on this Linux-only host; manual verification remains open in `.planning/WINDOWS.md` and `01-VALIDATION.md` Manual-Only | Plan-time disposition (01-01-PLAN threat model); auditor confirmed rationale in code | 2026-10-19 |
| AR-01-02 | T-01-18 | A failure between two renames in the staged commit can leave a partial update (no fsync, per D-01); the error reports how many files were already replaced (`src/sync/staging.rs`) | Plan-time disposition (01-06-PLAN threat model, D-01); auditor confirmed rationale in code | 2026-10-19 |
| AR-01-03 | T-01-SC | Nine plans install no packages | Plan-time disposition | 2026-10-19 |

*Accepted risks do not resurface in future audit runs.*

---

## Known Issues Outside the Register

- **Root-layout remote push silently lost edits (pre-existing, not introduced by Phase 01) — fixed.** For a remote with no `templates/` directory, `src/main.rs` derives the cache checkout as `tmpl_dir.parent()`, but `ensure_remote_repo` returns the checkout root itself; `stage_publication` then treats the failed `git status` (empty stdout) as "No changes to push", returns Ok, and the ancestor advances. Reproduced by the auditor through the binary: push exits 0 publishing nothing, the next pull overwrites the local edit. Negative control (remote with `templates/`) publishes and keeps the edit. Disposition: **resolved 2026-10-19** (operator chose fix-before-verification). 40c9c90/05546a5: `RemoteCache` carries `checkout_dir` and push uses it; e2e `test_sync_push_root_layout_remote_publishes_and_pull_keeps_edit` (templates/ control + root layout). cbd4877/c737383: a failed `git status` or `git diff --cached` in `stage_publication` is a hard error (exit 2); `test_stage_publication_fails_when_git_status_fails`. Untested: other remote layouts, `--pr` with a root-layout remote, the diff --cached failure branch.

- **Init symlink pre-flight is not race-safe (found 2026-10-21, outside the register: init writes were not in the Phase 01 threat model) — deferred to Phase 2.** Init refuses symlinked destinations before writing, but its later writes follow links, so a symlink planted in the project while init runs is written through. Host-reproduced at c868d1b: a FIFO template stalls init after the pre-flight, `.agents/AGENTS.md -> <outside file>` is planted, and the outside file is overwritten; control without the symlink leaves it unchanged. Requires a concurrent process that can write into the project; a committed symlink is still refused. Severity medium (below block_on: high). Operator decision 2026-10-21: Phase 2 todo `2026-10-21-make-init-and-sync-writes-refuse-a-symlink-created-mid-run.md`; the code comment now states the limit (ea87575). Sync's staged writes probably share the window between validation and rename (not checked).
- **Unmanaged files could still be published or deleted (found 2026-10-21 by the external adversarial review; relates to T-01-22) — fixed.** An unreadable committed file with no template skipped its kept tombstone, so push published it; a user's own uncommitted file with no template was recorded as an ancestor, so pull deleted it. b5c11a3: init stops with exit 2 on the unreadable file and tombstones the user's file; e2e `test_init_refuses_unreadable_committed_file_without_template` and `test_init_leaves_uncommitted_file_without_template_unmanaged` (controls: readable file → Unmanaged; template present → In Sync). Still open, tracked as a todo: an uncommitted `.gitignore` with no template is publishable.

- **A missing or foreign template root made every managed file read as deleted upstream (found 2026-10-21 by the code review and the re-verification) — fixed.** Init with no ude templates found recorded the user's files as managed; sync without a remote fell back to a nonexistent path or took a project's own `templates/` (Django, Flask, Helm); a `--remote` that is not a ude template repository did the same. In each case pull deleted every unedited managed file with exit 0. 464bfc4 (local: a templates dir counts only with `projects/generic`, init and sync exit 2 when none is found) and 8349e70 + 577c520 (remote: `ensure_remote_repo` refuses a repo that holds no template for any managed file; 8349e70 alone checked only for `projects/`, which a foreign repo with its own `projects/` passed); e2e `test_sync_ignores_a_project_templates_dir_that_is_not_ude`, `test_sync_refuses_a_remote_that_is_not_a_template_repo`. The no-templates init path is verified only with a templates-hidden scratch build (the test binary always finds the repo's templates).
- **A thin remote made pull delete files it never deleted (found 2026-10-21 by the re-verification) — fixed.** A remote holding at least one template passed 577c520, and every template it lacked read as deleted upstream. Operator decision: confirm deletions from history. 20cf077 checks each upstream deletion against the cache checkout's history (`git log --diff-filter=D`, unshallowing the depth-1 clone first); a template the remote never deleted reads Missing upstream and is kept. e2e `test_pull_deletes_only_templates_the_remote_history_deleted` (control: a real deletion commit behind a later commit is still applied). d875afe closes two gaps the next re-verification found: a deletion made while resolving a merge conflict (merges are diffed against their first parent; `test_pull_confirms_a_deletion_made_while_resolving_a_merge`) and a locally edited file whose template the remote never had, which read as a deletion conflict (`test_edited_file_with_template_the_remote_never_had_is_missing_upstream`). 539cbb7 gives push the same check, so push no longer skips such a file as an unresolved conflict that resolve cannot clear (the next re-verification's gap).
- **TypeSafe API key on curl's command line (found 2026-10-21 by the re-verification) — fixed.** The bearer token was a `-H` argument, readable by other local users in the process list. 03fcc78 sends header and body as a curl config on stdin; e2e `test_typesafe_key_is_not_passed_on_the_curl_command_line`, unit `test_curl_config_round_trips_through_curl` (real curl, loopback; fails with escaping removed).

---

## Security Audit Trail

| Audit Date | Threats Total | Closed | Open | Run By |
|------------|---------------|--------|------|--------|
| 2026-10-19 | 43 | 43 | 0 | gsd-security-auditor (ASVS L1) via /gsd-execute-phase verify:post |

---

## Sign-Off

- [x] All threats have a disposition (mitigate / accept / transfer)
- [x] Accepted risks documented in Accepted Risks Log
- [x] `threats_open: 0` confirmed
- [x] `status: verified` set in frontmatter

**Approval:** verified 2026-10-19; re-audited 2026-10-21 (register unchanged, threats_open 0; two findings outside the register recorded above)

## Security Audit 2026-10-21

| Metric | Count |
|---|---|
| Threats found | 43 |
| Closed | 43 |
| Open | 0 |
