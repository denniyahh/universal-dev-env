I have launched the test execution command to verify the push failure and ancestor advancement behavior, and am waiting for the background task to complete.
## CONFIRMED

1. **Critical — `push_direct` omits commit rollback on non-fast-forward push failures, bricking the remote cache and falsely advancing ancestor state.**
   - **Severity:** Critical
   - **Citation:** [src/sync/mod.rs:1736-1752](file:///tmp/claude-1000/-var-home-denniyahh-Github-universal-dev-env/fed05489-da0a-42af-b089-55a3255ee8fe/scratchpad/xreview/snapshot/src/sync/mod.rs#L1736-L1752), [src/sync/mod.rs:1493-1495](file:///tmp/claude-1000/-var-home-denniyahh-Github-universal-dev-env/fed05489-da0a-42af-b089-55a3255ee8fe/scratchpad/xreview/snapshot/src/sync/mod.rs#L1493-L1495), [src/main.rs:983-1006](file:///tmp/claude-1000/-var-home-denniyahh-Github-universal-dev-env/fed05489-da0a-42af-b089-55a3255ee8fe/scratchpad/xreview/snapshot/src/main.rs#L983-L1006).
   - **Quoted code:**
     ```rust
     if !push_out.status.success() {
         // Porcelain stdout starts with a `To <url>` line; it is never printed.
         let stdout = String::from_utf8_lossy(&push_out.stdout);
         if classify_push_porcelain(&stdout) == PushPorcelain::RejectedNonFastForward {
             self.rollback_commit(remote_checkout);
             return Err(PushRemoteError::UpstreamMoved {
                 branch: branch.to_string(),
             });
         }
         return Err(Failed(with_excerpt(
             format!("git push origin {branch} failed"),
             &push_out,
         )));
     }
     ```
   - **Failing Scenario:**
     When `git push` fails for reasons other than a non-fast-forward rejection (e.g. pre-receive hook rejection, branch protection rules, network timeout during push, or permission denied), `classify_push_porcelain` returns `PushPorcelain::Other`. In this branch, `self.rollback_commit(remote_checkout)` is **never called**.
     The unpushed commit remains as `HEAD` in the local cache checkout. On a subsequent retry:
     1. `push_updates` writes the local files to `remote_checkout`.
     2. `stage_publication` runs `git status --porcelain`. Because `remote_checkout`'s `HEAD` already contains those exact changes, `git status` outputs nothing (`status.stdout.is_empty()`), and returns `Ok(None)`.
     3. `push_to_remote` returns `Ok("No changes to push to remote repository.")` without pushing anything.
     4. `src/main.rs` treats `Ok(...)` as a successful sync, prints `✓ No changes to push to remote repository.`, and updates `.ude/state.json` via `project_state.record_file(...)`.
     The user is told the changes are synchronized, when in reality the remote repository never received the commit. If the user later edits a different file and pushes, `check_continuity` fails because `HEAD~1` is the un-rolled-back commit instead of `fetched_sha`, permanently bricking the cache checkout with `Branch continuity check failed`.
   - **Why existing tests do not catch it:**
     `test_sync_push_remote_failure_does_not_advance_ancestor_state` ([tests/e2e_lifecycle.rs:552](file:///tmp/claude-1000/-var-home-denniyahh-Github-universal-dev-env/fed05489-da0a-42af-b089-55a3255ee8fe/scratchpad/xreview/snapshot/tests/e2e_lifecycle.rs#L552)) simulates a remote hook failure, removes the hook, and re-pushes. However, step 8 of the test only asserts `push_success.status.success()` and verifies that `state.json`'s hash changed — it never inspects the actual `bare_remote` git log to assert that the commit was published to the upstream repository.

---

2. **Critical — `BothDeleted` records a tombstone instead of dropping tracking, violating locked design decision D-16 and preventing re-initialization.**
   - **Severity:** Critical
   - **Citation:** [src/sync/mod.rs:1148-1152](file:///tmp/claude-1000/-var-home-denniyahh-Github-universal-dev-env/fed05489-da0a-42af-b089-55a3255ee8fe/scratchpad/xreview/snapshot/src/sync/mod.rs#L1148-L1152), [.planning/phases/01-sync-engine-state-hermeticity/01-CONTEXT.md:55](file:///tmp/claude-1000/-var-home-denniyahh-Github-universal-dev-env/fed05489-da0a-42af-b089-55a3255ee8fe/scratchpad/xreview/snapshot/.planning/phases/01-sync-engine-state-hermeticity/01-CONTEXT.md#L55), [src/main.rs:676-680](file:///tmp/claude-1000/-var-home-denniyahh-Github-universal-dev-env/fed05489-da0a-42af-b089-55a3255ee8fe/scratchpad/xreview/snapshot/src/main.rs#L676-L680).
   - **Quoted code:**
     ```rust
     for (rel, ancestor) in both_deleted {
         project_state.remove_file(&rel);
         project_state.record_tombstone(&rel, ancestor, false);
         report.deleted.push(rel);
     }
     ```
   - **Failing Scenario:**
     Locked design decision **D-16** explicitly states: *"Deleted on both sides → clean removal from state, never an empty 0-byte file"*, and the phase contract specifies: *"double deletion drops tracking"*. Furthermore, `ude sync status` advertises: `"Deleted on both sides (pull to drop tracking)"`.
     However, line 1150 calls `project_state.record_tombstone(&rel, ancestor, false)`. This persists a tombstone in `.ude/state.json`. Because `ProjectState::has_record` returns `true` if a path is in `tombstones` ([src/sync/state.rs:317](file:///tmp/claude-1000/-var-home-denniyahh-Github-universal-dev-env/fed05489-da0a-42af-b089-55a3255ee8fe/scratchpad/xreview/snapshot/src/sync/state.rs#L317)), tracking is **never dropped**. If a user later re-runs `ude init`, line 185 (`tracked = |rel| prior_state.has_record(rel)`) and line 443 skip materializing the file, leaving tracking permanently intact despite double deletion.
   - **Why existing tests do not catch it:**
     `test_pull_deletions` ([src/sync/mod.rs:3936](file:///tmp/claude-1000/-var-home-denniyahh-Github-universal-dev-env/fed05489-da0a-42af-b089-55a3255ee8fe/scratchpad/xreview/snapshot/src/sync/mod.rs#L3936)) explicitly asserts the defect: `let tombstone = state.tombstone(rel).expect("D-16 tombstone");`. The test verified that a tombstone was written instead of verifying clean state removal.

---

3. **Warning — `gitignore.repo` only ignores `.ude/.tmp*`, leaking unignored temp files into project root and subdirectories on crash.**
   - **Severity:** Warning
   - **Citation:** [templates/projects/generic/gitignore.repo:35-36](file:///tmp/claude-1000/-var-home-denniyahh-Github-universal-dev-env/fed05489-da0a-42af-b089-55a3255ee8fe/scratchpad/xreview/snapshot/templates/projects/generic/gitignore.repo#L35-L36), [src/sync/staging.rs:112-118](file:///tmp/claude-1000/-var-home-denniyahh-Github-universal-dev-env/fed05489-da0a-42af-b089-55a3255ee8fe/scratchpad/xreview/snapshot/src/sync/staging.rs#L112-L118).
   - **Quoted code:**
     ```
     # Temp files from staging state.json writes (tempfile prefix .tmp), left by a crash
     .ude/.tmp*
     ```
     vs. `staging.rs`:
     ```rust
     let parent = target
         .parent()
         .ok_or_else(|| format!("Target path has no parent: {}", target.display()))?
         .to_path_buf();
     self.create_missing_dirs(&parent)?;
     let stage_err = |e: std::io::Error| format!("Failed to stage {}: {e}", rel.display());
     let mut temp = tempfile::NamedTempFile::new_in(&parent).map_err(stage_err)?;
     ```
   - **Failing Scenario:**
     Per D-05, staged multi-file writes create temp files in each target's own directory (`&parent`), not in `.ude/`. When staging writes for `.gitconfig`, `.gitignore`, or `mise.toml`, `NamedTempFile` creates `.tmpXXXXXX` in the repository root directory (`.`); for hooks, it creates `scripts/hooks/.tmpXXXXXX`; for agent rules, `.agents/.tmpXXXXXX`.
     If a process is killed (SIGKILL, power failure, or OOM) during staging, these `.tmp*` files remain on disk. `gitignore.repo` only ignores `.ude/.tmp*`. The leftover temp files in the root and subdirectories are unignored, causing `git status` to flag untracked files and `git add -A` to commit them.
   - **Why existing tests do not catch it:**
     `test_project_gitignore_ignores_state_staging_temp_files` ([tests/e2e_lifecycle.rs:2662](file:///tmp/claude-1000/-var-home-denniyahh-Github-universal-dev-env/fed05489-da0a-42af-b089-55a3255ee8fe/scratchpad/xreview/snapshot/tests/e2e_lifecycle.rs#L2662)) exclusively tests creating a temp file under `.ude/` (`dir.join(".ude/.tmpXYZ")`). It never tests staged temp files beside managed targets in the root or other directories.

---

4. **Warning — `read_if_present` and `missing_tracked_files` classify dangling symlinks as `NotFound`, causing silent upstream template deletion.**
   - **Severity:** Warning
   - **Citation:** [src/sync/mod.rs:1959-1965](file:///tmp/claude-1000/-var-home-denniyahh-Github-universal-dev-env/fed05489-da0a-42af-b089-55a3255ee8fe/scratchpad/xreview/snapshot/src/sync/mod.rs#L1959-L1965), [src/main.rs:1158-1164](file:///tmp/claude-1000/-var-home-denniyahh-Github-universal-dev-env/fed05489-da0a-42af-b089-55a3255ee8fe/scratchpad/xreview/snapshot/src/main.rs#L1158-L1164).
   - **Quoted code:**
     ```rust
     fn read_if_present(path: &Path) -> Result<Option<Vec<u8>>, String> {
         use std::io::ErrorKind;
         match fs::read(path) {
             Ok(bytes) => Ok(Some(bytes)),
             Err(e) if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => Ok(None),
             Err(e) => Err(format!("Failed to read {}: {e}", path.display())),
         }
     }
     ```
   - **Failing Scenario:**
     `fs::read` and `fs::metadata` follow symbolic links. If a user has a dangling symlink at a managed path (e.g. `ln -s /tmp/missing .gitconfig`), `fs::read` returns `ErrorKind::NotFound`.
     `read_if_present` maps this to `Ok(None)`, treating the dangling symlink as an absent file. `scan_project` assigns `local_hash = None`, classifying the file as `SyncStatus::LocalDeleted`.
     This contradicts the explicit doc comment: *"an unreadable file must never be classified as deleted, because deletions are propagated."* A plain `ude sync push` will then propagate this false deletion and delete the central shared template upstream.
   - **Why existing tests do not catch it:**
     Tests only verify valid symlinks pointing outside the project or missing files where no filesystem inode exists. No negative control test asserts behavior against dangling/broken symlinks.

---

5. **Warning — `ude init` unconditionally warns that push will delete shared templates, contradicting `BothDeleted` and `Conflicted` states.**
   - **Severity:** Warning
   - **Citation:** [src/main.rs:1150-1175](file:///tmp/claude-1000/-var-home-denniyahh-Github-universal-dev-env/fed05489-da0a-42af-b089-55a3255ee8fe/scratchpad/xreview/snapshot/src/main.rs#L1150-L1175).
   - **Quoted code:**
     ```rust
     let consequence = match push_boundary(rel) {
         PushBoundary::Publishable => "a plain ude sync push will delete its shared template",
         PushBoundary::ForceOnly => {
             "a plain ude sync push refuses the deletion, but ude sync push --force will delete its shared template"
         }
         PushBoundary::Never => "ude sync push never publishes this path",
     };
     ```
   - **Failing Scenario:**
     `missing_tracked_files` only checks whether `prior_state.get_ancestor_hash(rel).is_some()` and the file is absent locally; it does not inspect upstream state.
     If upstream has already deleted the template (`BothDeleted`, row 5), `src/sync/mod.rs:1275` guarantees that `ude sync push` publishes nothing, and `ude sync status` prints `Deleted on both sides (pull to drop tracking)`.
     If upstream has modified the template (`Conflicted`, row 4), `ude sync push` skips conflicted files (D-10) and publishes nothing.
     Yet `ude init` prints: `"a plain ude sync push will delete its shared template"`, which is false in both cases and contradicts the actual engine behavior.
   - **Why existing tests do not catch it:**
     `test_reinit_warns_that_push_deletes_template_of_missing_tracked_file` ([tests/e2e_lifecycle.rs:2415](file:///tmp/claude-1000/-var-home-denniyahh-Github-universal-dev-env/fed05489-da0a-42af-b089-55a3255ee8fe/scratchpad/xreview/snapshot/tests/e2e_lifecycle.rs#L2415)) only evaluates the single scenario where upstream is unchanged.

---

## SUSPECTED

1. **Warning — `resolve_conflict` unlinks `.conflict` before committing `state.json`, destroying the conflict sidecar on write failure.**
   - **Severity:** Warning
   - **Citation:** [src/sync/mod.rs:727-736](file:///tmp/claude-1000/-var-home-denniyahh-Github-universal-dev-env/fed05489-da0a-42af-b089-55a3255ee8fe/scratchpad/xreview/snapshot/src/sync/mod.rs#L727-L736).
   - **Quoted code:**
     ```rust
     let staged = state.stage_to_dir(local_root)?;
     remove_conflict_sidecar(local_root, rel)?;
     let sidecar = sidecar_rel(rel);
     staged.commit().map_err(|e| {
         format!(
             "Removed {} but failed to save state: {e}; {} is still unresolved (the next ude sync pull regenerates the sidecar)",
             sidecar.display(),
             rel.display()
         )
     })?;
     ```
   - **Failing Scenario:**
     Line 728 removes the sidecar from disk before `staged.commit()` renames the new state ledger over `.ude/state.json`. If `staged.commit()` fails (e.g. symlink collision or I/O error), the conflict record remains open in `state.json`, but the `.conflict` sidecar is gone. The error message instructs the user that "the next ude sync pull regenerates the sidecar". However, `resolve` is designed to work offline ([src/sync/mod.rs:678](file:///tmp/claude-1000/-var-home-denniyahh-Github-universal-dev-env/fed05489-da0a-42af-b089-55a3255ee8fe/scratchpad/xreview/snapshot/src/sync/mod.rs#L678)); if the developer is offline, `ude sync pull` cannot run to regenerate the sidecar, leaving an open conflict with no visible diff markers.
   - **Why existing tests do not catch it:**
     Tests only exercise successful commits during resolve; there are no fault-injection tests during the commit phase of `resolve_conflict`.

---

2. **Info — Template recovery hint in init suggests copying non-existent template files.**
   - **Severity:** Info
   - **Citation:** [src/main.rs:1173](file:///tmp/claude-1000/-var-home-denniyahh-Github-universal-dev-env/fed05489-da0a-42af-b089-55a3255ee8fe/scratchpad/xreview/snapshot/src/main.rs#L1173), [src/main.rs:1198-1202](file:///tmp/claude-1000/-var-home-denniyahh-Github-universal-dev-env/fed05489-da0a-42af-b089-55a3255ee8fe/scratchpad/xreview/snapshot/src/main.rs#L1198-L1202).
   - **Quoted code:**
     ```rust
     let template = template_root.and_then(|root| sync::map_to_upstream(root, rel));
     ...
     (_, Some(template)) => format!(
         "To keep it, restore it before pushing (git restore {} if it is committed, or copy {}), then check ude sync status",
         rel.display(),
         template.display()
     ),
     ```
   - **Failing Scenario:**
     `template` is computed via path concatenation (`map_to_upstream`) without verifying `template.exists()`. If a template is deleted or absent from `root_template_dir`, `print_missing_tracked_warning` tells the user to `"copy <path>"`, giving a path that does not exist.
   - **Why existing tests do not catch it:**
     Existing tests execute in environments where all template files are present in the fixture directory.
