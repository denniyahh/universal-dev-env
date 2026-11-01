---
phase: 02-sanitizer-boundaries-security-hardening
plan: 12
subsystem: sync
tags: [rust, sync, push, git-hooks, publication-gate, commit-graph, bitmap, grafts, gap-closure]

requires:
  - phase: 02-sanitizer-boundaries-security-hardening (02-11)
    provides: "validate_committed_tree bound to the fetched head with raw reads; both pushes send the validated id; the hook test helpers"
provides:
  - "The committed gate binds the commit message to SYNC_COMMIT_MESSAGE plus one LF, secret-scans the rest of the header and refuses an empty directory in the tree, all from raw reads"
  - "The gate refuses while the cache checkout has a grafts file"
  - "Both pushes run with commit-graphs and bitmaps disabled and every GIT_TEST_* variable removed"
  - "A cache cloned from a gc'd local-path remote keeps publishing"
affects: [sync push, phase-02 verification gap 1, review WR-102, SEC-04]

actuals:
  tokens: 15957     # chars/4 over the realized src/ diff (63828 chars, git diff 83006ba..e0b4f12)
  tasks: 3
  commits: 5
plan_head_before: 83006ba52a68f453fb02bf569ed7341559bd8573
plan_head_after: e0b4f12c97982dfdde84b0cb8151fcb04d5134d5

tech-stack:
  added: []
  patterns:
    - "A no-leak test shows the forged state in effect and pushes it to a fresh bare repository without the protection, so a pass cannot come from a forge that did nothing"
    - "A git shim that prints fixed output reaches fail-closed branches a real git does not produce"

key-files:
  created: []
  modified:
    - src/sync/mod.rs

key-decisions:
  - "02-12: the committed gate requires the raw commit message to equal SYNC_COMMIT_MESSAGE plus exactly one LF, secret-scans the header (a scan, not a bind) and refuses any directory with no entry below it"
  - "02-12: the push is refused while the cache checkout has a grafts file; commit-graphs and bitmaps are disabled on both pushes instead of refused on presence, and every GIT_TEST_* variable is removed from both push commands"
  - "02-12: the grafts path is read as strict UTF-8, so a cache checkout under a non-UTF-8 path refuses rather than checking a lossy path that could read as absent"

patterns-established:
  - "Tests that find a git subcommand in a shim scan all arguments, because the pushes now pass global options first"

requirements-completed: [SEC-04]

coverage:
  - id: D1
    description: "A prepare-commit-msg or post-commit hook that writes the message is refused in direct and PR mode with the likely cause named; a hook that leaves the message alone still publishes"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_direct_push_refuses_prepare_commit_msg_hook_text"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_pr_push_refuses_prepare_commit_msg_hook_text"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_direct_push_refuses_hook_amended_message"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_pr_push_refuses_hook_amended_message"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_committed_gate_requires_exact_sync_message"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_raw_commit_parts"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_push_refuses_hook_replaced_message"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_direct_push_publishes_with_benign_prepare_commit_msg_hook"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_pr_push_publishes_with_benign_prepare_commit_msg_hook"
        status: pass
    human_judgment: false
  - id: D2
    description: "A token in the commit header and an empty directory in the tree are refused, without printing the hook's text; the listing is read raw"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_push_refuses_hook_secret_in_commit_header"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_push_refuses_hook_empty_directory"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_push_refuses_hook_replaced_tree_hiding_empty_directory"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_count_empty_directories"
        status: pass
    human_judgment: false
  - id: D3
    description: "A grafts file present when the gate runs is refused in direct and PR mode; the same injected commit with no grafts file publishes and sends nothing extra"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_direct_push_refuses_hook_grafts_file"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_pr_push_refuses_hook_grafts_file"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_direct_push_publishes_with_hook_commit_and_no_grafts_file"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_refuse_grafts_file_outcomes"
        status: pass
    human_judgment: false
  - id: D4
    description: "A forged commit-graph (local, split chain, alternate) and a graft-built pack or multi-pack-index bitmap send nothing outside the validated commit, each with a control that delivers the injected blob"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_direct_push_sends_no_object_from_forged_commit_graph"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_pr_push_sends_no_object_from_forged_commit_graph"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_push_sends_no_object_from_forged_split_commit_graph"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_push_sends_no_object_from_forged_alternate_commit_graph"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_direct_push_sends_no_object_from_graft_built_bitmap"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_pr_push_sends_no_object_from_graft_built_bitmap"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_push_sends_no_object_from_graft_built_multi_pack_index_bitmap"
        status: pass
    human_judgment: false
  - id: D5
    description: "GIT_TEST_COMMIT_GRAPH set through the engine's environment cannot re-enable the graph on either push; every GIT_TEST_ key is removed by the exact byte prefix"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_direct_push_ignores_inherited_git_test_commit_graph"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_pr_push_ignores_inherited_git_test_commit_graph"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_remove_git_test_env"
        status: pass
    human_judgment: false
  - id: D6
    description: "A cache cloned from a gc'd local-path remote, holding a commit-graph and a bitmap, publishes"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_push_from_clone_of_gc_remote_publishes"
        status: pass
    human_judgment: false
  - id: D7
    description: "The scoped mutation run and the phase's final gates pass on the final code commit"
    requirement: SEC-04
    verification:
      - kind: other
        ref: "target/mutants-binding/mutants.out (49 caught, 0 missed, 0 unviable, 0 timeouts)"
        status: pass
      - kind: e2e
        ref: "tests/e2e_lifecycle.rs#test_sync_push_pr_does_not_advance_ancestor_state"
        status: pass
      - kind: e2e
        ref: "tests/e2e_lifecycle.rs#test_sync_push_remote_failure_does_not_advance_ancestor_state"
        status: pass
      - kind: e2e
        ref: "tests/e2e_lifecycle.rs#test_sync_ignores_inherited_git_dir"
        status: pass
    human_judgment: false

duration: 32 min
completed: 2026-11-01
status: complete
---

# Phase 02 Plan 12: Commit Text Bound and Push Channels Closed Summary

**A hook in the cache checkout can no longer put its own text into the published commit, or get extra objects sent through a grafts file present at the gate, a forged commit-graph, a graft-built bitmap or GIT_TEST_COMMIT_GRAPH, in direct or PR mode. A cache git itself gc'd keeps publishing.**

Plans 02-11 and 02-12 together close 02-VERIFICATION gap 1 (review WR-102), pending re-verification. **One channel stays open by the operator's decision of 2026-10-28 (T-02-52):** a pre-push hook runs inside `git push`, after the gate, and a grafts file it writes then makes the push send the grafted objects. This plan does not claim that channel closed and adds no test of it. 02-REVIEW-DISPOSITION.md was not edited.

## Performance

- **Duration:** 32 min
- **Started:** 2026-11-01T15:11:37Z
- **Completed:** 2026-11-01T15:43:39Z
- **Tasks:** 3
- **Files modified:** 1 (src/sync/mod.rs)

## Accomplishments

- The message is bound byte for byte, with no trimming. Hook text no longer reaches the shared repository through prepare-commit-msg or a post-commit amend.
- The header is secret-scanned and an empty directory is refused, both without printing the hook's text.
- A grafts file present at the gate is refused. Commit-graphs and bitmaps are disabled on both pushes rather than refused on presence, so a clone of a gc'd local remote still works.
- Every `GIT_TEST_*` variable is removed from both push commands. `GIT_TEST_COMMIT_GRAPH` is the pinned case.

## Task Commits

1. **Task 1 (tracer): commit text bound**
   - RED `1f9ce45` test(02-12): add failing tests for hook-written commit text
   - GREEN `f552f78` feat(02-12): bind the commit message and check header and tree names
2. **Task 2: nothing sent outside the validated commit**
   - RED `998b8f5` test(02-12): add failing tests for objects sent outside the commit
   - GREEN `927e259` feat(02-12): send nothing outside the validated commit on push
3. **Task 3: mutation run and final gates.** No code change was forced by a mutant, so no commit of its own.
   - `e0b4f12` test(02-12): assert the replaced-ref refusals hide the hook's text. This came from the acceptance check before the final runs (see Deviations), and every result below is from this commit.

**Plan metadata:** recorded in the docs(02-12) commit that adds this file.

Tracer gate: `human_verify_mode` is end-of-phase and the tracer's verify is automated only, so its three commands were re-run on Task 1's final state (all exit 0) before Task 2 started.

## RED Evidence

Records are in `target/gsd-red/02-12-t1-*.json` and `02-12-t2-*.json` (gitignored), built from the nextest JUnit report. All twenty were classified `RED_EVIDENCE_OK` / `target_test_failed` by `gsd-tools check tdd-red-evidence`.

**Semantic check, all twenty:** each target ran and failed on its planned assertion, and each hook test failed after its `hook_ran` assertion passed (`test_committed_gate_requires_exact_sync_message` runs no hook). The right-hand columns come from throwaway probe tests on the same code, removed before each RED commit with a sha256 check.

### Task 1: nine failing

| Test | Failure line | Probe: what the bare remote held |
|---|---|---|
| `test_committed_gate_requires_exact_sync_message` | `no LF: Ok(())`, `two LFs: Ok(())`, `a line added: Ok(())`, each expected a refusal | n/a (direct call) |
| `test_direct_push_refuses_prepare_commit_msg_hook_text` | `the remote changed; push result: Ok("Directly pushed ...")` | message ends in `INJECTED_BY_HOOK` |
| `test_pr_push_refuses_prepare_commit_msg_hook_text` | `the remote changed; push result: Ok("Pushed changes to branch ...")` | `ude-sync-*` message ends in `INJECTED_BY_HOOK` |
| `test_direct_push_refuses_hook_amended_message` | same `Ok("Directly pushed ...")` | message has a second paragraph `INJECTED_BY_HOOK` |
| `test_pr_push_refuses_hook_amended_message` | same `Ok("Pushed changes to branch ...")` | `ude-sync-*` message has the same paragraph |
| `test_push_refuses_hook_replaced_message` | same `Ok("Directly pushed ...")` | message has `INJECTED_BY_HOOK` |
| `test_push_refuses_hook_secret_in_commit_header` | same | author name is the token |
| `test_push_refuses_hook_empty_directory` | same | `ls-tree -r -t` lists `INJECTED_DIR_NAME` |
| `test_push_refuses_hook_replaced_tree_hiding_empty_directory` | same | `ls-tree -r -t` lists `INJECTED_DIR_NAME` |

Passed at RED as expected: `test_direct_push_publishes_with_benign_prepare_commit_msg_hook`, `test_pr_push_publishes_with_benign_prepare_commit_msg_hook`.

### Task 2: eleven failing

| Tests | Failure line |
|---|---|
| `test_direct_push_refuses_hook_grafts_file`, `test_pr_push_refuses_hook_grafts_file` | `the remote changed; push result: Ok(...)`. Probe: the remote held the injected blob in both modes |
| The other nine (forged graph direct and PR, split chain, alternate, pack bitmap direct and PR, multi-pack-index bitmap, inherited variable direct and PR) | `the remote holds the injected blob` |

Each of those nine asserts the forge or bitmap in effect and the control push delivering the blob **before** that final assertion, so the failure cannot come from a forge that did nothing. The two inherited-variable tests assert the control push but not the forge check, because their control already proves the graph was read.

Passed at RED as expected: `test_direct_push_publishes_with_hook_commit_and_no_grafts_file`, `test_push_from_clone_of_gc_remote_publishes`.

## Per-Read Control (Task 1)

Run with `--no-fail-fast` over the five replace-ref tests. `sha256sum src/sync/mod.rs` before and after: `5888cd513c2e3dce0ea022ec0e579822f168105a385d6d01c83cec0ea3680007`.

| Option removed from | Failed |
|---|---|
| `cat-file` read | `test_push_refuses_hook_merge_replaced_by_validated_commit`, `test_push_refuses_hook_replaced_message` |
| `ls-tree` read | `test_push_refuses_hook_replaced_tree_hiding_empty_directory` |

## Per-Option Control (Task 2)

Run over the nine forge and bitmap tests. Each row matched the plan's expected set exactly, with the same sha256 before and after the sixth restore.

| Removed | Failed |
|---|---|
| (a) `push_direct` `core.commitGraph=false` | `test_direct_push_sends_no_object_from_forged_commit_graph`, `test_push_sends_no_object_from_forged_split_commit_graph`, `test_push_sends_no_object_from_forged_alternate_commit_graph`, `test_direct_push_ignores_inherited_git_test_commit_graph` |
| (b) `push_direct` `pack.useBitmaps=false` | `test_direct_push_sends_no_object_from_graft_built_bitmap`, `test_push_sends_no_object_from_graft_built_multi_pack_index_bitmap` |
| (c) `push_direct` remove_git_test_env call | `test_direct_push_ignores_inherited_git_test_commit_graph` |
| (d) `push_pr_branch` `core.commitGraph=false` | `test_pr_push_sends_no_object_from_forged_commit_graph`, `test_pr_push_ignores_inherited_git_test_commit_graph` |
| (e) `push_pr_branch` `pack.useBitmaps=false` | `test_pr_push_sends_no_object_from_graft_built_bitmap` |
| (f) `push_pr_branch` remove_git_test_env call | `test_pr_push_ignores_inherited_git_test_commit_graph` |

Each control was also run at its own GREEN state and again before `927e259`, with the same outcomes.

## Mutation Result

Scope: validate_committed_tree, resolve_commit, raw_commit_parents, raw_commit_parts, count_empty_directories, refuse_grafts_file, remove_git_test_env. cargo-mutants 27.1.0.

| Listed | Caught | Missed | Unviable | Timeouts | Exit | Wall time |
|---|---|---|---|---|---|---|
| 49 | 49 | 0 | 0 | 0 | 0 | 5m 29s |

Run on `e0b4f12`. An earlier run on `927e259` gave the same counts in 6m 09s. No test was added in response to a missed mutant; `test_refuse_grafts_file_outcomes` was written beforehand, because the listing showed a match-guard mutant the plan's test list could not kill (see Deviations). After each run `git diff --cached --quiet` exited 0 and `git config --local --list` equalled the saved copy.

## Final Gates

Run on `e0b4f12`, each exit code captured directly. The docs commit after it changes only `.planning/`.

| Gate | Exit | Detail |
|---|---|---|
| Scoped `cargo mutants` | 0 | above |
| `cargo nextest run --test e2e_lifecycle` over the three remote-push tests | 0 | 3 `PASS` lines |
| Hermeticity (decoy `GIT_DIR`, `cargo test --bin ude sync::`) | 0 | 183 passed; decoy has 0 refs and 0 object files |
| `cargo fmt --check` | 0 | |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 | |
| `cargo test` | 0 | 263 unit passed (1 ignored), 88 e2e passed |
| `cargo deny check advisories bans sources` | 0 | advisories ok, bans ok, sources ok |
| `cargo machete` | 0 | output has `Analyzing dependencies` |
| `git diff --exit-code -- Cargo.toml Cargo.lock` | 0 | |
| Task 1 named tests | 0 | 20 `PASS` lines |
| Task 2 named tests | 0 | 20 `PASS` lines |
| Both structure checks | 0 | |

Removed: `target/hermetic-gate`, `$HOME/.cache/ude-mutants-02-12-tmp`, `target/mutants-binding-config.before`. `target/mutants-binding` and its run log stay (gitignored build output).

## What These Checks Do Not Establish

- **The full rule against a pre-push hook.** T-02-52 is an accepted residual and has no test here.
- **That the channels are complete.** These are the channels known and probed. A commit-graph or bitmap forged by a pre-push hook was reasoned about in planning, not run.
- **A variable in UDE's real process environment.** The hook tests set `GIT_TEST_COMMIT_GRAPH` through the engine's extra environment, because tests must not change the test process's environment. `test_remove_git_test_env` injects the inherited keys. The `std::env::vars_os()` argument at each call site is held only by the structure check.
- **Header content.** The header check is a scan, not a bind: hex-only or single-case author text the scanner does not flag still publishes (T-02-45).
- **The message rule under other host settings.** The benign tests pin the fixture config. The commit.cleanup and commit.template results are the planner's probes on git 2.56.0, not re-run here.
- **Other hook mechanisms.** Only `.git/hooks/post-commit` and `prepare-commit-msg` were exercised, not a global `core.hooksPath`.
- **Mutation coverage of string arguments.** cargo mutants does not mutate string arguments or delete statements. The six push additions and two read options are pinned by the controls above; the other `ls-tree` flags (`-r`, `-t`, `-z`, `--full-tree`) were not removed one at a time.
- **PR mode against GitHub.** PR mode ran against a local bare remote, where `gh pr create` fails and the fallback message is returned.
- **A second push after a refusal.** A refused hook's grafts file stays in the cache checkout, so later pushes should keep refusing until it is removed (T-02-47, accepted). This was reasoned and not tested. The refusal names the path.
- **Strength.** The scenarios ran repeatedly on git 2.56.0 on this one host, which is evidence for these scenarios on this version only. Git also prints that grafts support is deprecated and will be removed, so this channel may change in a later version.

## Files Created/Modified

- `src/sync/mod.rs`: `raw_commit_parts`, `count_empty_directories`, `remove_git_test_env`, `refuse_grafts_file`, the extended `validate_committed_tree`, both pushes with the options and the environment removal, doc updates, and the test helpers and 28 tests (the plan's 27 plus `test_refuse_grafts_file_outcomes`).

## Decisions Made

- `count_empty_directories` uses no index arithmetic. A `tab + 1` form would have left an equivalent mutant (a leading tab on every path changes no prefix relation), so the record is split with `splitn` instead.
- The grafts path is read as strict UTF-8 and fails closed. A lossy path could name another file and read as absent. The cost is that a cache checkout under a non-UTF-8 path refuses every push.
- `remove_git_test_env` collects the keys from `inherited` and from `get_envs()` before removing any, as the plan requires.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] A plan 02-11 test's git shim stopped firing**
- **Found during:** Task 2 full test run
- **Issue:** `test_direct_push_publishes_the_validated_commit_after_head_moves` matched `$1 = push`. With `-c` options first, the shim no longer moved HEAD, and the test's own check that HEAD moved failed. The plan did not list this test as affected.
- **Fix:** The shim now scans all arguments for `push`. The test's assertions are unchanged.
- **Files modified:** src/sync/mod.rs
- **Verification:** The test passes, and its assertion that HEAD holds the injected line shows the shim fired.
- **Committed in:** 927e259

**2. [Rule 2 - Missing test] The grafts check's fail-closed branches had no test**
- **Found during:** Task 2, reading the mutant listing
- **Issue:** The match-guard mutant `NotFound -> true` turns "cannot inspect" into a pass, and no planned test reached that branch. A real git never reaches it: with `.git/info` as a file, `rev-parse` itself fails.
- **Fix:** Added `test_refuse_grafts_file_outcomes` with `grafts_verdict_with_git_printing`, a shim that prints fixed output. Rows: no file, an empty file, a symlink to a file, a dangling symlink, a file at `.git/info`, not a repository, git printing nothing, git printing invalid UTF-8, an uninspectable path. Added it to the mutation command's test list, which is a change to the plan's verify command.
- **Found while writing it:** `--path-format=absolute` resolves symlinks, so a dangling symlink at `info/grafts` passes the check. Probed: through a dangling link `rev-list --parents` shows 1 parent, so git reads no graft; once the target exists it shows 2 parents, and the symlink row shows that case is refused.
- **Files modified:** src/sync/mod.rs
- **Committed in:** 927e259

**3. [Rule 2 - Missing assertion] Two refusals did not assert the error hides the hook's text**
- **Found during:** Task 3, checking Task 1's acceptance criteria
- **Issue:** The replaced-message and replaced-tree tests did not assert the error omits `INJECTED_BY_HOOK` and `INJECTED_DIR_NAME`, which the acceptance criteria require.
- **Fix:** Added both assertions, then re-ran the mutation run, the controls and the final gates on that commit.
- **Files modified:** src/sync/mod.rs
- **Committed in:** e0b4f12

**4. [Process] The mutation command's temp-directory removal runs separately**
- **Found during:** Task 3
- **Issue:** Claude Code's safety check denied the plan's `bash -c '... rm -rf "$T" ...'` form because it could not check the removal target inside the script. The command did not run.
- **Fix:** Ran the same mutation command directly with a literal `TMPDIR`, then removed the directory with a separate `rm -rf` naming the literal path, which the check accepted. The directory was empty before removal.

### Minor differences (no behaviour change)

- Test helpers the plan does not name: `message_of_raw_commit`, `assert_refused_as_foreign_message`, `forge_script`, `dir_has_file`, `assert_published_exact`, `assert_graph_forge_in_effect`, `assert_leak_blocked`, `assert_refused_for_grafts`, `assert_pack_bitmap_without_grafts`, `engine_with_test_commit_graph`, `PUSH_HISTORY_OPTIONS`.
- Extra rows beyond the plan's tables in `test_raw_commit_parts` and `test_count_empty_directories`.
- Shell lines shared by hook tails are `macro_rules!` literals, as in plan 02-11, because `concat!` cannot take a `const`.

---

**Total deviations:** 3 auto-fixed (1 blocking, 2 missing test or assertion), 1 process.
**Impact on plan:** The plan adds 28 tests instead of 27. No other production file changed, and Cargo files are unchanged.

## Issues Encountered

- The first GREEN commit's subject exceeded the 72-character limit, so I amended it before any further work. The hash above is the amended one.

## TDD Gate Compliance

RED `1f9ce45` precedes GREEN `f552f78`, and RED `998b8f5` precedes GREEN `927e259` (`git merge-base --is-ancestor` exit 0). RED commits were gated on fmt and clippy, because their tests fail by design. No REFACTOR commit.

## User Setup Required

None.

## Next Phase Readiness

- This is the phase's last plan. Phase 02 is ready for re-verification (`/gsd-verify-work`).
- 02-REVIEW-DISPOSITION.md was not edited; the next code review records WR-102's closure.

## Self-Check: PASSED

- src/sync/mod.rs defines `raw_commit_parts`, `count_empty_directories`, `remove_git_test_env` and `refuse_grafts_file`, each exactly once.
- Commits 1f9ce45, f552f78, 998b8f5, 927e259, e0b4f12 are ancestors of HEAD; `git rev-list --count 83006ba..e0b4f12` = 5.
- No stub markers in the added lines (`rg` found none).

---
*Phase: 02-sanitizer-boundaries-security-hardening*
*Completed: 2026-11-01*
