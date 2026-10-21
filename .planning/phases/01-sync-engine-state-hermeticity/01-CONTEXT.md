# Phase 1: Sync Engine & State Hermeticity - Context

**Gathered:** 2026-10-16
**Status:** Ready for planning

<domain>
## Phase Boundary

Fix the Track 1 sync defects so `ude sync pull` / `ude sync push`:
- propagate deletions without resurrecting files (SYNC-01)
- never leave a half-written workspace on write failure (SYNC-02)
- cannot hang on `git` / `gh` (SYNC-03)
- ignore pure CRLF/LF differences when hashing (SYNC-04)
- report divergent edits as `.conflict` files with standard markers (SYNC-05)
- handle cleanly merged files and upstream races on push (SYNC-06)

**Bar for "done":** the acceptance tests and negative controls in `docs/REMEDIATION_PLAN.md` Track 1 (Phases 1.1–1.5). The bar was deliberately lowered on 2026-10-16: core functionality first. Stronger guarantees are deferred (see `<deferred>`).

Scale context: 18 template files, single user, on-demand CLI (no daemon).

</domain>

<decisions>
## Implementation Decisions

### Scope bar & libraries (decided before discussion, 2026-10-16)
- **D-01:** Target the remediation plan's Track 1 tests, not the stronger guarantees in `.planning/user/PHASE1_OPEN_DECISIONS.md`. Out: redb, on-disk write-ahead journal, power-loss fsync, tombstone TTL/dual-absence pruning.
- **D-02:** Use `diffy` (pin `=0.5.2` or `0.5`) for 3-way merge and conflict markers, with `ConflictStyle::Diff3` (LOCAL / ANCESTOR / UPSTREAM sections). It replaces the hand-written `merge_3way_text` in `src/sync/merge.rs`. The `similar` crate is used only there, so drop it if nothing else needs it.
  - Evidence (side-by-side test, 2026-10-16): the current merge turns CRLF into LF on clean merges, cannot emit markers, and silently auto-merges adjacent-line edits. diffy preserves CRLF, emits markers, and conflicts on adjacent edits.
  - **The adjacent-line change is intended:** it matches git.
- **D-03:** Use `process-wrap` 10.x (`std` feature): `ProcessGroup::leader()` on Unix, `JobObject` on Windows. On timeout, kill the whole process group.
  - Evidence (2026-10-16): with `std::process::Child::kill`, the output pipe stayed open for 3.7 s until the grandchild exited; group kill reached EOF in 0.3 ms.
  - `git` over https spawns `git-remote-https` (confirmed with `GIT_TRACE`), so a plain kill does not unhang a network op.
- **D-04:** Use std `File::lock` / `try_lock` (stable; toolchain is 1.99) to replace the hand-written `extern "C" flock` and the errno-11 check in `RemoteLockGuard` (`src/sync/mod.rs:55-136`). Do not add `fs4`.
- **D-05:** Single-file atomic writes use `tempfile::NamedTempFile` in the target's own directory, then `persist`. Multi-file pull follows the remediation plan's pattern: stage and validate every temp file first, abort before touching any target if any step fails, then rename. `state.json` is written last, the same way. No fsync and no on-disk journal (D-01).
- **D-06:** `ProjectState::load_from_dir` must return an error on unreadable or corrupt `state.json` instead of silently falling back to empty state (`src/sync/state.rs:38-47`).
- **D-07:** Bump the state-file `version` when the hash semantics change (CRLF normalization) or the schema changes (deletion/conflict records). On first load of an old version, migrate: re-hash, and treat an entry as in sync where the old raw hash matches. — **Reversibility:** one-way — existing `.ude/state.json` files on every machine get migrated; older `ude` binaries can't read the new version (they must fail loudly per D-06, not reset).
  - **Amended 2026-10-17 (operator, during plan-phase):** binaries already built cannot be made to fail loudly. No other machine runs an older `ude`, so: add a forward-version guard (a state file newer than the binary supports is an error, never a reset) and keep the v2 schema additive. "Fail loudly" applies to binaries built from this phase onward; this is an accepted limitation.

### Pull with conflicts
- **D-08:** When some files are clean and some conflicted, pull applies the clean files. Each conflicted file is left untouched, with `<file>.conflict` written beside it. Pull lists all conflicted files (not just the first) and exits 1.
  - This replaces the current all-or-nothing behavior. The test `test_pull_updates_conflict_blocked_no_partial_writes` (`src/sync/mod.rs:1423`) must be rewritten to match.
  - Clean files are still applied atomically as one group (D-05).
- **D-09:** Conflicts are resolved with a new explicit command, `ude sync resolve <file>`. It marks the current local content as resolved against the upstream version recorded when the conflict was detected, and removes the `.conflict` file. Deleting the `.conflict` by hand does **not** count as resolving. — **Reversibility:** costly — new CLI surface, and agents/scripts will start calling it.
  - **Clarified 2026-10-17 (operator, during plan-phase):** `resolve` takes no `--keep`/`--delete` flags. The local file's current state is the resolution: present → kept as current content (for D-13, kept as an unmanaged file); absent → resolved as a deletion.
  - **Amended 2026-10-17 (operator, during plan-phase):** pull also clears a conflict by itself when the local file has become identical to upstream ("conflict resolved by convergence"), removing the record and the sidecar. Hand-deleting the `.conflict` file still never resolves.
- **D-10:** While a file has an unresolved conflict, `ude sync push` skips that file only, publishes the rest, lists the unresolved files, and exits 1.
- **D-11:** Add `*.conflict` to the managed gitignore template (`templates/projects/generic/gitignore.repo`) so sidecars are never committed by accident.

### Deletion behavior
- **D-12:** Deleted upstream, local copy unchanged → pull deletes the local file, drops it from state, and reports it as deleted. *Amended 2026-10-21 (operator):* with a remote, pull deletes only when the remote's history shows the template's deletion; a template the remote never had reads Missing upstream and the file is kept.
- **D-13:** Deleted upstream, local copy edited → conflict. The local file is left untouched and reported as conflicted. `ude sync resolve` lets the user choose to keep it as an unmanaged file or delete it. This is the remediation plan's negative control.
- **D-14:** Deleted locally → push deletes the upstream template automatically and reports it. Undo is `git revert` in the upstream repo. Other machines then delete their copy on their next pull (D-12). *Amended 2026-10-21 (operator):* push never deletes a `.jinja` template that has template logic; it skips the deletion and says to restore the file or delete the template upstream directly.
- **D-15:** Deleted locally but not yet pushed → pull keeps it deleted (no resurrection) and reports "deleted locally — push to propagate". If upstream changed the file in the meantime, that is a conflict.
- **D-16:** Deleted on both sides → clean removal from state, never an empty 0-byte file (remediation plan 1.1 item 5: distinguish absent from empty).

### Push of merged files
- **D-17:** A file whose local and upstream edits merge cleanly is **not** published by push. Push publishes the other files, lists this one with "run `ude sync pull` first", and exits 1. Push never rewrites local working-tree files. This replaces today's silent "0 files synced" success.
- **D-18:** If upstream moved between UDE's fetch and its push, abort cleanly: publish nothing, undo the local cache commit (existing `rollback_commit`), exit 1 with "upstream changed, run `ude sync pull`". Never force-push.
  - **Scope 2026-10-17 (operator delegated to Claude during plan-phase):** applies to direct push only. `push --pr` pushes to a fresh `ude-sync-*` branch, cannot overwrite a moved base, and a stale base surfaces as a PR conflict.

### Claude's Discretion
- **Timeouts (SYNC-03):** tiered, within the requirement's 30–60 s range: about 30 s for local git queries and 60 s for network ops and `gh`. A single bounded-run helper replaces every `Command::new("git"/"gh")` call site. Every spawn sets `GIT_TERMINAL_PROMPT=0`, stdin to null, and scrubs inherited `GIT_DIR` / `GIT_WORK_TREE` / `GIT_INDEX_FILE` / `GIT_CONFIG` (remediation plan 1.5). Stdout and stderr are drained concurrently so a full pipe can't deadlock. A timeout reports which command timed out and after how long. Overriding the limits is not required.
- **Line endings (SYNC-04):** hash over LF-normalized bytes for text files and raw bytes for binary/non-UTF-8 files, keeping the existing binary handling. Writes preserve each file's existing line-ending style. Negative control: an LF file must never gain CRs.
- **`.conflict` content:** diffy diff3 output plus a short header naming the file and the command to run. Re-running pull while a conflict is open regenerates the sidecar against the latest upstream.
- **Exit codes:** 1 = drift/conflict/skipped files, 2 = hard failure. This is consistent with CLI-04 (Phase 3), but this phase doesn't enforce the contract globally.

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Requirements & acceptance tests
- `docs/REMEDIATION_PLAN.md` §Track 1 (Phases 1.1–1.5) — defect descriptions, implementation scope, and the **acceptance tests and negative controls that define done** (D-01)
- `.planning/REQUIREMENTS.md` — SYNC-01 … SYNC-06
- `.planning/ROADMAP.md` §Phase 1 — success criteria and the 4-plan split

### Background / rejected options
- `.planning/user/PHASE1_OPEN_DECISIONS.md` — the original option analysis (redb, tfio, tombstone policies, timeout tiers, sidecar vs inline). Superseded by the decisions above; read it for rationale only.
- `reviews/adversarial/review-codex.md` — source adversarial review (defect numbering)
- `.planning/codebase/CONCERNS.md` §Track 1 — concern summaries, including the stale-lock risk after SIGKILL (flock is released by the kernel on process death; verify the std `File::lock` replacement keeps that)

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `validate_safe_write_target` (`src/sync/mod.rs:1064`): anti-symlink check; run it on every staged target before the rename phase.
- `validate_committed_tree` and `rollback_commit` (`src/sync/mod.rs:1110-1181`): the secret gate and the commit undo; D-18 reuses `rollback_commit`.
- `SyncEngine::compute_hash` (`src/sync/mod.rs:155`, also called from `src/main.rs:325`): the place to add CRLF-aware hashing.
- `sanitize_branch_slug`, `RemoteLockGuard`: the lock path stays; swap the mechanism (D-04).

### Established Patterns
- Result enums `PullResult` / `PushResult` (`src/sync/mod.rs:33-50`) carry outcomes to `src/main.rs` (sync dispatch at ~341-510). New outcomes (multiple conflicts, deletions, skipped merged files, upstream moved) extend these enums rather than printing from inside the engine.
- `SyncStatus` (`src/sync/mod.rs:12`) needs deletion states (`LocalDeleted`, `UpstreamDeleted`, `BothDeleted`) and a pending-conflict state.
- Tests use offline git fixtures in `tempfile::TempDir` and must scrub `GIT_*` env (project constraint).
- Errors are `Result<_, String>` throughout sync/state; match that.

### Integration Points
- `src/main.rs:284-297`: `git` calls outside the sync module; these go through the bounded-run helper too.
- `src/main.rs:305`: builds `sync::ProjectState` directly; must follow schema/version changes (D-07).
- `src/semantic/mod.rs:130`: a `curl` subprocess. Not `git`/`gh`, so outside SYNC-03 wording; note for Phase 2 (TypeSafe guardrails).
- `src/cli/mod.rs:103` (`SyncAction`): add `Resolve` (D-09). Note that the existing `Push { --force }` flag is documented as "Force push ignoring non-critical warnings". It must not bypass D-18's upstream-moved abort or become a git force-push.
- `templates/projects/generic/gitignore.repo`: add `*.conflict` (D-11).

</code_context>

<specifics>
## Specific Ideas

- The user wants core sync working first and stronger guarantees only when a real need appears ("we can always add more later").
- Unattended AI agents run sync, so behavior must be non-interactive, report every affected file, and use exit codes that scripts can branch on.

</specifics>

<deferred>
## Deferred Ideas

- **Crash/power-loss durability:** an on-disk write-ahead journal with replay, fsync of files and directories (`atomic-write-file` 0.3.1 was evaluated and does both). Revisit if a real partial-write incident happens.
- **redb / binary state store:** revisit only if the state file becomes a performance problem (it isn't at 18 files).
- **Tombstone TTL / dual-absence pruning:** the ancestor record plus explicit deletion states (D-12–D-16) are expected to be enough.
- **Re-fetch-and-retry on an upstream race:** rejected for now (D-18). Revisit if multi-machine pushes collide often.
- **Timeout overrides via config or env:** not needed yet.
- **Bounding the `curl` call in `src/semantic/mod.rs`:** belongs with Phase 2 (TypeSafe guardrails).
- **Project-level `state.json` lock (accepted limitation, operator 2026-10-18):** `ude sync resolve` (D-09) reads, modifies and writes `.ude/state.json` without a lock, so a pull or push in the same project at nearly the same moment can overwrite its result: a resolve that doesn't stick, or a stale ancestor that later shows a false conflict. No local file content is lost and the next run re-detects it. This is the same class as today's unlocked local-templates mode, and it is accepted under D-01's single-user bar. Revisit if agents or scripts run sync commands concurrently in one project. A fix would reuse the 01-03 std file-lock guard on `.ude/`. (External adversarial review 2026-10-17, finding 8.)

</deferred>

---

*Phase: 01-sync-engine-state-hermeticity*
*Context gathered: 2026-10-16*
