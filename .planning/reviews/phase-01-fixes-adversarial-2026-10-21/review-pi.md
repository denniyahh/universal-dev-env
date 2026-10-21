## CONFIRMED

*(all confirmed by execution in a scratch copy of the snapshot; the snapshot itself was not modified. `CARGO_TARGET_DIR` shared — see note at end)*

- **[HIGH] src/main.rs:537-543 — the new "kept tombstone" is silently skipped when the file can't be read, and `sync push` then publishes the very file it was supposed to keep unmanaged.**
  ```rust
  Ok(None) if committed_keep => {
      if let Ok(bytes) = fs::read(project_path.join(rel)) {   // ← error swallowed
          project_state.record_tombstone(rel, hash, true);
  ```
  Any `fs::read` error (EACCES on a root-owned/ACL'd checkout, EIO/ESTALE on a network mount, EISDIR from a *committed symlink to a directory*) drops through with **no tombstone, no error, no message**. The file is then unrecorded → row 7 without `tombstoned` → `LocallyModified` → push treats it as a new local template.
  Repro (run in a copy): git repo, `templates/projects/generic/gitconfig.repo` deleted, `.gitconfig` written and committed, `chmod 000 .gitconfig`, `ude init`. Result: `init exit=0`, prints `✓ Kept committed .gitconfig (not overwritten); sync status compares it with the template` (*there is no template*), `state.json` has `"tombstones": null`, then `ude sync push` printed `✓ Successfully synchronized 1 file(s) into upstream template store` and created `templates/projects/generic/gitconfig.repo` containing the local bytes. This directly contradicts the comment 10 lines above (main.rs:529-533, *"push never resurrects or creates a template from it"*) and src/sync/mod.rs:1981-1983 (*"an unreadable file must never be classified as deleted"*).
  *Limits:* I verified the EACCES trigger only; the EISDIR/symlink-to-dir variant is reasoning from the same branch, not executed.

- **[HIGH] src/main.rs:563-578 — the "unmanaged" guarantee exists only in `state.json`, and that write failing is downgraded to a warning printed *before* the claim, with exit 0.**
  ```rust
  if let Err(e) = project_state.save_to_dir(&project_path) {   // warning only
  ...
  "  {} Kept committed {} (not overwritten); no shared template, so sync leaves it unmanaged",
  ```
  Repro: same fixture, `.ude/` pre-created mode `0500`. Output order: `! Warning: Failed to persist ancestor state: ... Permission denied` → `✓ Kept committed .gitconfig (not overwritten); no shared template, so sync leaves it unmanaged` → `==> Project initialization complete.`, `init exit=0`, no `state.json`. `ude sync push` then wrote `templates/projects/generic/gitconfig.repo` = the local `.gitconfig` (and spun off a bogus `mise.toml: unresolved conflict` because the lost state also loses `project_type` for the sync-side render context).
  A user reads a green ✓ telling them the file is unmanaged; the only surviving record of that decision is the file that failed to write.

- **[MED-HIGH] src/main.rs:1572-1586 (`committed_paths`) reached from main.rs:210-216 — "a `.git` exists above" is treated as "git must work here", so init hard-aborts on any ancestor `.git` git refuses to use.**
  ```rust
  if !absolute.ancestors().any(|dir| dir.join(".git").exists()) { return Ok(empty) }
  ...
  return Err(format!("git ls-files failed: {}", ...));
  ```
  Confirmed failures (project nested in a directory whose `.git` is unusable):
  - empty leftover `.git` directory above the project → `✗ Initialization failed: git ls-files failed: fatal: not a git repository (or any parent up to mount point /)` , **exit 1**, nothing initialized.
  - a **copied linked worktree** (`.git` is a file holding an absolute gitdir; any `cp -a` of a worktree, or a moved main repo) → `✗ Initialization failed: git ls-files failed: fatal: gitfile does not point to a valid repository: .../svc/.git`, **exit 1**.
  Same code path covers `safe.directory` / *"dubious ownership"* (root-owned repo in a container, shared CI checkout) — labelled reasoning, not executed. There is no `--no-git`/override. Before this diff, a project that itself had no `.git` never invoked git at all.
  Also: this failure exits **1** (`EXIT_DRIFT`), while every other init refusal added by this diff exits **2** (`EXIT_HARD_FAILURE`, main.rs:253/336); the new tests assert 2. `kept_ancestor` errors also exit 1 (main.rs:553-556). Three codes for "init could not proceed".

- **[MED] src/main.rs:475 + src/main.rs:638-669 — in a monorepo subdir the success line "Configured tracked .gitconfig & chained scripts/hooks" is printed although nothing was chained, in exactly the case the new monorepo test covers.**
  The git-config step is still gated on `if project_path.join(".git").exists()`, so in a subdir `core.hooksPath` / `include.path` are never set — while the step echo is computed from file *presence* only:
  ```rust
  if absent == 0 { println!("  {} {done}", "✓".green()); }
  ```
  Confirmed: `git init` at `mono/`, subdir `mono/svc`, init → exit 0, all green, `git -C mono config --get core.hooksPath` → `<unset>`, `include.path` → `<unset>`. The hooks this diff carefully *keeps* committed are therefore never active in the monorepo. `test_first_init_in_monorepo_subdir_keeps_committed_edits` (tests/e2e_lifecycle.rs:3120-3153) asserts only file content and `git diff`, so it certifies the keep while the feature that made the keep worthwhile is dead.

- **[MED] src/main.rs:527-536 + src/sync/mod.rs:490 + src/main.rs:814-818 — "kept tombstone" does not survive the state machine: a later upstream *addition* becomes a conflict, and the status label asserts something false.**
  Row 7 (`(None, Some(_), None) if tombstoned => Unmanaged`) only holds while no template exists. Add the template back with different bytes → `(None, Some, Some) if differ` → `Conflicted`. Confirmed: fresh clone, template removed, init → `.gitconfig: Unmanaged (deleted upstream; kept locally, never republished)`; restore the template with extra content → `.gitconfig: Conflicted (requires manual resolution)`, and `ude sync pull` exits **1** writing `.gitconfig.conflict` for a file whose template this user never had. The init message `"sync leaves it unmanaged"` (main.rs:578) is a permanent-sounding claim that silently expires.
  Sub-point (contradiction, inert today): the record written is `record_tombstone(rel, fingerprint(local_bytes), true)` on a file that was **never tracked and never deleted**, while src/sync/state.rs:18-29 documents the type as *"a managed path whose file was deleted while UDE tracked it"* with `deleted_hash: "Fingerprint of the last tracked version (the ancestor at deletion time)"`, and src/sync/state.rs:258-259 says *"The caller removes the files record"* — which this caller does not do. And main.rs:816 renders it as *"deleted upstream; kept locally"*, i.e. it also claims a user choice (D-13 `KeptUnmanaged`) that never happened. Nothing in production reads `deleted_hash` (only tests do), so this is misleading state, not yet a live bug.

- **[LOW-MED] src/main.rs:333-337 — the new `.gitignore` read failure aborts with `EXIT_HARD_FAILURE` *after* `.gitconfig` and the scaffolding were already written, contradicting the pre-flight's own atomicity claim.**
  main.rs:230-233: *"Every path init may create or write is checked before anything is touched."* Confirmed: `.gitignore` mode `0000`, init → `✗ Cannot read existing .gitignore: Permission denied (os error 13)`, `exit=2`, and `.gitconfig` exists on disk (state.json absent). The symlink refusal path checks the opposite property, and its test asserts it (`assert!(!project.join(".gitconfig").exists(), "init wrote files before refusing")`), so the two refusal paths in one diff have opposite atomicity and only one is tested for it.

## SUSPECTED

- **[MED, unverified] main.rs:230-256 is a check-then-use window, not a hardening.** `refuse_symlink_under` runs first; every write afterwards is a follow-links operation (`fs::write` at main.rs:347, `copy_template_file`/`render_and_write`, `create_dir_all` at main.rs:292/455). Nothing uses `O_NOFOLLOW`/`openat2`, and `copy_template_file` refuses symlinks only on the **source**. A `scripts`/`.gitignore`/`.agents` symlink created between the loop at main.rs:250 and the writes at 292-460 still writes outside the project. No deterministic repro attempted; claiming "checked before anything is touched" is not supported by this code.
- **[MED, unverified] Windows is out of scope despite the unix-only tests.** Both new tests are `#[cfg(unix)]` (tests/e2e_lifecycle.rs:2768, 2809). `refuse_symlink_under` relies on `symlink_metadata().file_type().is_symlink()`, which on Windows does not report **junctions / mount points** — so `scripts` as a reparse point would pass the pre-flight and be followed. No Windows run.
- **[LOW, unverified] `refuse_symlink_under` (src/sync/mod.rs:1963-1977) has two silent no-ops:** the project root itself is never a candidate (`take_while(|p| *p != root)`), so a symlinked project root is accepted by design; and when `path` does not lexically start with `root` it falls back to checking only `path`. `committed_paths` canonicalizes (`fs::canonicalize`) while all writes use the un-canonicalized `project_path`, so "which directory is the project" differs between the git check and the writer. No exploit constructed.

## TEST GAPS

- **`test_init_keeps_uncommitted_gitignore_when_template_is_absent` (tests/e2e_lifecycle.rs:3256-3275) is not falsifiable.** I built the pre-diff source with the new test file and ran it: it **passes** (`0 failed`), along with nothing else — all 9 sibling new tests fail there. Reason: the uncommitted branch never enters the new `Ok(None) if committed_keep` arm, so the test only re-asserts pre-existing behaviour. It also **stops at `sync pull`**; the adjacent push (verified above) creates `templates/projects/generic/gitignore.repo` from the local file — the exact "template that never existed" the diff's comment forbids. Add a `sync push` assertion here or the test blesses the hole.
- **`test_init_refuses_unreadable_gitignore` (tests/e2e_lifecycle.rs:2890-2910)** asserts exit 2 and that `.gitignore` is byte-identical, but not that the project is unmodified — so the partial init confirmed above passes it. Its sibling symlink test asserts exactly that; the asymmetry is in the test, not just the code.
- **No test for the state transition the fix creates**: tombstone + later-appearing template (confirmed `Conflicted` + sidecar + pull exit 1), and no test for a *matching* template (should become In Sync and clear the tombstone). `test_fresh_clone_does_not_resurrect_template_deleted_upstream` only exercises "template stays absent".
- **No negative control for the new `if let Ok` branch**: nothing makes `fs::read` fail in the tombstone path, so the fallback to pre-fix publish behaviour is untested in its failure direction (same shape as RULE ZERO (b)'s proxy-measurement trap — the test proves the happy path and is named as if it proved the invariant).
- **`test_first_init_in_monorepo_subdir_keeps_committed_edits` (3120-3153)** does not assert that `core.hooksPath`/`include.path` were configured (they are not), and its control `git diff --name-only` is blind to files init newly *creates* (untracked), so a partial regression that writes a new managed file would still pass. `test_init_refuses_symlinked_destination_file`'s negative control ends with `assert_ne!(fs::read_to_string(&agents).unwrap(), "")` — any non-empty content satisfies it.

Cleanup: my scratch copies (`work/{fixed,reverted,revsrc,probe,shell}`) and `~/.cache/ude-xr-target-rev` are deleted; no processes of mine remain. Caveat: the shared `~/.cache/ude-xr-target` now holds `ude`/`e2e_lifecycle` artifacts last built from those (now deleted) scratch copies, so the next `cargo test` in the snapshot will rebuild — the other review lanes running against that same target dir concurrently may have seen those binaries. I did not run the full test suite; all statements above are from the specific probes named.
