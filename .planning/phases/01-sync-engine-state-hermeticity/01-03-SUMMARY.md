---
phase: 01-sync-engine-state-hermeticity
plan: 03
subsystem: sync
tags: [rust, git, process-wrap, timeout, hermeticity, flock, std-file-lock, redaction]

requires:
  - phase: 01-01
    provides: "crate::process::run_bounded, LOCAL_TIMEOUT / NETWORK_TIMEOUT, GIT_LOCAL_ENV_VARS"
  - phase: 01-02
    provides: "GitFixture (sync tests and e2e copy) with gitconfig_env()"
provides:
  - "All 31 production git/gh spawns in src/sync/mod.rs run through process::run_bounded (clone/fetch/push/gh on the network tier, the rest local)"
  - "SyncEngine::with_extra_env / with_limits and pub struct ProcessLimits (test-only parameterisation)"
  - "process::redact_userinfo, process::stderr_excerpt, STDERR_EXCERPT_LINES"
  - "rollback_commit / validate_committed_tree as SyncEngine methods"
  - "RemoteLockGuard on std File::try_lock (D-04), LOCK_POLL_INTERVAL"
  - "Hermeticity gate (GIT_DIR decoy + empty global config) green on the sync unit suite"
affects: [01-04, 01-05, 01-06, 01-07, 01-08, 01-09, 01-10, 01-11, sync, e2e]

actuals:
  tokens: 15627
  tasks: 3
  commits: 6
plan_head_before: c35a330ef2d93f9445e1f0bc98143f596834e66d
plan_head_after: 18c34fb9fb0675701b75e53b4879ea78fe142f32

tech-stack:
  added: []
  patterns:
    - "Engine spawns: self.git(dir) / self.tool(program, dir) build the Command with extra env; self.run_local / self.run_network run it; labels are fixed strings, never argv"
    - "Failed checked git calls: with_excerpt(existing message, &out) appends the redacted last 5 stderr lines"
    - "Best-effort cleanups go through git_best_effort (still bounded, result discarded)"

key-files:
  created: []
  modified: [src/process.rs, src/sync/mod.rs, src/main.rs, tests/e2e_lifecycle.rs]

key-decisions:
  - "Task 1 RED extended with a second `ude sync status` run (existing-cache fetch path) instead of the push path: status-only passed in RED because git clone overrides GIT_DIR itself, and a push-path RED in Task 1 could not go green until Task 2"
  - "Push-path run_bounded errors keep their old context prefix (e.g. `Failed to push to remote: git push timed out after ...`); ensure_remote_repo surfaces run_bounded's message unprefixed, as the plan specified"
  - "validate_committed_tree parameters are (repo, paths) so the signature stays on one rustfmt line and the plan's non-multiline rg acceptance check can match it"
  - "Task 3's swap is committed as refactor(01-03): its two new tests pass on the old flock(2) code (characterization), so no RED is claimed"

patterns-established:
  - "Tests that drive engine git (ensure_remote_repo, push_to_remote, rollback, committed-tree gate) build SyncEngine::new().with_extra_env(fixture.gitconfig_env())"

requirements-completed: [SYNC-03]

coverage:
  - id: D1
    description: "Inherited GIT_DIR cannot redirect ude sync status (clone run and existing-cache fetch run) or ude sync push; the decoy repository stays byte-identical and the real remote advances"
    requirement: SYNC-03
    verification:
      - kind: e2e
        ref: "tests/e2e_lifecycle.rs#test_sync_ignores_inherited_git_dir"
        status: pass
    human_judgment: false
  - id: D2
    description: "A pre-receive hook sleeping 20 s is cancelled at the 1.5 s network limit: Err names `git push` and the limit, push_to_remote returns in under 8 s (1.79 s measured), remote main unchanged; accepting-hook negative control advances main"
    requirement: SYNC-03
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_push_timeout_hanging_hook"
        status: pass
    human_judgment: false
  - id: D3
    description: "All 31 production git/gh spawns in src/sync/mod.rs are bounded; no direct status/output/spawn call remains in the production region"
    requirement: SYNC-03
    verification:
      - kind: other
        ref: "production-region counter (0 now; same count on base c35a330 gives 31; test region still matches)"
        status: pass
    human_judgment: false
  - id: D4
    description: "Hermeticity gate: GIT_DIR=<decoy> GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 cargo test --bin ude sync:: passes"
    requirement: SYNC-03
    verification:
      - kind: other
        ref: "plan Task 2 gate command: 40/40 at HEAD (baseline at c35a330: 33/37, 4 failing)"
        status: pass
    human_judgment: false
  - id: D5
    description: "URL userinfo is redacted in the clone progress line, stderr excerpts and main.rs remote URL lines"
    requirement: SYNC-03
    verification:
      - kind: unit
        ref: "src/process.rs#process::tests::test_redact_userinfo"
        status: pass
      - kind: unit
        ref: "src/process.rs#process::tests::test_stderr_excerpt_redacts_and_truncates"
        status: pass
    human_judgment: false
  - id: D6
    description: "RemoteLockGuard on std File::try_lock: in-process contention times out, cross-process flock(1) holder blocks it, the lock is released when the holder's process group is SIGKILLed, and spawned children do not hold the lock file"
    requirement: SYNC-03
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_remote_lock_concurrency_serialization_and_timeout"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_remote_lock_released_when_holder_killed"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_remote_lock_not_inherited_by_children"
        status: pass
    human_judgment: false
  - id: D7
    description: "Windows behaviour of the std lock (LockFileEx) after removing the non-Unix no-op branch"
    requirement: SYNC-03
    verification: []
    human_judgment: true
    rationale: "Never compiled or run: only x86_64-unknown-linux-gnu is installed on this host and the new lock tests are Linux-only. Needs a Windows build and a contention run. Recorded in .planning/WINDOWS.md."

duration: 15min
completed: 2026-10-18
status: complete
---

# Phase 01 Plan 03: Bounded, Hermetic Sync Engine Spawns Summary

**Every git/gh spawn in the sync engine now runs through `process::run_bounded` (60 s network tier for clone/fetch/push/gh, 30 s local tier for the rest) with test-only env and limit injection. An inherited `GIT_DIR` no longer redirects `ude sync status` or `ude sync push`, a hanging remote hook is cancelled at the limit, remote URL userinfo is redacted in output, and `RemoteLockGuard` uses std `File::try_lock` instead of a hand-written flock binding.**

## Performance

- **Duration:** about 15 min
- **Started:** 2026-10-18T18:57:56Z
- **Completed:** 2026-10-18T19:13:00Z
- **Tasks:** 3
- **Files modified:** 4

## Accomplishments
- `src/process.rs`: `redact_userinfo` (regex over `scheme://userinfo@`), `stderr_excerpt` (last 5 non-empty stderr lines, ` | `-joined, redacted), `STDERR_EXCERPT_LINES`.
- `src/sync/mod.rs`: `ProcessLimits`, `with_extra_env`, `with_limits`, private `tool` / `git` / `run_local` / `run_network` helpers. `ensure_remote_repo` and `push_to_remote` converted, `rollback_commit` and `validate_committed_tree` are now methods. The production region has 0 direct spawns (31 at plan start).
- `src/main.rs`: both `Remote Upstream` lines and the `Failed to sync with remote` line print `redact_userinfo(url)`.
- `RemoteLockGuard`: std `try_lock` with `TryLockError::WouldBlock` polling every `LOCK_POLL_INTERVAL` (50 ms). Same timeout message. Drop unlocks best-effort. The `unsafe extern "C"` binding, the `LOCK_*` constants, the errno-11 check, the `cfg(not(unix))` no-op and the dead-code allowance are gone.
- Hermeticity: the gate went from 33/37 (4 failing) to 40/40. The orchestrator's re-measurements also pass: the e2e suite with `GIT_DIR=<decoy>` is 9/9 (01-02 had 1 failure), the full suite with `GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1` is 65 + 9 (01-02 had 1 sync failure), and the bin suite with `GIT_DIR=<decoy>` is 65/65.

## Task Commits

1. **Task 1 (tracer) RED:** `20f1227` test(01-03): add failing tests for userinfo redaction and inherited GIT_DIR
2. **Task 1 GREEN:** `78cea99` feat(01-03): bound remote cache git calls and redact URL userinfo
3. **Task 2 RED:** `3c6575d` test(01-03): add failing tests for hanging push hook and GIT_DIR push
4. **Task 2 GREEN:** `444cad3` feat(01-03): bound and isolate the sync push path
5. **Task 3 characterization tests:** `88eaa37` test(01-03): characterize remote lock release on kill and no inheritance
6. **Task 3 implementation:** `18c34fb` refactor(01-03): move RemoteLockGuard onto std File::try_lock (D-04)

Tracer gate (interactive, `end-of-phase`, automated-only verify): all three Task 1 verify commands were re-run green before Task 2 started.

## TDD Gate Compliance

RED and GREEN commits are present, in order, for Tasks 1 and 2. Task 3 has a `test` commit followed by a `refactor` commit. Its two new tests passed against the old flock(2) code, as the plan anticipated, because they test kernel semantics that both implementations share. They are characterization tests that guard the swap, and no RED is claimed for them.

RED evidence (nextest JUnit, `gsd_run check tdd-red-evidence`). Every record below returned `RED_EVIDENCE_OK` / `target_test_failed`:

| Target | Failed on | Semantic assessment |
|---|---|---|
| process::tests::test_redact_userinfo | first `assert_eq` (stub returned the token URL unchanged) | intended reason |
| process::tests::test_stderr_excerpt_redacts_and_truncates | excerpt `assert_eq` (stub returned `""`) | intended reason |
| test_sync_ignores_inherited_git_dir (Task 1) | status-success assertion of the fetch run: `git fetch` resolved the decoy (`'origin' does not appear to be a git repository`) | intended reason. The clone run passed in RED because git clone overrides GIT_DIR itself |
| sync::tests::test_push_timeout_hanging_hook (neutral identity-only `GIT_CONFIG_GLOBAL` on the cargo invocation) | `returned Ok after 20.04s`: the unbounded push waited out the hook | intended reason |
| sync::tests::test_push_timeout_hanging_hook (host config) | `returned Ok after 54ms`: the host `~/.gitconfig` `core.hooksPath` made receive-pack skip the remote's hook | supplementary. This shows the env-isolation half of the defect, not the missing bound |
| test_sync_ignores_inherited_git_dir (Task 2 push half) | remote-advanced assertion: `git status` ran against the decoy, so push printed "No changes" | intended reason |

Classifier negative control: the Task 1 record with a passing test as the target returned `INVALID_RED` / `no_target_test_failure`.

## Verification

| Check | Result |
|---|---|
| `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 before every commit (gated with `&&`) |
| `cargo test` (unit + e2e) | 65 + 9 pass, 5 consecutive runs |
| Hermeticity gate (plan command, includes the decoy self-check) | 40/40, exit 0 |
| Production spawn counter | 0 in the production region. Control: base gives 31 and the test region still matches |
| Lock remnant check | clean. Control: base gives 6 hits |
| Hook process survival | no `sleep 20` survives right after the hook test. Probe control: a live `sleep 20` is detected |
| Lock tests leave no `flock` / `sleep` processes | checked right after the run |

## Files Created/Modified
- `src/process.rs` - redaction and stderr excerpt helpers plus their tests
- `src/sync/mod.rs` - bounded and env-injected engine spawns, methods, std lock, new tests
- `src/main.rs` - redacted remote URL output
- `tests/e2e_lifecycle.rs` - `snapshot_tree` and `test_sync_ignores_inherited_git_dir`. The now-unneeded `#[allow(dead_code)]` on the e2e `GitFixture::output` was removed because the new test uses it

## Decisions Made
- Added small private helpers beyond the artifact table: `tool` (gh shares the env injection), `commit_command`, `git_best_effort`, `reset_index`, `abandon_sync_branch`, and the free fn `with_excerpt`. They replace 4 copies of the reset cleanup and 2 copies of the checkout + branch -D cleanup.
- An Err from a push-path `run_bounded` call (spawn failure or timeout) still returns early without the old best-effort cleanups. This keeps the previous spawn-failure behaviour. The next `ensure_remote_repo` resets the cache checkout (`checkout -B`, `reset --hard`, `clean`).
- Stderr excerpts are appended only where the old code inherited stderr (`add`, `checkout -b`, `commit`, `push`, and the ensure_remote_repo calls). The `show` and `diff-tree` gate messages are unchanged.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Task 1 RED did not fail on the status path alone**
- **Found during:** Task 1 RED
- **Issue:** With an inherited GIT_DIR, the first `ude sync status` passed: git clone sets its own GIT_DIR. The plan says to extend RED to the push path in that case. But Task 1's own verify requires the e2e test to pass after Task 1, and push_to_remote is Task 2's code, so that test could not have gone green.
- **Fix:** Extended RED with a second status run over the existing cache (the fetch / checkout -B / reset / clean code that Task 1 converts). It failed on `git fetch` against the decoy. Task 2 then added the push half as planned. Neither part was weakened.
- **Files modified:** tests/e2e_lifecycle.rs
- **Committed in:** 20f1227, 3c6575d

**2. [Rule 3 - Blocking] The acceptance regex for the method signature cannot match a multi-line signature**
- **Found during:** Task 2
- **Issue:** `rg -n 'fn validate_committed_tree\(\s*&self'` runs without `-U`, so it only matches when the signature is on one line. The plan's parameter names would push rustfmt past 100 columns.
- **Fix:** Named the parameters `repo` and `paths` (doc comment explains `paths`). The signature fits on one line and the check matches.
- **Committed in:** 444cad3

---

**Total deviations:** 2 auto-fixed (both blocking, both in tests or checks). **Impact on plan:** none on behaviour.

## Issues Encountered
- The host `~/.gitconfig` sets `core.hooksPath`. Any test that pushes to a local bare remote with a git that reads the host config skips that remote's hooks. The engine now gets the fixture gitconfig in tests, so this no longer affects the sync suite. In production the engine still reads the user's global config, which is intended. This only matters for local-path remotes, where `receive-pack` runs on the client.

## Known Stubs
None. The RED stubs in `src/process.rs` were replaced in 78cea99.

## Threat Flags
None. T-01-06 to T-01-09 are mitigated as planned. No new surface.

## What the verification does NOT establish
- **Windows:** the std lock path (LockFileEx), and the removal of the old non-Unix no-op, have never been compiled or run here (coverage D7, `.planning/WINDOWS.md`). The two new lock tests are Linux-only.
- **Real network remotes:** every push and clone test uses local-path remotes. The hanging-hook test proves group cancellation for `git push` → `receive-pack` → hook over the local transport. It does not exercise `git-remote-https`. Per 01-01's D-03 evidence the same group kill covers it, but this plan did not test it.
- **`gh pr create`:** it is now bounded and env-injected, but no test makes it succeed. In tests it fails fast (no GitHub remote) and the fallback message is taken.
- **Timing headroom:** the hook test took 1.79 s against an 8 s bound on an idle host, over 5 full-suite runs plus the individual runs. That is a weak flakiness bound. It was not run under load.
- **Lock tests:** they show kernel flock(2) semantics through std (an flock(1) holder blocks `try_lock`, which confirms std uses flock(2) on Linux). They passed on the old code too, so they cannot tell the two implementations apart. They only guard against a regression in the swap.
- **Decoy byte-identity** covers the decoy's files and directory entries only, not mtimes or permission bits.

## User Setup Required
None.

## Next Phase Readiness
- Every later plan can run the hermeticity gate on its wave merge. It is green at 18c34fb.
- Plan 01-07 still has to switch the `--name-only` parsing in `push_to_remote` and `validate_committed_tree` to name-status. This plan left it unchanged.

## Self-Check: PASSED
- FOUND: src/process.rs, src/sync/mod.rs, src/main.rs, tests/e2e_lifecycle.rs
- FOUND commits on this branch: 20f1227, 78cea99, 3c6575d, 444cad3, 88eaa37, 18c34fb (`check evaluation-scope --plan 01-03 --commits-only`: status resolved, 6 commits)

---
*Phase: 01-sync-engine-state-hermeticity*
*Completed: 2026-10-18*
