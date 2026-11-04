---
phase: 02-sanitizer-boundaries-security-hardening
plan: 18
subsystem: sync
tags: [git, push, ls-remote, redirect, hooks, publication, security]

requires:
  - phase: 02-17
    provides: RemoteCache.remote_url (absolute caller's URL), remote_head (ls-remote from /), ensure_remote_repo_for_push, push_after_hooked_ensure and the CLI shim
provides:
  - pub struct PublishTarget and RemoteCache::publish_target; push_to_remote(&target, open_pr, staged, validated)
  - both pushes name the caller's URL (`-- <remote_url>`), never `origin`
  - SyncEngine::confirm_published, which reads the pushed ref back with git ls-remote from / before any Ok (direct rolls back; PR removes the local sync branch before gh pr create)
  - remote_branch_is_at removed; the non-fast-forward arm reads remote_head of the URL
  - engine tests for every redirect in both modes, the remote moving after the push, and the grafts-during-push channel moved to remote.$U.receivepack over file://
  - CLI rows for origin pushurl and pushInsteadOf, and the two-read count with a working-directory-logging shim
affects: [02-19, 02-20, 02-21]

actuals:
  tokens: 17296   # chars/4 of `git diff da2ac99 5c98fa9 -- src tests` (diff text, context lines included)
  tasks: 3
  commits: 5      # git rev-list --count plan_head_before..plan_head_after
plan_head_before: da2ac99f94ab34af5f1536c1ee3502b1bb5ae261
plan_head_after: 5c98fa961c6dbed681b08a39a47c3dae4b93b226

tech-stack:
  added: []
  patterns:
    - "A push names the caller's URL after `--` and returns Ok only after `git ls-remote --refs -- <url> <ref>` run from the filesystem root reports the ref at the validated commit (whole-string compare)"
    - "Redirect tests assert that the other repository holds the commit, so a redirect that did not take effect cannot pass"

key-files:
  created: []
  modified:
    - src/sync/mod.rs
    - src/main.rs
    - tests/e2e_lifecycle.rs

key-decisions:
  - "02-18: both pushes go to `-- <RemoteCache.remote_url>`; Ok needs confirm_published (git ls-remote of the URL from /, whole ids). On a mismatch, direct mode rolls the cache commit back and PR mode removes the local sync branch before gh pr create, so main.rs records no ancestor"
  - "02-18: repository-level redirects that still apply to a URL push (url.*.pushInsteadOf, url.*.insteadOf, remote.<url>.pushurl) and global url.*.pushInsteadOf are refused by the read-back, with no list of config keys; the redirected repository still receives the validated commit (T-02-80, accepted)"
  - "02-18: the 02-17 redirect controls split by where the push lands: an origin-url redirect now publishes on the injected base to the caller's remote (Ok); an insteadOf redirect also rewrites the push URL, so its control lands in the other repository and is refused by the read-back"

patterns-established:
  - "push_with_hook_at(git, engine, open_pr, hook_name, guard, tail, file_url), with push_over_file_url and push_with_guarded_hook as wrappers; hook bodies get U='<push url>'"
  - "assert_redirected_and_refused(git, pushed, other, open_pr) for any redirect: Err with NOT_PUBLISHED, real remote unchanged, other holds one head built on the fetched head with the validated bytes"
  - "CliPushWorld::ude_logging_git logs `<pwd>\\t<args>` per git process"

requirements-completed: []   # SEC-04, SEC-02 declared; requirements.ready-ids reports both blocked by later sibling plans

coverage:
  - id: D1
    description: "A hook-written remote.origin.pushurl no longer sends the commit elsewhere: Ok, the real remote at the commit with the exact bytes, the other repository empty, in both modes; control: a push naming origin from the same checkout lands in the other repository"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_direct_push_ignores_hook_written_origin_pushurl, test_pr_push_ignores_hook_written_origin_pushurl"
        status: pass
    human_judgment: false
  - id: D2
    description: "Redirects that still apply to a URL push give no Ok: hook-written pushInsteadOf, insteadOf after the fetch, remote.<url>.pushurl over file://, pushInsteadOf written by a clean filter, and global pushInsteadOf, each in both modes. Each refusal is `not at the validated commit`, the real remote is unchanged, and the other repository holds the commit. Direct mode rolls HEAD back; PR mode leaves no sync branch"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_{direct,pr}_push_refuses_hook_written_push_instead_of, test_{direct,pr}_push_refuses_hook_written_instead_of_after_the_fetch, test_{direct,pr}_push_refuses_hook_written_url_keyed_pushurl, test_{direct,pr}_push_refuses_redirect_written_by_clean_filter, test_{direct,pr}_push_refuses_global_push_instead_of"
        status: pass
    human_judgment: false
  - id: D3
    description: "A remote branch that moves on right after the push is refused, not Ok, in both modes; confirm_published passes only the commit itself (a prefix, another id, the fetched head, one step on, an absent branch, an unreachable URL are refused, the last naming the read's error)"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_{direct,pr}_push_refuses_when_remote_branch_moved_after_push, test_confirm_published"
        status: pass
    human_judgment: false
  - id: D4
    description: "02-14's grafts-during-push channel stays pinned through remote.$U.receivepack over file://, three runs each mode, with .git/ude-ack asserted; hooks still run on the push (benign pre-push publishes)"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_{direct,pr}_push_sends_no_object_from_grafts_written_during_push, test_{direct,pr}_push_publishes_with_benign_pre_push_hook"
        status: pass
    human_judgment: false
  - id: D5
    description: "Through the CLI: origin pushurl exits 0 with the edit on the real remote and the other repository empty (direct advances the ancestor, PR does not); pushInsteadOf exits 2 with `not at the validated commit`, the real remote unmoved, the other repository holding the commit, and the ancestor unchanged, both modes. A successful push runs exactly two ls-remote from / with the URL, and the push names the URL and never origin"
    requirement: SEC-02
    verification:
      - kind: e2e
        ref: "tests/e2e_lifecycle.rs#test_cli_{direct,pr}_push_ignores_hook_written_origin_pushurl, test_cli_{direct,pr}_push_refuses_hook_written_push_instead_of, test_cli_{direct,pr}_push_reads_the_remote_twice_from_outside_the_checkout"
        status: pass
    human_judgment: false

duration: 20min
completed: 2026-11-04
status: complete
---

# Phase 02 Plan 18: The push goes to the caller's URL, and Ok depends on the remote Summary

**Both `ude sync push` modes now push to the caller's URL instead of `origin`. Each returns Ok only after `git ls-remote` of that URL, run from the filesystem root, reports the pushed ref at the validated commit. A hook-written `remote.origin.pushurl` no longer sends the commit elsewhere. Every hook-, filter- or global-config redirect that still applies to a URL push now ends in a refusal: no Ok, and no ancestor recorded. The redirected repository does still receive the validated commit; that is the accepted residual T-02-80.**

## Performance

- **Duration:** 20 min
- **Started:** 2026-11-04T02:42:51Z
- **Completed:** 2026-11-04T03:02:14Z
- **Tasks:** 3 of 3
- **Files modified:** 3

## Accomplishments

- `PublishTarget<'a> { checkout, remote_url, branch, fetched_sha }` (Debug, Clone, Copy) and `RemoteCache::publish_target(&branch)`. `push_to_remote(&target, open_pr, staged_files, validated)`, `push_direct(target, staged, validated)` and `push_pr_branch(target, staged, validated)`. `stage_publication` is unchanged and is called with `target.checkout`.
- `push_direct` and `push_pr_branch` pass `--` and then `target.remote_url` where they named `"origin"`. `rg -c 'target\.remote_url,$'` prints 2.
- `confirm_published(target, branch, commit)` returns Ok only on a whole-string match. Otherwise it returns `git push reported success, but the remote branch {branch} is not at the validated commit`. A failed read adds `: ` and remote_head's error. The text ends `; nothing was recorded as published; run ude sync pull, then push again`.
  - Direct mode calls it after a successful push and runs `rollback_commit` on Err.
  - PR mode calls it after the push and after `checkout <branch>`, before `gh pr create`, and runs `abandon_sync_branch` on Err.
  - `rg -c 'self\.confirm_published\('` prints 2. From the code: the PR confirm is at line 2215 and `self.tool("gh", ...)` at line 2221.
- `remote_branch_is_at` and `test_remote_branch_is_at` are removed (`rg` exits 1). The non-fast-forward arm compares `remote_head(target.remote_url, branch)` with the fetched head. Its refusal and failure texts say `git push to {branch}`.
- src/main.rs: the Push arm publishes through `cache.publish_target(&branch)`. `persist_immediately` and the `if let` now use `remote_cache`. The tuple's checkout element is bound as `_remote_checkout_root`, as status and pull already do.
- Doc comments on `push_direct`, `push_pr_branch` and `HOOK_GRAFTS_WRITTEN_DURING_PUSH` are updated. The grafts paragraph in both pushes now says "a command it wrote into config (a push to a URL not starting with `/` still runs `remote.<url>.receivepack`)".

## Task Commits

1. **Task 1 (tracer): hook-written origin pushurl, direct, plus confirm_published:** RED `b9dbbd3` (test), GREEN `9a83d3f` (feat)
2. **Task 2: PR mode, every redirect, the remote moving on, the grafts channel:** RED `f67c760` (test), GREEN `4a681a8` (feat)
3. **Task 3: CLI rows and the read count:** `5c98fa9` (test; no production change)

## Files Created/Modified

- `src/sync/mod.rs`:
  - Production: PublishTarget, publish_target, confirm_published, the new push signatures and destinations. remote_branch_is_at is removed.
  - Test helpers: NO_REMOTE_URL, push_with_hook_at, push_over_file_url, assert_grafts_written_during_push_ignored, PushGoesTo, redirect_target, assert_redirected_and_refused, assert_hook_redirect_refused, assert_filter_redirect_refused, assert_global_push_instead_of_refused, REMOTE_MOVES_PUSHED_REF, assert_moved_after_push_refused, assert_origin_pushurl_ignored, NOT_PUBLISHED.
  - Tests: the 18 call sites of push_to_remote, rewritten; the 18 new tests; the rewritten assert_redirect_refused.
  - push_to_fresh_bare sets `receive.shallowUpdate=true`.
- `src/main.rs`: the Push arm.
- `tests/e2e_lifecycle.rs`:
  - The shim logs `<pwd>\t<args>`.
  - New helpers cli_redirect_target and install_redirect_hook.
  - The 6 new CLI rows.

## RED Evidence

The records are in `target/gsd-red/02-18-*.json`, which is gitignored. Each is built from the nextest JUnit report of the run and classified by `gsd-tools check tdd-red-evidence`.

- **Task 1:** `sync::tests::test_direct_push_ignores_hook_written_origin_pushurl` at `b9dbbd3`, before any fix: `RED_EVIDENCE_OK / target_test_failed`.
  - The failure line was `assertion left != right failed: nothing was published` (`left` = `right` = `f5508cb…`, the fetched head).
  - The assertions before it passed: the hook ran and the push returned Ok. So Ok was returned while the real remote's main stayed at the fetched head.
- **Task 2:** the Task 2 test changes, run on Task 1's production code (PR mode still named `origin`) at `f67c760`: `RED_EVIDENCE_OK` for `sync::tests::test_pr_push_refuses_hook_written_push_instead_of` and `sync::tests::test_pr_push_ignores_hook_written_origin_pushurl`. 10 of 77 failed, all PR mode, each on its planned assertion:
  - six PR redirect rows: `expected the read-back's refusal, got Ok("Pushed changes to branch 'ude-sync-…'…")`
  - moved after push: `expected a hard push failure, got Ok(...)`
  - grafts: `the grafts file was never written during the push`
  - PR origin pushurl: no `ude-sync-*` on the real remote (`left: 0`)
  - PR origin-url fetch redirect: `the control did not deliver the injected blob`
  - All direct-mode rows passed, because Task 1 had already fixed direct mode.
- **Task 2, direct rows:** these passed when written, because the direct fix landed in Task 1. Their evidence comes from mutation runs of production code. After each run the file was restored with `git checkout -- src/sync/mod.rs`, and `git diff --quiet HEAD` confirmed the restore. Each run used a 21-test filter.
  - `noconfirm` (confirm_published always Ok): 15 failed. These were every redirect row in both modes, both moved-after-push rows, test_confirm_published, and both insteadOf fetch-redirect controls. `RED_EVIDENCE_OK` on `test_direct_push_refuses_global_push_instead_of`. The 6 that passed do not depend on the read-back: both origin-pushurl rows, both grafts rows, and both origin-url fetch redirects.
  - `prefix` (`head.starts_with(commit)`): only test_confirm_published failed, with `a prefix of the id: got Ok(())` (`RED_EVIDENCE_OK`).
  - `origindirect` (the direct push names `origin` again): 4 failed. These were direct origin pushurl, direct URL-keyed pushurl, the direct origin-url fetch-redirect control, and direct grafts during push (`RED_EVIDENCE_OK` on `test_direct_push_ignores_hook_written_origin_pushurl`).
- **Task 3:** the six new CLI rows, run with src/sync/mod.rs and src/main.rs as they were at the plan's base `da2ac99` and restored afterwards: 6 of 6 failed, `RED_EVIDENCE_OK` for `test_cli_direct_push_refuses_hook_written_push_instead_of` and `test_cli_direct_push_reads_the_remote_twice_from_outside_the_checkout`.
  - pushInsteadOf: `exit Some(0)`. This is the WR-301 bug: exit 0 with ancestors advanced.
  - origin pushurl: `the other repository received a ref`.
  - read count: one `ls-remote` line (`/\tls-remote --refs -- <url> refs/heads/main`) next to `fetch origin main`.

## Verification

- Task 1 verify: exit 0, 6 tests. The tracer feedback gate re-ran it inside the `9a83d3f` commit gate: exit 0.
- Task 2 verify 1 (name-uniqueness loop and the 21 tests): exit 0. Verify 2 (`self.confirm_published(` = 2, `target.remote_url,$` ≥ 2): exit 0. The `4a681a8` commit was gated on fmt, clippy, both checks and `cargo nextest run --bin ude`: 398 passed, 1 skipped.
- Task 3 verify 1 (name loop and 10 tests): exit 0. Verify 2 (`cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test`): exit 0, 398 unit (1 ignored) and 109 e2e passed. `5c98fa9` was committed only after both exits were 0.
- `rg -c 'ghp_[0-9A-Za-z]{30,}'` on both test files: no match. The gitleaks pre-commit hook passed on all five commits.
- Hermeticity: the sha256 of the repository's `git config --local -l` was the same before and after the Task 3 e2e runs (`ccedb838…`). The baseline was taken after the Task 2 suite, not before the first run. `git status --short` was empty after every commit. TMPDIR was never set and no environment variable was printed.
- Cleanup: every scratch file this run made is removed. Only the orchestrator's four files remain in the scratchpad. `pgrep` found no grafts-hook loop still running. The RED records stay in gitignored `target/gsd-red/`, as 02-16 and 02-17 left theirs.

**What these checks do not establish:**
- Only local paths and `file://` URLs were run. https, ssh and hosted remotes were not, and neither were `core.sshCommand` or `http.proxy`. The read-back is reasoned, not shown, to catch those.
- `gh pr create` never ran (no `gh` in the test environment). That the PR is opened only after the confirm is read from the code, and the refused PR rows return Err with no PR message.
- A redirected repository still receives the validated commit before the refusal. The tests assert that it does (T-02-80, accepted).
- A re-run after "moved after push" was reasoned, not run.
- Windows was not run.
- Each test ran once per run, and the grafts rows three times. That is no reliability bound.
- The mutation runs pin only the three production changes named above.

## Decisions Made

- `PublishTarget` is destructured into the old local names at the top of both pushes, so the change to their bodies stays small.
- The 02-17 `assert_redirect_refused` control takes `PushGoesTo`. The handoff from 02-17 expected both of its controls to reach the real remote, but only the origin-url one does. `url.*.insteadOf` also rewrites a push to the URL, so that control's commit lands in the other repository and the read-back refuses it. The control now asserts that, plus "the caller's remote changed: no".

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Bindings left unused by the new call form**
- **Found during:** Task 1
- **Issue:** 5 tests bound `let checkout = cached.checkout_dir.as_path();` (or `remote_checkout`, `remote_checkout_2`) only to pass it to push_to_remote. Clippy `-D warnings` failed on them.
- **Fix:** removed those 5 bindings.
- **Committed in:** `9a83d3f`

**2. [Process] Task 2 split into a RED and a GREEN commit**
- **Found during:** Task 2
- **Issue:** the tests and the PR production change were written together.
- **Fix:** `f67c760` carries the test half on Task 1's production code. That half was run and its failures recorded before `4a681a8` added the PR change. The production text of `f67c760` was checked byte-equal to `9a83d3f`.

**3. [Rule 2 - Missing verification] Assertions the plan did not list**
- test_confirm_published also asserts that a failed read's text contains `not at the validated commit: git ls-remote failed`.
- The moved-after-push tests assert that the remote's ref is a child of the pushed commit, so the move happened after the push landed.
- The clean-filter tests assert that the filter ran (a marker).
- Every engine redirect test checks the bytes the other repository received.
- The CLI origin-pushurl rows check that the real remote's ref is UDE's recorded commit.

---

**Total deviations:** 3 (1 blocking, 1 process, 1 added verification). **Impact:** the production code is as planned. The tests are stricter than planned.

## Needs Awareness

- **Between `9a83d3f` and `4a681a8` the unit suite is not green.** After `9a83d3f` three tests failed: direct grafts during push and the two direct fetch-redirect controls. They depended on the push naming `origin`, and the plan rewrites them in Task 2. After `f67c760` the PR rows fail, which is the intended RED. Every commit from `4a681a8` on passes the full suite. Bisecting through those two commits will see these failures.

## TDD Gate Compliance

- Task 1: RED `b9dbbd3`, then GREEN `9a83d3f`, in order.
- Task 2: RED `f67c760`, then GREEN `4a681a8`, in order.
- Task 3: one `test(02-18)` commit (`5c98fa9`) and no `feat` after it. The plan allows no production change in this task unless a test finds a defect, and none did. Its RED evidence is the base-code run above.

## Findings Against the Plan's Probes

- Every redirect row behaved as the interfaces table says. `remote.$U.receivepack` over `file://` ran (the `.git/ude-ack` marker exists in every run). A push from a `file://` clone into an empty repository needed `receive.shallowUpdate=true`.
- The only unit fallout was the three tests the plan names. All 103 e2e tests that existed before passed at `9a83d3f`.

## Threat Flags

| Flag | File | Description |
|------|------|-------------|
| threat_flag: T-02-80 | src/sync/mod.rs | accepted by the operator 2026-11-03. Question: "New risk from pushing to the caller's URL and reading the ref back: a hook-written `url.*.pushInsteadOf` in the cache checkout still sends the validated commit to another repository before the read-back refuses. `ude sync push` returns no Ok, and only validated bytes travel. How should this be handled? (Planner's finding, severity low, not a defect.)" Selected "Accept (Recommended)", whose option text read "Record it as an accepted residual. A refusal before the push would need a list of config keys and would still miss a process that writes the key later." Scope, the same day, a question ending "Does your acceptance cover all of them?" over every repository-level redirect key in NOT-04 (`url.*.pushInsteadOf`, `url.*.insteadOf`, `remote.<url>.pushurl`, and the reasoned `core.sshCommand` and `http.proxy`): selected "All redirect keys (Recommended)", whose option text read "Same effect and the same protection (no Ok, only validated bytes travel). A refusal for each key would need a key list and would still miss a process writing later." |
| threat_flag: T-02-81 | src/sync/mod.rs | accepted by the operator 2026-11-03. Question: "Second new risk: pushing to the caller's URL puts that URL on the git command line for every push and read, where before it appeared only at first clone. If the URL embeds a token, other local users can see it in the process list. How should this be handled? (Planner's finding, severity low, not a defect.)" Selected "Accept (Recommended)", whose option text read "Record it as an accepted risk. Safe if you don't use token-bearing URLs on a shared host; please say so under notes if you do." No notes were given. |

Status when this SUMMARY was written; the `## Operator answers` table of 02-21-PLAN.md holds the current one.

## Issues Encountered

None beyond the deviations.

## User Setup Required

None.

## Next Phase Readiness

- Ready for 02-19. `stage_publication` is unchanged and is called with `target.checkout`. `PublishTarget` is `Copy`. In sync::tests:
  - `push_with_hook_at`, with `push_over_file_url` and `push_with_guarded_hook` as wrappers. Hook bodies get `F` and `U`.
  - `redirect_target(git, shallow)` and `assert_redirected_and_refused`.
  - `NOT_PUBLISHED` and `NO_REMOTE_URL`.
  - `PushGoesTo` for the 02-17 redirect controls.
- tests/e2e_lifecycle.rs: `cli_redirect_target` and `install_redirect_hook`. `ude_logging_git` lines are now `<dir>\t<args>`.
- SEC-04 and SEC-02 stay unchecked. `requirements.ready-ids` reports both blocked by sibling plans.

## Self-Check: PASSED

- FOUND: b9dbbd3, 9a83d3f, f67c760, 4a681a8, 5c98fa9 (all ancestors of HEAD)
- FOUND in src/sync/mod.rs: `pub struct PublishTarget` (line 393), `fn confirm_published` (line 1191), two `self.confirm_published(` calls. No `fn remote_branch_is_at` and no `fn test_remote_branch_is_at`. No `git push origin {` text.
- FOUND in src/main.rs: `publish_target(` (line 1256). FOUND in tests/e2e_lifecycle.rs: `test_cli_direct_push_reads_the_remote_twice_from_outside_the_checkout`.

---
*Phase: 02-sanitizer-boundaries-security-hardening*
*Completed: 2026-11-04*
