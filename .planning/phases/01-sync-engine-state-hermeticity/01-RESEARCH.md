# Phase 1: Sync Engine & State Hermeticity - Research

**Researched:** 2026-10-16
**Domain:** Rust CLI sync engine — 3-way file reconciliation, atomic filesystem materialization, bounded subprocess execution, git publication safety
**Confidence:** HIGH for library APIs and current-code findings (verified by compiled probes and file reads this session); MEDIUM for proposed data-model shapes (they are design recommendations, not facts)

## Summary

The locked decisions (D-01..D-18) are implementable with the three chosen building blocks: `diffy =0.5.2`, `process-wrap 10.0.1` (feature `std`), and std `File::lock`. Each was checked this session with a throwaway probe crate. Group kill reached pipe EOF immediately after a 300 ms timeout; leader-only kill left the pipe open for 29.7 s. diffy keeps CRLF on clean merges and adds no CR to LF files. std `try_lock` returns `WouldBlock` across handles in one process, the kernel releases the lock when the holder gets SIGKILL, and spawned children do not inherit it.

Several findings change what the planner must schedule beyond the roadmap's 4-plan sketch:
1. **Normalizing the hash alone does not fix SYNC-04.** `scan_project` classifies text files by string equality of contents (`compare_3way_with_merge`), not by hash. A CRLF-only difference still reads as a modification unless classification moves to fingerprints.
2. **Two existing tests encode the old adjacent-line auto-merge** and fail under diffy (`test_merge_orthogonal_changes_clean`, `test_multi_pull_merge_idempotence`). Probe output confirms diffy conflicts on them, which D-02 intends.
3. **The publication gate rejects deletions.** `git show :path` / `git show HEAD:path` exit 128 for a deleted path, so D-14 (push deletes upstream) fails until the gate reads `--name-status` and skips `D` entries.
4. **`tempfile::NamedTempFile::persist` leaves the target at mode 0600** (probe: was 644, became 600). Every staged write must set permissions before persisting.
5. **The test suite is not hermetic today.** With `GIT_DIR` pointing at a decoy repo, 6 sync tests fail. With `GIT_CONFIG_GLOBAL=/dev/null`, 1 sync test and 1 e2e test fail. TESTING.md claims fixtures scrub `GIT_*`; `rg env_remove` finds none in `src/` or `tests/`.
6. **D-07's reversibility note asks for something code cannot deliver.** Old binaries silently fall back to empty state on parse failure, and their serde structs accept unknown fields. "Older binaries must fail loudly" cannot be enforced from new code; only a forward-version guard in the *new* binary can.

**Primary recommendation:** Build one crate-level bounded-process module first (it touches 41 production call sites). Then move classification to "classify by LF-normalized fingerprint, merge by normalized content, write back in the local file's line-ending style". Then run every pull and push write through one stage→validate→commit path that sets permissions explicitly and writes `state.json` last.

## Architectural Responsibility Map

Single-binary CLI. The "tiers" here are internal layers, not network tiers.

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| 3-way classification (deletion states, CRLF-insensitive) | Sync engine (`src/sync/mod.rs`) | State ledger (`src/sync/state.rs`) | Classification needs ancestor + local + upstream; ledger only stores |
| Fingerprint (LF-normalized SHA-256) | Sync engine (new fn beside `compute_hash`) | `src/main.rs` init (caller) | One definition, used by scan/pull/push/init |
| Text merge + conflict rendering | Merge module (`src/sync/merge.rs`, diffy) | — | Pure function over three texts |
| Atomic staging / commit of workspace writes | Sync engine (new staging helper) | Filesystem (`tempfile`) | Same-dir temp + rename; engine owns ordering |
| State schema, versioning, migration, corrupt-file errors | State ledger (`src/sync/state.rs`) | — | D-06, D-07 |
| Bounded `git`/`gh` execution, env scrub, group kill | New crate-level module (e.g. `src/process.rs`) | All callers: sync, template, audit, main | Used outside `src/sync` (template/audit/main), so must not live under sync |
| Remote cache lock | Sync engine (`RemoteLockGuard`) | std `File::lock` | D-04 |
| Publication gate (staged/committed tree validation incl. deletions) | Sync engine (`push_to_remote`, `validate_committed_tree`) | git | Existing gate, extended |
| Upstream-race detection (D-18) | Sync engine (`push_to_remote`) | git server (non-FF rejection) | Server-side rejection is the atomic check |
| Outcome reporting + exit codes (1 drift/conflict, 2 hard failure) | CLI dispatcher (`src/main.rs`) | Result enums from engine | Engine returns data; never prints outcomes (established pattern) |
| `ude sync resolve <file>` | CLI (`src/cli/mod.rs`) + engine method | State ledger | D-09 |

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

#### Scope bar & libraries (decided before discussion, 2026-10-16)
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

#### Pull with conflicts
- **D-08:** When some files are clean and some conflicted, pull applies the clean files. Each conflicted file is left untouched, with `<file>.conflict` written beside it. Pull lists all conflicted files (not just the first) and exits 1.
  - This replaces the current all-or-nothing behavior. The test `test_pull_updates_conflict_blocked_no_partial_writes` (`src/sync/mod.rs:1423`) must be rewritten to match.
  - Clean files are still applied atomically as one group (D-05).
- **D-09:** Conflicts are resolved with a new explicit command, `ude sync resolve <file>`. It marks the current local content as resolved against the upstream version recorded when the conflict was detected, and removes the `.conflict` file. Deleting the `.conflict` by hand does **not** count as resolving. — **Reversibility:** costly — new CLI surface, and agents/scripts will start calling it.
- **D-10:** While a file has an unresolved conflict, `ude sync push` skips that file only, publishes the rest, lists the unresolved files, and exits 1.
- **D-11:** Add `*.conflict` to the managed gitignore template (`templates/projects/generic/gitignore.repo`) so sidecars are never committed by accident.

#### Deletion behavior
- **D-12:** Deleted upstream, local copy unchanged → pull deletes the local file, drops it from state, and reports it as deleted.
- **D-13:** Deleted upstream, local copy edited → conflict. The local file is left untouched and reported as conflicted. `ude sync resolve` lets the user choose to keep it as an unmanaged file or delete it. This is the remediation plan's negative control.
- **D-14:** Deleted locally → push deletes the upstream template automatically and reports it. Undo is `git revert` in the upstream repo. Other machines then delete their copy on their next pull (D-12).
- **D-15:** Deleted locally but not yet pushed → pull keeps it deleted (no resurrection) and reports "deleted locally — push to propagate". If upstream changed the file in the meantime, that is a conflict.
- **D-16:** Deleted on both sides → clean removal from state, never an empty 0-byte file (remediation plan 1.1 item 5: distinguish absent from empty).

#### Push of merged files
- **D-17:** A file whose local and upstream edits merge cleanly is **not** published by push. Push publishes the other files, lists this one with "run `ude sync pull` first", and exits 1. Push never rewrites local working-tree files. This replaces today's silent "0 files synced" success.
- **D-18:** If upstream moved between UDE's fetch and its push, abort cleanly: publish nothing, undo the local cache commit (existing `rollback_commit`), exit 1 with "upstream changed, run `ude sync pull`". Never force-push.

### Claude's Discretion
- **Timeouts (SYNC-03):** tiered, within the requirement's 30–60 s range: about 30 s for local git queries and 60 s for network ops and `gh`. A single bounded-run helper replaces every `Command::new("git"/"gh")` call site. Every spawn sets `GIT_TERMINAL_PROMPT=0`, stdin to null, and scrubs inherited `GIT_DIR` / `GIT_WORK_TREE` / `GIT_INDEX_FILE` / `GIT_CONFIG` (remediation plan 1.5). Stdout and stderr are drained concurrently so a full pipe can't deadlock. A timeout reports which command timed out and after how long. Overriding the limits is not required.
- **Line endings (SYNC-04):** hash over LF-normalized bytes for text files and raw bytes for binary/non-UTF-8 files, keeping the existing binary handling. Writes preserve each file's existing line-ending style. Negative control: an LF file must never gain CRs.
- **`.conflict` content:** diffy diff3 output plus a short header naming the file and the command to run. Re-running pull while a conflict is open regenerates the sidecar against the latest upstream.
- **Exit codes:** 1 = drift/conflict/skipped files, 2 = hard failure. This is consistent with CLI-04 (Phase 3), but this phase doesn't enforce the contract globally.

### Deferred Ideas (OUT OF SCOPE)
- **Crash/power-loss durability:** an on-disk write-ahead journal with replay, fsync of files and directories (`atomic-write-file` 0.3.1 was evaluated and does both). Revisit if a real partial-write incident happens.
- **redb / binary state store:** revisit only if the state file becomes a performance problem (it isn't at 18 files).
- **Tombstone TTL / dual-absence pruning:** the ancestor record plus explicit deletion states (D-12–D-16) are expected to be enough.
- **Re-fetch-and-retry on an upstream race:** rejected for now (D-18). Revisit if multi-machine pushes collide often.
- **Timeout overrides via config or env:** not needed yet.
- **Bounding the `curl` call in `src/semantic/mod.rs`:** belongs with Phase 2 (TypeSafe guardrails).
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| SYNC-01 | `pull_updates` tracks deletion tombstones in `.ude/state.json` to propagate upstream deletions cleanly without resurrecting deleted files. | §Pattern 3 (deletion classification table), §Pattern 4 (schema v2: `tombstones` map + per-file `conflict`), Pitfall 3 (publication gate rejects deletions), Pitfall 8 (`""` ≠ absent) |
| SYNC-02 | Synchronization materialization uses a two-phase transactional staging directory before atomically updating project working trees. | §Pattern 5 (stage→validate→commit), Pitfall 4 (0600 permissions after persist), Pitfall 7 (dirs created during staging), verified `persist` failure semantics |
| SYNC-03 | All subprocess executions (`git`, `gh`) enforce bounded timeouts (30s-60s) with active process cancellation to prevent hangs. | §Pattern 1 (bounded runner), 41 call sites listed, probe evidence for group kill, env scrub list from `git rev-parse --local-env-vars`, Pitfall 6 (SIGTTIN for ssh prompts), Pitfall 9 (credential leakage in error text) |
| SYNC-04 | Cryptographic SHA-256 ancestor hash calculation normalizes CRLF line endings to prevent false drift detection across platforms. | §Pattern 2 (fingerprint + classify-by-hash), Pitfall 1 (string-equality classification path), D-07 migration recipe, keep raw `compute_hash` for cache keys |
| SYNC-05 | Divergent 3-way conflicts generate structured `.conflict` files with standard Myers conflict markers while preserving user modifications. | §Pattern 6 (diffy wrapper + label rewrite), verified hardcoded labels `ours`/`original`/`theirs`, `IncompleteHunkStyle::Git`, Pitfall 2 (two tests to rewrite) |
| SYNC-06 | `push_to_remote` reconciles push candidates safely and verifies branch head continuity before publishing. | §Pattern 7 (continuity + `--porcelain` rejection parse, verified output format), D-17 push report, D-18 rollback |
</phase_requirements>

## Project Constraints (from CLAUDE.md / GEMINI.md / operator rules)

No `./CLAUDE.md` in the repo. `config.json` sets `claude_md_path: ./.agents/GEMINI.md`, but `.agents/` does not exist; the root `GEMINI.md` was read instead. Directives that bind this phase:

- Rust Edition 2024, zero clippy warnings: `cargo clippy --workspace --all-targets -- -D warnings` (baseline is clean as of this session) [VERIFIED: ran it].
- Hermetic tests: `tempfile::TempDir` fixtures, scrub `GIT_DIR` / `GIT_WORK_TREE` / `GIT_INDEX_FILE` / `GIT_CONFIG` (and the rest of git's local-env list) on every spawned git. **Never** call `std::env::set_var` / `remove_var` in tests; configure env on the spawned `Command` instead.
- Every test needs a negative control: a case that must produce the opposite result.
- A failing assertion must print a failure, not hang. Timeout tests need an elapsed bound shorter than the hang they guard against.
- Bound every subprocess with a timeout. Stop processes by PID or group handle, never by name pattern.
- Prefer Rust-native dependencies (all three chosen crates are pure Rust; process-wrap pulls `nix`).
- Never emit raw credentials and never bypass publication gates (`GEMINI.md` Security & Integrity).
- Errors in sync/state are `Result<_, String>` (CONTEXT code_context). Match that style.
- No `dnf` (immutable Fedora). Tool installs are user-space (`cargo install`, `rustup target add`).
- Use `rg`, not `grep`, in any scripted verification.
- Conventional Commits; `cargo fmt` and clippy before commit; `?` instead of `.unwrap()` outside tests; `///` docs on public items.
- `config.json`: `tdd_mode: true`, `nyquist_validation: true`, `security_enforcement: true` (ASVS L1, block on high), `use_worktrees: true`, `parallelization: true`.

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `diffy` | `=0.5.2` (2026-08-31, MSRV 1.85) | 3-way merge, diff3 conflict markers | Locked D-02. Default features `[]`; only dep `hashbrown` [VERIFIED: crates.io API + `cargo tree` in probe] |
| `process-wrap` | `10.0.1` (2026-09-23, MSRV 1.87) | Spawn in a new process group / Job Object; group kill | Locked D-03. watchexec project, ~831k downloads/wk [VERIFIED: crates.io API + gsd package-legitimacy OK] |
| `tempfile` | `3.27.0` (already a normal dependency, `Cargo.lock` pins 3.27.0) | `NamedTempFile::new_in` + `persist` for atomic writes | Locked D-05. Already present: no new dependency [VERIFIED: Cargo.toml + Cargo.lock read] |
| std `File::lock` / `try_lock` | stable since Rust 1.89; toolchain is 1.99.0 | Remote cache lock | Locked D-04 [CITED: doc.rust-lang.org/std/fs/struct.File.html] [VERIFIED: probe] |
| `sha2` | 0.10.9 (present) | Fingerprints | Unchanged |

### Supporting
None new. `serde`/`serde_json` (present) handle the schema change.

### Alternatives Considered
Not applicable. Libraries are locked by D-02..D-05. `similar` 2.7.0 is **removed**: its only use is `src/sync/merge.rs:1` [VERIFIED: `rg similar` over src/ and tests/ shows only that line].

**Cargo.toml changes:**
```toml
[dependencies]
diffy = "=0.5.2"
process-wrap = { version = "10.0.1", default-features = false, features = ["std", "process-group", "job-object"] }
# remove: similar = "2.7"
```
Why `default-features = false`: process-wrap's defaults are `creation-flags, job-object, kill-on-drop, process-group, process-session, tracing`, and `std` is **not** among them. `kill-on-drop` has no std implementation (`src/std.rs` exports only `ChildWrapper, CommandWrap, CommandWrapper, CreationFlags, JobObject, JobObjectChild, ProcessGroup, ProcessGroupChild, ProcessSession, ResetSigmask`). `tracing` would add an unused dependency [VERIFIED: read `process-wrap-10.0.1/Cargo.toml [features]` and `src/std.rs:12-27`]. The probe built and ran with exactly this feature set [VERIFIED: probe].

Why `=0.5.2` and not `0.5`: the conflict text (labels `ours`/`original`/`theirs`, LF marker terminators) is asserted in tests and post-processed by our label rewrite. A patch release could change it; 0.5.1 → 0.5.2 shipped six weeks apart [VERIFIED: crates.io version list]. This is my recommendation inside D-02's "pin `=0.5.2` or `0.5`" allowance.

## Package Legitimacy Audit

| Package | Registry | Age | Downloads | Source Repo | Verdict | Disposition |
|---------|----------|-----|-----------|-------------|---------|-------------|
| diffy | crates.io | first published 2020-06-08 | ~375k/wk | github.com/bmwill/diffy | OK | Approved |
| process-wrap | crates.io | first published 2024-03-11 | ~831k/wk | github.com/watchexec/process-wrap | OK | Approved |
| tempfile | crates.io | first published 2015-04-14 | ~16.5M/wk | github.com/Stebalien/tempfile | OK | Approved (already present) |
| similar | crates.io | 2021-01-24 | ~4.6M/wk | github.com/mitsuhiko/similar | OK | To be REMOVED (unused after diffy) |

Source: `gsd-tools query package-legitimacy check --ecosystem crates diffy process-wrap tempfile similar`, run this session. Crates have no npm-style postinstall step; build scripts were not audited. A `cargo tree` in the probe showed `nix 0.31.3` and `indexmap` under process-wrap and `hashbrown 0.17.1` under diffy.

**Packages removed due to [SLOP] verdict:** none
**Packages flagged as suspicious [SUS]:** none

## Architecture Patterns

### System Architecture Diagram

```
 ude sync pull                                   ude sync push [--pr] [--force]          ude sync resolve <file>
      |                                                  |                                        |
      v                                                  v                                        v
 [RemoteLockGuard: File::try_lock poll, 10s] --> [ensure_remote_repo] --(bounded git: clone/fetch 60s,      [load state v2]
      |                                         checkout/reset/clean 30s)--> fetched_sha = origin/<b>         |
      v                                                  |                                           [conflict record?]--no--> exit 2
 [ProjectState::load_from_dir -> Result]  (v1 -> migrate to v2 in memory; corrupt -> Err exit 2; newer version -> Err)
      |                                                  |                                           yes: ancestor := recorded upstream
      v                                                  v                                                 remove sidecar, clear record
 [scan_project] per managed path:                                                                          (local absent + upstream deleted -> clean drop;
   local bytes  --fingerprint (LF-normalized if UTF-8)-->                                                   local present + upstream deleted -> tombstone "kept unmanaged")
   upstream bytes (render jinja) --fingerprint-->         classify by fingerprints (ancestor, local, upstream, tombstone, conflict)
   ancestor fp from state                                  |
      |      +-----------------+------------------+-------+---------+--------------+-------------+
      v      v                 v                  v                 v              v             v
   InSync  UpstreamModified  LocallyModified  LocalDeleted  UpstreamDeleted  BothDeleted  both-modified
                                                                                            |
                                                                    text? --> diffy merge on LF-normalized texts
                                                                            Ok  -> Merged(merged, re-styled to local EOL)
                                                                            Err -> Conflicted(diff3 text, labels rewritten)
 PULL:                                                   PUSH (push_updates -> push_to_remote):
  stage temps (same dir, mode set) for                    skip Conflicted/pending-conflict (D-10), skip Merged (D-17),
  UpstreamModified + Merged; stage deletions               skip tombstoned/Overridden; LocallyModified -> secret gate,
  (UpstreamDeleted); stage sidecars for Conflicted         parameterize, write into cache checkout; LocalDeleted -> remove
      | any staging error -> drop temps, Err (exit 2)      upstream template (D-14)
      v                                                        |
  commit: rename temps -> delete files -> sidecars           [publication gate: staged + committed, --name-status, skip D blobs]
      -> state.json temp+persist LAST                           |
      v                                                   continuity: HEAD~1 == fetched_sha  --no--> rollback_commit, UpstreamMoved
  PullReport{applied, deleted, conflicts,                       |
   locally_deleted_pending}  -> main.rs exit 0/1           git push --porcelain origin HEAD:refs/heads/<b> (60s)
                                                                | "!" + "[rejected]" -> rollback_commit, UpstreamMoved (exit 1)
                                                                | other failure -> hard failure (exit 2)
                                                                v
                                                          main.rs records ancestors for published files only (direct mode);
                                                          PushReport lists skipped files -> exit 1 if any skipped
```

### Recommended Project Structure
```
src/
├── process.rs          # NEW: bounded runner (process-wrap), env scrub, timeout tiers, RunOutput/RunError
├── sync/
│   ├── mod.rs          # classification, pull/push/resolve, staging commit, RemoteLockGuard (std lock)
│   ├── state.rs        # schema v2, migration from v1, load -> Result, atomic save
│   ├── merge.rs        # diffy wrapper: merge_texts(), render_conflict() with label rewrite
│   └── staging.rs      # OPTIONAL split: StagedWrite/StagedTxn (keeps mod.rs from growing past 2.5k lines)
├── template/mod.rs     # 2 git call sites -> process.rs
├── audit/mod.rs        # 5 git call sites -> process.rs
└── main.rs             # 3 git call sites -> process.rs; new report rendering; exit codes 1/2
```
`process.rs` sits at crate level because `template`, `audit`, and `main.rs` call git too. Under `sync/` it would invert the module dependency.

### Pattern 1: Bounded subprocess runner (SYNC-03, D-03)
**What:** One function that every production `git`/`gh` spawn goes through. It scrubs repo-local git env, sets non-interactive env, nulls stdin, pipes stdout and stderr into two reader threads, polls `try_wait` against a deadline, and on timeout kills the whole group and reaps it.
**When to use:** All 41 production call sites: 30 `git` + 1 `gh` in `src/sync/mod.rs` before line 1228, 3 in `src/main.rs` (284, 290, 297), 2 in `src/template/mod.rs` (56, 70), and 5 in `src/audit/mod.rs` (128, 146, 199, 224, 255) [VERIFIED: `rg -c` and an independent `grep -c` both gave 31 for sync/mod.rs lines 1–1227; other sites from `rg -n 'Command::new\("(git|gh)"\)'`]. The Discretion text says "every `Command::new("git"/"gh")` call site", so the audit and template sites are in scope. Test-fixture git calls are not production; they get their own scrubbed helper (Wave 0).

**Verified API (process-wrap 10.0.1, std):** `CommandWrap::from(Command)`, `.wrap(ProcessGroup::leader())` (unix) / `.wrap(JobObject)` (windows), `.spawn() -> io::Result<Box<dyn ChildWrapper>>`. `ChildWrapper` provides `stdout(&mut self) -> &mut Option<ChildStdout>`, `stderr(..)`, `id()`, `start_kill()`, `try_wait() -> io::Result<Option<ExitStatus>>`, `wait() -> io::Result<ExitStatus>`. For the unix group child, `start_kill` is `killpg(pgid, SIGKILL)`, and `wait` waits for the leader, then `waitpid(-pgid)` until `ECHILD` [VERIFIED: docs.rs trait page + read `src/std/process_group.rs`]. **There is no blocking wait-with-timeout and no std kill-on-drop.** Poll `try_wait`, and kill explicitly on every exit path, including the error paths.

**Env to scrub:** use git's own list rather than the four names in the remediation plan:
`GIT_ALTERNATE_OBJECT_DIRECTORIES GIT_CONFIG GIT_CONFIG_PARAMETERS GIT_CONFIG_COUNT GIT_OBJECT_DIRECTORY GIT_DIR GIT_WORK_TREE GIT_IMPLICIT_WORK_TREE GIT_GRAFT_FILE GIT_INDEX_FILE GIT_NO_REPLACE_OBJECTS GIT_REPLACE_REF_BASE GIT_PREFIX GIT_SHALLOW_FILE GIT_COMMON_DIR` [VERIFIED: output of `git rev-parse --local-env-vars`, git 2.56.0]. Do **not** scrub `GIT_CONFIG_GLOBAL`, `GIT_CONFIG_NOSYSTEM`, `HOME`, or `XDG_*`: the e2e tests rely on `GIT_CONFIG_GLOBAL` [VERIFIED: tests/e2e_lifecycle.rs:202,291]. Set `GIT_TERMINAL_PROMPT=0` [VERIFIED: `git help git`], and for `gh` set `GH_PROMPT_DISABLED=1` and `GH_NO_UPDATE_NOTIFIER=1` [VERIFIED: `gh help environment`].

**Example (skeleton; names are proposals):**
```rust
// Source: API verified against process-wrap 10.0.1 source + probe run 2026-10-16
use process_wrap::std::{ChildWrapper, CommandWrap};
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const LOCAL_TIMEOUT: Duration = Duration::from_secs(30);
pub const NETWORK_TIMEOUT: Duration = Duration::from_secs(60);
const POLL_INTERVAL: Duration = Duration::from_millis(10);
const GIT_LOCAL_ENV_VARS: &[&str] = &[ /* the 15 names above */ ];

pub struct RunOutput { pub status: std::process::ExitStatus, pub stdout: Vec<u8>, pub stderr: Vec<u8> }

/// `label` is a redacted description such as "git fetch" — never the full argv (may contain token URLs).
pub fn run_bounded(mut cmd: Command, label: &str, timeout: Duration) -> Result<RunOutput, String> {
    for var in GIT_LOCAL_ENV_VARS { cmd.env_remove(var); }
    cmd.env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut wrapped = CommandWrap::from(cmd);
    #[cfg(unix)] { wrapped.wrap(process_wrap::std::ProcessGroup::leader()); }
    #[cfg(windows)] { wrapped.wrap(process_wrap::std::JobObject); }
    let mut child = wrapped.spawn().map_err(|e| format!("Failed to spawn {label}: {e}"))?;
    let out_reader = spawn_reader(child.stdout().take());
    let err_reader = spawn_reader(child.stderr().take());
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) if start.elapsed() >= timeout => {
                let _ = child.start_kill();          // killpg(SIGKILL) / TerminateJobObject
                let _ = child.wait();                // reap leader + group
                let _ = (out_reader.join(), err_reader.join());
                return Err(format!("{label} timed out after {}s and was terminated", timeout.as_secs()));
            }
            Ok(None) => std::thread::sleep(POLL_INTERVAL),
            Err(e) => { let _ = child.start_kill(); let _ = child.wait(); return Err(format!("{label}: wait failed: {e}")); }
        }
    };
    let _ = child.start_kill(); // leader exited; reap stragglers still in the group (ESRCH ignored)
    let stdout = out_reader.join().map_err(|_| format!("{label}: stdout reader panicked"))?;
    let stderr = err_reader.join().map_err(|_| format!("{label}: stderr reader panicked"))?;
    Ok(RunOutput { status, stdout, stderr })
}

fn spawn_reader<R: Read + Send + 'static>(pipe: Option<R>) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || { let mut buf = Vec::new(); if let Some(mut p) = pipe { let _ = p.read_to_end(&mut buf); } buf })
}
```
Planner notes:
- The **timeout must be a parameter**, not hard-coded inside, so tests can pass 300 ms. Production call sites pass `LOCAL_TIMEOUT` or `NETWORK_TIMEOUT`. That is internal parameterization, not the deferred user-facing override.
- Network tier (60 s): `clone`, `fetch`, `push`, `ls-remote`, and every `gh` call. Local tier (30 s): everything else.
- **Test-env injection:** give `SyncEngine` an optional extra-env list (for example `Vec<(OsString, OsString)>`) that the runner applies after scrubbing. Unit tests can then pass `GIT_CONFIG_GLOBAL=<fixture>` and `GIT_CONFIG_NOSYSTEM=1` for the cache commit in `push_to_remote` without `set_var`. Today `test_direct_push_after_pr_mode_resets_branch_and_pushes_target` fails under `GIT_CONFIG_GLOBAL=/dev/null` because it relies on the host's identity [VERIFIED: ran it].
- Whether stragglers get killed after a normal leader exit is my recommendation (pipes cannot stay open forever). `git gc --auto` daemonizes with `setsid` and leaves the group, so it is unaffected [ASSUMED].

### Pattern 2: Fingerprint and classify-by-hash (SYNC-04, D-07)
**What:** `fingerprint(bytes)` is SHA-256 over `bytes` with `\r\n` → `\n` applied when `bytes` is valid UTF-8, and over raw bytes otherwise. It mirrors the existing text/binary split, where `std::str::from_utf8(..).ok()` decides text (`src/sync/mod.rs:333-336`). **Classification of all files uses fingerprints only.** Contents are used only to *merge* when both sides changed.
**Why:** Today, when local, upstream, and ancestor content are all `Some`, `scan_project` calls `compare_3way_with_merge`, which compares `local_content == upstream_content` and `local_content == Some(o)` as raw strings (`src/sync/mod.rs:229, 239-240, 373-382`). A hash-only fix leaves that path CRLF-sensitive [VERIFIED: read lines 222-281, 373-398].
- Keep `compute_hash` raw and unchanged for **non-content keys**: the cache-directory key `format!("{remote_url}#{branch}")` (`src/sync/mod.rs:428-430`) and the PR branch name (`:940`). Changing it would orphan the live caches in `~/.cache/ude` (8 entries exist on this machine) [VERIFIED: `ls ~/.cache/ude`].
- Normalize only `\r\n`. A lone `\r` is content.
- Store `ancestor_content` LF-normalized in v2. Then the merge base and the fingerprint describe the same text.
- **Write-back style:** detect the local file's dominant EOL (count `\r\n` against bare `\n`; tie or no newlines → LF). Convert content from `\n` to `\r\n` before staging only when the local file is CRLF-dominant. A file that does not exist locally is written as upstream renders it.
- **Merge:** diffy runs on normalized LF texts, and the local style is re-applied to the result. That satisfies "CRLF orthogonal edits retain `\r\n`" and "LF file must not acquire CR" even when the two sides differ in EOL style. Running diffy on mixed-EOL inputs would conflict on every line, because diffy compares lines with their terminators.

**D-07 migration recipe (v1 → v2, in memory on load; persisted on next save):** for each record:
- If `ancestor_content: Some(text)`: `ancestor_hash = fingerprint(text)` and `ancestor_content = normalize(text)`. This is exact and needs no local file.
- Else (binary): the hash is unchanged, because the raw hash equals the fingerprint for non-UTF-8 bytes.
- Fallback when a text record has no content: if `sha256_raw(local_bytes) == old ancestor_hash`, set `ancestor_hash = fingerprint(local_bytes)`. This is CONTEXT's "treat an entry as in sync where the old raw hash matches".
- `version > CURRENT_VERSION` → `Err("state.json version N was written by a newer ude")` (forward guard). Unknown or unparseable → `Err` (D-06). Missing file → `Ok(default)`.

### Pattern 3: Deletion-aware classification (SYNC-01, D-12..D-16)

`fp` = fingerprint; `A` = ancestor record; `T` = tombstone; `C` = open conflict record.

| ancestor | local | upstream | Result | Pull action | Push action |
|----------|-------|----------|--------|-------------|-------------|
| A | = A | absent | `UpstreamDeleted` | delete local, drop record, write tombstone (D-12) | none |
| A | ≠ A (present) | absent | `Conflicted` | leave local; sidecar; conflict record (upstream=None) (D-13) | skip (D-10) |
| A | absent | = A | `LocalDeleted` | no-op; report "deleted locally — push to propagate" (D-15) | remove upstream template; drop record after successful publish (D-14) |
| A | absent | ≠ A | `Conflicted` | leave absent; sidecar; conflict record (D-15 second clause) | skip |
| A | absent | absent | `BothDeleted` | drop record; never create a file (D-16) | drop record (no publish needed) |
| none, T | present | absent | `Unmanaged` (new status) | none | **skip**: never republish a path deleted upstream |
| none, T | any | present | clear T, then classify as no-ancestor | (upstream re-added the file) | |
| none | present | absent | `LocallyModified` (new file) | none | publish |
| none | absent | present | `UpstreamModified` (new file) | write | none |

- `Merged` and `Conflicted` arise only when ancestor, local, and upstream are **all present** and differ. Absent is never passed to diffy as `""`. Probe: `merge("base\n", "", "")` returns `Ok("")`, which is exactly the 0-byte resurrection the remediation plan describes [VERIFIED: probe].
- A present but empty local file (`""`) is a modification, not a deletion. Use `Option<Vec<u8>>` end to end.
- **Why a tombstone map, given that the ancestor record already drives most cases:** D-13 lets the user "keep it as an unmanaged file". Without a record, the next push classifies (no ancestor, local present, upstream absent) as `LocallyModified` and republishes the file. That is precisely the resurrection SYNC-01 forbids. Writing a tombstone on D-12 as well gives success criterion 1 a crisp test: pull the deletion, recreate the file locally, and push must not republish it. "Drops it from state" (D-12) is honored: the `files` record is removed, and the tombstone lives in a separate map. No TTL (D-01). This shape is my recommendation, not a locked decision.
- The existing `test_sync_status_deletions` (`src/sync/mod.rs:1308-1341`) asserts the *old* mapping (`LocallyModified` / `UpstreamModified` for deletions) and must be rewritten. That is intended.

### Pattern 4: State schema v2 (D-06, D-07, D-09, SYNC-01)
Current definitions (state.rs lines 7–33), verbatim:
```rust
pub struct FileStateRecord {
    pub ancestor_hash: String,
    pub last_synced_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ancestor_content: Option<String>,
}
pub struct ProjectState {
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_type: Option<String>,
    pub files: BTreeMap<String, FileStateRecord>,
}
// Default: version: 1, project_type: None, files: BTreeMap::new()
```
[VERIFIED: src/sync/state.rs:7-33]

Recommended additive v2 (keeps the `files` map shape, which `tests/e2e_lifecycle.rs` reads as `state["files"]["mise.toml"]["ancestor_hash"]`):
```rust
pub const CURRENT_STATE_VERSION: u32 = 2;
pub struct FileStateRecord {
    pub ancestor_hash: String,                       // v2: fingerprint (LF-normalized for text)
    pub last_synced_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ancestor_content: Option<String>,            // v2: LF-normalized
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conflict: Option<ConflictRecord>,            // D-09: upstream version captured at detection
}
pub struct ConflictRecord { pub upstream_hash: Option<String>, pub upstream_content: Option<String>, pub detected_at: Option<String> }
pub struct Tombstone { pub deleted_hash: String, pub deleted_at: Option<String>, pub kept_unmanaged: bool }
pub struct ProjectState {
    pub version: u32,
    pub project_type: Option<String>,
    pub files: BTreeMap<String, FileStateRecord>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tombstones: BTreeMap<String, Tombstone>,
}
```
- A D-13 conflict (upstream deleted, local edited) needs a `conflict` record on a path whose ancestor still exists, so the record stays in `files` until it is resolved.
- `load_from_dir(&Path) -> Result<ProjectState, String>` ripples to `scan_project`, `pull_updates`, `push_updates`, and `main.rs:520`. `scan_project` must return `Result` too (today it calls `load_from_dir` at `src/sync/mod.rs:310`).
- `save_to_dir` uses `NamedTempFile::new_in(.ude)`, writes, sets mode, and calls `persist(.ude/state.json)`.

### Pattern 5: Stage → validate → commit (SYNC-02, D-05, D-08)
```rust
// Source: tempfile 3.27 API verified by probe 2026-10-16
struct StagedWrite { rel: PathBuf, target: PathBuf, temp: tempfile::NamedTempFile }
// Stage: for each write candidate
//   target = validate_safe_write_target(local_root, rel)?          // existing anti-symlink check (mod.rs:1064)
//   reject if symlink_metadata(target).is_dir()                     // turns rename-time EISDIR into a staging error
//   create_dir_all(parent) — record dirs we created (remove on abort if still empty)
//   temp = NamedTempFile::new_in(parent)?; write_all(bytes)?
//   temp.as_file().set_permissions(mode)?   // existing target's mode, else 0o755 for scripts/hooks/scratch, else 0o644
// Stage sidecars (<rel>.conflict) the same way, also through validate_safe_write_target.
// Any error -> return Err; dropping StagedWrite deletes every temp (verified: temp removed on drop).
// Commit (only after ALL staging succeeded):
//   for w in writes: validate_safe_write_target again; w.temp.persist(&w.target)
//   for d in deletions: fs::remove_file(target)
//   sidecars persist; then state.json staged+persisted LAST
```
- Verified `persist` semantics: on success the target is replaced and the content is new. On failure (target is a directory) it returns `PersistError { error, file }`, the temp file still exists, and dropping `file` deletes it [VERIFIED: probe]. `rename(2)` over a **symlink** target replaces the link and does not write through it (victim file outside the project stayed unchanged) [VERIFIED: probe]. The parent-directory symlink check in `validate_safe_write_target` is still required, because `new_in(parent)` follows a symlinked parent.
- Temp files are named `.tmpXXXXXX` in the target directory [VERIFIED: probe printed `.tmpLtDcGV`]. Tests can assert "no leftovers" by walking the project for names starting with `.tmp`.
- A failure in the rename phase (file 2 renamed, file 3 fails) still leaves a partial update. D-01/D-05 accept this (no journal). The staging-time directory and symlink checks push almost every predictable failure into staging, where nothing has been touched yet.
- **Root-proof fault injection for tests:** make `scripts` a regular file in the local project. Staging `scripts/hooks/*` then fails with ENOTDIR regardless of uid, while `.gitconfig` and `.gitignore`, which come earlier in `managed_files` order, would already be staged. chmod-based injection does not work when tests run as root.

### Pattern 6: diffy wrapper and `.conflict` sidecar (SYNC-05, D-02)
```rust
// Source: diffy 0.5.2 src/merge/mod.rs (read) + probe outputs 2026-10-16
use diffy::{ConflictStyle, IncompleteHunkStyle, MergeOptions};

pub enum MergeOutcome { Clean(String), Conflict(String) }

pub fn merge_texts(ancestor_lf: &str, local_lf: &str, upstream_lf: &str) -> MergeOutcome {
    let mut opts = MergeOptions::new();
    opts.set_conflict_style(ConflictStyle::Diff3)
        .set_incomplete_hunk_style(IncompleteHunkStyle::Git); // markers always start a line
    match opts.merge(ancestor_lf, local_lf, upstream_lf) {     // ours = local, theirs = upstream
        Ok(merged) => MergeOutcome::Clean(merged),
        Err(marked) => MergeOutcome::Conflict(relabel(&marked)),
    }
}

/// diffy hardcodes the labels; rewrite only exact full marker lines.
fn relabel(text: &str) -> String {
    text.split_inclusive('\n').map(|line| match line {
        "<<<<<<< ours\n" => "<<<<<<< LOCAL\n",
        "||||||| original\n" => "||||||| ANCESTOR\n",
        ">>>>>>> theirs\n" => ">>>>>>> UPSTREAM\n",
        other => other,
    }).collect()
}
```
Verified diffy behavior [VERIFIED: probe + source]:
- `MergeOptions::new()` defaults to `ConflictStyle::Diff3`, `conflict_marker_length = 7`, and `IncompleteHunkStyle::Diff3`. `merge(&str, &str, &str) -> Result<String, String>`, where `Err` carries the marked text.
- Labels are hardcoded: `Some("ours")`, `Some("original")`, `Some("theirs")` (`output_result`, src/merge/mod.rs:616-642). There is no label API, which is why `relabel` is needed.
- Marker lines always end in `\n`, even inside CRLF content (`"<<<<<<< ours\nl\r\n||||||| original\n..."`). Because we merge LF-normalized text, this does not matter.
- With the default `IncompleteHunkStyle::Diff3`, a file with no trailing newline gets markers glued onto the last line (`"l||||||| original\n"`). `IncompleteHunkStyle::Git` inserts the newline instead.
- Clean merge of CRLF orthogonal edits → `"a=2\r\nb=1\r\nc=1\r\nd=2\r\n"`. LF orthogonal → no `\r`. Adjacent-line edits → `Err` (conflict). Edits separated by one unchanged line → clean. Identical edits → clean.

Sidecar content: a header (file path, "resolve with: `ude sync resolve <path>`", "LOCAL = your working copy, ANCESTOR = last synced, UPSTREAM = central template") followed by the relabeled diff3 body.
- Deletion conflicts: render with `""` for the absent side and say "(deleted upstream)" or "(deleted locally)" in the header. This is only a report, so using `""` is fine here, unlike in classification.
- Binary conflicts: header only, with both fingerprints.
- Sidecar mode copies the local file's mode (default 0o644). A secret-bearing 0600 file must not produce a world-readable sidecar.

### Pattern 7: Push continuity and upstream-race detection (SYNC-06, D-17, D-18)
- After fetch, `ensure_remote_repo` records `fetched_sha = git rev-parse origin/<branch>` and returns it alongside the path and lock.
- Direct mode, after the cache commit: assert `git rev-parse HEAD~1 == fetched_sha` (local continuity). Then run `git push --porcelain origin HEAD:refs/heads/<branch>`.
- Verified porcelain output when another clone pushed after our fetch: exit code `1`, and stdout contains the line `!\tHEAD:refs/heads/main\t[rejected] (fetch first)`. A cache that has the remote object reports `(non-fast-forward)` instead [ASSUMED for the non-fast-forward wording; the "fetch first" form was VERIFIED by probe in a `--depth 1` cache like production]. Positive control: a fast-forward push printed ` \tHEAD:refs/heads/main\t5891ce6..6b9fbf5` and exited 0 [VERIFIED: probe].
- Classification: a porcelain line with flag `!` and summary starting `[rejected]` means upstream moved. Run `rollback_commit` and return `UpstreamMoved` (exit 1). `[remote rejected]` (for example the pre-receive hook in `test_sync_push_remote_failure_does_not_advance_ancestor_state`) or any other failure is a hard failure (exit 2). Keep the substring `Failed to push to remote`, which that e2e test asserts (tests/e2e_lifecycle.rs:418-420 region).
- The server-side fast-forward check is the atomic compare-and-swap. A pre-push `ls-remote` adds a network call and still has a time-of-check/time-of-use window, so it is **not** recommended. Never use `--force` or `--force-with-lease` (D-18).
- **PR mode:** the push goes to a fresh `ude-sync-<hash>-<epoch>` branch, so a moved `main` cannot be clobbered. Apply D-18 to direct mode only. This is my reading of D-18, flagged in the Assumptions Log.
- `push_to_remote` returns `Result<String, String>` today. Callers need to tell `UpstreamMoved` (1) apart from hard failure (2), so introduce a small error enum.
- `--force` (`SyncAction::Push { force, pr }`) currently only sets `allow_agent_rules` (main.rs:451). It must not affect D-10, D-17, or D-18.

### Pattern 8: `RemoteLockGuard` on std locking (D-04)
```rust
// Source: doc.rust-lang.org/std/fs/struct.File.html (try_lock, 1.89.0) + probe on 1.99.0
match file.try_lock() {
    Ok(()) => return Ok(Self { file, lock_path: lock_path.to_path_buf() }),
    Err(std::fs::TryLockError::WouldBlock) => { /* poll until timeout */ }
    Err(std::fs::TryLockError::Error(e)) => return Err(format!("lock error on {}: {e}", lock_path.display())),
}
// Drop: nothing needed — lock released when `file` closes; optionally file.unlock().
```
Verified on Linux, Rust 1.99.0 [VERIFIED: probe]:
- A second handle in the same process gets `WouldBlock`, so `test_remote_lock_concurrency_serialization_and_timeout` still works.
- util-linux `flock -n` exits 1 while the lock is held and 0 after release, which shows std uses `flock(2)`. The docs say the same [CITED].
- After the holder process is SIGKILLed (lock file still present), `try_lock` returns `Ok(())`.
- A child (`sleep`) spawned by the holder does **not** keep the lock after the parent is SIGKILLed (std opens files close-on-exec).
- So the CONCERNS.md "stale lock after SIGKILL" risk does not apply to flock-based locking. The lock file's presence is irrelevant.
- The `unsafe extern "C" flock`, `LOCK_*` constants, and errno-11 check (`src/sync/mod.rs:53-62, 99-106, 130-140`) are deleted, and the `#[cfg(not(unix))]` no-op branch goes away (std locks on Windows too).

### Anti-Patterns to Avoid
- **Passing `unwrap_or("")` for an absent side into a merge:** this is how both-deleted files come back as 0-byte files (current `src/sync/mod.rs:252-253`).
- **Changing `compute_hash` in place:** it also keys the cache directories and the PR branch name. Add a separate `fingerprint`.
- **`fs::write` or `fs::rename` straight onto targets during pull/push:** go through the staging helper.
- **`Command::new("git")...status()`:** it inherits the terminal and has no deadline. Every production spawn goes through `run_bounded`.
- **Printing outcomes from inside the engine:** extend `PullResult`/`PushResult` (report structs). `main.rs` renders them and picks exit codes.
- **Full argv in error messages:** remote URLs can embed tokens (`https://x-access-token:...@github.com/...`).
- **`let _ = Command::new("git")...` in test fixtures:** a failed fixture step silently produces a meaningless test. The Wave 0 fixture helper should assert success.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| 3-way merge + markers | Edit-overlap logic (current `merge.rs`) | `diffy::MergeOptions` | Current code strips CRLF, misses adjacent edits, emits no markers (D-02) |
| Killing a git process tree | `Child::kill` + pid tracking | `process-wrap` `ProcessGroup`/`JobObject` | `git-remote-https` and hooks are grandchildren; probe showed a 29.7 s pipe hang with leader-only kill |
| File locking | `extern "C" flock`, errno matching | std `File::try_lock` | Portable, typed `WouldBlock`, no `unsafe` |
| Atomic file replace | `<target>.tmp.<pid>` naming + manual cleanup | `tempfile::NamedTempFile::new_in` + `persist` | Unique names, removal on drop, error returns the temp |
| Repo-local git env list | The 4 names in the remediation plan | The 15-name list from `git rev-parse --local-env-vars` | Also covers `GIT_COMMON_DIR`, `GIT_OBJECT_DIRECTORY`, `GIT_CONFIG_PARAMETERS`, etc. |
| Detecting push races | Comparing `ls-remote` output before push | Server non-FF rejection via `git push --porcelain` | The server check is atomic; ls-remote is TOCTOU |

**Key insight:** each hand-rolled piece failed on an edge case the library already handles: CRLF, grandchildren, a lock leaking into child processes, temp-file cleanup.

## Runtime State Inventory

This phase migrates the schema (v1 → v2) and changes hash semantics, so the inventory applies.

| Category | Items Found | Action Required |
|----------|-------------|------------------|
| Stored data | `.ude/state.json` (v1) in every initialized project on every machine. On this machine: none found under `~/Github` (depth 4) [VERIFIED: `find`]. Other machines: unknown | Code: in-memory migration on load (Pattern 2), persisted at next save. No separate data-migration task |
| Live service config | The upstream template repo (remote) gains `*.conflict` in `templates/projects/generic/gitignore.repo` (D-11). Existing projects will see `.gitignore` as `UpstreamModified` after the change lands upstream | Code edit to the template in this repo. Projects pick it up on the next pull |
| OS-registered state | None: no daemon, timers, or services (on-demand CLI) [VERIFIED: REQUIREMENTS "Out of Scope: Background Daemon"] | None |
| Secrets/env vars | None renamed. New env *set* on children: `GIT_TERMINAL_PROMPT=0`, `GH_PROMPT_DISABLED=1`, `GH_NO_UPDATE_NOTIFIER=1` | None |
| Build artifacts / caches | `~/.cache/ude/remote-<12hex>-<branch>` and `.lock` (8 entries on this machine), keyed by `compute_hash(url#branch)` [VERIFIED: ls]. **Installed old `ude` binaries on other machines** (none on this machine's PATH) | Keep `compute_hash` raw so cache paths stay stable. Old binaries: see Pitfall 5 |

## Common Pitfalls

### Pitfall 1: CRLF fix applied only to hashes
**What goes wrong:** CRLF-only local copies still show as `LocallyModified` or `Conflicted`.
**Why:** `scan_project` routes all-present text files through `compare_3way_with_merge`, which compares raw strings (`src/sync/mod.rs:373-382`, `229`, `239-240`).
**How to avoid:** Classify by fingerprint only (Pattern 2). Merge only when local fp ≠ ancestor fp and upstream fp ≠ ancestor fp.
**Warning signs:** A test with a CRLF local file, LF upstream, and LF ancestor reports anything other than `InSync`.

### Pitfall 2: Existing tests encode the old adjacent-line auto-merge
**What goes wrong:** `merge::tests::test_merge_orthogonal_changes_clean` (base `a=1\nb=1\n`, local edits line 1, upstream edits line 2) and `sync::tests::test_multi_pull_merge_idempotence` (same shape; step 3 is local `a=2\nb=2` / ancestor `a=1\nb=2` / upstream `a=1\nb=3`) get `Err` (conflict) from diffy [VERIFIED: probe].
**How to avoid:** Rewrite their fixtures with at least one unchanged line between the edits. The probe shows `a=1\nx\nb=1\n` merges cleanly. Add an explicit "adjacent edits conflict" test as the D-02 intended-behavior control. `test_merge_disjoint_append` still passes [VERIFIED: probe].
**Also rewritten by decision:** `test_pull_updates_conflict_blocked_no_partial_writes` (D-08), `test_push_updates_blocked_on_conflict` (D-10: now skip-and-report), `test_sync_status_deletions` (Pattern 3).

### Pitfall 3: Publication gate rejects deletions
**What goes wrong:** D-14 pushes fail with "failed to inspect staged git blob" or "failed to inspect committed blob".
**Why:** `git show :<deleted>` and `git show HEAD:<deleted>` exit 128. `git add -- <deleted>` does stage the removal (exit 0, `D` in `--name-status`) [VERIFIED: probe].
**How to avoid:** Use `git diff --cached --name-status` and `git diff-tree --root --no-commit-id --name-status -r HEAD`. Keep the allowlist check for every entry, but run the secret scan only on non-`D` entries.
**Warning signs:** The deletion push test passes only because the gate was skipped. Keep the existing secret-gate tests green as the control.

### Pitfall 4: `persist` leaves files at mode 0600
**What goes wrong:** Pulled configs become owner-only. Hooks lose their exec bit unless set explicitly. Today `fs::write` keeps the existing mode.
**Why:** `NamedTempFile` creates files at 0600, and `persist` renames that inode into place [VERIFIED: probe: target was 644, became 600].
**How to avoid:** Before `persist`, set the existing target's mode if it exists. Otherwise use 0o755 for `scripts/hooks/*`, `scratch/*`, and `scripts/check-in-container.sh` (the current rule at `src/sync/mod.rs:670-680`) and 0o644 for everything else. Apply the same rule to `state.json` and sidecars.
**Warning signs:** A mode assertion on a non-script pulled file reads `0o600`.

### Pitfall 5: D-07 "older binaries must fail loudly" cannot be enforced
**What goes wrong:** An old `ude` reading a v2 `state.json` either (a) parses it, because serde ignores unknown fields and `load_from_dir` never checks `version`, then drops `tombstones`/`conflict` on its next save while keeping `version: 2`; or (b) falls back to empty state on a parse error (`src/sync/state.rs:38-47`).
**Why:** Old code is already deployed. Nothing in the new file format can make old code fail loudly.
**How to avoid / mitigate:** Add a forward guard in the new code (`version > CURRENT` → `Err`) so this cannot recur at v3. Keep v2 additive, which turns (b) into (a). Tell the operator to upgrade every machine before the first v2 write. On this machine, no installed `ude` binary and no `.ude/state.json` were found.
**Warning signs:** A v2 `state.json` with `version: 2` but no `tombstones` key after a run.

### Pitfall 6: Process-group isolation and terminal prompts
**What goes wrong:** An ssh passphrase prompt (ssh reads `/dev/tty`, not stdin) from a child in a background process group gets SIGTTIN and stops. The run then hits the 60 s timeout instead of prompting [ASSUMED: standard POSIX job control; not probed, because the agent shell has no controlling tty].
**How to avoid:** Acceptable for the unattended-agent design. Make the timeout message for network operations say: "ensure your SSH key is loaded in ssh-agent / credentials are cached; UDE runs git non-interactively".
**Warning signs:** Interactive users report a 60 s hang on `ude sync push` over ssh.

### Pitfall 7: Staging that mutates the workspace
**What goes wrong:** `create_dir_all(parent)` during staging creates directories even when staging later aborts.
**How to avoid:** Record the directories you created. On abort, remove them deepest-first with `remove_dir` (succeeds only when empty).
**Warning signs:** The "file 2 of 3 fails" test leaves new empty directories behind.

### Pitfall 8: Absent versus empty
**What goes wrong:** `Option<String>` becomes `""` somewhere (for example `unwrap_or("")`) and a deletion turns into a 0-byte file (remediation 1.1 item 5).
**How to avoid:** Use `Option` end to end. Test with `!path.exists()`, not `len == 0`. Probe: `merge("base\n", "", "")` returns `Ok("")`.

### Pitfall 9: Credentials in timeout and error messages
**What goes wrong:** "Report which command timed out" leaks `https://token@host/...` from clone/fetch argv into stderr and logs (ASVS V7/V8).
**How to avoid:** The runner takes an explicit redacted `label` (`"git fetch"`, `"gh pr create"`). Do not echo the remote URL raw. If it must appear, strip userinfo.

### Pitfall 10: Tests are not hermetic today (pre-existing, blocks SYNC-03 verification)
**What goes wrong:** With `GIT_DIR=<decoy bare repo>`, 6 sync tests fail (positive control: 36/36 pass without it). With `GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1`, 1 sync test and 1 e2e test (`test_doctor_critical_security_exit_code_2_and_negative_control`, Phase 3 territory) fail [VERIFIED: ran both this session]. TESTING.md's claim that "fixture helpers explicitly clear Git environment variables" is false: `rg env_remove` finds nothing in `src/` or `tests/`. TESTING.md's `cargo test --lib` also fails: "no library targets found" (there is no `src/lib.rs`) [VERIFIED: ran it].
**How to avoid:** In Wave 0, add a `git_fixture(dir) -> Command` test helper that scrubs the 15 local env vars and sets `GIT_CONFIG_GLOBAL` to a fixture file containing identity and `commit.gpgsign=false`, plus `GIT_CONFIG_NOSYSTEM=1`. Fixture steps assert success. Use it in all sync tests.

## Code Examples

### Fingerprint
```rust
// Source: design per CONTEXT Discretion "Line endings"; sha2 0.10 API as already used at src/sync/mod.rs:155-159
pub fn fingerprint(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) if text.contains("\r\n") => Self::compute_hash(text.replace("\r\n", "\n").as_bytes()),
        _ => Self::compute_hash(bytes), // LF text (already canonical) or non-UTF-8 binary: raw
    }
}
```
Negative controls: `fingerprint(b"a\r\nb\r\n") == fingerprint(b"a\nb\n")`; `fingerprint(b"a\nb\n") != fingerprint(b"a\nc\n")`; and the bytes `[0xff, b'\r', b'\n']` and `[0xff, b'\n']` (invalid UTF-8) keep **different** fingerprints.

### Existing outcome enums (to extend, not replace in spirit)
Verbatim from `src/sync/mod.rs:11-19, 32-51`:
```rust
pub enum SyncStatus { InSync, LocallyModified, UpstreamModified, Merged, Conflicted, Overridden }
pub enum PushResult {
    Success { synced_files: usize, staged_files: Vec<PathBuf>, updated_records: Vec<(PathBuf, String, Option<String>)> },
    SecretBlocked { path: PathBuf, reason: String },
    ConflictBlocked { path: PathBuf },
    BoundaryBlocked { path: PathBuf },
    IoFailed { path: PathBuf, reason: String },
    GitRemoteFailed { reason: String },
}
pub enum PullResult { Success { applied_files: usize }, ConflictBlocked { path: PathBuf }, IoFailed { path: PathBuf, reason: String } }
```
[VERIFIED: src/sync/mod.rs:11-51] Proposed additions:
- `SyncStatus`: `LocalDeleted`, `UpstreamDeleted`, `BothDeleted`, `ConflictPending` (open conflict record), and `Unmanaged` (tombstoned).
- `PullResult::Success` becomes a report `{ applied: Vec<PathBuf>, deleted: Vec<PathBuf>, conflicts: Vec<PathBuf>, locally_deleted_pending: Vec<PathBuf> }`.
- `PushResult::Success` gains `deleted: Vec<PathBuf>`, `skipped_conflicts: Vec<PathBuf>`, and `skipped_merged: Vec<PathBuf>`.
- New outcome `UpstreamMoved`.
`main.rs` status rendering (`src/main.rs:399-420`) is an exhaustive `match`, so the compiler will flag every place that needs a new arm.

### CLI surface (D-09)
Verbatim `src/cli/mod.rs:102-118`:
```rust
pub enum SyncAction {
    Status,
    Pull,
    Push {
        #[arg(long)] force: bool,
        #[arg(long)] pr: bool,
    },
}
```
[VERIFIED: src/cli/mod.rs:102-118 (doc comments elided)] Add `Resolve { file: PathBuf }`.
- Validate `file`: it must be one of the managed relative paths, with only `Component::Normal` components (reuse the component check in `validate_safe_write_target`), and it must have an open `conflict` record. Otherwise exit 2.
- Resolve semantics (recommendation): "the current local state is the resolution."
  - Local present, upstream version present: ancestor := recorded upstream. The next push publishes the local content as a normal modification.
  - Local absent: ancestor := recorded upstream, so the next push deletes upstream (D-14 path).
  - Upstream deleted and local present: write a tombstone `kept_unmanaged`.
  - Upstream deleted and local absent: drop the record.
  - The user expresses D-13's "keep or delete" choice by deleting the file or not before running `resolve`. No flags needed.

### Managed paths and upstream mapping (for test fixtures)
Verbatim `src/sync/mod.rs:314-326`: `.gitconfig`, `.gitignore`, `scripts/hooks/commit-msg`, `scripts/hooks/pre-commit`, `scripts/hooks/pre-push`, `scripts/check-in-container.sh`, `mise.toml`, `.agents/AGENTS.md`, `scratch/sync-personal-env.sh`, `scratch/new-worktree.sh`, `scratch/check-publish-boundary.sh` [VERIFIED: src/sync/mod.rs:314-326]. Upstream mapping, verbatim from `map_to_upstream` (`src/sync/mod.rs:1184-1205`): `.gitconfig` → `projects/generic/gitconfig.repo`; `.gitignore` → `projects/generic/gitignore.repo`; `scripts/hooks/<n>` → `projects/generic/hooks/<n>`; `scripts/check-in-container.sh` → `projects/generic/container/check-in-container.sh`; `scratch/<n>` → `projects/agents/scratch/<n>`; `mise.toml` → `projects/agents/mise.toml.jinja`; `.agents/AGENTS.md` → `projects/agents/agents.md.jinja` [VERIFIED: src/sync/mod.rs:1184-1205]. Note: the list has 11 entries, while CONTEXT says "18 template files". The scan only covers these 11.

`*.conflict` in `.gitignore` ignores `.gitconfig.conflict` and `scripts/hooks/pre-commit.conflict` (dotfiles match `*`) and does not ignore `.gitconfig` or `mise.toml` [VERIFIED: `git check-ignore` probe]. `templates/projects/generic/gitignore.repo` has no `conflict` entry today [VERIFIED: rg exit 1].

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `extern "C" flock` + errno 11 | std `File::lock`/`try_lock` + `TryLockError::WouldBlock` | Rust 1.89.0 | Removes `unsafe` and gives Windows support for free |
| `similar`-based custom 3-way | `diffy` 0.5 `MergeOptions` (Diff3 default, `IncompleteHunkStyle` added in 0.5.x) | diffy 0.5.0 (2026-04-27) | Markers, CRLF-safe, git-like adjacency |
| `command-group` crate | `process-wrap` (same watchexec authors; composable wrappers) | process-wrap 10.0.0 (2026-08-24) | `std` feature must be enabled explicitly |

**Deprecated/outdated in this repo:** `merge::merge_3way_text` and `MergeResult` (replaced), `similar` dependency (removed), `RemoteLockGuard`'s `#[cfg(not(unix))]` no-op branch.

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | A child in a background process group that reads `/dev/tty` (ssh passphrase) gets SIGTTIN and stops, so it times out rather than prompting | Pitfall 6 | Low: either way it is bounded by the timeout. Only the error-message hint would be wrong |
| A2 | `git gc --auto` daemonizes with setsid and so survives the post-exit group kill | Pattern 1 | Low: worst case an auto-gc in the cache checkout is cut short and re-runs later |
| A3 | Porcelain summary for a cache that already has the remote object is `[rejected] (non-fast-forward)` | Pattern 7 | Low: classify on the `!` flag + `[rejected]` prefix, not the parenthetical |
| A4 | D-18 applies to direct push only; PR-mode pushes to a fresh branch and is unaffected | Pattern 7 | Medium: if the operator wants PR mode to also abort on a moved base, add the same continuity check before `git checkout -b` |
| A5 | Tombstone map plus per-file `conflict` record is the right v2 shape; resolve takes the local state as the resolution (no flags) | Patterns 3, 4, CLI | Medium: D-09 is a costly-to-reverse CLI surface. If explicit `--keep`/`--delete` flags are preferred, decide before planning the resolve task |
| A6 | Dominant-EOL detection (tie → LF) is the right rule for "preserve each file's existing line-ending style" | Pattern 2 | Low: mixed-EOL files are rare in these 11 managed paths |
| A7 | Converting audit/template git calls (Phase 3/4 files) belongs in this phase per "every call site" | Pattern 1 | Low: mechanical. If deferred, SYNC-03's "all subprocess executions" wording is only partly met |

## Open Questions

1. **Should a conflict that converges by itself (upstream later equals local) auto-clear?** (RESOLVED: yes, operator approved 2026-10-17, recorded as the D-09 amendment in 01-CONTEXT.md; implemented in plan 01-09 Task 2.)
   - What we know: D-09 says only `resolve` resolves, and deleting the sidecar by hand does not.
   - What's unclear: if local and upstream become identical, the conflict record keeps that file out of push until `resolve` runs, even though there is nothing left to choose.
   - Recommendation: pull clears the record and removes the sidecar when local fp == upstream fp, and reports it as "conflict resolved by convergence". It needs a one-line confirmation because it narrows D-09.
2. **Mode for newly created non-script files: fixed 0o644, or 0o666 & umask?** (RESOLVED: fixed 0o644, existing files keep their mode, scripts 0o755; planner decision under Claude's discretion, implemented in plan 01-06 Task 2.)
   - Recommendation: 0o644. It matches typical umask 022 and avoids an `unsafe` libc umask call.
3. **Windows `JobObject` branch cannot be compiled here** (only `x86_64-unknown-linux-gnu` is installed). (RESOLVED: the Windows path is labelled untested on this host and covered by a manual-only human-check in plan 01-01 Task 2, recorded in the phase UAT.)
   - Recommendation: optional `rustup target add x86_64-pc-windows-gnu && cargo check --target x86_64-pc-windows-gnu`. This is user-space and fine on an immutable OS. Otherwise label the Windows path as untested.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| rustc / cargo | build, tests | ✓ | 1.99.0 / 1.99.0 | — |
| git | sync fixtures, runtime | ✓ | 2.56.0 | — |
| gh | PR mode (runtime only; tests must not call real gh) | ✓ | 2.102.0 | PR-create failure already falls back to a "create a PR manually" message |
| sh, sleep | timeout tests (`sh -c 'sleep 5 & sleep 5'`) | ✓ | coreutils | — |
| flock (util-linux) | optional cross-check only | ✓ | linuxbrew | not needed by tests |
| cargo-nextest, cargo-mutants, cargo-llvm-cov, cargo-machete, cargo-deny | optional quality gates | ✓ | installed | `cargo test` |
| Windows target | compiling the `cfg(windows)` JobObject branch | ✗ | — | Leave the Windows path untested (Open Question 3) |
| Network | none: all fixtures are local `file://` / bare repos | n/a | — | — |

**Missing dependencies with no fallback:** none.
**Missing dependencies with fallback:** Windows cross-target (optional).

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in test harness (`cargo test`); unit tests in `#[cfg(test)]` modules of the **bin** target, plus `tests/e2e_lifecycle.rs` |
| Config file | none (no `src/lib.rs`, so `cargo test --lib` errors: use `--bin ude`) |
| Quick run command | `cargo test --bin ude sync::` (36 tests, ~0.6 s after build) and `cargo test --bin ude process::` |
| Full suite command | `cargo test && cargo clippy --workspace --all-targets -- -D warnings` |
| Hermeticity gate (new) | `GIT_DIR=<decoy bare repo> cargo test --bin ude sync::` must pass (6 failures today) |
| Baseline (this session) | 53 unit + 8 e2e pass; clippy clean |

### Phase Requirements → Test Map
Every row pairs the behavior with a **negative control** (NC). File: `src/sync/mod.rs` tests unless noted.

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| SYNC-01 | Upstream delete + unchanged local → local removed, record dropped, tombstone written, no ENOENT. **NC:** upstream delete + local edit → `Conflicted`, local bytes unchanged, sidecar written | unit | `cargo test --bin ude sync::tests::test_pull_upstream_deletion` | ❌ Wave 0 |
| SYNC-01 | Local delete → push removes upstream template and lists it as deleted. **NC:** an untouched second file in the same push is not removed | unit | `cargo test --bin ude sync::tests::test_push_local_deletion` | ❌ |
| SYNC-01 | Publication gate accepts a staged/committed deletion (`D` entry) and still scans non-deleted blobs. **NC:** existing secret-gate tests stay green | unit (git fixture) | `cargo test --bin ude sync::tests::test_push_to_remote_deletion` | ❌ |
| SYNC-01 | Both deleted → record dropped, `!path.exists()`. **NC:** local present-but-empty + upstream deleted → `Conflicted` (empty ≠ absent) | unit | `cargo test --bin ude sync::tests::test_both_deleted` | ❌ |
| SYNC-01 | D-15: local deleted, upstream unchanged → pull does not recreate, reports pending. **NC:** upstream changed → `Conflicted` | unit | `cargo test --bin ude sync::tests::test_pull_keeps_local_deletion` | ❌ |
| SYNC-01 | Tombstone blocks resurrection: after D-12 pull, recreate the local file → push does not publish it. **NC:** same file with fresh state (no tombstone) → classified `LocallyModified` | unit | `cargo test --bin ude sync::tests::test_tombstone_prevents_republish` | ❌ |
| SYNC-02 | Staging failure on a later file (`scripts` made a regular file) → `.gitconfig`/`.gitignore` byte-identical, `state.json` byte-identical, no `.tmp*` left, no new empty dirs. **NC:** same setup without the collision → all applied | unit | `cargo test --bin ude sync::tests::test_pull_staging_failure_no_partial_writes` | ❌ |
| SYNC-02 | Simulated crash before commit (stage state, drop the temp without persist) → `state.json` unchanged. **NC:** commit path → `state.json` updated | unit | `cargo test --bin ude sync::state::tests::test_state_save_atomic` | ❌ |
| SYNC-02 | D-06: corrupt `state.json` → `Err`; version 99 → `Err`. **NC:** missing file → `Ok(default)` | unit | `cargo test --bin ude sync::state::tests::test_load_` | ❌ (existing `test_project_state_load_nonexistent_returns_default` is the NC) |
| SYNC-02 | Modes after staged write: hook 0o755; existing 0o644 file stays 0o644. **NC:** assert mode is not 0o600 (the tempfile default) | unit (`cfg(unix)`) | `cargo test --bin ude sync::tests::test_staged_write_modes` | ❌ |
| SYNC-02 | Symlinked target and symlinked sidecar path are refused. **NC:** a regular path succeeds | unit | `cargo test --bin ude sync::tests::test_pull_updates_refuses_to_write_through_symlink` (+ new sidecar case) | ✅ / ❌ |
| SYNC-03 | Timeout kills the group: `sh -c 'sleep 5 & sleep 5'`, 300 ms limit → `Err` naming the label and duration, elapsed < 2 s (broken group kill fails at ~5 s with a message, not a hang). **NC:** `sh -c 'echo ok'` → `Ok`, stdout `ok\n` | unit | `cargo test --bin ude process::tests::test_timeout_kills_group` | ❌ |
| SYNC-03 | No pipe deadlock: child writes 1 MiB to stdout **and** stderr → exact lengths returned. **NC:** a 0-byte case returns empty buffers | unit | `cargo test --bin ude process::tests::test_drains_large_output` | ❌ |
| SYNC-03 | Env scrub: the built command's `get_envs()` shows all 15 local vars removed (`None`) and `GIT_TERMINAL_PROMPT=0`. **NC:** a raw `Command` lacks those entries | unit | `cargo test --bin ude process::tests::test_env_scrub` | ❌ |
| SYNC-03 | Real-git hang: bare remote with a `pre-receive` hook that sleeps 5 s; push with a 500 ms limit → timeout error, remote ref unchanged. **NC:** hook `exit 0` → push succeeds | unit (git fixture) | `cargo test --bin ude sync::tests::test_push_timeout_hanging_hook` | ❌ |
| SYNC-03 | Remediation 1.5: run the `ude` binary with `.env("GIT_DIR", decoy)` against a fixture remote → sync succeeds and decoy refs/objects are unchanged. **NC:** a plain `git` command given the same `.env("GIT_DIR", decoy)` resolves to the decoy | e2e | `cargo test --test e2e_lifecycle test_sync_ignores_inherited_git_dir` | ❌ |
| SYNC-03 | Lock contention: second handle → timeout error < 500 ms; after drop → acquired. **NC** built in (the drop step) | unit | `cargo test --bin ude sync::tests::test_remote_lock_concurrency_serialization_and_timeout` | ✅ (keep, re-point to std lock) |
| SYNC-04 | `fingerprint` CRLF == LF. **NC:** different text differs; invalid-UTF-8 bytes are not normalized | unit | `cargo test --bin ude sync::tests::test_fingerprint` | ❌ |
| SYNC-04 | CRLF local vs LF upstream/ancestor (same text) → `InSync`. **NC:** a real edit in the CRLF file → `LocallyModified` | unit | `cargo test --bin ude sync::tests::test_scan_crlf_only_difference_in_sync` | ❌ |
| SYNC-04 | Merge of CRLF orthogonal edits keeps `\r\n`. **NC:** LF orthogonal merge output contains no `\r` | unit | `cargo test --bin ude sync::merge::tests::test_merge_crlf` | ❌ |
| SYNC-04 | Pull into a CRLF local file writes CRLF. **NC:** pull into an LF file writes no CR | unit | `cargo test --bin ude sync::tests::test_pull_preserves_eol_style` | ❌ |
| SYNC-04 | v1 → v2 migration: v1 record with raw CRLF-hash + content → v2 normalized, file `InSync`. **NC:** v1 record whose old hash does not match local → still modified | unit | `cargo test --bin ude sync::state::tests::test_migrate_v1` | ❌ |
| SYNC-05 | Pull with two conflicted files + one clean file → two sidecars containing `<<<<<<< LOCAL`, `||||||| ANCESTOR`, `=======`, `>>>>>>> UPSTREAM`; the clean file applied; locals untouched; report lists both. **NC:** clean pull leaves zero `*.conflict` files in the project (walk) | unit | `cargo test --bin ude sync::tests::test_pull_multiple_conflicts_sidecars` | ❌ (rewrite `test_pull_updates_conflict_blocked_no_partial_writes`) |
| SYNC-05 | Adjacent-line edits conflict (D-02 intended). **NC:** edits separated by one line merge cleanly | unit | `cargo test --bin ude sync::merge::tests::test_adjacent_edits_conflict` | ❌ (rewrite `test_merge_orthogonal_changes_clean`, `test_multi_pull_merge_idempotence` fixtures) |
| SYNC-05 | `relabel` rewrites only exact marker lines. **NC:** a content line `<<<<<<< oursX` is untouched | unit | `cargo test --bin ude sync::merge::tests::test_relabel` | ❌ |
| SYNC-05 | `resolve` sets ancestor to the recorded upstream, removes the sidecar, clears the record, and the file is then publishable. **NC:** deleting the sidecar by hand → push still skips the file | unit | `cargo test --bin ude sync::tests::test_resolve` | ❌ |
| SYNC-05/06 | Push with one conflicted, one clean-mergeable, one modified file → only the modified file published; report lists the skipped files; CLI exits 1. **NC:** modified-only push → exit 0 | unit + e2e | `cargo test --bin ude sync::tests::test_push_skips_conflicts_and_merged` | ❌ (rewrite `test_push_updates_blocked_on_conflict`) |
| SYNC-06 | Upstream moved between fetch and push (second clone pushes) → `UpstreamMoved`, remote unchanged, cache `HEAD` == fetched sha. **NC:** no concurrent push → success | unit (git fixture) | `cargo test --bin ude sync::tests::test_direct_push_aborts_when_upstream_moved` | ❌ |
| SYNC-06 | Remote `pre-receive` reject → hard failure (exit 2), ancestor not advanced. **NC:** the upstream-moved case exits 1 | e2e | `cargo test --test e2e_lifecycle test_sync_push_remote_failure_does_not_advance_ancestor_state` | ✅ (adjust exit-code expectations) |
| SYNC-06 | `--force` does not bypass D-18. **NC:** without the race, `--force` push succeeds | e2e | `cargo test --test e2e_lifecycle test_force_does_not_bypass_upstream_moved` | ❌ |

### Sampling Rate
- **Per task commit:** `cargo test --bin ude sync:: && cargo test --bin ude process::` (< 5 s after build), plus `cargo clippy --workspace --all-targets -- -D warnings`.
- **Per wave merge:** `cargo test`, then the hermeticity gate `GIT_DIR=$(mktemp -d)/decoy.git cargo test --bin ude sync::`, with the decoy initialized as a bare repo first.
- **Phase gate:** full suite + clippy + hermeticity gate green, then `/gsd-verify-work`. Optional: `cargo mutants -v --file src/sync/merge.rs` and `--file src/process.rs` (operator's Rust harness rules for algorithm and state-machine changes).

### Wave 0 Gaps
- [ ] Test helper `git_fixture(dir: &Path) -> Command` in `src/sync/mod.rs` tests (and a copy in `tests/e2e_lifecycle.rs`). It scrubs the 15 local env vars, sets `GIT_CONFIG_GLOBAL=<tempdir>/gitconfig` (identity + `commit.gpgsign=false`) and `GIT_CONFIG_NOSYSTEM=1`, and asserts fixture step success. All existing sync git-fixture tests use it.
- [ ] `SyncEngine` extra-env injection (Pattern 1), so in-process tests can point the runner at the fixture gitconfig without `set_var`.
- [ ] `src/process.rs` with a `#[cfg(test)]` module (new file).
- [ ] Rewrite the 5 tests named in Pitfall 2 **before** changing the implementation (TDD mode). They are the red tests for D-02, D-08, D-10, and the deletion statuses.
- [ ] No new framework install needed.

## Security Domain

### Applicable ASVS Categories (Level 1)

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | git/gh credentials are delegated to their own helpers; UDE never handles them |
| V3 Session Management | no | — |
| V4 Access Control | yes (filesystem boundary) | `validate_safe_write_target` on every staged target **and** sidecar; `resolve <file>` limited to managed relative paths with `Component::Normal` only |
| V5 Input Validation | yes | Reject absolute paths and `..` in `resolve`; treat `state.json` as untrusted input (D-06 errors, version guard) |
| V6 Cryptography | yes (integrity only) | `sha2` SHA-256. Never hand-roll. Fingerprints are integrity markers, not authentication |
| V7/V8 Error handling & data protection | yes | Redacted command labels in timeout/error text (Pitfall 9); sidecar inherits local file mode; `*.conflict` gitignored (D-11) |
| V12 Files & Resources | yes | Same-dir temp + rename (rename replaces a symlink and does not follow it: verified); explicit modes (Pitfall 4); bounded subprocesses (DoS) |
| V14 Configuration | yes | Scrub repo-local git env; `GIT_TERMINAL_PROMPT=0`; never force-push |

### Known Threat Patterns for this stack

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Symlink planted at a target or `<file>.conflict` to write outside the project | Tampering / Elevation | `validate_safe_write_target` at stage time and again immediately before `persist` |
| Inherited `GIT_DIR`/`GIT_INDEX_FILE` makes UDE commit into or corrupt another repo | Tampering | Scrub the `git rev-parse --local-env-vars` list on every spawn; e2e decoy test |
| Hung `git-remote-https`, hooks, or credential helpers block automation indefinitely | Denial of service | process-group / Job Object kill at 30/60 s |
| Token-bearing remote URL echoed into a timeout message or logs | Information disclosure | Redacted labels; strip userinfo |
| Publication-gate bypass via deletion entries or a skipped scan | Tampering / Info disclosure | `--name-status`; allowlist every entry; scan every non-`D` blob; keep post-commit validation |
| Upstream race overwritten by a forced push | Tampering / Repudiation | Plain push; server non-FF rejection → `rollback_commit` |
| Downgrade: old binary rewrites v2 state and drops conflict/tombstone records | Tampering | Forward-version guard in new code; operator upgrades all machines (Pitfall 5) |

## Sources

### Primary (HIGH confidence: compiled/executed this session)
- Probe crate (`diffy =0.5.2`, `process-wrap 10.0.1` with features std/process-group/job-object, `tempfile 3.27`), built and run on rustc 1.99.0: merge outputs, group kill against leader-only kill timing, `try_lock` across handles, `persist` modes and failure path. Removed afterwards.
- Lock-holder probe: SIGKILL release; children spawned by the holder do not keep the lock.
- Git probes (git 2.56.0): `push --porcelain` rejection format, deletion staging and `git show` exit 128, `check-ignore` for `*.conflict`, rename over a symlink (python `os.rename`).
- Crate sources read: `diffy-0.5.2/src/merge/mod.rs` (ConflictStyle, IncompleteHunkStyle, MergeOptions defaults, `output_result` labels); `process-wrap-10.0.1/Cargo.toml` features, `src/std.rs`, `src/std/process_group.rs`.
- Repo files read: `src/sync/{mod,state,merge}.rs`, `src/main.rs` 270–598, `src/cli/mod.rs` 90–118, `tests/e2e_lifecycle.rs` (313–420 + grep), `Cargo.toml`, `Cargo.lock`, `.planning/*`, `docs/REMEDIATION_PLAN.md` Track 1, `GEMINI.md`.
- Test runs: baseline `cargo test` (53 + 8 pass), clippy clean, `GIT_DIR` decoy run (6 fail), `GIT_CONFIG_GLOBAL=/dev/null` run (1 + 1 fail), `cargo test --lib` (no lib target).

### Secondary (MEDIUM, per gsd classify-confidence for context7)
- ctx7 `/websites/rs_process-wrap`: quick-start, `ProcessGroup::leader`, `CommandWrap::spawn`, default `kill()` implementation.
- docs.rs `process_wrap::std::ChildWrapper` (10.0.1) and `diffy::MergeOptions` (0.5.2) pages.
- crates.io API: version and date data for diffy, process-wrap, tempfile.

### Tertiary (LOW per seam for webfetch; corroborated by probe)
- doc.rust-lang.org `std::fs::File` lock methods (1.89.0 stabilization, flock/LockFileEx mapping, release-on-close wording).

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH. Versions from crates.io; APIs compiled and exercised.
- Architecture: MEDIUM-HIGH. Current-code facts are verified; the v2 schema, tombstone map, resolve semantics, and report shapes are recommendations (A4, A5).
- Pitfalls: HIGH for 1–5, 7, 8, and 10 (each reproduced or read directly); MEDIUM for 6 and 9 (reasoned, not probed).

**Research date:** 2026-10-16
**Valid until:** 2026-11-15 (crate versions are pinned; re-check only if pins change)

## RESEARCH COMPLETE
