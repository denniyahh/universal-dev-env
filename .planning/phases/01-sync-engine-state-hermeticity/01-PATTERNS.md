# Phase 1: Sync Engine & State Hermeticity - Pattern Map

**Mapped:** 2026-10-17
**Files analyzed:** 12 (10 modified, 2 new)
**Analogs found:** 12 / 12. Two are partial matches: `src/process.rs` and `src/sync/staging.rs` have no same-role file in the repo.

All analog paths below are git-tracked (checked with `git ls-files`). `src/process.rs` and `src/sync/staging.rs` are new and untracked. No CLAUDE.md, `.claude/skills`, or `.agents/skills` exist in the repo. Conventions come from GEMINI.md, as summarized in RESEARCH.md.

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `src/process.rs` (NEW) | utility (crate-level bounded runner) | request-response (subprocess) | `src/semantic/mod.rs:130-162` (piped spawn) + `src/sync/mod.rs:81-118` (deadline poll loop) | partial (no bounded runner exists) |
| `src/sync/mod.rs` | service (engine) | file-I/O + subprocess + transform | itself (in-file analogs per section) | exact |
| `src/sync/state.rs` | model (persisted ledger) | file-I/O (JSON) | itself, lines 7-60 | exact |
| `src/sync/merge.rs` | utility (pure function) | transform | itself, lines 1-7, 75-87, 191-248 | exact (shape); body replaced by diffy |
| `src/sync/staging.rs` (NEW, optional split) | utility | file-I/O (batch, transactional) | `src/sync/mod.rs:645-701` (pull Pass 2) + `:1064-1107` (`validate_safe_write_target`) | role-match |
| `src/main.rs` | controller (CLI dispatcher) | request-response | itself, lines 391-563 | exact |
| `src/cli/mod.rs` | config (clap surface) | n/a | itself, lines 102-118 | exact |
| `src/template/mod.rs` | service | subprocess (2 git calls) | itself, lines 56-82 | exact (mechanical swap) |
| `src/audit/mod.rs` | service | subprocess (5 git calls) | itself, lines 128-151, 199-205, 224-230, 255-261 | exact (mechanical swap) |
| `templates/projects/generic/gitignore.repo` | config (template) | n/a | itself (comment-header sections) | exact |
| `tests/e2e_lifecycle.rs` | test (e2e, binary) | request-response | `tests/e2e_lifecycle.rs:313-475` | exact |
| `Cargo.toml` | config | n/a | itself `[dependencies]` | exact |

---

## Pattern Assignments

### `src/process.rs` (NEW, utility, subprocess request-response)

**Analogs:** `src/semantic/mod.rs` (the only piped spawn in the repo) and `RemoteLockGuard::acquire` (the only deadline-poll loop). Use RESEARCH.md Pattern 1 as the skeleton. Copy the conventions below from the analogs.

**Module registration.** There is no `src/lib.rs`, and modules are declared in `src/main.rs:1-7` in alphabetical order. Add `pub mod process;` between `manifest` and `sanitizer`:
```rust
pub mod audit;
pub mod cli;
pub mod manifest;
pub mod sanitizer;
pub mod semantic;
pub mod sync;
pub mod template;
```
Callers reach it as `crate::process::run_bounded(...)`. The module is used from `sync`, `template`, `audit`, and `main`.

**Spawn with piped stdio and the `String` error style** (`src/semantic/mod.rs:130-162`). This is the pattern to replace, because `wait_with_output` has no deadline:
```rust
let mut child = Command::new("curl")
    .args([ ... ])
    .stdin(std::process::Stdio::piped())
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::piped())
    .spawn()
    .map_err(|e| format!("Failed to spawn curl: {e}"))?;
...
let output = child
    .wait_with_output()
    .map_err(|e| format!("curl execution failed: {e}"))?;
```
Keep the `format!("Failed to spawn {label}: {e}")` error wording. Use `Stdio::null()` for stdin, not `piped()`.

**Deadline poll loop** (`src/sync/mod.rs:84-117`). Copy its shape (`Instant` start, `Duration` timeout, `elapsed() >= timeout` check, `thread::sleep(poll_interval)`) and the "Timed out after {}s ..." message wording:
```rust
use std::time::{Duration, Instant};
let start = Instant::now();
let timeout = Duration::from_secs(timeout_secs);
let poll_interval = Duration::from_millis(50);
loop {
    // try operation; return Ok on success
    if start.elapsed() >= timeout {
        return Err(format!(
            "Timed out after {}s waiting for remote cache lock on {}",
            timeout_secs, lock_path.display()
        ));
    }
    std::thread::sleep(poll_interval);
}
```
For process.rs, poll `child.try_wait()` instead. The runner must take `timeout: Duration` as a parameter so tests can pass 300 ms. Use named constants `LOCAL_TIMEOUT` (30 s), `NETWORK_TIMEOUT` (60 s), and `POLL_INTERVAL` (10 ms). Operator rules forbid magic numbers.

**Test module convention** (`src/sync/state.rs:99-102`):
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
```
Tests for the group-kill timeout, the 1 MiB drain, and the `get_envs()` scrub check go here. Run them with `cargo test --bin ude process::`.

---

### `src/sync/mod.rs` (service, file-I/O + subprocess + transform)

The file is 2472 lines. Each change has an in-file analog:

**1. Outcome enums to extend** (lines 11-51). Add variants and report fields here. Do not print from the engine.
```rust
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum SyncStatus { InSync, LocallyModified, UpstreamModified, Merged, Conflicted, Overridden }

#[derive(Debug, PartialEq, Eq)]
pub enum PullResult {
    Success { applied_files: usize },
    ConflictBlocked { path: PathBuf },
    IoFailed { path: PathBuf, reason: String },
}
```
`PushResult` (lines 32-44) has the same style: struct variants carrying a `PathBuf` and a `reason: String`. Keep `#[derive(Debug, PartialEq, Eq)]`, because tests compare whole values (`assert_eq!(pull_res, PullResult::Success { applied_files: 3 })` at line 1382).

**2. `RemoteLockGuard` (D-04)** (lines 53-140). Delete `unsafe extern "C" flock`, the `LOCK_*` constants (53-62), the errno-11 branch (99-106), the `#[cfg(not(unix))]` no-op (120-126), and the `Drop` body (130-140). Keep the `OpenOptions` open at lines 73-79 unchanged, keep the struct shape (`file`, `pub lock_path`), and keep the timeout message text. `test_remote_lock_concurrency_serialization_and_timeout` (lines 1755-1777) asserts `"Timed out" || "waiting for remote cache lock"` and a 500 ms bound.

**3. Fingerprint beside `compute_hash`** (lines 154-159). Add `fingerprint` directly below. Do not change `compute_hash`, because it keys the cache dir at line 429 and the PR branch at line 940.
```rust
/// Computes the sha256 checksum of a byte slice
pub fn compute_hash(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}
```

**4. Classification** (`scan_project`, lines 303-412). The text/binary split to mirror is at 332-336:
```rust
let local_bytes = fs::read(&local_path).ok();
let local_content = local_bytes
    .as_ref()
    .and_then(|b| std::str::from_utf8(b).ok().map(|s| s.to_string()));
let local_hash = local_bytes.as_ref().map(|b| Self::compute_hash(b));
```
Change `local_hash` and `upstream_hash` (line 367) to `fingerprint`. Replace the three-branch dispatch at 373-398 with classification by fingerprint only (RESEARCH Patterns 2 and 3). The string-equality path that must go is `compare_3way_with_merge` (223-281); its `unwrap_or("")` at 252-253 is the both-deleted resurrection bug. `ProjectState::load_from_dir(local_root)` at line 310 becomes `?`, so `scan_project` returns `Result<Vec<FileSyncState>, String>`. Callers to update: `src/main.rs:398` and `src/sync/mod.rs:506, 721`, plus 7 test call sites (1411, 1939, 1976, 2404, 2425, 2457, 2468).

**5. Subprocess call sites.** 31 production spawns sit before line 1228. Representative shapes and how to convert each:
- **Checked status** (lines 437-445), the most common pattern:
  ```rust
  let status = Command::new("git")
      .args(["clone", "--depth", "1", "-b", branch, remote_url])
      .arg(&target_checkout)
      .status()
      .map_err(|e| format!("Failed to execute git clone: {e}"))?;
  if !status.success() {
      return Err(format!("git clone failed with exit status {:?}", status.code()));
  }
  ```
  Convert to `crate::process::run_bounded(cmd, "git clone", NETWORK_TIMEOUT)?` and keep the `status.success()` check on `RunOutput.status`. The label must not contain `remote_url` (Pitfall 9: token URLs).
- **Captured output** (lines 845-849, 882-886, 907-911): `.output()` becomes `run_bounded`, and the code reads `.stdout` from `RunOutput`.
- **Best-effort cleanup** (`let _ = Command::new("git")...status();` at 478-481, 876, 898, 914, 923, 960-961, 967-968, 979, and 1119-1123): keep discarding the result, but route through the runner (`let _ = run_bounded(...)`).
- **gh** (lines 986-1010): the network tier, plus `GH_PROMPT_DISABLED=1` and `GH_NO_UPDATE_NOTIFIER=1`. The fallback `match` on `Ok(output) if output.status.success()` stays the same.

**6. Pull write path to replace with staging** (lines 645-701). This is the current non-atomic loop and the mode rule to preserve:
```rust
if let Err(e) = fs::write(&target, &content_bytes) { return PullResult::IoFailed { ... }; }
#[cfg(unix)]
{
    use std::os::unix::fs::PermissionsExt;
    let rel_str = rel.to_string_lossy();
    if rel_str.starts_with("scripts/hooks/")
        || rel_str.starts_with("scratch/")
        || rel_str == "scripts/check-in-container.sh"
    {
        let _ = fs::set_permissions(&target, fs::Permissions::from_mode(0o755));
    }
}
project_state.record_file(&rel, ancestor_hash, ancestor_content);
```
Lift the 0o755 path rule (670-680) into a named helper used by staging, for state.json and for sidecars. `NamedTempFile` defaults to 0600 (Pitfall 4).

**7. Conflict gates to rewrite.** Pull at lines 509-516 and push at 726-731 both early-return on the first `Conflicted`. D-08 and D-10 replace them with collect-and-continue:
```rust
for s in &states {
    if s.status == SyncStatus::Conflicted {
        return PullResult::ConflictBlocked { path: s.relative_path.clone() };
    }
}
```

**8. Publication gate (Pitfall 3).** The staged check is at 881-929 (`git diff --cached --name-only` followed by `git show :path`). The committed check is `validate_committed_tree` at 1129-1182 (`diff-tree ... --name-only` followed by `git show HEAD:path`). Switch both to `--name-status`, keep the allowlist check on every entry, and skip the blob/secret scan for `D` entries. Keep the error strings `"Publication gate violation: ..."` and `"Post-commit publication gate violation: ..."`, because tests match on them.

**9. Direct push and rollback (D-18).** The direct-mode branch is at lines 1011-1038. Reuse `rollback_commit(remote_checkout)` (1110-1125) exactly as the existing post-commit failure path does (1022-1025):
```rust
if let Err(err) = validate_committed_tree(&self.sanitizer, remote_checkout, &expected_set) {
    rollback_commit(remote_checkout);
    return Err(err);
}
```
Change the push args at 1029 to `["push", "--porcelain", "origin", &format!("HEAD:refs/heads/{branch}")]`. `push_to_remote` returns `Result<String, String>` (line 844); it needs a small error enum to tell `UpstreamMoved` from a hard failure. `ensure_remote_repo` (416-491) returns `(PathBuf, RemoteLockGuard)` today and must also return `fetched_sha`.

**10. Path safety** (`validate_safe_write_target`, 1064-1107). Call it on every staged target and sidecar, again before `persist`, and in `resolve` to validate the `<file>` argument. Its `Component::Normal` check (1065-1070) is the input validation that `resolve` should reuse.

**11. Test fixtures.** Today's git fixture style (1685-1712) uses `-c` identity flags and asserts success only on some steps:
```rust
let commit_status = Command::new("git")
    .current_dir(fake_remote.path())
    .args([
        "-c", "user.name=Tester",
        "-c", "user.email=tester@example.com",
        "-c", "commit.gpgsign=false",
        "commit", "--no-gpg-sign", "-m", "init fake remote",
    ])
    .status()
    .unwrap();
assert!(commit_status.success());
```
Wave 0 adds `fn git_fixture(dir: &Path) -> Command` in this `mod tests` (line 1229). It scrubs the 15 local env vars, sets `GIT_CONFIG_GLOBAL` and `GIT_CONFIG_NOSYSTEM=1`, and fixture steps assert success. That replaces the `let _ = Command::new("git")...` lines at 1696, 1723-1737, 1846-1853, 1997-2027, 2135-2153, 2176-2198, and 2256-2294.

**Unit test shape for pull/push.** Use `test_pull_updates_conflict_blocked_no_partial_writes` (1423-1473) and `test_pull_updates_refuses_to_write_through_symlink` (2323-2373). The setup is: `tempdir()` local and upstream, `ProjectState::default()` + `record_file` + `save_to_dir`, write `upstream_dir/projects/generic/gitconfig.repo`, then `TemplateContext::detect_for_project(local, "test-proj", "rust")`. Each test asserts an explicit negative control with a message (`"Atomicity violation: ..."`). For `cfg(unix)` mode assertions, copy lines 1392-1397:
```rust
#[cfg(unix)]
{
    use std::os::unix::fs::PermissionsExt;
    let meta = fs::metadata(&loc_hook).unwrap();
    assert_eq!(meta.permissions().mode() & 0o111, 0o111, "Hook must be executable");
}
```
Tests to rewrite by decision: 1308 (`test_sync_status_deletions`), 1423 (D-08), 1476 (D-10), and 2065 (`test_multi_pull_merge_idempotence`, adjacent lines).

---

### `src/sync/state.rs` (model, JSON file-I/O)

**Analog:** itself (133 lines).

**Struct and serde style to extend additively** (lines 7-23):
```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileStateRecord {
    pub ancestor_hash: String,
    pub last_synced_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ancestor_content: Option<String>,
}
```
New fields (`conflict`, `tombstones`) follow the same `#[serde(default, skip_serializing_if = ...)]` pattern. `Default` hardcodes `version: 1` at line 28; replace it with a `CURRENT_STATE_VERSION` const.

**Load to replace (D-06)** (lines 38-47). It swallows every error:
```rust
pub fn load_from_dir(project_dir: &Path) -> Self {
    let state_path = project_dir.join(".ude/state.json");
    if state_path.exists()
        && let Ok(content) = fs::read_to_string(&state_path)
        && let Ok(state) = serde_json::from_str::<ProjectState>(&content)
    { return state; }
    Self::default()
}
```
The new version returns `Result<Self, String>`. A missing file gives `Ok(default)`; a read or parse error gives `Err`; `version > CURRENT` gives `Err`; v1 is migrated in memory.

**Save error-mapping style to keep** (lines 50-60). Swap the `fs::write` for `NamedTempFile::new_in(&ude_dir)` + write + `set_permissions(0o644)` + `persist(&state_path)`:
```rust
fs::create_dir_all(&ude_dir)
    .map_err(|e| format!("Failed to create .ude directory: {e}"))?;
let json = serde_json::to_string_pretty(self)
    .map_err(|e| format!("Failed to serialize state: {e}"))?;
fs::write(&state_path, json)
    .map_err(|e| format!("Failed to write state file {}: {e}", state_path.display()))?;
```

**Accessor style** (lines 62-96): `rel_path.to_string_lossy().to_string()` as the key, and timestamp via `SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs().to_string()).ok()` (77-80). New accessors (`record_conflict`, `clear_conflict`, `record_tombstone`, `tombstone`) copy this.

**Tests** (lines 99-133): roundtrip plus missing-file default. `assert_eq!(loaded.version, 1)` at lines 118 and 130 must change to the new const. `test_project_state_load_nonexistent_returns_default` is the negative control for the new corrupt-file and future-version tests. Ripple: 6 `load_from_dir` call sites in `src/sync/mod.rs` (310, 646, 823, plus tests 1405 and 1814) and `src/main.rs:520`.

---

### `src/sync/merge.rs` (utility, pure transform)

**Analog:** itself. Keep the public shape (enum outcome + one entry function + a `mod tests` of `match` assertions) and replace the body.

**Current public surface** (lines 1-7, 78). `similar` is imported only here:
```rust
use similar::{capture_diff_slices, Algorithm, DiffOp};

#[derive(Debug, PartialEq, Eq)]
pub enum MergeResult {
    Clean(String),
    Conflict,
}
pub fn merge_3way_text(base: &str, local: &str, upstream: &str) -> MergeResult {
```
Replace it with RESEARCH Pattern 6 (`MergeOutcome::{Clean(String), Conflict(String)}`, `merge_texts`, `relabel`). Delete `Edit`, `extract_edits`, and `edits_overlap` (9-73). The only external caller is `src/sync/mod.rs:254-256`.

**Test style to keep** (lines 195-207):
```rust
#[test]
fn test_merge_orthogonal_changes_clean() {
    let base = "a=1\nb=1\n";
    let local = "a=2\nb=1\n";
    let upstream = "a=1\nb=2\n";
    match merge_3way_text(base, local, upstream) {
        MergeResult::Clean(merged) => { assert_eq!(merged, "a=2\nb=2\n"); }
        MergeResult::Conflict => panic!("Expected clean merge for orthogonal changes"),
    }
}
```
This fixture must change to `a=1\nx\nb=1\n` (Pitfall 2). Add `test_adjacent_edits_conflict`, `test_merge_crlf`, and `test_relabel`. `test_merge_overlapping_changes_conflict_negative_control` (209-219), `test_merge_identical_changes_clean`, and `test_merge_disjoint_append` keep their fixtures.

---

### `src/sync/staging.rs` (NEW, optional, utility, transactional file-I/O)

**Analogs:** `src/sync/mod.rs:645-701` (current write loop, mode rule, error-to-`IoFailed` mapping) and `:1064-1107` (path validation). Nothing transactional exists today; use RESEARCH Pattern 5 for the stage → validate → commit ordering. Only create this file if `mod.rs` would otherwise grow far past its current 2.5k lines. If created, declare it as `pub mod staging;` (or a private `mod staging;`) next to `pub mod merge;` and `pub mod state;` at `src/sync/mod.rs:1-2`.

The error mapping to copy from `src/sync/mod.rs:654-668` keeps the `rel` path and a human-readable reason:
```rust
if let Some(parent) = target.parent()
    && let Err(e) = fs::create_dir_all(parent)
{
    return PullResult::IoFailed { path: rel, reason: format!("Failed to create parent directory: {e}") };
}
```
`validate_safe_write_target` is currently private (`fn`, line 1064). If staging moves to its own module, make it `pub(super)` or `pub(crate)`.

---

### `src/main.rs` (controller, CLI dispatcher)

**Analog:** itself.

**Git calls outside sync** (lines 281-302). All are best-effort `let _`. Route them through the runner on the local tier and keep the non-fatal semantics:
```rust
let _ = Command::new("git")
    .current_dir(&project_path)
    .args(["config", "--local", "include.path", "../.gitconfig"])
    .status();
```
Once all three are converted, `use std::process::Command;` (line 13) may become unused. Remove it if so (orphan rule).

**Init state baseline** (lines 305-334). `compute_hash(&bytes)` at 325 becomes `fingerprint`, and stored content is LF-normalized (D-07):
```rust
if let Ok(bytes) = fs::read(&p) {
    let hash = sync::SyncEngine::compute_hash(&bytes);
    let content = String::from_utf8(bytes).ok();
    project_state.record_file(rel, hash, content);
}
```

**Result rendering and exit codes** (lines 391-563). Every match arm prints with `colored` symbols (`"✓".green()`, `"!".yellow()`, `"✗".red()`, `"ℹ".blue()`) and calls `std::process::exit(1)` on failure:
```rust
sync::PullResult::ConflictBlocked { path } => {
    eprintln!("  {} Pull aborted: {} has conflicting local and upstream modifications.", "✗".red(), path.display());
    eprintln!("     Resolve local modifications or create a .local override before pulling.");
    std::process::exit(1);
}
sync::PullResult::IoFailed { path, reason } => {
    eprintln!("  {} Pull failed due to I/O error on {}: {}", "✗".red(), path.display(), reason);
    std::process::exit(1);
}
```
New split: drift, conflict, or skipped files exit 1; hard failures (`IoFailed`, `GitRemoteFailed`, the remote-ensure failure at 380-383, corrupt state) exit 2. Name the codes as constants (for example `EXIT_DRIFT = 1` and `EXIT_HARD_FAILURE = 2`), because operator rules forbid magic numbers. The status `match` (399-420) is exhaustive, so the compiler will flag the new `SyncStatus` arms. Keep the exact strings `"Locally Modified (ready to push)"` (405) and `"Failed to push to remote"` (536), because e2e tests assert them (tests/e2e_lifecycle.rs:420, 446).

**Ancestor write after a direct push** (lines 516-526). Record only the files that were actually published, and save through the new `Result`-returning load:
```rust
let mut project_state = sync::ProjectState::load_from_dir(&local_project);
for (rel, hash, content) in updated_records { project_state.record_file(&rel, hash, content); }
if let Err(e) = project_state.save_to_dir(&local_project) { eprintln!(...); }
```
`force` currently only sets `allow_agent_rules` (451). It must not reach D-10, D-17, or D-18 logic.

**New `Resolve` arm.** Follow the `cli::SyncAction::Pull` arm (422-446): a bold header with `println!`, an engine call, then a `match` on the outcome.

---

### `src/cli/mod.rs` (config, clap derive)

**Analog:** the `Push` variant (lines 108-117). Every variant has a `///` doc comment, and fields carry `#[arg(...)]`:
```rust
/// Push sanitized local updates back to the central repository
Push {
    /// Force push ignoring non-critical warnings
    #[arg(long)]
    force: bool,
    /// Open a GitHub Pull Request via gh instead of pushing directly to target branch
    #[arg(long)]
    pr: bool,
},
```
Add the new variant as a struct variant with a positional path:
```rust
/// Mark a conflicted file as resolved using the current local content
Resolve {
    /// Managed file path relative to the project root
    file: PathBuf,
},
```
`use std::path::PathBuf;` is already imported (`src/cli/mod.rs:2`).

---

### `src/template/mod.rs` (service, 2 git calls)

**Analog:** itself, lines 56-82. Keep the "fallback on any failure" semantics: a timeout or spawn error must still fall back to `"Developer"` / `"developer@example.com"`.
```rust
let git_user_name = std::process::Command::new("git")
    .current_dir(project_dir)
    .args(["config", "--get", "user.name"])
    .output()
    .ok()
    .and_then(|out| {
        if out.status.success() {
            Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
        } else { None }
    })
    .unwrap_or_else(|| "Developer".to_string());
```
Converted form: `crate::process::run_bounded(cmd, "git config user.name", LOCAL_TIMEOUT).ok().and_then(|out| ...)`. The closure body does not change. `detect_for_project` is called by every sync unit test, so this path runs under the hermeticity gate.

---

### `src/audit/mod.rs` (service, 5 git calls)

**Analog:** itself. All five sites use the same `.output().ok().map(...).unwrap_or_default()` chain. Keep that and only swap the spawn:
```rust
let gpg_format = std::process::Command::new("git")
    .args(["config", "--global", "--get", "gpg.format"])
    .output()
    .ok()
    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    .unwrap_or_default();
```
Sites: 128 and 146 (no `current_dir`; `--global` reads depend on `GIT_CONFIG_GLOBAL`, which the runner must NOT scrub), 199 (`include.path`), 224 (`core.hooksPath`), and 255 (`ls-files --stage`). All are on the local tier. The e2e doctor tests at tests/e2e_lifecycle.rs:74-230 are the regression control.

---

### `templates/projects/generic/gitignore.repo` (config template)

**Analog:** itself. Sections are a `# Heading` comment followed by patterns, separated by blank lines:
```
# Local agent ephemeral state
**/.claude/settings.local.json
**/.claude/scheduled_tasks.lock
**/.claude/checkpoints/
```
Add a section such as:
```
# UDE sync conflict sidecars (resolve with `ude sync resolve <file>`)
*.conflict
```
Note: no `{{` in the content. That matters because `scan_project` (lines 343-344) renders any upstream file containing `{{`.

---

### `tests/e2e_lifecycle.rs` (test, e2e binary)

**Analog:** `test_sync_push_remote_failure_does_not_advance_ancestor_state` (lines 313-475). Copy:
- `let bin = env!("CARGO_BIN_EXE_ude");` and `tempdir()` root (314-315).
- Bare remote + seed repo + `push origin main` (340-370), with the pre-receive hook written and chmod 0o755 under `cfg(unix)` (372-385).
- Fake home and gitconfig (387-396):
  ```rust
  fs::write(&gitconfig, "[user]\n  name = UDE Tester\n  email = tester@example.com\n[commit]\n  gpgsign = false\n").unwrap();
  ```
- Env injection on the binary call (402-410). Configure the spawned `Command`; never call `set_var`:
  ```rust
  let push_fail = Command::new(bin)
      .current_dir(&local_proj)
      .args(["sync", "push", "--remote", &bare_remote.to_string_lossy()])
      .env("HOME", &fake_home)
      .env("XDG_CONFIG_HOME", &fake_config)
      .env("GIT_CONFIG_GLOBAL", &gitconfig)
      .env("XDG_CACHE_HOME", &cache_dir)
      .output()
      .expect("ude sync push execution");
  ```
- State inspection via `serde_json::Value` (`state["files"]["mise.toml"]["ancestor_hash"]`, 328-333). The v2 schema must keep this path.
- An explicit negative control at the end (450-474).
- Exit-code assertions use `status.code()` (as in lines 88 and 118, `test_doctor_critical_security_exit_code_2...`). Use that for the new 1-vs-2 assertions. The existing test at 413 only asserts `!success()`, which stays compatible with exit 2.

New tests: `test_sync_ignores_inherited_git_dir` (adds `.env("GIT_DIR", decoy)` to the binary call) and `test_force_does_not_bypass_upstream_moved`. Add the Wave 0 `git_fixture` helper copy at file top, after the `use` lines (1-3). The raw `let _ = Command::new("git")` seeding at 342-370 should move to it.

---

### `Cargo.toml` (config)

**Analog:** the `[dependencies]` block. It uses bare string versions and inline tables only when features are needed (`clap = { version = "4.5", features = ["derive", "env"] }`):
```toml
tempfile = "3.17"
directories = "6.0"
similar = "2.7"
```
Change: remove `similar = "2.7"`, then add `diffy = "=0.5.2"` and `process-wrap = { version = "10.0.1", default-features = false, features = ["std", "process-group", "job-object"] }`. `tempfile` is already a normal (non-dev) dependency at `"3.17"` (the lockfile has 3.27.0), so no change is needed for `NamedTempFile`.

---

## Shared Patterns

### Error type: `Result<_, String>` with `format!` context
**Source:** `src/sync/state.rs:52-58` and `src/sync/mod.rs:422-423, 441`
**Apply to:** process.rs, state.rs, staging, sync/mod.rs, resolve
```rust
.map_err(|e| format!("Failed to create cache dir {}: {e}", cache_dir.display()))?;
```
Inline `{e}` captures, with path context via `.display()`. Never put full argv or remote URLs into messages.

### Engine returns data, CLI prints and exits
**Source:** `src/sync/mod.rs:32-51` (enums) and `src/main.rs:428-445` (rendering)
**Apply to:** every new outcome (deletions, conflict lists, skipped merged, `UpstreamMoved`, resolve). The engine already prints in one place, `ensure_remote_repo` line 436 (`println!("  Cloning ...")`). Do not add more.

### Path safety before every write
**Source:** `src/sync/mod.rs:1064-1107` (`validate_safe_write_target`)
**Apply to:** staged targets, `.conflict` sidecars, the `resolve <file>` argument, and deletions. Call it at stage time and again before `persist`/`remove_file`, as the current code does at lines 533 and 650.

### Executable-mode rule
**Source:** `src/sync/mod.rs:670-680`
**Apply to:** staged writes, sidecars (copy the local file's mode, default 0o644), and state.json (0o644). Lift it into one named helper so pull, sidecar, and state writes share it.

### Bounded subprocess plus env scrub
**Source:** new `src/process.rs` (RESEARCH Pattern 1)
**Apply to:** all 41 production `git`/`gh` sites: `src/sync/mod.rs` (31, lines 437-1159), `src/main.rs` (284, 290, 297), `src/template/mod.rs` (56, 70), and `src/audit/mod.rs` (128, 146, 199, 224, 255). This was rechecked with `rg -n 'Command::new\("(git|gh)"\)'` in this session; the non-sync counts match RESEARCH. `src/semantic/mod.rs:130` (curl) is out of scope (deferred to Phase 2).

### Hermetic test fixtures
**Source:** `tests/e2e_lifecycle.rs:387-410` (env on the spawned `Command`) and `src/sync/mod.rs:1702-1712` (`-c` identity flags)
**Apply to:** every new and rewritten git-fixture test, through the Wave 0 `git_fixture` helper. Never call `std::env::set_var`.

### Negative control in every test
**Source:** `src/sync/mod.rs:1463-1472` and `2366-2371`, `tests/e2e_lifecycle.rs:450-474`
**Apply to:** all new tests. Assertion messages name the violated property (`"Atomicity violation: ..."`).

## No Analog Found

| File | Role | Data Flow | Reason |
|------|------|-----------|--------|
| `src/process.rs` | utility | subprocess with deadline + group kill | No bounded runner or process-group code exists. Only the conventions (spawn, error style, poll loop) carry over. Use RESEARCH Pattern 1 for the body |
| `src/sync/staging.rs` (or the staging section inside `mod.rs`) | utility | transactional batch file-I/O | No temp + rename or stage/commit code exists. Every write today is a direct `fs::write`. Use RESEARCH Pattern 5 for the ordering |

## Notes for the Planner

- **Ripple from changing a signature to return `Result`:** `ProjectState::load_from_dir` has 7 call sites (5 in `src/sync/mod.rs`, 1 in `src/main.rs:520`, 1 in state.rs tests). `scan_project` has 10 (main.rs:398, mod.rs:506 and 721, and 7 in tests). Counted with `rg` this session.
- `compute_hash` has 18 uses in `src/sync/mod.rs` and 1 in `src/main.rs`. Only the content-hash uses become `fingerprint`. Lines 429 (cache key) and 940 (PR branch) must stay `compute_hash`.
- These counts are text matches, so they also include doc and test references. The compiler is the authoritative check once the signatures change.

## Metadata

**Analog search scope:** `src/**`, `tests/**`, `templates/projects/generic/`, `Cargo.toml`
**Files scanned:** 12 (state.rs and merge.rs read in full; cli/mod.rs (imports + lines 80-118), sync/mod.rs, main.rs, template, audit, semantic, and e2e read in targeted ranges)
**Pattern extraction date:** 2026-10-17

## PATTERN MAPPING COMPLETE
