# Phase 01 — External Adversarial Review (2026-10-20)

Scope: `git diff eaa0afe..HEAD` (HEAD 35e079e) of src/, tests/, templates/; snapshot review.
Lanes (all non-Claude, all completed, exit 0): agy (gemini-3.8-flash-high, 14 citations),
codex (gpt-5.6-terra high, read-only sandbox, 10 citations), pi (deepseek-flash, 14 citations).
Dropped lanes: none. Every item below was checked against source by the orchestrator;
"reproduced" means re-run with target/debug/ude at HEAD by the orchestrator.

## Confirmed — blocking
1. **Symlinks in the template tree are followed on pull** (codex, critical). `read_if_present` /
   `fs::read` follow symlinks (src/sync/mod.rs:1959-1966, :916). Reproduced: `gitconfig.repo` →
   host file; `ude sync pull` exit 0 wrote the host file's contents into the project `.gitconfig`.
   A compromised remote template repo can plant such a symlink. The push direction (local managed
   file is a symlink → its target is published) uses the same unchecked read — code trace only.
2. **First `ude init` on a fresh clone silently overwrites committed edits** (pi C1). Consequence
   of per-machine state.json (no records → every file "untracked" → init rewrites all).
   Reproduced: machine A commits a `.gitconfig` edit; clone on B; `ude init` replaced it, printed
   no warning, `ude sync status` says In Sync, `git diff` shows the edit removed.

## Confirmed — warnings
3. **Init's "push will delete its shared template" warning ignores upstream** (all three lanes).
   `missing_tracked_files` (src/main.rs:1150-1168) uses only the local record. Wrong for
   local-deleted + upstream-edited (push skips it as Conflicted; `resolve` says nothing to resolve)
   and for both-deleted (push deletes nothing; the "copy <template>" remedy names a missing path).
4. **Dangling symlinks read as deleted** (agy, codex). NotFound from a dangling managed-file symlink
   classifies as LocalDeleted, so a plain push deletes the shared template; a dangling
   `.ude/state.json` symlink loads as empty state, contrary to D-06 (src/sync/state.rs:126-128).
5. **First init reports success when template sources are missing** (pi C4, pre-existing):
   `copy_template_file` / `render_and_write` no-op when `src` is absent (src/main.rs:1320, :1341),
   yet init prints the ✓ lines and "complete".
6. **Per-machine state ignore rule is in only one of the gitignore layers** (pi C5):
   `template/.gitignore` (the copier template) and this repo's `.gitignore` lack `.ude/state.json`;
   doctor passes tracked-and-not-ignored (src/audit/mod.rs:368-375). Extends the existing deferred item.
7. **Staged temp files outside `.ude/` are not ignored** (agy, pi; already noted by the executor):
   `NamedTempFile::new_in(&parent)` beside each target (src/sync/staging.rs:118).

## Disputed (checked; not defects)
- **agy #1 "push failure without rollback bricks the cache / falsely says no changes"** — false.
  Every remote action re-runs fetch + `checkout -B` + `reset --hard origin/<branch>` + `clean -fd`
  (src/sync/mod.rs:777-814) before staging, so an unpushed cache commit is discarded (T-01-31).
- **agy #2 "BothDeleted writes a tombstone, violating D-16"** — by design: plan 01-07 row 5
  specifies "remove files record, tombstone(A, false)"; the files record is dropped, the tombstone
  blocks resurrection.

## Suspected / residual risks
- Check-then-use symlink validation in staging/state writes is not descriptor-anchored (codex) —
  needs a concurrent attacker inside the project/template tree.
- `resolve` removes the sidecar before committing state.json (agy) — the failure message says so;
  offline the sidecar cannot be regenerated until the next pull.
- Doctor's state warning keeps exit 0 and "All required baselines verified" (pi).
- Init's warning resolves templates from the local/bundled dir, never `--remote` (pi, speculation).

## Disposition (2026-10-20, operator: fix 1-4 now, log the rest)
- 1 fixed: 3b70773 (red), 2023d5b — every sync/init read refuses a symlink at the path or a parent
  below its root.
- 2 fixed: 3b70773 (red), 42803ee — first init keeps managed files git tracks; ancestor = what init
  would have written from the template.
- 3 fixed: 3b70773 (red), 4dd94e4 — the warning follows the file's sync status (conflict,
  both-deleted, deletion).
- 4 fixed by 2023d5b (dangling file and dangling state.json links are refused).
- 5, 6, 7 and the residual risks: logged in the phase's deferred-items.md.
