## CONFIRMED

### 1. Plan 01-02 automated verification check hard-breaks on unconverted `tests/e2e_lifecycle.rs` calls
- **Evidence**:
  - `01-02-PLAN.md:128-129`: `<automated>bash -c '... test "$(rg -c "Command::new\(\"git\"\)" tests/e2e_lifecycle.rs)" = 1 && ...'`
  - `01-02-PLAN.md:134`: `<acceptance_criteria>- In the sync test module exactly 2 git Command constructions remain...; in tests/e2e_lifecycle.rs exactly 1 (its GitFixture::cmd)`
  - `01-02-PLAN.md:121`: Action restricts conversion to: *"seeding in test_sync_push_pr_does_not_advance_ancestor_state... and test_sync_push_remote_failure_does_not_advance_ancestor_state"*
  - `tests/e2e_lifecycle.rs:99, 104, 193, 212`: Four other direct `Command::new("git")` calls remain in doctor test functions (`test_cli_doctor_fix_creates_and_tracks_allowed_signers` and `test_cli_doctor_catches_all_missing_configurations`).
- **Reproduction/Impact**: Converting only the two specified seeding blocks leaves 4 doctor commands plus `GitFixture::cmd` (5 total). The verification command runs `test "5" = 1`, exits 1, and halts the automated executor on Wave 2. Furthermore, lines 99–108 set `commit.gpgsign = true` in `fake_home`; converting them to `GitFixture` would redirect `GIT_CONFIG_GLOBAL` to `GitFixture`'s tempdir where `gpgsign=false`, breaking `test_cli_doctor_fix_creates_and_tracks_allowed_signers`.

---

### 2. Plan 01-09 conflict auto-clear on convergence is dead code due to `scan_project` classification
- **Evidence**:
  - `01-09-PLAN.md:170`: *"In scan_project, when a conflict record exists for the path: if local and upstream are both present with equal fingerprints, or both absent, the file is converged and is classified as if it had no conflict record; otherwise its status is ConflictPending."*
  - `01-09-PLAN.md:160`: *"after a conflicted pull, the user edits the local file to equal upstream; scan reports InSync; pull removes the record and the sidecar, sets the ancestor to the upstream fingerprint... and lists the file in report.converged."*
  - `src/sync/mod.rs:500-525`: `pull_updates` iterates over `states: Vec<FileSyncState>` returned by `scan_project`.
- **Reproduction/Impact**: When `scan_project` classifies a converged file "as if it had no conflict record", it assigns `SyncStatus::InSync` (for present/equal) or `SyncStatus::BothDeleted` (for absent/absent). `pull_updates` skips all `InSync` files with `continue`, and `FileSyncState` does not store whether an entry previously had an open conflict. `pull_updates` therefore never executes convergence handling: it never deletes `.conflict`, never invokes `clear_conflict()`, and never populates `report.converged`. The test `test_conflict_auto_clears_on_convergence` will fail because the conflict record remains permanently in `state.json`.

---

### 3. Plan 01-11 T-01-31 guarantee violated by `push_to_remote` error handling leaving unpublished commits in the cache
- **Evidence**:
  - `01-11-PLAN.md:217`: Threat register `T-01-31`: *"rollback_commit on UpstreamMoved and on continuity violation, so the cache never carries an unpublished commit; tests assert cache HEAD"*
  - `01-11-PLAN.md:106`: *"If the porcelain stdout has a line whose first tab-separated field is `!` and whose third field starts with `[rejected]`, rollback_commit and return UpstreamMoved; any other non-success returns Failed with the existing prefix git push origin {branch} failed plus the redacted stderr excerpt."*
- **Reproduction/Impact**: `rollback_commit` is invoked solely when porcelain output matches fast-forward rejection (`! [rejected]`). For any other push failure—including pre-receive hook rejections (`[remote rejected]`), network timeouts, broken pipes, or SSH credential drops—`rollback_commit` is omitted. The cache checkout repository retains an unpushed commit at `HEAD`. Any subsequent push without an intervening `ensure_remote_repo` fetch will fail the parent continuity check (`HEAD~1 == fetched_sha`) with a hard exit code 2.

---

### 4. Plan 01-07 deletion truth table omits post-deletion state, causing false `In Sync` for deleted files
- **Evidence**:
  - `01-07-PLAN.md:95-106`: Deletion truth table defines `none + T | present | absent | Unmanaged` (line 102) and `none + T | any | present | classify as no-ancestor` (line 103), but completely omits `none + T | absent | absent`.
  - `01-10-PLAN.md:99`: `MANAGED_FILES` contains 11 static paths scanned regardless of filesystem existence.
  - `src/sync/mod.rs:211-216`: In `compare_3way_hashes`, `(None, None)` with `ancestor_hash: None` maps directly to `SyncStatus::InSync`.
  - `src/main.rs:402`: `SyncStatus::InSync => println!("  {} {}: In Sync", "✓".green(), s.relative_path.display())`
- **Reproduction/Impact**: Immediately following an upstream deletion propagation (D-12) or mutual deletion (D-16), a file has no ancestor record, has a tombstone `T`, and is absent locally and upstream (`none + T, absent, absent`). Because this state is omitted from the tombstone classifier, it falls through to hash comparison where `(None, None)` yields `SyncStatus::InSync`. On every subsequent run, `ude sync status` reports `✓ <file>: In Sync` for a file that does not exist anywhere.

---

### 5. Plan 01-05 v1->v2 migration induces false `Conflicted` status on edited legacy files lacking ancestor content
- **Evidence**:
  - `01-05-PLAN.md:157`: *"for a record without content... when compute_hash of those bytes equals the stored hash, set ancestor_hash to fingerprint of the same bytes; otherwise leave the record as is. Then set version to 2."*
  - `01-CONTEXT.md:37`: Locked decision D-07.
  - `src/sync/mod.rs:367-370`: `local_hash` and `upstream_hash` are computed using `fingerprint` (LF-normalized SHA-256).
- **Reproduction/Impact**: In v1 `state.json`, `ancestor_content` was optional (`None`). If a user edited a tracked file locally prior to running the v2 CLI, `compute_hash` does not match the stored raw hash, leaving `ancestor_hash` as the unmigrated raw SHA-256 string. In `scan_project`, `local_hash` and `upstream_hash` are LF-normalized fingerprints. Comparing LF fingerprints against an unmigrated raw hash fails both equality checks (`local != ancestor` and `upstream != ancestor`), causing `scan_project` to classify an unmodified upstream file as `SyncStatus::Conflicted`.

---

## SUSPECTED

### 6. Plan 01-11 post-checkout hook race injection defeated by host `core.hooksPath`
- **Evidence**:
  - `01-11-PLAN.md:178`: Task 3 installs a hook at `<XDG_CACHE_HOME>/ude/remote-*-main/.git/hooks/post-checkout`.
  - `01-11-PLAN.md:184`: *"The hook script must be POSIX sh... Locate the cache checkout by listing `<cache>/ude`"*
- **Vulnerability**: If the runner or host environment defines `core.hooksPath` in `/etc/gitconfig` or system-level git configuration (e.g. global dotfiles or container security baselines), Git ignores `.git/hooks/*` inside local repository checkouts. While the e2e test sets `GIT_CONFIG_GLOBAL` to a temporary directory, it does not set `GIT_CONFIG_NOSYSTEM=1` during the test run. If system git configs set `core.hooksPath`, the hook never triggers, and the test's strict negative assertion (`if it does not, the test fails with 'post-checkout hook did not fire'`) will fail.
- **What would confirm it**: Run `git -C <cache_checkout> checkout -B main origin/main` on a system with `/etc/gitconfig` containing `[core] hooksPath = ...` and check if `.git/hooks/post-checkout` executes.

---

## SPECULATION

### 7. Detached reader threads leaking open file descriptors on setsid child processes
- **Evidence**:
  - `01-01-PLAN.md:140-142`: Dropping unjoined `JoinHandle` instances when `limit` passes, while `start_kill` targets only the group leader (`killpg(pgid, SIGKILL)`).
- **Speculative Risk**: When a subprocess leader spawns a grandchild that invokes `setsid` and inherits stdout/stderr pipes, `start_kill` cannot kill the grandchild. `run_bounded` detaches the reader threads after `KILL_GRACE`. In standard CLI executions, process exit reaps these threads. However, within long-running in-process test harnesses or repeated runner invocations in a daemonized agent, unjoined threads blocked on pipe read operations will accumulate, potentially exhausting process file descriptors or thread limits before the test suite completes.

---

## Verdict

**No, an automated executor should not run these 11 plans as written.** The plans contain lethal mechanical blockers and architectural holes: Plan 01-02's automated verification command will fail immediately on Wave 2 due to an uncounted `Command::new("git")` mismatch in `tests/e2e_lifecycle.rs`, halting execution. If bypassed, Plan 01-07's truth table creates a ghost state where deleted files report `In Sync`, Plan 01-09 specifies conflict convergence auto-clearing as unreachable dead code that cannot pass its acceptance tests, and Plan 01-11 violates its own security contract by abandoning unpublished commits in the cache checkout on common push failures. Running these plans unattended will trap the executor in circular repair loops and produce a corrupted synchronization engine.
