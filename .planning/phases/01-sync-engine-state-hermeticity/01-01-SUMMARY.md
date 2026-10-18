---
phase: 01-sync-engine-state-hermeticity
plan: 01
subsystem: infra
tags: [rust, process-wrap, subprocess, timeout, process-group, git, nextest]

requires: []
provides:
  - "crate::process::run_bounded(cmd, label, timeout) -> Result<RunOutput, String>: process-group kill at deadline, concurrent stdout/stderr drain, KILL_GRACE-bounded reader join"
  - "crate::process::prepare_command: scrubs the 15 git local env vars, GIT_TERMINAL_PROMPT=0, GH_PROMPT_DISABLED=1, GH_NO_UPDATE_NOTIFIER=1, stdin null"
  - "LOCAL_TIMEOUT (30 s), NETWORK_TIMEOUT (60 s), GIT_LOCAL_ENV_VARS, RunOutput"
  - "All 10 git spawns outside src/sync bounded (main.rs 3, template 2, audit 5)"
  - ".config/nextest.toml JUnit report for TDD RED evidence"
affects: [01-03, sync, audit, template, doctor]

actuals:
  tokens: 9374
  tasks: 2
  commits: 3
plan_head_before: eaa0afefc76e375b9b9047628e5a04936d96ab76
plan_head_after: d7a2d12d5b7d9ff83133f3bd1857500a78fcd412

tech-stack:
  added: ["process-wrap 10.0.1 (std, process-group, job-object; pulls nix 0.31.3, indexmap)"]
  patterns:
    - "Every git/gh spawn goes through crate::process::run_bounded with a static redacted label and a timeout tier constant"
    - "Reader JoinHandles are joined only after is_finished(); otherwise detached after KILL_GRACE"
    - "Tests kill recorded PIDs via a PidGuard that runs the kill binary on Drop (no FFI)"

key-files:
  created: [src/process.rs, .config/nextest.toml]
  modified: [Cargo.toml, Cargo.lock, src/main.rs, src/template/mod.rs, src/audit/mod.rs]

key-decisions:
  - "Formatting gate replaced: repo baseline fails cargo fmt --check (167 pre-existing hunks); 01-01 gated on rustfmt --check of src/process.rs plus no increase in workspace hunk count"
  - "Post-exit path does not kill the group while the pipes close by themselves; only pipe holders still open at the deadline are killed (credential-cache survival), as the plan specified"

patterns-established:
  - "Bounded runner call shape: build Command, then process::run_bounded(cmd, \"git <verb>\", process::LOCAL_TIMEOUT) and keep the caller's existing fallback chain"

requirements-completed: [SYNC-03]

coverage:
  - id: D1
    description: "run_bounded kills the whole process group on timeout and returns a label-naming timeout error within the bound"
    requirement: SYNC-03
    verification:
      - kind: unit
        ref: "src/process.rs#process::tests::test_timeout_kills_group"
        status: pass
    human_judgment: false
  - id: D2
    description: "Concurrent stdout/stderr drain: 1 MiB on each stream returns exact lengths; silent child returns empty buffers"
    requirement: SYNC-03
    verification:
      - kind: unit
        ref: "src/process.rs#process::tests::test_drains_large_output"
        status: pass
    human_judgment: false
  - id: D3
    description: "Env scrub of the 15 git local vars plus non-interactive vars; global git config override passes through"
    requirement: SYNC-03
    verification:
      - kind: unit
        ref: "src/process.rs#process::tests::test_env_scrub"
        status: pass
    human_judgment: false
  - id: D4
    description: "Errors name only the label, never argv (token-bearing URL never appears)"
    requirement: SYNC-03
    verification:
      - kind: unit
        ref: "src/process.rs#process::tests::test_spawn_failure_names_label"
        status: pass
    human_judgment: false
  - id: D5
    description: "Background job without pipes survives a normal exit; pipe-holding group member is killed at the deadline"
    requirement: SYNC-03
    verification:
      - kind: unit
        ref: "src/process.rs#process::tests::test_background_process_without_pipes_survives"
        status: pass
    human_judgment: false
  - id: D6
    description: "A setsid'd pipe holder outside the group cannot block the call (timeout and post-exit paths), holder left alive"
    requirement: SYNC-03
    verification:
      - kind: unit
        ref: "src/process.rs#process::tests::test_pipe_holder_outside_group_timeout_path"
        status: pass
      - kind: unit
        ref: "src/process.rs#process::tests::test_pipe_holder_outside_group_post_exit_path"
        status: pass
    human_judgment: false
  - id: D7
    description: "All 10 non-sync git spawns (init, template, doctor) run through run_bounded with unchanged fallbacks"
    requirement: SYNC-03
    verification:
      - kind: e2e
        ref: "cargo test --test e2e_lifecycle (8/8, incl. five doctor tests with negative controls)"
        status: pass
      - kind: other
        ref: "! rg -n '\\.output\\(\\)|\\.status\\(\\)|\\.spawn\\(\\)' src/main.rs src/template/mod.rs src/audit/mod.rs (control: semantic/mod.rs spawn count = 1)"
        status: pass
    human_judgment: false
  - id: D8
    description: "Windows JobObject branch of run_bounded"
    requirement: SYNC-03
    verification: []
    human_judgment: true
    rationale: "Never compiled or run: only x86_64-unknown-linux-gnu is installed on this host. Needs cargo check --target x86_64-pc-windows-msvc and a timeout run on a Windows host (plan Task 2 human-check)."

duration: 11min
completed: 2026-10-18
status: complete
---

# Phase 01 Plan 01: Bounded Subprocess Runner Summary

**`crate::process::run_bounded` on process-wrap 10.0.1: process-group (Unix) / Job Object (Windows) kill at the deadline, two-thread output drain, a KILL_GRACE-bounded reader join that a setsid'd pipe holder cannot defeat, git env scrub and label-only errors; all 10 non-sync git spawns now go through it.**

## Performance

- **Duration:** about 11 min
- **Started:** 2026-10-18T17:22:53Z
- **Completed:** 2026-10-18T17:33:30Z
- **Tasks:** 2
- **Files modified:** 7 (2 created, 5 modified)

## Accomplishments
- `src/process.rs`: `run_bounded`, `prepare_command`, `RunOutput`, `LOCAL_TIMEOUT` (30 s), `NETWORK_TIMEOUT` (60 s), `POLL_INTERVAL` (10 ms), `KILL_GRACE` (1 s), `GIT_LOCAL_ENV_VARS` (15 names).
- Eight tests, each with a negative control; suite runs in about 1.4 s.
- Tracer: `TemplateContext::detect_for_project` git `user.name` / `user.email` lookups run through the runner (exercised by every `ude sync` path and the e2e lifecycle test).
- `ude init` (3 calls) and `ude doctor` (5 queries) converted with their original fallbacks unchanged.

## Task Commits

1. **Task 1 (tracer, TDD) RED:** `8870f29` test(01-01): add failing tests for bounded subprocess runner
2. **Task 1 GREEN:** `1e41157` feat(01-01): implement bounded subprocess runner with group kill
3. **Task 2:** `d7a2d12` feat(01-01): route init and doctor git calls through run_bounded

No REFACTOR commit (nothing to clean up after GREEN).

## TDD Evidence

**RED** (`cargo nextest run --bin ude process:: --no-fail-fast`, exit 100, JUnit at target/nextest/default/junit.xml, run started 2026-10-18T17:26:44.823Z; stub `run_bounded` = plain std `output()`, stub `prepare_command` = no-op). Classifier `gsd_run check tdd-red-evidence` returned `RED_EVIDENCE_OK` / `target_test_failed` for each of the six targets:

| Target test | Failed on (planned assertion) |
|---|---|
| test_env_scrub | `GIT_ALTERNATE_OBJECT_DIRECTORIES is not removed` (left: None, right: Some(None)) |
| test_spawn_failure_names_label | `error lacks spawn context: No such file or directory (os error 2)` |
| test_timeout_kills_group | `hung child returned Ok after 5.00s` |
| test_background_process_without_pipes_survives | negative-control bound: `call took 6.007s` (first, pipe-less case passed) |
| test_pipe_holder_outside_group_timeout_path | `hung child returned Ok after 10.005s` |
| test_pipe_holder_outside_group_post_exit_path | `pipes held open but returned Ok after 10.009s` |

Two tests passed in RED, as expected for behaviour std already has: `test_run_bounded_captures_git_output` and `test_drains_large_output` (std `output()` already drains both pipes). They are regression controls, not new-behaviour targets.

Semantic assessment: every failure is the named target on its planned assertion; no compile, fixture, setup or panic-before-assertion failures. Classifier negative control: the same record with `targetTest` set to the passing `test_drains_large_output` returned `INVALID_RED` / `no_target_test_failure`.

**GREEN:** all 8 pass (`cargo test --bin ude process::`, 1.35-1.42 s, 6 consecutive runs). **Mutation checks** (manual, file restored byte-identical after each):
- Removing the `ProcessGroup::leader()` wrap (leader-only kill): `test_timeout_kills_group` fails on the dead-grandchild assertion (its time bound still passes, as the plan predicted) and `test_background_process_without_pipes_survives` fails.
- Making the reader poll ignore its limit (blind join): both pipe-holder tests fail at about 10 s and the background test at 6 s. The sed also hit the identically-written loop in the test helper `dies_within`; the failures shown come from the production path.

## Files Created/Modified
- `src/process.rs` - bounded runner and its tests
- `.config/nextest.toml` - JUnit report for RED evidence
- `Cargo.toml`, `Cargo.lock` - process-wrap 10.0.1 (`default-features = false`, std/process-group/job-object)
- `src/main.rs` - `pub mod process;` and 3 init git calls
- `src/template/mod.rs` - 2 git config lookups
- `src/audit/mod.rs` - 5 doctor git queries

## Decisions Made
- `run_bounded` takes `cmd: Command` (not `mut cmd`) and rebinds it inside, so the signature matches the plan's acceptance regex on one line.
- The reader helper is split into `Readers::finish_by(limit) -> bool` and `Readers::collect(label)`, rather than one `join_readers_by` returning an Option. This lets the post-exit path poll the same handles twice (until the deadline, then for KILL_GRACE after the kill) without giving them up. The plan named its helper only as an example.
- The test helper `dies_within(pid, 1 s)` polls for death instead of checking once. A SIGKILLed process closes its pipes slightly before the kernel marks it a zombie. A leader-only kill leaves the grandchild alive for about 5 s, so the 1 s window still tells the two cases apart (confirmed by mutant A).

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] `cargo fmt --check` gate cannot pass on this repository**
- **Found during:** Task 1 (first `cargo fmt` run)
- **Issue:** The baseline at eaa0afe fails `cargo fmt --check` (exit 1, 167 hunks in 9 pre-existing files; no rustfmt.toml). My first `cargo fmt` reformatted all 9 files. I reverse-applied that diff from a saved patch. A blanket `git checkout` was blocked by the permission classifier; the patch route kept the change reversible.
- **Fix:** Gate on `rustfmt --check --edition 2024 src/process.rs` (new file, exact) plus "workspace hunk count <= 167 baseline, none in process.rs". Edited regions in main/template/audit were written to rustfmt's output, so the count ended at 166 (the old update-index line was rewritten). Negative control: a misformatted copy of process.rs fails `rustfmt --check`.
- **Files modified:** none beyond plan files. Logged in `deferred-items.md`.
- **Verification:** gate script exit 0 before each commit.

**2. [Rule 1 - Bug, test hygiene] RED stub needed a temporary `#![allow(dead_code)]`**
- **Found during:** Task 1 RED
- **Issue:** With no production caller yet, the stub module would trip `-D warnings`. The plan requires clippy to pass on the RED commit.
- **Fix:** Module-level allow in the RED commit only, removed in GREEN (1e41157). In GREEN, clippy with `-D warnings` reported no dead-code warning for `NETWORK_TIMEOUT`, which has no caller until 01-03.
- **Committed in:** 8870f29 (added), 1e41157 (removed)

---

**Total deviations:** 2 auto-fixed (1 blocking, 1 test hygiene). **Impact on plan:** None on behaviour. The fmt gate is weaker than the plan's literal `cargo fmt --check`: it proves that no new drift was added, not that the repository is formatted.

## Issues Encountered
- Actual size (about 9.4k tokens by chars/4 over the diff) is far below the 70k estimate. The estimate seems to have counted context reading, not the diff.

## Known Stubs
None.

## Threat Flags
None. No surface beyond the plan's threat model.

## What the verification does NOT establish
- **Windows:** the JobObject branch has never been compiled or run (no Windows target on this host). This is coverage item D8, and it is recorded in `.planning/WINDOWS.md` as an unrun verify.
- **Credential-cache daemon survival** is assumed from git's behaviour. Only the mechanism (a pipe-less background job survives) is tested.
- **git daemonizing children** (`gc --auto`) closing their standard streams is still the plan's [ASSUMED] item. If one does not, the result is an Err after timeout + KILL_GRACE, never a hang.
- **Load:** six consecutive green runs on an otherwise idle host are a weak flakiness bound. The time bounds keep 2 s or more of headroom by design, but they were not exercised under heavy load.
- **pgid reuse:** after a normal exit, `start_kill` on an empty group is a `killpg` on a PID that could in theory be reused. The kernel does not reuse a PID while it is a live group's ID, so the only window is when the group is already empty. Judged negligible and not tested.

## User Setup Required
None.

## Next Phase Readiness
- 01-03 can move the 31 `src/sync/mod.rs` spawns onto `process::run_bounded` (network tier `NETWORK_TIMEOUT`).
- Repo-wide formatting is still open (`deferred-items.md`). Every later plan's `cargo fmt --check` gate will fail the same way until it is resolved.

## Self-Check: PASSED
- FOUND: src/process.rs, .config/nextest.toml, Cargo.toml, src/main.rs, src/template/mod.rs, src/audit/mod.rs
- FOUND commits on this branch: 8870f29, 1e41157, d7a2d12 (`check evaluation-scope --plan 01-01 --commits-only`: status resolved, 3 commits)

---
*Phase: 01-sync-engine-state-hermeticity*
*Completed: 2026-10-18*
