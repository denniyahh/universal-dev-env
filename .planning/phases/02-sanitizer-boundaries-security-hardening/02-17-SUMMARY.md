---
phase: 02-sanitizer-boundaries-security-hardening
plan: 17
subsystem: sync
tags: [git, push, ls-remote, tracking-ref, hooks, publication-gate, security]

requires:
  - phase: 02-16
    provides: push source `<id>^{commit}` under --no-replace-objects; CliPushWorld, cli_push_world, install_cache_hook, holds_object, with_injected_token
provides:
  - SyncEngine::ensure_remote_repo_for_push: the fetched head must equal what `git ls-remote` reports for the caller's URL from the filesystem root before the fetch; a second read separates a real move (UpstreamMoved, exit 1) from a moved or redirected tracking ref (Failed, exit 2)
  - SyncEngine::remote_head (exact refname match), fn absolute_remote_url, RemoteCache.remote_url (absolute form of the caller's URL, for plan 02-18's push)
  - src/main.rs open_push_templates; `ude sync push` uses it, status and pull keep open_sync_templates
  - engine tests for every route on the operator's list in both modes, each with a delivering control where one exists; CLI rows on the cached and clone paths; CliPushWorld::ude_logging_git (git shim)
affects: [02-18, 02-19, 02-20, 02-21]

actuals:
  tokens: 14829   # chars/4 of `git diff 389627d HEAD -- src tests` (diff text, context lines included)
  tasks: 3
  commits: 4      # git rev-list --count plan_head_before..plan_head_after
plan_head_before: 389627d1fefd89b6202b7ef36d442a3ef18fd365
plan_head_after: 50ed8eb663a71e3720ee2c3c7465e720f2325d80

tech-stack:
  added: []
  patterns:
    - "A security read of remote state runs `git ls-remote --refs -- <url> <ref>` with the filesystem root as working directory and takes only the line whose refname is exact"
    - "Channel tests run the same state twice: the checked entry point must refuse, plain ensure_remote_repo must deliver the injected blob"

key-files:
  created: []
  modified:
    - src/sync/mod.rs
    - src/main.rs
    - tests/e2e_lifecycle.rs

key-decisions:
  - "02-17: ude sync push opens its remote through ensure_remote_repo_for_push: git ls-remote from the filesystem root before the fetch, whole-string compare with the fetched head; on a difference a second read equal to the fetched head is UpstreamMoved (exit 1), anything else is a fixed-text Failed (exit 2); status and pull keep ensure_remote_repo and run no ls-remote"
  - "02-17: RemoteCache.remote_url is the caller's URL with an existing relative path made absolute; the cache key keeps the caller's spelling; the second read already uses remote_url, plan 02-18's push is meant to"
  - "02-17: the origin-url and insteadOf controls deliver to the other repository, because the push still goes to `origin`; plan 02-18 must rewrite assert_redirect_refused's control when the push goes to the caller's URL"

patterns-established:
  - "push_after_hooked_ensure(git, engine, open_pr, setup, checked) with assert_checked_refused / assert_control_delivers in sync::tests"
  - "CliPushWorld::ude_logging_git(args) logs each git process's args through a PATH shim"

requirements-completed: []   # SEC-04, SEC-02 declared; requirements.ready-ids reports both blocked by later sibling plans

coverage:
  - id: D1
    description: "A tracking ref moved by a hook or kept by config in the cache checkout stops the push in both modes, cached and clone paths, with nothing on the remote; each route's control (plain ensure) publishes the injected blob"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_{direct,pr}_push_refuses_tracking_ref_moved_by_{post_checkout_hook,reference_transaction_hook,clean_filter}, test_{direct,pr}_push_refuses_tracking_ref_moved_on_clone_path, test_{direct,pr}_push_refuses_tracking_ref_kept_by_hook_written_fetch_config, test_{direct,pr}_push_refuses_token_on_moved_tracking_ref"
        status: pass
    human_judgment: false
  - id: D2
    description: "A fetch redirected by checkout config (remote.origin.url, url.*.insteadOf) is refused in both modes; the control fetches the injected base and publishes on it"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_{direct,pr}_push_refuses_fetch_redirected_by_hook_written_{origin_url,instead_of}, test_remote_head_ignores_repository_config"
        status: pass
    human_judgment: false
  - id: D3
    description: "Smudge filter refused both ways; a tag named origin/main fails ensure; a real move during the fetch is UpstreamMoved; a benign post-checkout hook still publishes through the checked route"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_{direct,pr}_push_refuses_tracking_ref_moved_by_smudge_filter, test_push_refuses_tag_named_after_the_tracking_ref, test_ensure_for_push_reports_real_upstream_move_as_upstream_moved, test_{direct,pr}_push_with_head_check_publishes_with_benign_post_checkout_hook"
        status: pass
    human_judgment: false
  - id: D4
    description: "remote_head takes the exact refname past a decoy listed first, returns None for a missing branch, Err for an unreachable URL, the whole id; absolute_remote_url changes only an existing relative path"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_remote_head, test_absolute_remote_url"
        status: pass
    human_judgment: false
  - id: D5
    description: "Through the CLI: post-checkout hook (direct, --pr) and clone path (direct, --pr) exit 2 with nothing published and the ancestor unchanged; a real move during the fetch exits 1 with the pull-first line; a relative --remote publishes through one cache checkout; status and pull run no ls-remote"
    requirement: SEC-02
    verification:
      - kind: e2e
        ref: "tests/e2e_lifecycle.rs#test_cli_{direct,pr}_push_refuses_tracking_ref_moved_by_post_checkout_hook, test_cli_{direct,pr}_push_refuses_tracking_ref_moved_on_clone_path, test_cli_direct_push_reports_upstream_moved_during_fetch, test_cli_push_with_relative_remote_path_publishes, test_cli_status_and_pull_run_no_ls_remote, test_force_does_not_bypass_upstream_moved"
        status: pass
    human_judgment: false

duration: 20min
completed: 2026-11-04
status: complete
---

# Phase 02 Plan 17: Push base taken from the remote, not the checkout Summary

**`ude sync push` now reads the branch head with `git ls-remote` from the filesystem root before the fetch and refuses when the cache checkout's fetched head differs. A hook, filter or config in the checkout that moves or redirects the tracking ref can no longer put an injected commit under the validated one. 25 engine tests and 7 new CLI rows prove it, each refusal paired with a control that publishes the injected blob when the check is skipped.**

## Performance

- **Duration:** 20 min
- **Started:** 2026-11-04T02:15:27Z
- **Completed:** 2026-11-04T02:36:00Z
- **Tasks:** 3 of 3
- **Files modified:** 3

## Accomplishments

- `SyncEngine::ensure_remote_repo_for_push(remote_url, branch, cache_dir) -> Result<RemoteCache, PushRemoteError>`: it reads the head first, using the absolute URL. Then it calls `ensure_remote_repo` with the caller's spelling, so the cache key and directory stay the same, and compares the two ids as whole strings. On a difference it reads again. If the second read equals the fetched head, it returns `UpstreamMoved`. Otherwise it returns `Failed("the cache checkout's fetched head is not the commit the remote branch holds; a hook or config in the cache checkout most likely moved or redirected it; nothing was published")`. No ref or id from the checkout appears in that text.
- `remote_head`: runs `git ls-remote --refs -- <url> refs/heads/<branch>` from `MAIN_SEPARATOR_STR` on the network tier, labelled `git ls-remote`. It returns only the line whose refname is exact.
- `absolute_remote_url` and `RemoteCache.remote_url`. The second read already uses `remote_url`.
- `open_push_templates` in src/main.rs. With no URL it falls through to `open_sync_templates`. `UpstreamMoved` prints the two existing lines and exits 1. `Failed` exits 2. `rg -c 'ensure_remote_repo_for_push\(' src/main.rs` prints 1 (0 at the base).
- Updated the docs on `RemoteCache.fetched_sha` and `PushRemoteError::UpstreamMoved` to say what each now means.

## Task Commits

1. **Task 1 (tracer): moved tracking ref refuses `ude sync push`, direct, cached path**: RED `cc4b187` (test), GREEN `7eb7c6b` (feat)
2. **Task 2: every route, both modes, controls; the read's edges**: `457291d` (test; no production change)
3. **Task 3: the rest of the CLI rows; no ls-remote for status and pull**: `50ed8eb` (test; no production change)

## Files Created/Modified

- `src/sync/mod.rs`: RemoteCache.remote_url, ensure_remote_repo_for_push, remote_head, absolute_remote_url, two doc comments. Test helpers: moved_tracking_ref_tail, EnsuredPush, ChannelSetup, only_cache_checkout, push_after_hooked_ensure, assert_checked_refused, assert_control_delivers, install_checkout_hook, sh_in, other_remote_with_injected_base, filter_setup. The 25 Task 2 tests.
- `src/main.rs`: open_push_templates; the Push arm calls it.
- `tests/e2e_lifecycle.rs`: moved_tracking_ref_hook, assert_cli_push_refused_moved_base, CliPushWorld::ude_logging_git, the Task 1 test and the 7 Task 3 tests. `ude_with_env` no longer carries `#[expect(dead_code)]`.

## RED Evidence

The records are in `target/gsd-red/02-17-*.json` (gitignored). Each is built from the nextest JUnit report and classified by `gsd-tools check tdd-red-evidence`.

- **Task 1:** `test_cli_direct_push_refuses_tracking_ref_moved_by_post_checkout_hook` at `cc4b187`, before any fix: `RED_EVIDENCE_OK / target_test_failed`. The failure line was `assertion left == right failed: exit Some(0)` (`left: Some(0)`, `right: Some(2)`). The hook-ran assertion passed first. The push exited 0, building on the hook's injected base. That run stopped at the exit-code assertion, so the delivery of templates/leak.txt to the remote was not observed through the CLI. Task 2's plain-ensure controls show that delivery through the engine.
- **Task 2:** the fix landed in Task 1, so these tests passed when written. Their RED evidence comes from mutation runs against production code. After each run the file was restored and checked with `cmp` and a sha256 check.
  - The head check was disabled (`if true || head_before... == fetched`): 19 of 21 failed (`02-17-t2-nocheck`, `RED_EVIDENCE_OK` on the qualified name `sync::tests::test_direct_push_refuses_tracking_ref_moved_by_post_checkout_hook`).
    - Every checked channel failed with `expected the head check's refusal, got Ok("Directly pushed changes...")` (or the PR equivalent).
    - Both smudge tests failed with `Branch continuity check failed`, as the plan predicted.
    - The upstream-move test failed with `left: Ok(...)`.
    - The two benign-hook tests passed. They must pass with or without the check.
  - `remote_head` was made to match by suffix (`name.ends_with(&refname)`): `test_remote_head` failed on its decoy row. It got `refs/heads/a/refs/heads/main`'s id. `test_remote_head_ignores_repository_config` still passed (`02-17-t2-firstline`, `RED_EVIDENCE_OK`).
  - `absolute_remote_url` was made a no-op: `test_absolute_remote_url` failed with `src: got src, want <crate>/src` (`02-17-t2-noabs`, `RED_EVIDENCE_OK`).
- **Task 3:** these also passed when written. Mutation runs:
  - The Push arm was set back to `open_sync_templates`: 6 of 8 failed (`02-17-t3-nopushroute`, `RED_EVIDENCE_OK`). These were the four tracking-ref rows and the upstream-moved row, each failing on `exit Some(0)`, plus the shim row's control (`push: [...]` with no ls-remote). The relative-path row and test_force_does_not_bypass_upstream_moved passed, because neither depends on the check.
  - `absolute_remote_url` was made a no-op: the relative-path row failed with `exit Some(2)` and `git ls-remote failed with exit status Some(128): fatal: '../remote.git' does not appear to be a git repository` (`02-17-t3-noabs`, `RED_EVIDENCE_OK`). That is the failure the plan measured.

## Delivering controls (plain ensure_remote_repo + push_to_remote, same state)

| Channel | Direct | PR | What the control showed |
|---|---|---|---|
| post-checkout hook, cached path | delivered | delivered | fetched head ≠ remote head; the remote holds the injected blob |
| token-shaped blob (ghp_ shape, built at runtime) | delivered | delivered | same; the gate never lists base files |
| clone path, global core.hooksPath | delivered | delivered | same |
| reference-transaction hook | delivered | delivered | same |
| fetch config `+refs/heads/does-not-exist:refs/remotes/origin/nothing`, ref moved earlier | delivered | delivered | same; the config was still in place after the ensure |
| remote.origin.url → other repository | published on the injected base | same | see the first item under Needs Awareness |
| url.<other>.insteadOf | published on the injected base | same | see the first item under Needs Awareness |
| clean filter on a stat-dirty file | delivered | delivered | same |
| smudge filter | none, by design | none | the unchecked run is refused by the continuity check; the test asserts that |
| tag `refs/tags/origin/main` | none, by design | n/a | ensure fails at `git checkout -B main` |

## Needs Awareness

- **The two redirect controls cannot show delivery to the caller's remote yet.** The push still goes to `origin`, and the checkout's config points `origin` at the other repository. So the control publishes there, and that repository already held the blob. The control asserts two things: the fetched head is the other repository's injected base, and the published commit's parent is that base. The checked route's refusal is unaffected. Plan 02-18 sends the push to the caller's URL, and at that point `assert_redirect_refused`'s control must be rewritten. A push built on the injected base would then reach the real remote.

## Verification

- Task 1 verify: exit 0. The tracer feedback gate re-ran it after `7eb7c6b`: exit 0. The full nextest suite before that commit: 456 passed.
- Task 2 verify (name-uniqueness loop and 25 tests): exit 0. `457291d` was gated on `cargo fmt --check && cargo clippy ... -D warnings && cargo nextest run --bin ude`, which passed 384 tests.
- Task 3 verify 1 (name loop and 8 tests): exit 0. Verify 2 (`cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test`): exit 0, with 384 unit tests passed and 1 ignored, and 103 e2e tests passed. `50ed8eb` was committed only after both exits were 0.
- Name-uniqueness gates in both verify scripts passed: each listed name matched exactly one test.
- `rg -c 'ghp_[0-9A-Za-z]{30,}'` on both test files found no match. The gitleaks pre-commit hook passed on all four commits.
- Hermeticity: the sha256 of the repository's `git config --local -l` was the same after every e2e run (baseline taken after the first RED run). `git ls-files` lists 215 entries and nothing is staged. TMPDIR was never set, no background process was started, and no environment variable was printed.

**What these checks do not establish:**
- Only local paths were run, not hosted remotes over https or ssh.
- Windows was not run (the working directory `MAIN_SEPARATOR_STR`).
- Global and system redirect keys were not tested (T-02-77, accepted).
- The push destination is plan 02-18's work.
- Each test ran once per run, so this is no statistical reliability bound.
- The mutation runs pin only the four production changes listed above, not every line.

## Decisions Made

- The second read uses `cache.remote_url`, not a new `absolute_remote_url` call. That makes the field used in production now, so it needs no `dead_code` attribute, and it reads the same spelling the clone used.
- `absolute_remote_url` also keeps the given URL when the absolute form is not UTF-8. A lossy conversion would name another path.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing verification] The CLI refusal asserts the head check's text, not just exit 2**
- **Found during:** Task 1 GREEN
- **Issue:** Exit 2 is also what a failing `git ls-remote` or any other ensure error gives, so exit 2 alone could pass for the wrong reason.
- **Fix:** `assert_cli_push_refused_moved_base` also requires `fetched head is not the commit the remote branch holds`. `assert_checked_refused` does the same in the engine tests.
- **Committed in:** `7eb7c6b` (the RED test at `cc4b187` asserted exit code only)

**2. [Rule 1 - Test correctness] Redirect control compared against the wrong commit**
- **Found during:** Task 2
- **Issue:** In direct mode the control's push moves the other repository's main, so comparing the fetched head with that main after the push failed even though the control was right.
- **Fix:** The setup records the injected base in `.git/ude-test-injected-base`, and the control compares against that.
- **Committed in:** `457291d`

**3. [Rule 2 - Missing verification] Extra assertions the plan did not list**
- The pull row in `test_cli_status_and_pull_run_no_ls_remote` asserts exit 0 exactly, and each shim run asserts the log is not empty, so a shim that logged nothing cannot pass.
- The clone-path CLI rows assert that no cache directory exists before the push.
- The fetch-config tests assert that the bogus refspec is still in place after the ensure.

---

**Total deviations:** 3 (1 test correctness, 2 added verification). **Impact:** the production code is as planned. The tests are stricter than planned.

## TDD Gate Compliance

- Task 1: RED `cc4b187` (test), then GREEN `7eb7c6b` (feat), in order.
- Tasks 2 and 3: one `test(02-17)` commit each (`457291d`, `50ed8eb`) and no `feat` after them. The plan forbids a production change in these tasks unless a test finds a defect, and none did. Their fix is `7eb7c6b`, which comes before them. Their RED evidence comes from the mutation runs above.

## Findings Against the Plan's Probes

- Every route behaved as the interfaces table says. This includes the smudge filter, where HEAD stays at the remote's head and the continuity check refuses without the head check, and the tag, where `checkout -B` fails.
- `git ls-remote --refs -- <url> refs/heads/main` listed `refs/heads/a/refs/heads/main` first, which the test asserts before the decoy row.
- `remote.origin.uploadpack` is honoured for a local-path fetch, and the head reads from `/` do not go through it.

## Threat Flags

| Flag | File | Description |
|------|------|-------------|
| threat_flag: T-02-77 | src/sync/mod.rs | accepted by the operator on 2026-11-02 for global url.*.insteadOf, widened on 2026-11-03 to every global redirect key |
| threat_flag: T-02-78 | src/sync/mod.rs | accepted by the operator on 2026-11-02: one or two extra `git ls-remote` reads per push; a failed read or a real move inside the fetch window refuses the push; status and pull are unaffected (test_cli_status_and_pull_run_no_ls_remote) |

## Issues Encountered

None beyond the deviations.

## User Setup Required

None.

## Next Phase Readiness

- Ready for 02-18. `RemoteCache.remote_url` is the absolute form of the caller's URL, made for the push destination. When the push goes there, rewrite the control in `assert_redirect_refused` (origin-url and insteadOf). `remote_branch_is_at` (02-14) still reads through the checkout's `origin` and affects only a refusal's wording.
- SEC-04 and SEC-02 stay unchecked: `requirements.ready-ids` reports both blocked by sibling plans.

## Self-Check: PASSED

- FOUND: cc4b187, 7eb7c6b, 457291d, 50ed8eb (all ancestors of HEAD)
- FOUND in src/sync/mod.rs: `fn ensure_remote_repo_for_push`, `pub remote_url: String`, `fn absolute_remote_url`, `fn push_after_hooked_ensure`, `fn remote_head`. FOUND in src/main.rs: `fn open_push_templates`, with one `ensure_remote_repo_for_push(` call. FOUND in tests/e2e_lifecycle.rs: `fn ude_logging_git`, and no `dead_code` attribute.
- `check evaluation-scope --plan 02-17 --commits-only`: resolved, 4 commits on this branch

---
*Phase: 02-sanitizer-boundaries-security-hardening*
*Completed: 2026-11-04*
