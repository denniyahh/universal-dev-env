---
phase: 01-sync-engine-state-hermeticity
plan: 02
subsystem: testing
tags: [rust, git, test-fixtures, hermeticity, tempfile]

requires: []
provides:
  - "sync test module: GitFixture { new, cmd, run, output, gitconfig_env } and FIXTURE_GIT_LOCAL_ENV_VARS (15 names)"
  - "tests/e2e_lifecycle.rs: scrubbed_git(dir, global_config) as the file's only git Command constructor, run_checked, and a GitFixture copy"
  - "Every fixture git step in sync unit tests and e2e tests asserts success (no `let _ =` discards left in test code)"
affects: [01-03, 01-07, 01-09, 01-10, 01-11, sync, e2e]

actuals:
  tokens: 8460
  tasks: 2
  commits: 2
plan_head_before: 82a89a895cfe35903cc1837f39f4f2d0992e55f7
plan_head_after: 746535a41c0cc0c2162aad8d9eecd045897b6021

tech-stack:
  added: []
  patterns:
    - "Fixture git commands come only from GitFixture::cmd (sync tests) or scrubbed_git (e2e); run/output panic with the git args and stderr on failure"
    - "Tests that need `git config --global` for the ude binary write through scrubbed_git with GIT_CONFIG_GLOBAL=<fake HOME>/.gitconfig and pin the same file on the ude invocation"

key-files:
  created: []
  modified: [src/sync/mod.rs, tests/e2e_lifecycle.rs]

key-decisions:
  - "Fixture commits drop -c user.*, --no-gpg-sign and --no-verify, and the per-repo `git config user.name/email` steps, because the fixture gitconfig supplies identity and gpgsign=false and no host hooks are read. Kept the steps rustfmt-clean without wrapping. None of the engine code in these tests commits in those fixture repos."
  - "The e2e GitFixture::output carries #[allow(dead_code)]: no e2e test reads git output yet, and the plan asks for the same API in both copies"
  - "Fixture gitconfig identity check added to test_git_fixture_scrubs_local_env (git reads `user.name = UDE Tester` from it), so the self-test covers runtime behaviour, not only Command::get_envs"

patterns-established:
  - "New git-fixture tests build a GitFixture and use git.run / git.output; raw Command::new(\"git\") in test code is reserved for GitFixture::cmd, scrubbed_git and the one negative control"

requirements-completed: [SYNC-03]

coverage:
  - id: D1
    description: "GitFixture command removes all 15 repo-local git variables, pins GIT_CONFIG_GLOBAL to the fixture file and GIT_CONFIG_NOSYSTEM=1, and git actually reads the fixture identity; a plain Command carries none of these entries"
    requirement: SYNC-03
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_git_fixture_scrubs_local_env"
        status: pass
    human_judgment: false
  - id: D2
    description: "All sync unit-test fixture git steps go through GitFixture and assert success; no test assertion changed"
    requirement: SYNC-03
    verification:
      - kind: unit
        ref: "cargo test --bin ude sync:: (37/37)"
        status: pass
      - kind: other
        ref: "test-module count of Command::new(\"git\") = 2 and no `let _ = Command::new` (plan Task 2 count checks; base tree gives 44 and 34)"
        status: pass
    human_judgment: false
  - id: D3
    description: "All 13 e2e git spawns go through scrubbed_git and assert success; doctor security test writes and reads one pinned <fake HOME>/.gitconfig"
    requirement: SYNC-03
    verification:
      - kind: e2e
        ref: "cargo test --test e2e_lifecycle (8/8)"
        status: pass
      - kind: other
        ref: "e2e count of Command::new(\"git\") = 1 (base tree gives 13); GIT_CONFIG_GLOBAL=<scratch file> run leaves that file byte-identical"
        status: pass
    human_judgment: false
  - id: D4
    description: "Production code of src/sync/mod.rs (above `mod tests {`) is byte-identical to the plan base"
    verification:
      - kind: other
        ref: "cmp of `sed -n '1,/^mod tests {/p'` against 82a89a8 and HEAD"
        status: pass
    human_judgment: false

duration: 7min
completed: 2026-10-18
status: complete
---

# Phase 01 Plan 02: Hermetic Git Test Fixtures Summary

**`GitFixture` test helper (scrubs the 15 repo-local git variables, fixture gitconfig with a test identity and no signing, `GIT_CONFIG_NOSYSTEM=1`, no prompts, panics with args and stderr on failure) in the sync test module, plus an e2e copy built on one `scrubbed_git` builder. All 44 sync-test and 13 e2e fixture git spawns now go through it.**

## Performance

- **Duration:** about 7 min
- **Started:** 2026-10-18T17:37:07Z
- **Completed:** 2026-10-18T17:44:11Z
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments
- `src/sync/mod.rs` test module: `GitFixture { new, cmd, run, output, gitconfig_env }`, `FIXTURE_GIT_LOCAL_ENV_VARS`, and `test_git_fixture_scrubs_local_env` (env checks, a runtime identity check, and a plain-Command negative control).
- Six sync tests converted: `test_remote_repo_cache_management` (tracer), `test_defect2_publication_gate_allowlist_staging_blocks_unvalidated_files`, `test_publication_gate_scans_staged_blob_rejecting_clean_filter_secrets`, `test_slash_branch_name_lock_and_cache`, `test_direct_push_after_pr_mode_resets_branch_and_pushes_target`, `test_post_commit_verification_catches_hook_injected_secrets_and_rolls_back`.
- `tests/e2e_lifecycle.rs`: `scrubbed_git`, `run_checked` and `GitFixture`. All 13 sites converted. The doctor security test writes its `--global` settings into `<fake HOME>/.gitconfig` and both ude doctor runs pin `GIT_CONFIG_GLOBAL` to that file.

## Task Commits

1. **Task 1 (tracer): GitFixture helper proven on test_remote_repo_cache_management** - `c9947a7` (test)
2. **Task 2: Convert every remaining fixture git spawn** - `746535a` (test)

Tracer gate (interactive, `end-of-phase`, automated-only verify): verify re-run end-to-end before Task 2 and passed.

## TDD Gate Compliance

Both commits are `test(01-02)` and there is no `feat(01-02)` commit. That is expected here and not a violation: the plan is `type: execute`, neither task has `tdd="true"` or a `<behavior>` block, and `gsd_run query task.is-behavior-adding` returned `false`. The plan changes only test plumbing, so no production behaviour exists to drive RED/GREEN. In place of RED evidence, the new self-test was mutation-checked: with the `env_remove` loop removed it fails on `GIT_ALTERNATE_OBJECT_DIRECTORIES must be removed from fixture git commands`, and it passes again once restored (file restored byte-identical).

## Verification

| Check | Result |
|---|---|
| `cargo test --bin ude sync::` | 37/37 pass (36 before plus the new self-test) |
| `cargo test --test e2e_lifecycle` | 8/8 pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 |
| Plan count check (test-module git Commands = 2, e2e = 1, no set_var/remove_var) | pass. Negative control: the base tree gives 44 / 13 |
| Plan discard check (no `let _ = Command::new` in test region or e2e) | pass. Negative control: the base tree gives 34 / 13 |
| Production region byte-identical (vs HEAD and vs plan base 82a89a8) | pass |
| Fail-loud negative control: seed push changed to `no-such-branch` | panics with `fixture step \`git push origin no-such-branch\` failed (exit status: 1): error: src refspec ...`, file restored |
| Inherited `GIT_CONFIG_GLOBAL=<scratch file>` during the e2e run | 8/8 pass, scratch file byte-identical. Control: plain `git config --global` with that env does write into it |

**Formatting gate (substitute; repo-wide formatting is still an open decision):** the literal `cargo fmt --check` still fails, as it does on the baseline. In its place: (a) the total `Diff in` count must not rise above 166 at plan start (it ended at 150, because rewritten fixture blocks replaced pre-existing misformatted ones), and (b) no rustfmt hunk may have one of this plan's added lines on its current side (scripted check; its negative control flagged 9 hunks before they were fixed). **Limits:** this proves that the plan added no new drift. It does not prove that either file is rustfmt-clean, because pre-existing lines in the same tests still have hunks. Lines consisting only of `}`, `);`, `{` or `],` are not counted by the check.

## Decisions Made
- Dropped the redundant identity, signing and `--no-verify` flags and the per-repo `config user.*` steps from fixture commits (the fixture gitconfig covers them). This kept the converted lines rustfmt-clean.
- The e2e `GitFixture::output` is kept with `#[allow(dead_code)]` for API parity. `#[expect]` was rejected: it would fail `-D warnings` in whichever later plan first uses `output`.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] `cargo fmt --check` verify step cannot pass on this repository**
- **Found during:** Task 1 verify
- **Issue:** The baseline fails `cargo fmt --check` (166 hunks at plan start, open item in `deferred-items.md`).
- **Fix:** Used the substitute gate described above, as instructed by the orchestrator and as 01-01 did. No repo-wide `cargo fmt` was run.
- **Files modified:** none beyond plan files
- **Verification:** gate script exit 0 before each commit

---

**Total deviations:** 1 (blocking, pre-existing). **Impact on plan:** none on behaviour. The fmt gate is weaker than the literal plan gate (see the limits above).

## What the verification does NOT establish
- **Full hermeticity under an inherited `GIT_DIR`:** not achieved, and not expected from this plan. With `GIT_DIR=<decoy bare repo>`, 4 sync tests and 1 e2e test still fail. Every one of those failures is at an engine or ude assertion (for example `git fetch failed with exit status Some(128)`, `PR branch must be pushed`), and none is a `fixture step` panic. The engine still inherits the test process environment until 01-03. The research recorded 6 sync failures at baseline; I did not re-measure that baseline.
- **Empty host git config (`GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1`):** 1 sync test still fails, `test_direct_push_after_pr_mode_resets_branch_and_pushes_target` with `git commit failed in remote checkout`. The cause is the engine committing in its cache clone without an identity. 01-03 fixes this by injecting `gitconfig_env()`. The e2e doctor security test, which failed under this env per the research, now passes because of the `GIT_CONFIG_GLOBAL` pin.
- **Runtime env scrub:** `test_git_fixture_scrubs_local_env` checks `Command::get_envs()`, not a git process run with an inherited `GIT_DIR`. Setting `GIT_DIR` in the parent would need `set_var`, which is forbidden. The decoy probe above covers the runtime side for the converted tests, but only by observation, not as a committed test.
- **Flakiness:** each suite was run about 4 to 6 times on an idle host, all green. That is a weak bound.

## Issues Encountered
None.

## Known Stubs
None.

## Threat Flags
None. T-01-05 is mitigated as planned. No new surface.

## User Setup Required
None.

## Next Phase Readiness
- 01-03 can replace `FIXTURE_GIT_LOCAL_ENV_VARS` with `crate::process::GIT_LOCAL_ENV_VARS`, inject `GitFixture::gitconfig_env()` into the engine, and run the `GIT_DIR` decoy gate.
- Repo-wide formatting is still an open decision (`deferred-items.md`).

## Self-Check: PASSED
- FOUND: src/sync/mod.rs, tests/e2e_lifecycle.rs
- FOUND commits on this branch: c9947a7, 746535a (`check evaluation-scope --plan 01-02 --commits-only`: status resolved, 2 commits)

---
*Phase: 01-sync-engine-state-hermeticity*
*Completed: 2026-10-18*
