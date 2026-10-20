---
phase: 01-sync-engine-state-hermeticity
reviewed: 2026-10-19T21:10:00Z
depth: standard
scope: fix diff a5675dd..HEAD (06f3c79, cdfa567, 956c5e7) for 01-REVIEW-REFIXES.md CR-01, WR-01, WR-02, IN-01, IN-02
files_reviewed: 4
files_reviewed_list:
  - src/main.rs
  - src/sync/mod.rs
  - src/sync/state.rs
  - tests/e2e_lifecycle.rs
findings:
  critical: 0
  warning: 3
  info: 1
  total: 4
status: issues_found
---

# Phase 01: Code Review Report (re-init / gate-reorder fix review)

**Reviewed:** 2026-10-19T21:10:00Z (HEAD 956c5e7, diff base a5675dd)
**Depth:** standard. I reproduced each finding with the HEAD binary and, as a negative control, with a binary built from a5675dd. Both ran in scratch projects with HOME, XDG and git config redirected and TYPESAFE_API_KEY unset.
**Files Reviewed:** 4
**Status:** issues_found

## Summary

**All five 01-REVIEW-REFIXES.md findings are closed. Nothing in this diff is a blocker. The diff does change three behaviours, all confirmed with the binary, and one of them answers your question about deleted files: yes, a plain push now deletes a shared template that re-init used to repair (WR-01).**

Closure of the prior findings:
- **CR-01 (init's graphify append on a tracked `.gitignore`):** closed. The append is skipped whenever `.gitignore` has any record.
- **WR-01 (status vs push for agent rules under template logic):** closed. Push now checks template logic before the `--force`-only rule.
- **WR-02 (re-init adopting local edits as ancestors):** closed. Any record is carried forward unchanged, and a tracked file is not rewritten.
- **IN-01 (re-init dropping open conflicts):** closed. Conflicts are carried forward. `resolve` works after re-init, and the sidecar notes survive.
- **IN-02 (tests not covering the gaps):** closed. Seven new e2e tests were added. I copied HEAD's `tests/e2e_lifecycle.rs` onto the a5675dd source and ran it: all 7 new tests fail there on their target assertion, not during setup. At HEAD they pass.

Regression probes:
- **First-time init is unchanged.** On a fresh `--type rust` init, the old and new binaries produce the same sorted `state.json`: 11 files, no tombstones, no conflicts, version 2. The hash includes ancestor content. Status is all In Sync.
- **The state written by init is still valid and atomic.** It is `prior_state.clone()` saved through `save_to_dir`, which stages a temp file and renames it. A migrated v1 state carries `version = 2` in the clone. Not re-tested here: the v1 re-init migration itself, which the previous review confirmed.
- **The push gate reorder is safe for the cases I tried:**
  - A path that is never published (`scratch/`) is still checked first. A local deletion there exits 1 and is never published.
  - A local deletion of `.agents/AGENTS.md` is still refused by a plain push (exit 1). `--force` deletes the template, which matches the status hint.
  - The template-logic check can now raise an I/O error for an agent-rules path even on a plain push. In practice that error cannot be reached: the scan reads the template first. With a mode-000 `agents.md.jinja`, both binaries fail identically at the scan (exit 2) and publish nothing.
- **`cargo test`** passes (135 unit tests and 30 e2e tests). **`cargo clippy --all-targets -- -D warnings`** exits 0.

What this does not establish:
- I did not test `--remote` (git-backed upstream) for any of the new behaviours. I used local `templates/` only.
- I did not test a re-init over a v1 state that has a tombstone or conflict. v1 has no such records, so this is likely moot.
- The fixes were proven only on the paths the tests exercise. The tests do not cover the three warnings below.

## Warnings

### WR-01: Re-init no longer repairs a missing tracked file and says nothing about it; a plain push then deletes the shared template, with no remaining ude command that restores the file

**File:** `src/main.rs:181-185` (the `tracked` closure, which counts a record as ownership even when the file is absent), `src/main.rs:237-344` (each materialization skipped when tracked), `src/main.rs:440-445` (record kept), and the unconditional success lines at `src/main.rs:476-488`.

**Issue:** You accepted that re-init no longer resurrects a locally deleted managed file (D-15), and I am not re-reporting that. This finding is about two consequences the operator may not have weighed.

1. **The case where nobody deleted anything.** Neither the project `.gitignore` template nor this repo's `.gitignore` excludes `.ude/state.json`, so it is committed by default. On a second machine, any publishable managed file that was not committed is "Deleted locally". Running `ude init` there, the natural setup step, now skips that file, and a plain `ude sync push` deletes the shared template with exit 0. Before this diff, init materialized the file and recorded it In Sync. The publishable paths are `.gitconfig`, `.gitignore`, the hooks, `scripts/check-in-container.sh` and `mise.toml`.
2. **No way back.** `pull` keeps the file deleted (D-15). `init` now skips it. `resolve` needs an open conflict. The only status hint is "push to propagate". So the user can restore a file only by copying the template by hand, or by rendering it by hand for `mise.toml` and `.agents/AGENTS.md`, whose templates are Jinja. Meanwhile init prints "Created project directories and scaffolding" as if it had done so.

**Failure scenario (confirmed with both binaries):**
- **Accidental delete.** Run init, `rm scripts/hooks/pre-push`, then re-init.
  - Old binary: the file is restored, status is In Sync, and push changes nothing.
  - New binary: the file is still absent. Status shows `scripts/hooks/pre-push: Deleted locally (push to propagate)`. `ude sync push` prints `Deleted upstream template for scripts/hooks/pre-push`, exits 0, and the template is gone.
- **Second machine.** Copy machine A's `state.json` into a project that has every managed file except `.gitconfig` and `scripts/hooks/pre-push`, then run init.
  - New binary: both files stay absent, and a plain push deletes `gitconfig.repo` and `hooks/pre-push` upstream.
  - Old binary: init materializes both, and push changes nothing.
  - Upstream recovery is `git revert` (D-14). This is recoverable, but it is silent data loss on a shared repository.

**Fix (needs your decision; each option keeps D-15):**
- (a) **Minimum.** Init lists every tracked managed file that is missing locally and warns that `ude sync push` will delete its shared template. Example: `! scripts/hooks/pre-push is tracked but missing; init leaves it to sync. ude sync push will delete the shared template; restore it to keep it`.
- (b) **Restore path.** Let init restore a tracked file only when it is missing and its ancestor still equals the current upstream template, which is the LocalDeleted row. Ask first or require a flag, for example `ude init --restore-missing`.
- (c) **Exclude the state file.** Add `.ude/state.json` to the project `.gitignore` template so that per-machine ancestor state is never committed. That removes case 1 at its root.

In every option, add an e2e test that deletes a tracked publishable file, re-runs init, and asserts what is printed and what a plain push does.

### WR-02: Re-running init with a different `--type` no longer applies it; `mise.toml` keeps the old toolchain while init reports success

**File:** `src/main.rs:321-332` (the `mise.toml` render is skipped when tracked) and `src/main.rs:425` (`project_type` overwritten in the carried state).

**Issue:** `state.project_type` is updated to the new type, but `mise.toml` is not re-rendered. The change takes effect only after a separate `ude sync pull`. If the user has edited `mise.toml`, it becomes a conflict instead. Init's output ("Initializing … (type: node)", "Initialized workflow tasks (mise.toml, …)") claims otherwise. Before this diff, re-init re-rendered `mise.toml` for the new type. It did so by overwriting local edits, and that part was the bug.

**Failure scenario (confirmed):** Run `init --type rust`, then `init --type node`.
- Old binary: `mise.toml` line 3 is `node = "24"`, and status is In Sync.
- New binary: line 3 is still `rust = "latest"`, and status shows `mise.toml: Upstream Updated (ready to pull)`.

**Fix:** when the requested type differs from `prior_state.project_type` and `mise.toml` is tracked, print a line telling the user to run `ude sync pull` to apply the new type (or that it will conflict if edited). Alternatively, refuse a type change on re-init without an explicit flag. Add a test for re-init with a changed type.

### WR-03: Re-init now stages the user's uncommitted edit of a tracked hook into the git index

**File:** `src/main.rs:402-419`. `git update-index --add --chmod=+x scripts/hooks/{hook}` still runs for every hook, including tracked hooks that init no longer writes.

**Issue:** `update-index` with a path stages that path's working-tree content. Before this diff, init first overwrote the hook with the template, which was the old destructive bug, so what it staged was template content. Now init keeps the local edit and silently stages it. The user's next `git commit` of unrelated files includes their work-in-progress hook change.

**Failure scenario (confirmed):** In a git repo, run init, commit, append `# wip` to `scripts/hooks/pre-push`, then re-init.
- New binary: `git diff --cached --name-only` shows `scripts/hooks/pre-push`, and the file contains `# wip`.
- Old binary: nothing is staged, and the edit is gone, overwritten.

**Fix:** run `update-index --chmod=+x` only for hooks init wrote in this run. For example, collect the written hook names in step 2 and loop over that list in step 3, or skip `tracked(&format!("scripts/hooks/{hook}"))`. Add the repro as a test.

## Info

### IN-01: Re-init no longer restores a tracked hook's lost executable bit, and sync does not detect it

**File:** `src/main.rs:278-289` (`set_executable` is skipped together with the copy for tracked hooks).

**Issue:** A hook that loses its exec bit stays non-executable after re-init. Status shows In Sync and pull does nothing, because hashes ignore the mode. Git then ignores the hook with only a hint, which silently disables the pre-commit and pre-push guards. Before this diff, re-init restored `rwx`.

**Confirmed:** after `chmod -x` and a re-init, the old binary gives `-rwxr-xr-x` and the new one `-rw-r-xr-x`. In both, status is In Sync and pull does nothing.

**Fix:** for tracked hooks and scripts that exist locally, still call `set_executable`. A mode change is not a content edit and does not affect the record.

---

_Reviewed: 2026-10-19T21:10:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
