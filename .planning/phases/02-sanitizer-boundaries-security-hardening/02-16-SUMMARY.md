---
phase: 02-sanitizer-boundaries-security-hardening
plan: 16
subsystem: sync
tags: [git, push, refspec, replace-refs, publication-gate, security]

requires:
  - phase: 02-15
    provides: the committed gate, run_push, hook fixtures (push_with_post_commit_hook, push_to_fresh_bare, assert_published_exact)
provides:
  - both pushes send `<id>^{commit}` under `--no-replace-objects`, so no local ref and no replace ref can stand in for the validated commit
  - resolve_commit reads past replace refs, so a replace ref to a tag cannot change the id the gate reads
  - engine tests over every ref namespace, a token, five rebuilt commits, a clean filter and replace refs, in both modes, each with a delivering control where one exists
  - CliPushWorld / cli_push_world / install_cache_hook / consumer_pull for plans 02-17 to 02-19, and the CLI rows for review CR-300
affects: [02-17, 02-18, 02-19, 02-20]

actuals:
  tokens: 10807   # chars/4 of `git diff d221cee HEAD -- src tests` (diff text, context lines included)
  tasks: 3
  commits: 5      # git rev-list --count plan_head_before..plan_head_after
plan_head_before: d221cee683ea941475b868deda73cb485fb9bbd1
plan_head_after: c82f110f0dfdad2ef651a1f8a363e51dc2e42e63

tech-stack:
  added: []
  patterns:
    - "A push source spelled `<id>^{commit}`: no ref name can hold `^`, so push cannot resolve it to a local ref"
    - "Shadow-ref tests pair the engine push with a plain-id push of the same checkout that must deliver the object"

key-files:
  created: []
  modified:
    - src/sync/mod.rs
    - tests/e2e_lifecycle.rs

key-decisions:
  - "02-16: push_direct and push_pr_branch send `<id>^{commit}` and pass `--no-replace-objects` first; resolve_commit's rev-parse passes `--no-replace-objects` too (reverses 02-11's choice to honour replace refs there)"
  - "02-16: test_direct_push_replaced_commit_refusal_does_not_say_upstream_moved is replaced by test_{direct,pr}_push_publishes_validated_commit_past_replace_ref: a parentless replace ref now publishes the validated commit"
  - "02-16: CliPushWorld runs `ude init` through its own runner (fixture HOME, XDG dirs, global gitconfig); init_rust_project and its other callers are unchanged, so review IN-301 stays open for them"

patterns-established:
  - "shadow_ref_tail(prefix) / behind_shadow_tag(tail) / assert_shadow_ignored / assert_plain_id_delivers in sync::tests"
  - "CliPushWorld::install_cache_hook(name, body) installs a one-shot hook in the cache checkout after a first `sync status --remote`"

requirements-completed: []   # SEC-04, SEC-02 declared; requirements.ready-ids reports both blocked by later sibling plans

coverage:
  - id: D1
    description: "A ref named after the commit id (refs/tags, refs/heads, refs/remotes) no longer changes what either push sends; refs/<id> and a ref file spelled like the source do not either"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_{direct,pr}_push_ignores_{tag,branch,remote_ref,top_level_ref}_named_after_the_commit, test_direct_push_ignores_ref_file_spelled_like_the_source"
        status: pass
    human_judgment: false
  - id: D2
    description: "A token behind a shadow ref, and the five rebuilt commits plan 02-13 refuses (duplicate tree, `.` tree, deleted file, reverted file, amended message) behind a shadow tag, stay off the remote in both modes; a clean filter that installs the shadow hook during `git add` changes nothing"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_{direct,pr}_push_ignores_{token,duplicate_tree,dot_tree,deleted_file,reverted_file,amended_message}_behind_shadow_ref, test_{direct,pr}_push_ignores_shadow_ref_left_by_clean_filter"
        status: pass
    human_judgment: false
  - id: D3
    description: "A replace ref cannot change what is sent: a parentless replacement publishes the validated commit, and a replace ref to a tag holding an injected commit is ignored by resolve_commit and both pushes"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_{direct,pr}_push_publishes_validated_commit_past_replace_ref, test_{direct,pr}_push_ignores_replace_ref_to_a_tag"
        status: pass
    human_judgment: false
  - id: D4
    description: "Through the CLI: direct and --pr pushes with a post-commit shadow-tag hook holding a token exit 0 with no token on the remote or in a consumer's pull; no-hook token and high-entropy lines exit 1 with the remote unchanged; a benign edit exits 0"
    requirement: SEC-02
    verification:
      - kind: e2e
        ref: "tests/e2e_lifecycle.rs#test_cli_direct_push_ignores_tag_named_after_the_commit, test_cli_pr_push_ignores_tag_named_after_the_commit, test_cli_direct_push_with_post_commit_hook_and_no_shadow_ref_publishes, test_cli_direct_push_refuses_token_line_with_no_hook, test_cli_direct_push_refuses_high_entropy_line_with_no_hook, test_cli_direct_push_publishes_benign_edit_with_no_hook"
        status: pass
    human_judgment: false
  - id: D5
    description: "Pre-push hooks still run on both pushes"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_{direct,pr}_push_publishes_with_benign_pre_push_hook"
        status: pass
    human_judgment: false

duration: 20min
completed: 2026-11-04
status: complete
---

# Phase 02 Plan 16: Push source no ref can name Summary

**Both pushes now send `<id>^{commit}` under `--no-replace-objects`, and resolve_commit reads past replace refs, so a hook's ref or replace ref named after the validated commit can no longer make `git push` publish another commit. 28 engine tests and 6 CLI rows prove it, each shadow case with a plain-id control that does deliver the injected object.**

## Performance

- **Duration:** 20 min
- **Started:** 2026-11-04T01:52:48Z
- **Completed:** 2026-11-04T02:12:28Z
- **Tasks:** 3 of 3
- **Files modified:** 2

## Accomplishments

- push_direct and push_pr_branch: source `{commit}^{{commit}}`, `--no-replace-objects` as the push's first argument. `rg -c 'commit\}\^\{\{commit\}\}:refs/heads/' src/sync/mod.rs` prints 2 (0 at the base).
- resolve_commit: `git --no-replace-objects rev-parse --verify <rev>^{commit}`.
- Comments rewritten to say what git does: the block above each refspec, push_direct's doc comment, the non-fast-forward arm (a shallow file can still cause it, a replace ref no longer can), HOOK_REPLACE_WITH_PARENTLESS's doc, resolve_commit's comment. The shallow refusal still says `still at the commit UDE fetched` (`rg -c 'most likely caused this'` prints 1).
- 28 new engine tests (the test Task 2 told me to delete is gone) and 6 CLI rows. A reusable CLI world for plans 02-17 to 02-19.

## Task Commits

1. **Task 1 (tracer): a tag named after the commit, direct mode**: RED `d661792` (test), GREEN `a891d58` (feat)
2. **Task 2: both modes, every namespace, token, rebuilt commits, replace refs, comments**: RED `ce51a6c` (test), GREEN `f1628d9` (feat)
3. **Task 3: CLI rows and CliPushWorld**: `c82f110` (test; the plan forbids a production change in this task)

## Files Created/Modified

- `src/sync/mod.rs`: the two pushes and resolve_commit; comments; test helpers shadow_ref_tail, behind_shadow_tag, assert_shadow_ignored, assert_plain_id_delivers, HOOK_REF_FILE_SPELLED_LIKE_SOURCE, HOOK_REPLACE_WITH_TAG; every test named in Tasks 1 and 2.
- `tests/e2e_lifecycle.rs`: CliPushWorld, cli_push_world, the methods ude / ude_with_env / install_cache_hook / consumer_pull / push / remote_refs / remote_holds; injected_token_hook; the six Task 3 tests.

## RED Evidence

The records are in `target/gsd-red/02-16-*.json` (gitignored). They are built from the nextest JUnit report and classified by `gsd-tools check tdd-red-evidence`.

- **Task 1:** `test_direct_push_ignores_tag_named_after_the_commit` got `RED_EVIDENCE_OK / target_test_failed`. It failed at `the remote holds the object recorded in ude-test-injected-blob`, after the hook-ran and Ok assertions had passed. This is the planned failure: the direct push sent the shadow tag's commit.
- **Task 2:** 26 run, 12 failed, 14 passed.
  - 12 got `RED_EVIDENCE_OK`. Ten PR shadow tests (tag, branch, remote ref, token, duplicate, dot, deleted, reverted, message, clean filter) failed at `the remote holds the object recorded in ude-test-{injected-blob,shadow-target}`. Both replace-to-tag tests failed at the Ok assertion with `Err(Failed("...the message of the commit to publish is not the sync commit message..."))`. That is resolve_commit peeling through the replace ref to the injected commit, so the gate refused: it failed closed, as the plan predicted.
  - 14 passed at RED. These are the direct-mode tests Task 1 had already fixed, plus the cases where no ref can shadow. **The negative control for them:** I ran the 13 direct tests against push_direct with Task 1's change temporarily reverted. 11 failed: 10 at `the remote holds ...`, and `test_direct_push_publishes_validated_commit_past_replace_ref` at `refused as not a fast-forward`. The two that passed are `test_direct_push_ignores_top_level_ref_named_after_the_commit` and `test_direct_push_ignores_ref_file_spelled_like_the_source`. They also pass before the fix, because nothing shadows in those cases (the plan says so). Their tests assert that the hook's ref or file really exists. `test_pr_push_publishes_validated_commit_past_replace_ref` and `test_pr_push_ignores_top_level_ref_named_after_the_commit` pass with or without the fix, as the plan's probes predicted.
- **Task 3:** all 6 passed when written, because Tasks 1 and 2 had already fixed the code. **RED evidence comes from a second run:** I rebuilt with `push_pr_branch` through `resolve_commit` restored to their text at `d661792`, then restored the file and checked it with `cmp`. In that run both shadow CLI rows failed at `the remote holds the token blob`. Both got `RED_EVIDENCE_OK / target_test_failed`. The no-shadow control row passed. Limit: those two records' `runStartedAt` is approximate (report mtime minus 60 s), because I did not capture the start time for that run.

## Negative controls for each change (mutation runs, file restored after each, `cmp` identical)

| Change reverted | Tests that failed | Tests that still passed |
|---|---|---|
| `--no-replace-objects` on the PR push | test_pr_push_ignores_replace_ref_to_a_tag (remote holds blob) | test_pr_push_ignores_tag_named_after_the_commit |
| `--no-replace-objects` on the direct push | test_direct_push_ignores_replace_ref_to_a_tag, test_direct_push_publishes_validated_commit_past_replace_ref | test_direct_push_ignores_tag_named_after_the_commit |
| `--no-replace-objects` in resolve_commit | both replace-to-tag tests (gate refusal) | — |
| `^{commit}` on the PR source | PR tag, branch, remote-ref tests (remote holds blob) | test_pr_push_ignores_replace_ref_to_a_tag |

Each of the four changes is pinned by at least one test. Each test fails only for the change it covers.

## Verification

- Task 1 verify: exit 0. The tracer feedback gate re-ran it after the commit: exit 0.
- Task 2 verify 1: the name-uniqueness loop and 29 tests, exit 0. Verify 2: the refspec count is 2, and `sync::tests:: process::tests::` is 263 passed, exit 0.
- Task 3 verify 1: the name-uniqueness loop and 6 tests, exit 0. Verify 2: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --test e2e_lifecycle` exit 0 (96 passed), and that exit code gated the commit.
- Final: full `cargo test` exit 0. 359 unit tests passed; 1 was ignored: `test_dump_parity_corpus`, which existed before and was not touched. 96 e2e tests passed.
- `rg -c 'ghp_[0-9A-Za-z]{30,}' tests/e2e_lifecycle.rs` exits 1 (no token literal). The gitleaks pre-commit hook passed on every commit.
- Hermeticity: the repository's `git config --local -l` hash was unchanged after all the e2e runs. The index matched HEAD with 214 entries, the same count as the base tree. TMPDIR was never set. No background process was started.

**What these checks do not establish:** hosted remotes (GitHub), Windows, the CR-301 route (plan 02-17) and the push destination (plan 02-18). The replace-ref-to-tag row ran through the engine against a local bare remote, not a network remote. A replace ref written after `git push` starts is not tested here; the plan's probe saw no diversion from it. Each test ran once per run, so this is not a statistical reliability bound.

## Decisions Made

- I added `--no-replace-objects` to resolve_commit, as the plan ordered. This reverses 02-11's recorded choice (honouring replace refs there "is harmless"). With a replace ref to a tag it was not harmless.
- `ude_with_env` has no caller yet, so it carries `#[expect(dead_code, reason = ...)]`. The first plan that calls it must remove the attribute, or `unfulfilled_lint_expectations` will fail clippy.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Task 1's GREEN commit gate left out the one test Task 2 rewrites**
- **Found during:** Task 1 GREEN
- **Issue:** The plan's gate `cargo test --bin ude sync::` cannot pass after Task 1. `test_direct_push_replaced_commit_refusal_does_not_say_upstream_moved` fails once the direct push reads past replace refs. The plan predicted exactly this, and Task 2 deletes that test.
- **Fix:** I gated `a891d58` on fmt, clippy and nextest `sync::` minus that one test (252 passed). Task 2's GREEN commit used the plan's full gate (278 passed).
- **Process slip:** I piped that nextest run into `rg Summary`, so the chain checked rg's exit code, not nextest's. The output showed 252 passed and 0 failed, and every later gate (`f1628d9`, `c82f110`) checked the real exit code. No broken state was committed. The slip was in how the gate was wired.

**2. [Rule 1 - Test correctness] The high-entropy CLI row was refused by the wrong rule**
- **Found during:** Task 3
- **Issue:** `# key = <cycling token>` was refused by the gitleaks `generic-api-key` rule (because of the word `key`), not by the entropy rule this row is for.
- **Fix:** The line is now `# <cycling token>`, and the test asserts `high-entropy` in the output. Both no-hook refusal rows also assert `Push rejected on mise.toml`. Without that, any exit-1 path would have passed them.

**3. [Rule 2 - Missing verification] Extra checks the plan did not list**
- Clean-filter tests: the filter-ran marker, plus the plain-id delivering control. The plan listed only "blob absent".
- refs/<id> and ref-file tests: each asserts that the hook's ref or file exists. Those cases have no delivering control, so without this a hook that never wrote anything would also pass.
- CLI shadow rows: the consumer must receive the published edit (not just "no token"), and the push output must not contain the token.

---

**Total deviations:** 3 (1 blocking, 1 test correctness, 1 added verification). **Impact:** production code is exactly as planned. The tests are stricter than planned.

## TDD Gate Compliance

- Task 1: RED `d661792`, then GREEN `a891d58`, in order.
- Task 2: RED `ce51a6c`, then GREEN `f1628d9`, in order.
- Task 3: one `test(02-16)` commit `c82f110` with no `feat` after it. The plan forbids a production change in this task, and its fix already exists in `a891d58` and `f1628d9`, which come before it. The RED evidence for this task comes from a run against the pre-fix production code (see RED Evidence).

## Findings Against the Plan's Probes

- Every engine test behaved as the interfaces' probes said it would. The only sync failure caused by Task 1 was the test Task 2 replaces. `refs/<id>` did not shadow. The ref file named `refs/tags/<id>^{commit}` did not match. A parentless replace ref in PR mode published the validated commit even without the flag. A replace ref to a tag fails closed at the gate until resolve_commit gets the flag.
- One finding, about test design rather than the probes: a cycling 32-character token after `key =` is caught by gitleaks' `generic-api-key` rule before the entropy rule sees it (see deviation 2).

## Issues Encountered

None beyond the deviations.

## User Setup Required

None.

## Next Phase Readiness

- Ready for 02-17. It can reuse CliPushWorld, cli_push_world, install_cache_hook, consumer_pull, ude_with_env (remove its `#[expect(dead_code)]`), injected_token_hook and cache_recorded_id.
- SEC-04 and SEC-02 stay unchecked: `requirements.ready-ids` reports both blocked by sibling plans that have no SUMMARY yet.

## Self-Check: PASSED

- FOUND: d661792, a891d58, ce51a6c, f1628d9, c82f110 (all ancestors of HEAD)
- FOUND: src/sync/mod.rs, tests/e2e_lifecycle.rs; `struct CliPushWorld`, `fn cli_push_world` and `fn install_cache_hook` are present; `fn test_direct_push_ignores_tag_named_after_the_commit` is present
- `test_direct_push_replaced_commit_refusal_does_not_say_upstream_moved` is absent from src/sync/mod.rs (rg exit 1)

---
*Phase: 02-sanitizer-boundaries-security-hardening*
*Completed: 2026-11-04*
