---
phase: 01-sync-engine-state-hermeticity
reviewed: 2026-10-19T17:51:28Z
depth: deep
files_reviewed: 15
files_reviewed_list:
  - src/process.rs
  - src/sync/mod.rs
  - src/sync/state.rs
  - src/sync/staging.rs
  - src/sync/merge.rs
  - src/main.rs
  - src/cli/mod.rs
  - src/audit/mod.rs
  - src/template/mod.rs
  - src/sanitizer/mod.rs
  - src/semantic/mod.rs
  - tests/e2e_lifecycle.rs
  - Cargo.toml
  - .config/nextest.toml
  - templates/projects/generic/gitignore.repo
findings:
  critical: 3
  warning: 1
  info: 7
  total: 11
status: issues_found
---

# Phase 01: Code Review Report

**Reviewed:** 2026-10-19T17:51:28Z (HEAD 0c3ff71, diff base eaa0afe)
**Depth:** deep
**Files Reviewed:** 15
**Status:** issues_found

## Summary

I reviewed the phase diff (`eaa0afe..HEAD`) and the call chains it touches: process runner, then scan, pull, push, publication and resolve, state load, migration and save, staging, and the CLI dispatch in `main.rs`. `src/sanitizer` and `src/semantic` only got formatting changes; I skimmed them and read just the boundary and parameterization functions that push calls.

The phase's core mechanisms hold up. That covers the bounded runner and group kill, the env scrub, staged writes with re-validation, the corrupt and newer-version state guards, conflict sidecars and convergence, porcelain classification, and the D-18 rollback. I found no defect in them. Accepted risks AR-01-01..03 and the fixed root-layout push bug are not re-reported.

Three blocking defects were reproduced by running the built binary in a scratch project. Only CR-01 was introduced by this phase; CR-02 and CR-03 already existed but sit on the same pull and push paths:
- **CR-01 (phase):** a locally deleted scratch script blocks every push, and the tool offers no way out.
- **CR-02 (pre-existing):** push overwrites `.jinja` templates with rendered text, destroying their logic.
- **CR-03 (pre-existing):** `ude init` "preserves" a custom `.gitignore`, and the first `ude sync pull` then silently replaces it.

One unit test is flaky: measured 3/200 failures run alone and 24/480 under 8-way parallel load. It also failed once in a full `cargo test` run during this review.

Baseline checks: `cargo clippy --all-targets -- -D warnings` exits 0. `cargo test` passes apart from that flaky test.

## Critical Issues

### CR-01: A locally deleted boundary file blocks every push and the tool offers no way out (phase-introduced)

**File:** `src/sync/mod.rs:1224-1232` (new LocalDeleted branch), also `src/sync/mod.rs:1244-1251` (pre-existing LocallyModified branch), `src/main.rs:643-649`, `src/sync/mod.rs:688-693`

**Issue:** Push returns `PushResult::BoundaryBlocked` as soon as any `LocalDeleted` file is a publish-boundary path. Those paths are `scratch/*`, which covers 3 of the 11 managed files, and `.agents/AGENTS.md` without `--force`. The early return aborts the whole push, so no other file is published, and `--force` does not help for `scratch/` (`sanitizer/mod.rs:137-138`). At the same time:
- pull keeps the deletion (D-15) and tells the user "deleted locally — push to propagate", which is advice the next push refuses;
- `ude sync resolve` refuses because no conflict record exists;
- so every later push exits 1 until the user restores the file by hand.

This contradicts D-10's skip-and-publish-the-rest model and D-14. Editing a scratch file instead of deleting it hits the same early return; that variant already existed before this phase.

**Failure scenario (confirmed via binary in a scratch project):** `ude init`, then `rm scratch/new-worktree.sh`, then append a line to `.gitconfig`. `ude sync push` exits 1 ("within private publish boundary") and the `.gitconfig` edit never reaches `gitconfig.repo`. `ude sync pull` exits 0 and prints "push to propagate". `ude sync resolve scratch/new-worktree.sh` exits 2 ("No open conflict"). Pushing again still exits 1. Existing unit test `test_push_local_deletion_respects_publish_boundary` asserts the abort, so the test suite pins this behavior.

**Fix:** Treat boundary paths like D-10 skips: collect them, publish the rest, list them, and exit 1. Make pull's message conditional on the path being publishable:
```rust
SyncStatus::LocalDeleted | SyncStatus::LocallyModified
    if self.sanitizer.is_local_only_path(&state.relative_path, allow_agent_rules) =>
{
    skipped_boundary.push(state.relative_path.clone());
    continue;
}
```
Separately, decide what a local deletion of a never-publishable managed file should mean. One option is to tombstone it locally (Unmanaged), so it stops reporting as pending.

### CR-02: Push replaces `.jinja` templates with rendered output, erasing template logic for every consumer (pre-existing)

**File:** `src/sync/mod.rs:1263-1303`, `src/sync/mod.rs:2069-2072` (`mise.toml` maps to `mise.toml.jinja`, `.agents/AGENTS.md` maps to `agents.md.jinja`)

**Issue:** `push_updates` publishes the parameterized local file, which is the rendered text, as the new upstream template bytes. For a `.jinja` source this deletes every `{% if %}` and `{{ }}` construct except the home and XDG paths that `parameterize_paths` re-inserts. Other projects and machines then pull a template specialized to one project type.

**Failure scenario (confirmed via binary):** `ude init --type rust`, append `[env]` to `mise.toml`, run `ude sync push` (exit 0). `templates/projects/agents/mise.toml.jinja` now begins `[tools]\n\nrust = "latest"` with no `{%` left. A node project's next pull drops `node = "24"`.

**Fix:** Never write rendered text over a template that contains template syntax. Either block push for files whose upstream source has `{%` or `{{` constructs (report them like a D-17 skip, telling the user to edit the template directly), or make a 3-way merge against the raw template a precondition. The minimal safe change is a guard:
```rust
if upstream_target.as_deref().is_some_and(|p| template_has_logic(p)) {
    skipped_templated.push(state.relative_path.clone());
    continue;
}
```

### CR-03: `ude init` keeps a custom `.gitignore` but records it as the ancestor, so the first pull silently overwrites it (pre-existing)

**File:** `src/main.rs:229-251` (preserve-existing branch), `src/main.rs:404-413` (ancestor = current local bytes)

**Issue:** Init deliberately keeps an existing `.gitignore`, appending only the graphify block. It then records that customized file as the ancestor. The upstream template differs from that ancestor, so scan classifies the file `UpstreamModified` (`l == o`, `u != o`), and pull replaces the user's rules with the template, exiting 0 and reporting only "materialized 1 upstream update(s)". The phase's `*.conflict` rule (D-11) reaches such projects only through this destructive overwrite.

**Failure scenario (confirmed via binary):** create a project directory whose `.gitignore` holds two custom rules, run `ude init`, then `ude sync pull`. Status first shows ".gitignore: Upstream Updated". After the pull, neither custom rule remains.

**Fix:** For a preserved file, record the template's rendered fingerprint and content as the ancestor, not the local bytes. The custom file then reads as `LocallyModified`, or merges or conflicts properly, instead of being overwritten:
```rust
// for a preserved .gitignore
let template = fs::read(&src_gitignore)?;
project_state.record_file(rel, SyncEngine::fingerprint(&template),
    String::from_utf8(template).ok().map(|t| SyncEngine::normalize_eol(&t)));
```

## Warnings

### WR-01: `test_remote_lock_not_inherited_by_children` is flaky (it races the child's exec and loader)

**File:** `src/sync/mod.rs:5463-5468` (`open_fd_targets`), `src/sync/mod.rs:5518-5550`

**Issue:** The comment at line 5537 claims "std's spawn returns after exec, so the child's descriptor table is final". In reality the dynamic loader opens and closes descriptors after exec, and `/proc/<pid>/fd` is briefly inaccessible while exec runs. `open_fd_targets` `expect`s on every `read_dir` and `read_link`, so a descriptor that closes between listing and reading it, or a read during exec, panics the test.

**Failure scenario (confirmed):** running the compiled test binary in a loop gave 3/200 failures alone and 24/480 with 8 parallel copies. The failures were `read fd link: NotFound` at 5466:73 and `list /proc/<pid>/fd: PermissionDenied` at 5465:14. It also failed once in a full `cargo test` run during this review.

**Fix:** Tolerate transient entries and retry until the fd set is stable:
```rust
fn open_fd_targets(pid: u32) -> Vec<PathBuf> {
    for _ in 0..50 {
        if let Ok(dir) = fs::read_dir(format!("/proc/{pid}/fd")) {
            return dir.filter_map(|e| e.ok().and_then(|e| fs::read_link(e.path()).ok())).collect();
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("/proc/{pid}/fd stayed unreadable");
}
```
Also wait for `/proc/<pid>/exe` to resolve to `sleep`, so the loader phase is over before sampling.

## Info

### IN-01: A successful git command is reported as a hard failure when an escaped descendant holds the pipes

**File:** `src/process.rs:201-210`, consumed at `src/sync/mod.rs:1702-1704`
**Issue:** When the leader exits successfully but a descendant that left the process group still holds stdout or stderr, `run_bounded` returns `Err`. A successful `git push` then becomes `PushRemoteError::Failed` (exit 2), and the ancestors are not advanced. This self-heals: the next pull sees local equal to upstream and records it. But exit 2 tells an agent the publish failed when it did not. Confirmed by reading; not exercised.
**Fix:** Return `Ok` with the leader's status and a truncated-output flag, and let callers decide. At minimum, mention in the error text that the leader's exit status was success.

### IN-02: Remote direct push exits 0 when the state save fails after publishing

**File:** `src/main.rs:856-863`
**Issue:** If `save_to_dir` fails after a successful publish, main prints a warning and exits 0, or 1 only if files were skipped. Local mode returns `IoFailed` (exit 2) for the same failure (`sync/mod.rs:1389-1393`). This self-heals on the next pull through convergence or BothDeleted, but the exit-code contract (2 = hard failure) differs between modes. Confirmed by reading.
**Fix:** `std::process::exit(EXIT_HARD_FAILURE)` after the message, stating that the upstream was published.

### IN-03: A v1 hash-only record whose file was edited migrates into a false conflict for CRLF templates

**File:** `src/sync/state.rs:169-176`
**Issue:** When the raw hash no longer matches the local file, the record keeps its raw-byte hash. For a CRLF upstream template, `fingerprint(upstream)` never equals that raw hash, so an unchanged upstream reads as modified. The file is then Conflicted (no ancestor text, so no merge) instead of LocallyModified, and push skips it. The impact is narrow, because hash-only v1 records were mostly binary files, where fingerprint equals raw hash. Confirmed by reading.
**Fix:** In `scan_project`, when the ancestor is a legacy raw hash, also compare `compute_hash` of the raw upstream bytes, or mark migrated hash-only records and accept either hash for the upstream comparison.

### IN-04: The status scan falls back to the raw template when rendering fails, while pull aborts entirely (pre-existing)

**File:** `src/sync/mod.rs:608-615` versus `src/sync/mod.rs:928-943`
**Issue:** Any managed file containing `{{` is rendered, and an undefined variable renders as empty text (lenient minijinja). When a render fails, `scan_project` silently uses the raw text, but `pull_updates` returns `IoFailed` for the whole run. One un-renderable file therefore blocks every other upstream update, and status does not predict the failure. Confirmed by reading.
**Fix:** Render only `.jinja` sources, use strict undefined behavior, and make scan report render errors instead of masking them.

### IN-05: The managed-file list is duplicated in init

**File:** `src/main.rs:391-403` (duplicates `sync::MANAGED_FILES`, `src/sync/mod.rs:185-197`)
**Issue:** If a path is added to one list but not the other, init never records an ancestor for it. The first scan then classifies it as no-ancestor (Conflicted or Merged), even though it is untouched.
**Fix:** Iterate `sync::MANAGED_FILES.iter().map(PathBuf::from)`.

### IN-06: `ude sync resolve ./.gitconfig` is refused as an "unsafe path"

**File:** `src/sync/mod.rs:1987-1993`
**Issue:** Shell completion and agents commonly produce `./`-prefixed paths. A `CurDir` component is rejected with a message that suggests an attack rather than a formatting issue.
**Fix:** Strip leading `CurDir` components before the all-`Normal` check. Keep refusing `..`, `RootDir` and `Prefix`.

### IN-07: Test hermeticity gaps (pre-existing pattern, now on new tests)

**File:** `tests/e2e_lifecycle.rs:101-165`, `167-221`, `883-977`; `src/sync/mod.rs:1189-1190` with `src/template/mod.rs:60-95`
**Issue:** Several e2e tests run `ude sync status` and `ude sync pull` in local-templates mode without a `templates/` directory in cwd and without isolating `HOME` or XDG. The binary then reads the developer's working-tree `templates/` through the compile-time `CARGO_MANIFEST_DIR` fallback (`main.rs:1002-1007`), so results depend on uncommitted template edits, and a future local-mode `push` test would write into the repository. Separately, `TemplateContext::detect_for_project` spawns `git config user.name/email` without the engine's `extra_env`, so unit tests that inject a fixture gitconfig still read the host identity.
**Fix:** Give these tests a copied `templates/` in cwd plus fake `HOME`/`XDG_*` (as `sync_cli` already does). Thread the engine's extra env into the template context's git calls.

---

_Reviewed: 2026-10-19T17:51:28Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: deep_
