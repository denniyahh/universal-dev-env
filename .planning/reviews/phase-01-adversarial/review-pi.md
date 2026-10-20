## CONFIRMED

### C1 — critical-path regression from the new ignore rule: a fresh clone's first `ude init` now overwrites every managed file instead of preserving recorded ones (warning)

**where:** `templates/projects/generic/gitignore.repo:32-34` (the added rule) + `src/main.rs:185` `let tracked = |rel: &str| prior_state.has_record(Path::new(rel));` + `src/main.rs:240-244` (`if !tracked(".gitconfig") && let Err(e) = copy_template_file(...)`).

**scenario (reproduced with the built binary):**
1. Machine A: `ude init`, append `# A LOCAL EDIT not yet pushed` to `.gitconfig` and `# A mise edit` to `mise.toml`, `git add -A && git commit`. `.ude/state.json` is **not** in the commit because line 34 now ignores it — verified: `git ls-files --error-unmatch .ude/state.json` → no, `git check-ignore -q` → yes.
2. Machine B: `git clone`, then `ude init --type rust` (the "natural setup step" from 01-VERIFICATION). Result: `tail -1 .gitconfig` changes from `# A LOCAL EDIT not yet pushed` to `    versiontag = v`, `tail -1 mise.toml` from `# A mise edit` to `run = "./scratch/ci-mac.sh"`. `git diff --stat` → `.gitconfig | 1 -`, `mise.toml | 1 -`. `ude sync status` immediately reports **`✓ .gitconfig: In Sync` / `✓ mise.toml: In Sync`**, and init printed no warning (`tracked but missing` count 0).
3. Control: same clone with A's `state.json` copied in (the pre-ignore-rule default) → both edits survive, empty `git diff`.

So the fix for WR-01 case 1 ("state.json committed") trades it for the mirror hazard ("state.json absent"): with no records, `tracked()` is false for all 11 paths and init unconditionally re-materialises/re-renders them. The discarded content is recoverable from git, but the next `git add -A && git commit` on B silently reverts A's commit, and nothing in the output says a file was replaced.

**why tests miss it:** `test_project_gitignore_ignores_per_machine_state` (`tests/e2e_lifecycle.rs:2371`) only asserts `git check-ignore`. 01-VERIFICATION truth 10 case 1 explicitly treats "B's init recreates both files" as the pass condition, so the behaviour is pinned as success; no test puts a local edit in the clone before the first init.

### C2 — init's per-file warning tests the conflict *record*, not the status push acts on, so it warns "push will delete the shared template" for a file push will skip (warning)

**where:** `src/main.rs:1156`
```rust
.filter(|rel| state.get_ancestor_hash(rel).is_some() && state.conflict(rel).is_none())
```
versus `src/sync/mod.rs:480` `(Some(_), None, Some(_)) => SyncStatus::Conflicted` and `src/sync/mod.rs:1212` `SyncStatus::Conflicted | SyncStatus::ConflictPending => { skipped_conflicts.push(...); continue; }`.

**scenario (reproduced):** `ude init`; `rm scripts/hooks/pre-push`; append `# upstream edit` to `templates/projects/generic/hooks/pre-push`; do **not** pull; `ude init`. Output:
```
! scripts/hooks/pre-push is tracked but missing; init does not recreate it, and a plain ude sync push will delete its shared template
```
Then `ude sync status` → `✗ scripts/hooks/pre-push: Conflicted (requires manual resolution)`; `ude sync push` → exit 1, `✗ ... unresolved conflict, skipped (run ude sync resolve scripts/hooks/pre-push)`, template byte-identical (verified `tail -1` = `# upstream edit`); `ude sync resolve scripts/hooks/pre-push` → exit 2, `No open conflict ... nothing to resolve`. The message asserts the exact opposite of what push does, and the one remedy it implies is unavailable.

**why tests miss it:** `test_reinit_does_not_warn_of_deletion_for_open_conflict` (`tests/e2e_lifecycle.rs:2517-2547`) first runs `ude sync pull`, which writes `state["conflicts"]`, so `state.conflict(rel).is_none()` is false. The record-less Conflicted row (local deleted + upstream edited, pull not yet run) — the common case — is never exercised; the guard is one call away from the classification it needs (`scan_project`/`compare_3way_hashes`).

### C3 — the same warning ignores the upstream side: a `BothDeleted` file is announced as a template deletion, and the printed recovery cannot do what it says (warning)

**where:** `src/main.rs:1155-1167` (only `get_ancestor_hash` + local `fs::metadata`), `src/main.rs:1175` (`"a plain ude sync push will delete its shared template"`), `src/main.rs:1198-1200` (`"...or copy {}), then check ude sync status"`).

**scenario (reproduced):** `ude init`; delete local `scripts/hooks/pre-push` **and** `templates/projects/generic/hooks/pre-push`; re-init → warning says a plain push deletes the shared template and tells the user to `copy templates/projects/generic/hooks/pre-push` — a path that does not exist. Actual state: `ude sync status` → `↓ scripts/hooks/pre-push: Deleted on both sides (pull to drop tracking)`; `ude sync push` → `Successfully synchronized 0 file(s)`, exit 0, nothing deleted. Following the advice by restoring the ancestor from `state.json` gives `↓ ... Deleted upstream (pull to remove)`, and `ude sync pull` then prints `✓ Removed ... (deleted upstream)`. "restore it … to keep it" cannot keep it: the only way to keep a file upstream has deleted is to *modify* it. The consequence text is derived from `push_boundary(rel)` alone (`src/main.rs:1174`) with no upstream lookup, even though `template` is already computed on the previous line.

**why tests miss it:** every new test keeps the upstream template present: `test_reinit_warns_that_push_deletes_template_of_missing_tracked_file` removes the local hook only, `test_reinit_missing_file_warning_follows_push_boundary` removes local `AGENTS.md`/`scratch`/`mise.toml` only. No test asserts anything about the `(ancestor, None, None)` row, which is the row D-16 exists for.

### C4 — the new step-line accounting only counts *tracked* missing files, so a first init with absent template sources still prints three ✓ lines and "Project initialization complete" (warning)

**where:** `src/main.rs:479` (`let missing = missing_tracked_files(&project_path, &prior_state, &managed_rels);`), `src/main.rs:509-535` (`absent` is computed from `missing` only, `if absent == 0 { println!("✓ {done}") }`, `src/main.rs:523` `"==> Project initialization complete."`), `src/main.rs:1319-1320` `if src.exists() { ... }` `Ok(())`, `src/main.rs:1345-1346` same in `render_and_write`.

**scenario (reproduced):** delete `templates/projects/agents/mise.toml.jinja` and `templates/projects/generic/hooks/pre-push`, run `ude init` on a fresh project:
```
✓ Configured tracked .gitconfig & chained scripts/hooks
✓ Initialized workflow tasks (mise.toml, scratch/)
==> Project initialization complete.
```
Actual: `ude sync status` → `ℹ scripts/hooks/pre-push: Not present locally or upstream (not tracked)`, `ℹ mise.toml: ... (not tracked)`; `state.json` holds 9 records, not 11. A missing `pre-push` means the guard silently does not run, and init claims it is configured.

**why tests miss it:** the new test asserts the *opposite* condition only for recorded files (`assert!(!init.contains("Configured tracked ..."), "init claims the hooks are in place")` in `test_reinit_warns_...`), because `missing` is sourced from `prior_state`. There is no negative record on a first init, so `missing` is empty and all three success lines print. The silent `if src.exists()` no-op is exactly the `render_and_write`/`copy_template_file` path the step lines are supposed to describe.

### C5 — the per-machine-state fix went into one of three ignore layers, and the doctor check cannot see the projects the other two produce (info; part of this is already listed in 01-VERIFICATION's anti-pattern table)

**where:** `templates/projects/generic/gitignore.repo:34` (added) vs `/gitignore:5-9`
```
# Local state & overrides
*.local.*
*.local
.ude/overrides/
.ude/cache/
```
(no `.ude/state.json`) and `template/.gitignore:23-27`, which ends at `!graphify-out/manifest.json` with no `.ude/` rule at all. `template/` is the copier template that produces the dev-environment repo — the layout `MANAGED_FILES` (`src/sync/mods.rs`… actually `src/sync/mod.rs:185-197`) describes (`scratch/`, `scripts/hooks/`, `.agents/AGENTS.md`, `mise.toml`). The review that requested the fix named both files: 01-REVIEW-REINIT WR-01 option (c) — "Neither the project `.gitignore` template nor this repo's `.gitignore` excludes `.ude/state.json`".

**scenario (reproduced):** project with a pre-existing `.gitignore` (what copier's `template/.gitignore` yields), `ude init` (keeps it, appends only the graphify block), `git add -A` → `.ude/state.json` in the index, `git check-ignore -q --no-index -- .ude/state.json` → exit 1. `ude doctor --repo .` → `✓ [Repository Baseline] Per-machine sync state (.ude/state.json)`, because `src/audit/mod.rs:368-375` only warns on `(Ok(true), Ok(true))` and `_ => CheckStatus::Pass` covers tracked-and-not-ignored. The `git rm --cached` remedy the check exists to print is unreachable in precisely the projects whose `.gitignore` init kept. Related artifact inconsistency: `.ude/overrides/` is machine-local by design (`docs/ARCHITECTURE.md:66`, `src/manifest/mod.rs:69` default exclude `.ude/overrides/**`), ignored by the repo's own `.gitignore`, but *not* by the project template — and `tests/e2e_lifecycle.rs:2674-2675` pins that with `// Negative control: committed .ude/ content is not ignored. assert!(!check_ignored(&git, &project, override_file));`.

**why tests miss it:** `test_doctor_reports_tracked_per_machine_state_with_remedy` reaches the warning only via `git add -f` while the ignore rule is present, and the only tracked-state negative control is after `git rm --cached` (untracked). Tracked + not-ignored is untested and returns `Pass`. (`01-VERIFICATION.md` anti-pattern row for `sync_state_tracking_status` already notes the tracked-not-ignored blind spot; the `template/.gitignore` omission and the `.ude/overrides/` mismatch are not recorded there.)

All six new e2e tests pass (`cargo test --test e2e_lifecycle -- test_reinit_warns test_reinit_missing test_reinit_does_not_warn test_doctor_reports test_project_gitignore` → 6 passed) while C1–C4 reproduce with the same binary.

## SUSPECTED

1. **Crash leftovers outside `.ude/` are neither ignored nor cleaned (info).** `src/sync/staging.rs:118` `tempfile::NamedTempFile::new_in(&parent)` stages beside *each* target, but `commit()` only renames and `Drop` is the only cleanup (`src/sync/staging.rs:229-234`) — a SIGKILL between stage and commit leaves the temp behind. The new rule covers only the state temp (`templates/projects/generic/gitignore.repo:36` `.ude/.tmp*`). Verified that `?? .tmpQ1w2E3` and `?? scripts/hooks/.tmpAbCdEf` appear in `git status` in an initialised project (check-ignore exit 1); I did not run an actual kill-mid-pull probe.
2. **Doctor's warning leaves the "all verified" claim and exit 0 intact (info).** `src/main.rs:138-144` only fails on `Critical`/`Fail`, so a project with a tracked state file prints the warning and then `==> Doctor check completed. All required baselines verified.` with exit 0. Same shape as the other repository warnings, but this one describes silent shared-template deletion on another machine.
3. **`missing_tracked_files` and the engine disagree on "absent" (info, speculation).** `src/main.rs:1159-1163` treats only `NotFound | NotADirectory` as missing, while `src/sync/mod.rs:1959-1967` (`read_if_present`) returns `Err` for every other error. An unreadable managed file (e.g. `chmod 000` `mise.toml`, root-owned file) therefore gets no init warning, and the next `ude sync status`/`push` returns a hard failure instead — the run after the warning is the one that fails, which is the ordering the warning exists to prevent.
4. **Remote projects get the warning computed against the wrong tree (info, speculation on `--remote`).** `src/main.rs:1173` resolves the template through `root_template_dir`, which comes from `find_templates_dir()` (`src/main.rs:1285-1317`: cwd `templates/`, then `CARGO_MANIFEST_DIR`, then the installed data dir), never from `--remote`. For a git-backed upstream, the consequence line and the "copy `<path>`" remedy describe the local/bundled templates, not the repository `push`/`pull` will actually touch. The new tests and 01-VERIFICATION both state `--remote` was not probed for these behaviours.
