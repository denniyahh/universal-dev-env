---
phase: 01-sync-engine-state-hermeticity
reviewed: 2026-10-20T15:45:00Z
depth: deep
files_reviewed: 13
files_reviewed_list:
  - README.md
  - deny.toml
  - mise.toml
  - scripts/hooks/pre-commit
  - scripts/hooks/pre-push
  - src/audit/mod.rs
  - src/main.rs
  - src/process.rs
  - src/sync/mod.rs
  - src/sync/staging.rs
  - src/sync/state.rs
  - templates/projects/generic/gitignore.repo
  - tests/e2e_lifecycle.rs
findings:
  critical: 2
  warning: 4
  info: 5
  total: 11
status: issues_found
---

# Phase 01: Code Review Report (incremental re-review)

**Reviewed:** 2026-10-20T15:45:00Z (HEAD 0e808cf, diff base 1ebc2a7)
**Depth:** deep
**Files Reviewed:** 13
**Status:** issues_found

**Scope note:** the evaluation-scope resolver reported `degraded` (reason `no-task-commits-since`) and widened the scope to the phase-directory range. The review therefore covers `git diff 1ebc2a7..HEAD` over the 13 listed files, with whole files read for context.

## Summary

This pass covers the fixes that followed the previous review and the external review. Those are: the CR-01, CR-02 and CR-03 fixes; init keeping prior records when re-run; init keeping git-tracked managed files on the first init of a fresh clone; symlink refusal on reads; init's missing-file warning, which now follows sync status; Windows cfg-gating; and the new supply-chain tooling.

The previous review's CR-01..CR-03 and WR-01 fixes hold as far as I traced them: boundary skips, the template-logic gate and the fd-settle probe. I re-ran none of their tests. Items already logged in `deferred-items.md` or the pending todos are not re-reported. That includes init-kept files being publishable, re-init with an explicit `--type`, the fresh-clone ancestor being the current template, and the doctor and gitignore-layer gaps.

Five scenarios were reproduced with `target/debug/ude` at HEAD in scratch projects, with HOME and git config isolated. The fixtures have been removed.
- **CR-01:** `ude init` still writes through symlinks. It overwrote a file outside the project and put hooks in a directory outside the project. The phase's symlink hardening covered reads only.
- **CR-02:** `ude init` replaces a non-UTF-8 `.gitignore` with just the graphify block, which drops rules such as `.env` and `secrets/`. This bug already existed before the phase.
- **WR-01:** a committed symlinked `.agents/AGENTS.md` (a common AGENTS.md -> CLAUDE.md layout) makes init report "Kept committed". After that, every sync command exits 2 for all 11 files.
- **WR-02:** the fresh-clone keep does nothing for a project in a git subdirectory (monorepo). Committed `mise.toml` and `.gitconfig` were overwritten.
- **WR-03:** re-running a plain `ude init` on a node project changes the recorded type to `rust`. The next pull then rewrites `mise.toml`.

What these runs do not establish: they ran on Linux only, and I did not run `cargo test` or clippy in this pass. The supply-chain hooks were checked by reading them plus one real run of the pinned `cargo-deny check bans licenses sources`, which exited 0. No negative control was run for gitleaks or cargo-deny in this pass.

## Critical Issues

### CR-01: `ude init` writes through symlinked managed paths, overwriting files outside the project

**File:** `src/main.rs:1406-1421` (`copy_template_file`: `fs::copy(src, dst)`), `src/main.rs:1423-1442` (`render_and_write`: `fs::write(dst, ..)`), `src/main.rs:305` (`.gitignore` append), `src/main.rs:254-269` (`create_dir_all` through a linked `scripts/`)

**Issue:** Commit 2023d5b refuses symlinks on the *source* side only: template reads, state.json and local reads in scan, pull and push. Init's *destination* writes still follow links. A managed path with no state record that is not git-tracked gets written through `fs::copy` / `fs::write`, which overwrite the link target. This applies to a symlinked file, or to a file under a symlinked directory (`scripts/`, `.agents/`, `scratch/`). The case of a fresh clone with an untracked symlink, or a project not under git, is reachable on the first init. Pull already refuses this through `validate_safe_write_target` (`test_pull_updates_refuses_to_write_through_symlink`), so init is the remaining write path that does not.

**Reproduced:** in a project with no git repo, `.agents/AGENTS.md -> <outside>/CLAUDE.md` holding "# precious shared rules", and `scripts -> <outside>/scripts`. `ude init --type rust` exits 0. The outside `CLAUDE.md` is replaced by the rendered agents template, and `commit-msg`, `pre-commit`, `pre-push` and `check-in-container.sh` are created in `<outside>/scripts`. After that, `ude sync status` exits 2 ("Refusing to follow symlink ./scripts"), so the damage happens before any check fires.

**Fix:** Validate every init destination with the same guard pull uses, before any directory is created or any file is written. Report the refused path and fail:
```rust
// in copy_template_file / render_and_write / the .gitignore append, before writing
let rel = dst.strip_prefix(&project_path).map_err(|_| format!("{} escapes the project", dst.display()))?;
let dst = sync::validate_safe_write_target(&project_path, rel)?; // refuses symlinked file or parent
```
Also check `scripts/`, `scripts/hooks/`, `scratch/` and `.agents/` with `refuse_symlink_under(&project_path, dir)` before the `create_dir_all` calls at 254-269. Add an e2e test for a symlinked destination file and one for a symlinked destination directory, each with a negative control showing a regular file is still written.

### CR-02: Init replaces an unreadable or non-UTF-8 `.gitignore` with only the graphify block (pre-existing)

**File:** `src/main.rs:297-308`

**Issue:** `fs::read_to_string(&target_gitignore).unwrap_or_default()` turns every read error into `""`, including `InvalidData` for non-UTF-8 bytes and `PermissionDenied`. The empty string does not contain `graphify-out/*`, so init writes `"\n" + append_block` over the file and loses every existing rule. Since CR-03, the kept `.gitignore` also gets the template as its ancestor (lines 487-505). The now-truncated file therefore reads as Locally Modified, so a plain `ude sync push` would publish this five-line `.gitignore` as the shared `gitignore.repo`. That publish step is from code trace; push was not run.

**Reproduced:** a `.gitignore` of `# caf\xe9 rules\nsecrets/\n.env\n` (Latin-1). `ude init` exits 0, and the file now holds only the graphify lines. `secrets/` and `.env` are no longer ignored, so a following `git add -A` would stage them.

**Fix:** Never write when the read failed. Treat the bytes as opaque and append to them:
```rust
let existing = match fs::read(&target_gitignore) {
    Ok(bytes) => bytes,
    Err(e) => {
        eprintln!("  {} Cannot read existing .gitignore: {e}", "✗".red());
        std::process::exit(EXIT_HARD_FAILURE);
    }
};
if !existing.windows(b"graphify-out/*".len()).any(|w| w == b"graphify-out/*") {
    let mut updated = existing;
    if !updated.ends_with(b"\n") { updated.push(b'\n'); }
    updated.extend_from_slice(append_block.as_bytes());
    // write via the CR-01 safe-target guard
}
```

## Warnings

### WR-01: One symlinked managed file disables every sync command, and init keeps a committed symlink as if sync could manage it

**File:** `src/sync/mod.rs:600` (`read_if_present(local_root, ..)?` aborts `scan_project`), `src/sync/mod.rs:1963-1982`; `src/main.rs:210-217` (`kept_committed` accepts any `symlink_metadata`), `src/main.rs:520-526`

**Issue:** `scan_project` propagates the symlink refusal with `?`. A single linked managed file therefore makes status, pull and push fail with exit 2 for all 11 files, and the tool offers no per-file way out. Init's fresh-clone keep counts a committed symlink (git mode 120000) as "present" and prints "Kept committed .agents/AGENTS.md (not overwritten); sync status compares it with the template". In fact sync status never runs to completion afterwards. `AGENTS.md -> CLAUDE.md` is a common committed layout.

**Reproduced:** commit `CLAUDE.md` and the link `.agents/AGENTS.md -> ../CLAUDE.md`, then run `ude init --type rust`. Init exits 0 and prints the "Kept committed" line. `ude sync status` exits 2 with "Refusing to follow symlink ./.agents/AGENTS.md".

**Fix:** Keep refusing to *follow* the link, but classify it per file instead of aborting the scan. One option is a `SyncStatus::Refused { reason }` (or reuse `Unmanaged`) that status prints and that push and pull skip with exit 1, following the D-10 pattern. In init, exclude symlinks from `kept_committed` and warn that sync will not manage that path:
```rust
.filter(|rel| fs::symlink_metadata(project_path.join(rel)).is_ok_and(|m| m.file_type().is_file()))
```

### WR-02: The fresh-clone keep is skipped for projects in a git subdirectory (monorepo), so init overwrites committed files

**File:** `src/main.rs:1495-1497` (`if !project.join(".git").exists() { return Ok(empty) }`)

**Issue:** `committed_paths` decides "not a git repo" by looking for `<project>/.git`. A project at `<repo>/svc` has no `.git` of its own. `committed` is then empty, nothing is kept, and the pre-42803ee overwrite returns for exactly the fresh-clone case 42803ee was meant to fix. Uncommitted edits to those committed files are lost as well.

**Reproduced:** in a repo with committed `svc/mise.toml` (node pin) and `svc/.gitconfig`, `ude init svc --type rust` exits 0 and prints no "Kept committed" line. `git status` shows both files modified. The control at the repo root (`ude init .`) keeps `mise.toml`.

**Fix:** Ask git rather than the filesystem. Run `git ls-files` unconditionally, and treat only a "not a git repository" result as "nothing committed":
```rust
let out = process::run_bounded(cmd, "git ls-files", process::LOCAL_TIMEOUT)?;
if !out.status.success() {
    let err = String::from_utf8_lossy(&out.stderr);
    if err.contains("not a git repository") { return Ok(HashSet::new()); }
    return Err(format!("git ls-files failed: {}", err.trim()));
}
```
The same `.git` check gates the git-registration block at line 425, which leaves `include.path` and `core.hooksPath` unset for subdirectory projects. That is consistent but is a separate gap.

### WR-03: A plain re-run of `ude init` silently overwrites the recorded project type with the CLI default `rust`; the next pull rewrites `mise.toml`

**File:** `src/main.rs:474` (`project_state.project_type = Some(init_args.project_type.clone())`), `src/cli/mod.rs:67` (`default_value = "rust"`)

**Issue:** Re-init promises that it "neither writes [a tracked file] nor replaces its record" (lines 181-184), but it unconditionally replaces `project_type` with whatever clap supplied. When `--type` is omitted, that value is the default `rust`. Status, pull and push take the type from state (`template/mod.rs:104-112`). The next pull then renders `mise.toml.jinja` for rust, sees the unedited node file as Upstream Updated, and replaces it, exiting 0. The deferred item "State-preserving re-init side effects" #1 covers a user who *explicitly* passes another `--type`. This trigger needs no intent to change type.

**Reproduced:** `ude init . --type node`, then `ude init .`. `state.json` now has `"project_type": "rust"`, status reports "mise.toml: Upstream Updated", and pull rewrites `node = "24"` to `rust = "latest"`.

**Fix:** Make `--type` an `Option<String>` with no default. On re-init, keep `prior_state.project_type` unless `--type` was given explicitly. On first init, fall back to detection or `rust`:
```rust
project_state.project_type = init_args.project_type.clone()
    .or(prior_state.project_type.clone())
    .or(Some("rust".into()));
```

### WR-04: The TypeSafe drift check reads `.agents/AGENTS.md` through symlinks and sends it to the external API before push's symlink refusal runs

**File:** `src/main.rs:880-906` (runs before `engine.push_updates` at 957)

**Issue:** The phase's stated symlink policy (`refuse_symlink_under` doc, `src/sync/mod.rs:1959-1962`) is that following a link "would copy or publish whatever it points at". The TypeSafe pre-step does `candidate_rules.exists()` and `fs::read_to_string(&candidate_rules)`, both of which follow links. It then transmits the content to the TypeSafe API whenever it differs from the raw `.jinja` text. Because local text is rendered and upstream text is raw, the two almost always differ. Only the secret pre-scan stands in the way, and it catches credential patterns, not private prose. Push's own symlink refusal fires afterwards, so it is too late to stop the transmission. This only applies when `TYPESAFE_API_KEY` is configured. Found by code trace; not exercised.

**Fix:** Before the read, call `sync::refuse_symlink_under(&local_project, &candidate_rules)` and do the same for `upstream_rules` under `root_template_dir`. On refusal, skip the semantic check with a warning. Better still, run the drift check only for files `push_updates` has already validated.

## Info

### IN-01: A local deletion of a templated file deletes the shared `.jinja` template; the CR-02 gate covers edits only

**File:** `src/sync/mod.rs:1250-1283`
**Issue:** Gate 2 runs only for `LocallyModified`. A local deletion of `mise.toml` reaches line 1276 and queues deletion of `mise.toml.jinja`, which holds the node, rust and generic branches every project renders from. Status and init do warn ("push deletes the shared template"), and D-14 specifies delete-on-push. Still, the reasoning behind CR-02 (one project must not erase logic shared by every consumer) applies to deletion even more strongly.
**Fix:** Decide explicitly. Either apply `template_has_logic` to `LocalDeleted` as well and skip with the CR-02 remedy, or record in D-14 that templated sources are intentionally deletable.

### IN-02: The managed-file list is still duplicated, and init's copy now drives keep, tracked and missing decisions

**File:** `src/main.rs:186-198` versus `src/sync/mod.rs:188-200` (`MANAGED_FILES`)
**Issue:** This is previous IN-05, still open. It now carries more weight: `managed_rels` decides `kept_committed`, the record carry-forward and `missing_tracked_files`. A path added only to `MANAGED_FILES` would be neither kept nor recorded by init.
**Fix:** `let managed_rels: Vec<PathBuf> = sync::MANAGED_FILES.iter().map(PathBuf::from).collect();`

### IN-03: When init's scan fails, the missing-file warning falls back to the "push will delete" prediction

**File:** `src/main.rs:532-549`, `src/main.rs:1258-1262`; `missing_tracked_files` at `src/main.rs:1225-1233` uses `fs::metadata`, which follows links
**Issue:** If `scan_project` errors, `status` is `None` and the consequence falls through to `PushBoundary::Publishable` => "a plain ude sync push will delete its shared template". Scan errors include the WR-01 symlink refusal and an unreadable template. In those same conditions push actually fails hard. A dangling managed-file symlink is reported as "missing" by `fs::metadata`, while scan refuses it.
**Fix:** When `status` is `None`, print "push outcome unknown: <scan error>" instead of a prediction, and use `symlink_metadata` in `missing_tracked_files`.

### IN-04: Symlink refusal is not applied uniformly on read paths that feed decisions

**File:** `src/sync/mod.rs:2122-2135` (`template_has_logic` uses a bare `fs::read`), `src/sync/mod.rs:2183-2192` (`check_for_override` uses `.exists()`), `src/template/mod.rs:108` (reads `.ude/state.json` through links for `project_type`)
**Issue:** These reads follow links, while `ProjectState::load_from_dir` refuses the same `state.json` link. `detect_for_project` can therefore pick `project_type` from a linked or dangling state file that state loading rejects. The impact is limited to classification and hints, since the hard reads and writes are guarded.
**Fix:** Route these through `refuse_symlink_under` (for `detect_for_project`, read via `ProjectState::load_from_dir`) so one policy decides.

### IN-05: Supply-chain tooling pins versions but not artifacts, and `install-hooks` silently shadows any global hooks path

**File:** `mise.toml:5-7`, `mise.toml:14-16`, `scripts/hooks/pre-commit`, `scripts/hooks/pre-push`
**Issue:** (a) `gitleaks = "8.30.1"` and `cargo-deny = "0.20.2"` pin versions with no committed `mise.lock`, so checksums depend on mise backend defaults rather than a reviewed lockfile. (b) `git config --local core.hooksPath scripts/hooks` overrides a developer's global `core.hooksPath`, which may carry signing or commit-msg checks. This repository ships no `commit-msg` hook, so those checks stop running here with no message. The hook scripts themselves are sound: `set -euo pipefail`, a missing mise or a missing pinned tool fails, `$(mise which ..)` aborts under `set -e`, both are committed 100755, and the pinned cargo-deny run exited 0 ("bans ok, licenses ok, sources ok").
**Fix:** Commit a `mise.lock` (`mise lock`), and have `install-hooks` warn when a global `core.hooksPath` is set, or chain to it.

---

_Reviewed: 2026-10-20T15:45:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: deep_
