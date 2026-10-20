## CONFIRMED

- **warning — `src/main.rs:1155-1185`**

  `missing_tracked_files` only checks the stored ancestor and local absence, then prints `"a plain ude sync push will delete its shared template"`. It does not inspect the current upstream template.

  Scenario: delete `scripts/hooks/pre-push`, modify its upstream template, then run `ude init` before any pull. Init emits the deletion warning. A subsequent push scans current upstream, classifies `(ancestor, None, changed-upstream)` as `Conflicted` in `src/sync/mod.rs:479-480`, skips it at `src/sync/mod.rs:1211-1215`, and preserves the template. This contradicts D-15: upstream movement during a local deletion is a conflict, not a deletion.

  Existing test `tests/e2e_lifecycle.rs:2524-2540` first runs `sync pull`, which creates the conflict record before re-init; it never tests the stale-ledger path that produces the false warning.

- **warning — `src/sync/state.rs:124-153`, `src/main.rs:465-469`**

  State loading follows a non-dangling `.ude/state.json` symlink, and treats a dangling one as absent:

  > `Ok(content) => content,`  
  > `Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),`

  There is no `symlink_metadata` or safe-target validation before this read. A dangling `state.json` therefore becomes an empty ledger, violating D-06’s requirement that unreadable state errors rather than silently reset. `ude init` then materializes managed files, fails later at the write-side symlink check, but merely prints:

  > `"Warning: Failed to persist ancestor state: {e}"`

  and exits successfully after making side effects. A symlink to valid external JSON is worse: it is accepted as project state and controls which files init considers tracked.

  `src/sync/state.rs:407-418` tests a directory as unreadable; `src/sync/state.rs:691-745` tests write-time symlinks. Neither tests loading a dangling or readable `state.json` symlink, nor init’s exit code and side effects.

- **critical — `src/sync/mod.rs:596-608`, `src/sync/mod.rs:1283-1335`, `src/sync/mod.rs:1959-1965`**

  Sync treats managed symlinks as ordinary content sources. Both scan and push use `fs::read`, which follows symlinks:

  > `let local_bytes = read_if_present(&local_path)?;`  
  > `match fs::read(path) {`  
  > `let raw_bytes = match fs::read(&local_path) {`

  Scenario: initialize a project, replace publishable `.gitconfig` with a symlink to any readable file whose contents differ from the ancestor, then run `ude sync push`. It becomes `LocallyModified`, and push stages the symlink target’s bytes into `gitconfig.repo`. The text secret scanner is not an adequate barrier: arbitrary non-secret text passes, while non-UTF-8 data bypasses it at `src/sync/mod.rs:1323-1325`.

  The same flaw applies upstream: a malicious template checkout can contain `gitconfig.repo` as a symlink to an arbitrary host-readable path. `scan_project` follows it at `src/sync/mod.rs:608`; pull later follows it again at `src/sync/mod.rs:916`, copying host content into the project.

  `test_pull_updates_refuses_to_write_through_symlink` at `src/sync/mod.rs:6793-6843` only proves that pull will not overwrite a symlinked destination. It has no local-source push control or symlinked-upstream-source control.

## SUSPECTED

- **warning — `src/sync/staging.rs:107-118`, `src/sync/staging.rs:186-200`, `src/sync/state.rs:193-203`**

  The anti-symlink checks are check-then-use, not descriptor-anchored. `validate_safe_write_target` returns a pathname; a concurrent writer can replace its parent with a symlink after validation and before `NamedTempFile::new_in(&parent)`, or after commit-time validation and before `temp.persist(&target)`. That can stage or rename outside the intended root.

  This requires a concurrent actor able to mutate the project/template tree, so it is a race finding rather than a fully reproduced defect. The symlink tests only replace paths deterministically before commit; they do not coordinate a swap in either validation-to-use window.
