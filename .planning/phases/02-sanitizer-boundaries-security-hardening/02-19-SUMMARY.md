---
phase: 02-sanitizer-boundaries-security-hardening
plan: 19
subsystem: sync
tags: [git, push, status, assume-unchanged, replace-refs, hooks, publication, security]

requires:
  - phase: 02-18
    provides: PublishTarget (Copy), push to the caller's URL with confirm_published, push_with_hook_at and push_after_hooked_ensure helpers, CliPushWorld.install_cache_hook
provides:
  - stage_publication takes &PublishTarget; both "nothing to push" early returns go through SyncEngine::require_fetched_head_holds
  - SyncEngine::require_fetched_head_holds - HEAD is target.fetched_sha, its tree read with its own `--no-replace-objects ls-tree -r -t -z --full-tree` holds every recorded path, and every staged path with no record (a deletion) is gone from it
  - SyncEngine::first_path_not_held, shared by require_validated_content (signature and message unchanged) and the empty-status check
  - the continuity refusal names only the fetched head, never the raw parent text
  - engine tests in both modes for index flags during and after ensure, a commit on the fetched head, a hidden deletion, a replace ref on the fetched head's tree, the legitimate no-change case, and the parent-line text; an edge table for require_fetched_head_holds
  - CLI rows for the index flag (direct, PR, no-op control) and for the parent-line bytes (direct, PR)
affects: [02-20, 02-21]

actuals:
  tokens: 11191   # chars/4 of `git diff 3cab244 84c2c86 -- src tests` (diff text, context lines included)
  tasks: 3
  commits: 5      # git rev-list --count plan_head_before..plan_head_after
plan_head_before: 3cab24409e597d138394bc8eb7e3db2f87fe9e00
plan_head_after: 84c2c866d22836021a476feaf3a0d522d7ebc3ac

tech-stack:
  added: []
  patterns:
    - "An empty `git status` is not evidence of nothing to push: the fetched head's tree, read past replace refs, must hold the record, whatever emptied status"
    - "A refusal in the committed gate prints no text a hook wrote into a commit header; it names ids UDE read and confirmed"

key-files:
  created: []
  modified:
    - src/sync/mod.rs
    - tests/e2e_lifecycle.rs

key-decisions:
  - "02-19: on empty git status (or nothing staged) with a non-empty record or staged list, the push returns Ok(No changes) only when HEAD is the fetched head, the fetched head's tree (its own --no-replace-objects ls-tree) holds every recorded path with its recorded content, and no staged path without a record is still listed; otherwise it fails hard (exit 2) and records no ancestor"
  - "02-19: the continuity refusal reads `the cache commit's parent is not the fetched head <id>` and prints no parent text; raw_commit_parents is unchanged"
  - "02-19: the staged-deletion check counts a path as present for any entry type in the listing (symlink, gitlink, tree), not only the regular files tree_listing_blobs keeps"

patterns-established:
  - "push_with_empty_status(git, engine, open_pr, step) asserts git status is empty after the step, so a test cannot pass off the route it claims to cover"
  - "push_with_index_hook_during_ensure installs a post-index-change hook after a first ensure; the ensure under test runs it once"

requirements-completed: []   # SEC-04, SEC-02 declared; requirements.ready-ids reports both blocked by later sibling plans

coverage:
  - id: D1
    description: "Through the CLI, a post-index-change hook that sets assume-unchanged on the template during ensure gives exit 2 with `does not hold the validated content`, no remote ref moved and the ancestor unchanged, direct and --pr; the same hook with no flag publishes with exit 0 and advances the ancestor"
    requirement: SEC-04
    verification:
      - kind: e2e
        ref: "tests/e2e_lifecycle.rs#test_cli_direct_push_refuses_index_flag_and_keeps_state, test_cli_pr_push_refuses_index_flag_and_keeps_state, test_cli_direct_push_with_noop_index_hook_publishes"
        status: pass
    human_judgment: false
  - id: D2
    description: "Engine, both modes: assume-unchanged set during ensure (with a no-flag control that publishes) and after ensure, a commit holding the content on the fetched head (refused as not the fetched head, neither id printed), a hidden deletion (refused naming the path), and a replace ref on the fetched head's tree written by post-checkout (refused, with the control that an ordinary ls-tree lists the validated blob and --no-replace-objects the seed's) are each refused with no remote head moved and no sync branch left"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_{direct,pr}_push_refuses_assume_unchanged_set_during_ensure, test_{direct,pr}_push_refuses_assume_unchanged_set_after_ensure, test_{direct,pr}_push_no_changes_refuses_head_off_the_fetched_head, test_{direct,pr}_push_no_changes_refuses_deleted_path_still_held, test_{direct,pr}_push_no_changes_refuses_replace_ref_on_the_fetched_tree"
        status: pass
    human_judgment: false
  - id: D3
    description: "The legitimate no-change case still returns Ok(`No changes to push to remote repository.`) in both modes with the remote unchanged; a real deletion still publishes; one of two recorded paths not held refuses naming it; a deleted path absent from HEAD passes; nothing staged and nothing recorded returns Ok without reading HEAD"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_{direct,pr}_push_no_changes_publishes_nothing_when_content_is_held, test_require_fetched_head_holds_edges, test_push_to_remote_deletion, test_pr_push_publishes_deletion, test_stage_publication_fails_when_git_status_fails"
        status: pass
    human_judgment: false
  - id: D4
    description: "A hook-built commit whose only parent line follows author and holds an escape sequence or a token shape is refused in both modes with `is not the fetched head <fetched id>` and neither the text nor an ESC byte in the message; through the CLI, direct and --pr: exit 2, the hook's text and byte 0x1b absent from stdout and stderr"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_{direct,pr}_push_refusal_omits_hook_text_in_parent_line, test_{direct,pr}_push_refusal_omits_token_in_parent_line, test_push_continuity_violation_is_hard_failure"
        status: pass
      - kind: e2e
        ref: "tests/e2e_lifecycle.rs#test_cli_direct_push_refusal_prints_no_hook_bytes, test_cli_pr_push_refusal_prints_no_hook_bytes"
        status: pass
    human_judgment: false

duration: 20min
completed: 2026-11-04
status: complete
---

# Phase 02 Plan 19: An empty git status no longer reports a hidden change as "nothing to push", and refusals print no hook text Summary

**When `git status` in the cache checkout prints nothing, `ude sync push` now returns "No changes" only if HEAD is the fetched head and that head's tree, read past replace refs, holds every recorded path with its recorded content and no longer lists any staged deletion. Every way of emptying status that the plan names (assume-unchanged set during or after ensure, a commit on the fetched head, a hidden deletion, a replace ref on the fetched head's tree) is now refused with exit 2 and no ancestor recorded. The branch-continuity refusal no longer prints the raw `parent` text a hook can write, so neither escape sequences nor token shapes reach the terminal.**

## Performance

- **Duration:** 20 min
- **Started:** 2026-11-04T03:05:35Z
- **Completed:** 2026-11-04T03:26:02Z
- **Tasks:** 3 of 3
- **Files modified:** 2

## Accomplishments

- `stage_publication(&self, target: &PublishTarget<'_>, staged_files, validated)`, with `let remote_checkout = target.checkout;`. Both early returns (`status.stdout.is_empty()` and `staged_files.is_empty()`) call `self.require_fetched_head_holds(target, staged_files, validated)?` first. `rg -c 'self\.require_fetched_head_holds\('` prints 2.
- `require_fetched_head_holds` (src/sync/mod.rs:2790):
  - returns Ok with no read when `staged_files` and the record are both empty;
  - resolves HEAD with `resolve_commit` and refuses when it is not `target.fetched_sha`. The text names no id: `Publication gate violation: git status reports nothing to push, but HEAD in the cache checkout is not the fetched head; ...`;
  - lists that commit with its own `--no-replace-objects ls-tree -r -t -z --full-tree` command (a separate invocation from validate_committed_tree's), and refuses a failed listing or any TreeDefect;
  - refuses the first recorded path the tree does not hold: `... but the fetched head does not hold the validated content of {path}; a hook in the cache checkout most likely hid the change`;
  - refuses each staged path with no record that the listing still holds: `... but {path} is staged for deletion and still present in the fetched head; ...`.
  - Paths printed come from the record and the staged list, never from the tree.
- `first_path_not_held` (src/sync/mod.rs:2768) returns `Option<&Path>` borrowed from the record. `require_validated_content` keeps its signature and message and now calls it.
- The continuity arm matches `[_]` and prints `Branch continuity check failed: the cache commit's parent is not the fetched head {base}`. `raw_commit_parents` and `test_raw_commit_parents` are unchanged.
- The 11 test call sites of `stage_publication` take `checkout_target(checkout, fetched_sha)`, a new helper next to `NO_REMOTE_URL`. Only the clean-tree control in `test_stage_publication_fails_when_git_status_fails` reaches the empty-status route. It now passes `ValidatedBlobs::from_files` and the HEAD it was read at.

## Task Commits

1. **Task 1 (tracer): CLI row and require_fetched_head_holds:** RED `cb1e0fa` (test), GREEN `c9acbbf` (feat)
2. **Task 2: engine rows in both modes:** `735426a` (test; written after the fix, no production change; RED evidence from mutation runs)
3. **Task 3: the parent line and the remaining CLI rows:** RED `18146a0` (test), GREEN `84c2c86` (feat)

## Files Created/Modified

- `src/sync/mod.rs`:
  - Production: stage_publication's signature and early returns, require_fetched_head_holds, first_path_not_held, require_validated_content's body, and the continuity arm.
  - Test helpers: checkout_target, EMPTY_STATUS_REFUSAL, NOT_HELD, remote_heads_holding, assert_no_local_sync_branch, push_with_index_hook_during_ensure, EmptyStatusPush, StepResult, push_with_empty_status, assert_empty_status_refused, write_hooked_template, HOOK_REPLACE_FETCHED_TREE, hook_parent_line, assert_parent_line_refused, HOOK_PARENT_ESCAPE, hook_parent_token.
  - Tests: the 12 Task 2 rows, test_require_fetched_head_holds_edges and the 4 parent-line rows.
- `tests/e2e_lifecycle.rs`: index_flag_hook_body, assert_cli_index_flag_refused, CLI_HOOK_PARENT_TEXT, assert_cli_refusal_prints_no_hook_bytes, and the 5 CLI rows.

## RED Evidence

The records are in `target/gsd-red/02-19-*.json`, which is gitignored. Each was built from the nextest JUnit report of its run and classified by `gsd-tools check tdd-red-evidence`; every one returned `RED_EVIDENCE_OK / target_test_failed`.

- **Task 1:** `test_cli_direct_push_refuses_index_flag_and_keeps_state` at `cb1e0fa`, before any fix.
  - The failure line was `assertion left == right failed: exit Some(0)` (`left: Some(0)`, `right: Some(2)`).
  - The output was `✓ No changes to push to remote repository.`, after the hook-ran assertion had passed.
  - Not observed in this run: the ancestor change. The exit-code assertion fails first, so this run does not show that the ancestor moved.
- **Task 2:** the rows were written after Task 1's fix and passed when written (18 of 18). Their evidence comes from five mutations of the production code, each run with the 18-name filter. Before the runs the file was copied to the scratchpad; after them it was copied back and its sha256 matched the copy.
  - `nocheck`: both `require_fetched_head_holds` calls removed, which is the pre-fix behaviour. 10 of 18 failed: every refusal row in both modes. 6 failed with `expected a hard push failure, got Ok("No changes to push to remote repository.")` and 4 with `expected the empty-status refusal, got Ok("No changes...")`. The no-change rows, the deletion controls and the edge table (which calls the function directly) passed. Recorded target: `sync::tests::test_direct_push_refuses_assume_unchanged_set_during_ensure`.
  - `noreplace`: `--no-replace-objects` dropped from the new ls-tree. Only the two replace-ref rows failed, with `got Ok("No changes...")`.
  - `nohead`: the HEAD check disabled. The two head-off rows and the edge table failed (`HEAD is not the fetched head: ()`).
  - `nodelete`: the deletion check disabled. The two hidden-deletion rows and the edge table failed (`a deleted path still in HEAD: ()`).
  - `noshort`: the no-read shortcut removed. Only the edge table failed: with an empty fetched head the read refuses.
  - The JUnit files are copies made right after each run, so `reportModifiedAt` is the copy's time. That time is still after `runStartedAt`.
- **Task 3, engine:** at `18146a0`, before the fix. 4 of 7 failed and 3 passed: test_push_continuity_violation_is_hard_failure, test_raw_commit_parents and test_committed_gate_compares_full_object_ids. Each failure was `the refusal printed the hook's text: "Branch continuity check failed: cache commit parent HOOK_CHOSEN_TEXT_\u{1b}[31m is not the fetched head <id>"` (escape rows) or `... parent ghp_<body> ...` (token rows). Recorded: `test_direct_push_refusal_omits_hook_text_in_parent_line` and `test_direct_push_refusal_omits_token_in_parent_line`.
- **Task 3, CLI:** at `18146a0`, 2 of 5 failed: `stderr holds the hook's text: "  ✗ Failed to push to remote: Branch continuity check failed: cache commit parent HOOK_CHOSEN_TEXT_\u{1b}[31m ..."`. The index-flag PR row and the no-op control passed, because Task 1 had already fixed them. Recorded: `test_cli_direct_push_refusal_prints_no_hook_bytes`.
- **Task 3, base-code run for the index-flag CLI rows:** src/sync/mod.rs as at the plan's base `3cab244` (src/main.rs is unchanged since then). It was restored afterwards and `git diff --quiet HEAD` confirmed the restore. Both index-flag rows failed with `exit Some(0)` and the no-op control passed. Recorded: `test_cli_pr_push_refuses_index_flag_and_keeps_state`.

## Verification

- Task 1: both automated verifies exited 0: 2 e2e tests, and the `sync::tests::` plus `process::tests::` unit filter with 302 tests. The GREEN commit `c9acbbf` was made after a full `cargo nextest run` (508 passed, 1 skipped). Tracer feedback gate (interactive, end-of-phase, automated-only verify): the verify was re-run on the committed code and passed, so the plan expanded.
- Task 2: verify exited 0, covering the name-uniqueness loop and the 17 planned names plus test_require_fetched_head_holds_edges. The full unit suite before `735426a`: 411 passed, 1 skipped.
- Task 3:
  - verify 1 (name loop and 7 unit tests) exited 0;
  - verify 2 (name loop and 5 CLI tests) exited 0;
  - verify 3 (`cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test`) exited 0: 415 unit passed (1 ignored) and 114 e2e passed;
  - `84c2c86` was committed only after all three exits were 0.
  - Deviation from the plan's command: verify 3 ran with `PROPTEST_DISABLE_FAILURE_PERSISTENCE=1`, so proptest writes no regression files into the checkout.
- Negative control on the name loop: the same loop with an absent name exits 1 and prints `not exactly one test named test_no_such_test_named_here`.
- `rg -c 'ghp_[0-9A-Za-z]{30,}'` on both test files: no match. The gitleaks pre-commit hook passed on all five commits.
- Hermeticity:
  - The sha256 of the repository's `git config --local -l` was `ccedb838…` before the first test run and still `ccedb838…` after the last.
  - `git status --short` showed only the files each task changed before each commit.
  - TMPDIR was never set and no environment variable was printed.

**What these checks do not establish:**
- The CLI rows ran only the assume-unchanged channel. skip-worktree, `.git/info/exclude`, `core.fsmonitor`, `status.showUntrackedFiles` and similar ways of emptying status were not run. The check reads the fetched head's tree, not status, so the reasoning says they are refused too, but that is reasoning, not a test.
- "HEAD is the fetched head" means the head ensure read and confirmed. A remote that moves after that read is the read-back's case (02-18) and is not exercised on this route.
- The edge table and the hidden-deletion rows cover deletion of a regular file only. A staged path that the tree lists as a symlink or gitlink is reasoned to refuse (the presence check reads every listed path), not tested.
- When several recorded paths are not held, which one the refusal names depends on HashMap order. Only a single-not-held case was asserted.
- Each test ran once per run. That is no reliability bound.
- Windows was not run. https, ssh and hosted remotes were not run.
- The five mutations pin the HEAD check, the replace-ref option, the deletion check, the shortcut and the two calls. They are not a full mutation run of the new functions.

## Decisions Made

- The HEAD-check refusal names no id. The plan said "ids absent from the message", and the fetched head is not needed to act on it.
- The staged-deletion check scans every path in the `ls-tree` listing rather than `tree_listing_blobs`' regular-file map. A deleted template that the fetched head lists as a symlink or gitlink would otherwise read as gone.
- I left the comment above `raw_diff_entries`' use ("a refusal below never prints a listed name that has not passed the allowlist") unchanged. It covers only the refusals below it, it is still true, and the parent arm above it now has its own comment saying why it prints no parent text.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing verification] Tests and assertions the plan did not list**
- **Found during:** Task 2
- **What was added:**
  - `test_require_fetched_head_holds_edges` covers the must-have edge truth ("one path held and one not refuses; a deleted path present refuses and absent passes"), which no listed test pinned directly. It also shows that the shortcut does no read: an empty fetched head passes with nothing to check and refuses with a record.
  - `push_with_empty_status` asserts that `git status --porcelain` is empty before the push, so each row really took the route it names.
  - The head-off rows assert that HEAD holds the validated bytes, so the HEAD check is the only thing that can refuse.
  - The parent-line rows assert that the refusal contains `is not the fetched head <fetched id>`.
- **Committed in:** `735426a`, `18146a0`

**2. [Process] Task 2 has no RED commit of its own**
- **Issue:** the plan expected no production change in Task 2, and Task 1's fix already covered every Task 2 row. So the tests passed when written.
- **Fix:** the RED evidence is the five mutation runs above, the same approach 02-18 used for its direct rows.

**3. [Process] Full `cargo test` ran with `PROPTEST_DISABLE_FAILURE_PERSISTENCE=1`**
- This stops proptest writing regression files into the checkout. It does not change which tests run or what passes.

---

**Total deviations:** 3 (1 added verification, 2 process). **Impact:** the production code is as planned. The tests are stricter than planned.

## TDD Gate Compliance

- Task 1: RED `cb1e0fa`, then GREEN `c9acbbf`, in order.
- Task 2: one `test(02-19)` commit (`735426a`) after Task 1's GREEN, and no production change. Its RED evidence is the mutation runs.
- Task 3: RED `18146a0`, then GREEN `84c2c86`, in order.
- Every commit from `c9acbbf` on passes the full suite. `cb1e0fa` and `18146a0` are red only on their new rows, as intended.

## Findings Against the Plan's Probes

- The two hook mechanisms the plan relies on worked as its probes describe. A post-index-change hook installed after the first ensure and run by the second sets the flag. A post-checkout hook writes the replace ref before ensure's `reset --hard`, which then fills the index and working tree from the replaced tree, so `git status` was empty: `push_with_empty_status`' assertion and the refusal's `git status reports nothing to push` both show it.
- assume-unchanged on a file removed from the working tree leaves `git status --porcelain` empty, as the plan measured.

## Issues Encountered

None beyond the deviations.

## User Setup Required

None.

## Next Phase Readiness

- Ready for 02-20.
- `require_fetched_head_holds` has its own `--no-replace-objects ls-tree` invocation (the `ls_tree.args([` block inside it), so 02-21's row 42 can remove the option at this read alone. Mutation `noreplace` shows that the two replace-ref rows fail when it is removed.
- New sync::tests helpers:
  - `checkout_target(checkout, fetched_sha)`;
  - `push_with_empty_status(git, engine, open_pr, step)`, whose step is `Fn(checkout, managed) -> (staged, record)`;
  - `push_with_index_hook_during_ensure`;
  - `assert_empty_status_refused`;
  - `remote_heads_holding`;
  - `assert_no_local_sync_branch`;
  - `hook_parent_line(text)` (a printf format).
- New e2e helpers: `index_flag_hook_body` and `assert_cli_index_flag_refused`.
- Still open, as the plan states: a failed git command's stderr excerpt (NOT-09). The empty-status route's listing and `resolve_commit` failures still go through `with_excerpt`.
- SEC-04 and SEC-02 stay unchecked. `requirements.ready-ids` reports both blocked by sibling plans.

## Self-Check: PASSED

- FOUND: cb1e0fa, c9acbbf, 735426a, 18146a0, 84c2c86 (all ancestors of HEAD)
- FOUND in src/sync/mod.rs: `fn require_fetched_head_holds` (line 2790), `fn first_path_not_held` (line 2768), two `self.require_fetched_head_holds(` calls. FOUND in tests/e2e_lifecycle.rs: `fn test_cli_direct_push_refuses_index_flag_and_keeps_state` (line 2665).

---
*Phase: 02-sanitizer-boundaries-security-hardening*
*Completed: 2026-11-04*
