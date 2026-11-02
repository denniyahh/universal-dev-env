---
phase: 02-sanitizer-boundaries-security-hardening
plan: 14
subsystem: sync
tags: [rust, sync, push, git-hooks, grafts, publication-gate, gap-closure]

requires:
  - phase: 02-sanitizer-boundaries-security-hardening (02-13)
    provides: "the committed gate bound to the staged tree, and the hook test helpers HookedPush, holds_object, push_to_fresh_bare, assert_published_exact"
provides:
  - "Both pushes run with GIT_GRAFT_FILE at the null device, so a grafts file written after the gate sends nothing outside the validated commit"
  - "process::run_bounded_ignoring_grafts: sets a constant after the scrub; prepare_command is byte-identical"
  - "A failed push's excerpt holds the remote's text and not git's grafts deprecation hint"
  - "A direct push refused as not a fast-forward says upstream moved only when git ls-remote does not report the branch at the fetched commit"
affects: [sync push, phase-02 verification gap 3, review WR-203, T-02-52, SEC-04, SEC-02, plan 02-15]

actuals:
  tokens: 7727     # chars/4 over the realized src/ and tests/ diff (30906 chars, git diff 04872ae..0aab389)
  tasks: 3
  commits: 6
plan_head_before: 04872aee7cc6491c0b0043650a21d5bd8360c23d
plan_head_after: 0aab389cde53c9a9db4613ecc82cd74941e3bd20

tech-stack:
  added: []
  patterns:
    - "A hook-written remote.origin.receivepack that signals a bounded background process and waits for its write forces a write to land after the gate and before push's object walk, in every run"
    - "A git shim first on PATH that logs one variable per git process tests what a real inherited value reaches, through the CLI"

key-files:
  created: []
  modified:
    - src/process.rs
    - src/sync/mod.rs
    - tests/e2e_lifecycle.rs

key-decisions:
  - "02-14: push_direct and push_pr_branch run through SyncEngine::run_push, which sets GIT_GRAFT_FILE to the constant process::NULL_DEVICE after prepare_command's scrub; refuse_grafts_file stays by operator decision 2026-10-28"
  - "02-14: both pushes add -c advice.graftFileDeprecated=false, because with GIT_GRAFT_FILE naming an existing file the hint takes lines of a refusal's 5-line excerpt"
  - "02-14: push_direct returns Failed (exit 2) instead of UpstreamMoved when a non-fast-forward refusal comes with git ls-remote reporting the branch at the fetched commit; that read is hook-steerable, so only the wording of a refusal depends on it"

patterns-established:
  - "A carve-out from the environment scrub is a separate function that sets a constant after the scrub, never a value check inside prepare_command"

requirements-completed: [SEC-04, SEC-02]

coverage:
  - id: D1
    description: "A grafts file written by a pre-push hook sends nothing outside the validated commit, in direct and PR mode; the same commit pushed without the variable delivers the injected blob"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_direct_push_sends_no_object_from_pre_push_grafts"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_pr_push_sends_no_object_from_pre_push_grafts"
        status: pass
    human_judgment: false
  - id: D2
    description: "A grafts file written during the push by a process a post-commit hook left sends nothing, in direct and PR mode, three runs per test, each with the negative control"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_direct_push_sends_no_object_from_grafts_written_during_push"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_pr_push_sends_no_object_from_grafts_written_during_push"
        status: pass
    human_judgment: false
  - id: D3
    description: "An inherited GIT_GRAFT_FILE reaches no git process UDE spawns; only the push sees the null device, byte for byte"
    requirement: SEC-04
    verification:
      - kind: e2e
        ref: "tests/e2e_lifecycle.rs#test_cli_direct_push_alone_gets_the_null_grafts_file"
        status: pass
      - kind: e2e
        ref: "tests/e2e_lifecycle.rs#test_cli_pr_push_alone_gets_the_null_grafts_file"
        status: pass
      - kind: unit
        ref: "src/process.rs#test_run_bounded_ignoring_grafts_sets_the_null_device"
        status: pass
      - kind: unit
        ref: "src/process.rs#test_env_scrub"
        status: pass
    human_judgment: false
  - id: D4
    description: "A failed push reports the remote's error and not the grafts hint, in both modes; a benign pre-push hook still publishes"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_direct_push_failure_excerpt_has_no_grafts_hint"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_pr_push_failure_excerpt_has_no_grafts_hint"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_direct_push_publishes_with_benign_pre_push_hook"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_pr_push_publishes_with_benign_pre_push_hook"
        status: pass
    human_judgment: false
  - id: D5
    description: "A direct push refused as not a fast-forward blames upstream only when the branch is not confirmed at the fetched commit; a real move still says so"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_direct_push_shallow_file_refusal_does_not_say_upstream_moved"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_direct_push_replaced_commit_refusal_does_not_say_upstream_moved"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_pr_push_with_shallow_file_publishes_nothing"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_remote_branch_is_at"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_direct_push_aborts_when_upstream_moved"
        status: pass
      - kind: e2e
        ref: "tests/e2e_lifecycle.rs#test_force_does_not_bypass_upstream_moved"
        status: pass
    human_judgment: false

duration: 12min
completed: 2026-11-02
status: complete
---

# Phase 2 Plan 14: Pushes that ignore a late grafts file Summary

**Both pushes now run with GIT_GRAFT_FILE at the null device, so a grafts file written after the gate sends nothing outside the validated commit, and a direct-push refusal no longer blames upstream when `git ls-remote` reports the branch at the fetched commit. Gap 3 is closed pending plan 02-15's controls and re-verification.**

## Read this first

**Every RED and GREEN prediction in the plan held; nothing needed a changed hook or a weakened test.** The one thing the plan's text does not separate: **a git process run by a pre-push hook also sees the null device**, because a hook inherits the push's environment. See below.

**What this does not establish.**

- **"No other git process sees the variable" means no git process UDE spawns.** The CLI tests have no hook installed. A plain-git probe (git 2.56.0, one run, `GIT_GRAFT_FILE=/dev/null git push` with a pre-push hook and a `receivepack` command) logged `hook:/dev/null` and `receivepack:unset`. So a git command inside a hook reads the null device, and the value is still only the constant. The remote side on a local path sees it unset, because git clears local variables for it. No test pins either result.
- **The race test forces the order.** It shows the channel is closed when the write lands inside the push. It does not measure how often the unforced race hits, and three runs per test are a weak bound on anything except the deterministic handshake.
- **A hostile hook is not stopped from pushing by itself.** It can run its own `git push` with the user's credentials, as the operator recorded in AR-02-10. The rule is about what UDE's own push sends.
- **Other state written during a push is not claimed.** `refs/replace/*`, `.git/shallow` and overwritten loose objects were probed by the planner on plain git and were not run through UDE here with the handshake (T-02-64, T-02-65). The channel list is the channels probed, not a proven complete list.
- **"A successful push makes no `git ls-remote` call" rests on where the read is.** It is inside the non-fast-forward arm and the structure check passes. No test counts `ls-remote` calls on a successful push.
- **`still at the commit UDE fetched` means what `git ls-remote` printed.** A hook-written `remote.origin.url` or `uploadpack` can make it print the fetched commit when the branch has moved. Both outcomes are refusals, so this affects wording only (T-02-68, planner's probes, not repeated here).
- **The exit code for the new refusal is read from `src/main.rs`, not run.** The new case returns `PushRemoteError::Failed`, whose arm is the existing one. The tests assert on the error value in the engine; no CLI test drives a shallow file or a replace ref.
- **Platform and remote.** Every hook test is on git 2.56.0 on Linux against a local bare remote. No hosted remote and no Windows run. `NUL` under `cfg(windows)` is unverified (T-02-67).
- **No mutation run and no per-check controls here.** Both are plan 02-15's. `cargo mutants --list` was run only to count what it will see (below).

## Performance

- **Duration:** 12 min
- **Started:** 2026-11-02T16:49:45Z
- **Completed:** 2026-11-02T17:01:09Z
- **Tasks:** 3
- **Files modified:** 3

## Accomplishments

- `process::run_bounded` is now `prepare_command` then the private `spawn_prepared`, and its behaviour is unchanged. `prepare_command`'s text hashes the same at the plan's base commit and at `0aab389`. `GIT_LOCAL_ENV_VARS` and the e2e copy stay 15 entries.
- `process::run_bounded_ignoring_grafts` sets `GIT_GRAFT_FILE` to `NULL_DEVICE` after the scrub, and `SyncEngine::run_push` is its only caller.
- `refuse_grafts_file` stays. `test_direct_push_refuses_hook_grafts_file` and `test_pr_push_refuses_hook_grafts_file` pass, and its comment now says the refusal stays by operator decision and the pushes do not read the file.
- A real inherited value is tested through `ude sync push`: the shim's own negative control logs the decoy, and every UDE-spawned git process other than the push logs `unset`.
- The deprecation hint is off on both pushes, and a benign pre-push hook publishes in both modes.
- `push_direct` reads `git ls-remote` only after a non-fast-forward rejection. A real move still gives `UpstreamMoved`.

## Task Commits

1. **Task 1: Tracer - both pushes run with GIT_GRAFT_FILE at the null device**
   - RED `beb0e0e` test(02-14): add failing tests for a grafts file written after the gate
   - GREEN `d08e5fe` feat(02-14): run both pushes with GIT_GRAFT_FILE at the null device
2. **Task 2: The deprecation hint stays out of a failed push's error**
   - RED `9b47b17` test(02-14): add failing tests for the grafts hint in a push failure
   - GREEN `9a561f4` feat(02-14): keep the grafts hint out of a failed push's error
3. **Task 3: A refusal blames upstream only when the branch is not confirmed at the fetched commit**
   - RED `0a2004c` test(02-14): add failing tests for a refusal blaming upstream wrongly
   - GREEN `0aab389` fix(02-14): blame upstream only when ls-remote does not clear it

The tracer gate ran after Task 1 (`human_verify_mode` end-of-phase, automated-only verify): the plan's commands were run, then the fmt, clippy and `cargo test` chain was re-run as the commit's gate, before Task 2 started.

## RED Evidence

Records are in `target/gsd-red/02-14-*.json` (gitignored), built from the nextest JUnit report. All 10 were classified `RED_EVIDENCE_OK` / `target_test_failed` by `gsd-tools check tdd-red-evidence`. No test's hook needed a fix.

**Task 1, unit: 4 run, 4 failed, as predicted.** Each failed on `the remote holds the injected blob`, after `hook_ran` and the Ok assertion passed:

`test_direct_push_sends_no_object_from_pre_push_grafts`, `test_pr_push_sends_no_object_from_pre_push_grafts`, `test_direct_push_sends_no_object_from_grafts_written_during_push`, `test_pr_push_sends_no_object_from_grafts_written_during_push`. The during-the-push tests failed in their first run, so the loop did not reach runs 2 and 3 at RED.

**Task 1, e2e: 2 run, 2 failed, as predicted.** `test_cli_direct_push_alone_gets_the_null_grafts_file` and `test_cli_pr_push_alone_gets_the_null_grafts_file` failed at `assertion left == right failed: the push's value: [...]`, `left: "unset"`, `right: "/dev/null"`. Exit 0, one push line and the shim's negative control had already passed.

**Task 2: 4 run, 2 failed, 2 passed, as predicted.** Controls passed: `test_direct_push_publishes_with_benign_pre_push_hook`, `test_pr_push_publishes_with_benign_pre_push_hook`.

| Test | Failure line (the assertion on `graftFileDeprecated`, after the remote's text was found) |
|------|------|
| `test_direct_push_failure_excerpt_has_no_grafts_hint` | `git push origin main failed: hint: \| hint: Turn this message off by running \| hint: "git config set advice.graftFileDeprecated false" \| remote: REMOTE_REFUSED_THIS_PUSH \| error: failed to push some refs to '/tmp/...'` |
| `test_pr_push_failure_excerpt_has_no_grafts_hint` | `git push failed for branch ude-sync-...: hint: "git config set advice.graftFileDeprecated false" \| remote: REMOTE_REFUSED_THIS_PUSH \| To /tmp/... \| ! [remote rejected] ... (pre-receive hook declined) \| error: failed to push some refs to '/tmp/...'` |

These match the plan's stderr probe exactly: 3 hint lines in the `--porcelain` form, 1 in the plain form, and the remote's text kept in both.

**Task 3: 4 run, 2 failed, 2 passed, as predicted.** `test_direct_push_shallow_file_refusal_does_not_say_upstream_moved` and `test_direct_push_replaced_commit_refusal_does_not_say_upstream_moved` failed on `expected a hard push failure, got Err(UpstreamMoved { branch: "main" })`, after `hook_ran` and `assert_remote_unchanged` passed. Controls passed: `test_pr_push_with_shallow_file_publishes_nothing`, `test_direct_push_aborts_when_upstream_moved`. `test_force_does_not_bypass_upstream_moved` also passed at RED (run separately through e2e). So plans 02-13 and 02-14 did not change what these hooks reach, which the plan had only reasoned.

`test_run_bounded_ignoring_grafts_sets_the_null_device` and `test_remote_branch_is_at` were written with GREEN, as the plan says, so they have no RED record. They need code that did not exist. Each holds its own opposite direction: `run_bounded` prints `unset`, and the `true` row of the table.

## GREEN Evidence

After `0aab389`: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, full `cargo test` and `git diff --exit-code -- Cargo.toml Cargo.lock` exit 0 as one chain gating the commit (332 unit passed, 90 e2e passed, 1 ignored: `test_dump_parity_corpus`, which this plan did not touch). The plan's name-list and structure commands exit 0 for all three tasks. The repository's `.git/config` hash and its 203 index entries were unchanged after the e2e runs, and TMPDIR was never set. No `ude-go` or `ude-ack` background process remained after the runs.

Each GREEN commit was gated on that chain's exit code in one command; RED commits on fmt and clippy, because their tests fail by design.

## Plan predictions checked

**Matched:** the pre-push and handshake rows of the engine table in both modes; the CLI shim row (exit 0, one `push` line with `/dev/null`, every other line `unset`); the excerpt lines above; the wrong message through the engine, with plans 02-13 and 02-14 applied (the plan had reasoned this); the PR-mode result, which was already accurate; `cargo mutants --list` shows 4 mutants for `remote_branch_is_at`, the same count as the prototype's. **Not repeated:** the prototype's mutation result (4 of 4 caught), which is plan 02-15's run.

**Did not hold:** nothing.

**A qualifier:** the must-have says every other git process sees the variable unset. That holds for what UDE spawns (tested). A git process a pre-push hook runs inherits the null device (probed once, see "Read this first").

## Deviations from Plan

### Auto-fixed Issues

None. No fix was needed beyond the plan's tasks.

### Notes on shape

- Test helpers beyond the plan's table: `assert_only_the_push_sees_the_null_device` (e2e), `push_refused_by_remote` and the constant `GRAFTS_DURING_PUSH_RUNS` (`mod tests`).
- The CLI decoy is an existing empty file rather than a bare path, so a leaked value would be one git can open.
- `PushPorcelain::RejectedNonFastForward`'s doc also said the rejection meant the branch moved, so it was reworded with `UpstreamMoved`'s, as the plan's instruction to update such docs covers.
- `0aab389` uses type `fix`, because it removes a wrong message. The GREEN gate's `feat` commits are `d08e5fe` and `9a561f4`.

**Total deviations:** 0 auto-fixed.
**Impact on plan:** none; no test was weakened and no scope was removed.

## Issues Encountered

- **My mistake, undone before any commit:** I first ran `requirements.mark-complete SEC-04 SEC-02` as the executor prompt's short form says. That flipped both from `Gaps Found` to `Complete`. The workflow gates this step on `requirements.ready-ids`, which reports both blocked because plan 02-15 also declares them and has no SUMMARY yet. I restored `.planning/REQUIREMENTS.md` with `git checkout` on that one file, so it still reads `Gaps Found` for both. Recorded in the GSD manual-fix incident log.

## For plan 02-15

**Names that differ from these plans' text.**

- Plan 02-14: no non-test function or test name differs. Added beyond the plan: the test-only names under "Notes on shape".
- Plan 02-13, non-test: `require_validated_content`, `split_at_nul`, and the type check inside `read_blob_by_id`. Tests: `test_committed_gate_refuses_tag_object_at_validated_path`, `test_direct_push_refuses_index_swap_to_tag_object`, `test_pr_push_refuses_index_swap_to_tag_object`, `test_push_refuses_index_swap_to_injected_blob`, `test_push_publishes_with_index_swap_to_the_staged_tree`.

**What may change the control table or the mutation scope. Plan 02-15 was not edited.**

- `cargo mutants --list` over `run_push|run_bounded_ignoring_grafts|remote_branch_is_at` gives 6 mutants. The 2 for `run_push` and `run_bounded_ignoring_grafts` are `Ok(Default::default())`, and `RunOutput` has no `Default`, so I expect both to come out unviable (not run). The mutation run would then say nothing about those two functions; rows 18, 19 and 22 are what cover them.
- `spawn_prepared` holds `run_bounded`'s former body and is outside the mutation regex. Its code moved without a behaviour change, and the existing process tests run through it. Whether to add it is that plan's call.
- Rows 18 to 23 name functions and tests that all exist under those names. The rows themselves were not run here.

## Threat Flags

| Flag | File | Description |
|------|------|-------------|
| threat_flag: T-02-64 | src/sync/mod.rs | pending the operator, not accepted |
| threat_flag: T-02-65 | src/sync/mod.rs | pending the operator, not accepted |
| threat_flag: T-02-67 | src/process.rs | pending the operator, not accepted |

Status when this SUMMARY was written; ## Operator answers in 02-15-PLAN.md holds the current one.

## TDD Gate Compliance

RED `beb0e0e` precedes GREEN `d08e5fe`; RED `9b47b17` precedes GREEN `9a561f4`; RED `0a2004c` precedes GREEN `0aab389`. No REFACTOR commit.

## User Setup Required

None.

## Next Phase Readiness

Plan 02-15 (controls, mutation run, final gates) is next. Gap 3 is closed pending plan 02-15's controls and re-verification; T-02-52 is ready to move from accepted to mitigated by the hand edit plan 02-15 names. This plan did not edit 02-REVIEW-DISPOSITION.md, 02-SECURITY.md or 02-VALIDATION.md. `requirements-completed` above copies the plan's frontmatter as the template asks and is not a verdict.

## Self-Check: PASSED

src/process.rs, src/sync/mod.rs and tests/e2e_lifecycle.rs exist; beb0e0e, d08e5fe, 9b47b17, 9a561f4, 0a2004c and 0aab389 are ancestors of HEAD (`git merge-base --is-ancestor`, checked before writing this file). `git rev-list --count 04872ae..0aab389` is 6.

---
*Phase: 02-sanitizer-boundaries-security-hardening*
*Completed: 2026-11-02*
