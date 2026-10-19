---
phase: 01-sync-engine-state-hermeticity
plan: 11
subsystem: sync
tags: [rust, sync, git, push, porcelain, continuity, d-18, proptest, cargo-mutants]

requires:
  - phase: 01-03
    provides: bounded, hermetic engine git spawns (run_local / run_network), RemoteLockGuard, ensure_remote_repo fetch path (checkout -B, reset --hard)
  - phase: 01-04
    provides: EXIT_DRIFT=1 / EXIT_HARD_FAILURE=2, push arm exit mapping
  - phase: 01-10
    provides: open_sync_templates (remote setup for status/pull/push only), sync_cli e2e helper, push skip reporting
provides:
  - "pub struct RemoteCache { templates_dir, fetched_sha, lock }; ensure_remote_repo -> Result<RemoteCache, String> records refs/remotes/origin/<branch>"
  - "pub enum PushRemoteError { UpstreamMoved { branch }, Failed(String) }; push_to_remote(.., fetched_sha: &str) -> Result<String, PushRemoteError>"
  - "Direct push: branch continuity check (HEAD~1 == fetched_sha, else rollback + Failed), plain `git push --porcelain` (never forced), `[rejected]` -> rollback + UpstreamMoved"
  - "pub enum PushPorcelain { Accepted, RejectedNonFastForward, Other } and pub fn classify_push_porcelain(stdout: &str) -> PushPorcelain"
  - "CLI: UpstreamMoved prints the D-18 message and exits 1; Failed prints `Failed to push to remote` and exits 2; neither records ancestors"
affects: [phase-01-verification, 02-sanitizer-boundaries]

actuals:
  tokens: 12886
  tasks: 3
  commits: 6
plan_head_before: 63575d7467cc67e841a4d56e4c5e164465df8334
plan_head_after: 8d5d6eaee7d4b3a407745b8b2aa39c2ef657946a

tech-stack:
  added: []
  patterns:
    - "push_to_remote = stage_publication (allowlist + index secret gate, shared) then push_pr_branch or push_direct; only push_direct returns UpstreamMoved"
    - "The remote's fast-forward check is the compare-and-swap for concurrent publishers; no pre-push ls-remote, no force or lease option"
    - "Race injection in e2e tests: a post-checkout hook in the cache checkout fires during ensure_remote_repo's checkout -B (after the fetch), proven by a marker file"

key-files:
  created: []
  modified:
    - src/sync/mod.rs
    - src/main.rs
    - tests/e2e_lifecycle.rs

key-decisions:
  - "01-11: direct push verifies continuity (cache commit parent == fetched head; mismatch = rollback + exit 2) and pushes with plain `git push --porcelain`; a `!` line with a `[rejected]` summary rolls the commit back and exits 1 with `upstream changed, run ude sync pull` (D-18). PR mode is not subject to the abort (D-18 scope note)"
  - "01-11: classify_push_porcelain reads only the flag field (exactly `!`) and the summary prefix (`[rejected]`); any `[rejected]` line decides, whatever its position; other `!` lines are Other (hard failure)"
  - "01-11: sync_cli (e2e) sets GIT_CONFIG_NOSYSTEM=1 for every test that uses it, so a system core.hooksPath cannot change test behaviour"

patterns-established:
  - "Classifiers that choose an exit code get fixed cases with negative controls, a proptest property pair, a scoped cargo-mutants run, and hand mutants for semantic points cargo-mutants does not generate"

requirements-completed: [SYNC-06]

coverage:
  - id: D1
    description: "When upstream moves between UDE's fetch and its direct push, the remote rejects the plain push; UDE publishes nothing, rolls the cache commit back to the fetched head and returns UpstreamMoved; without the race the same push lands on top of the fetched head"
    requirement: SYNC-06
    verification:
      - kind: integration
        ref: "src/sync/mod.rs#sync::tests::test_direct_push_aborts_when_upstream_moved"
        status: pass
    human_judgment: false
  - id: D2
    description: "A cache commit whose parent is not the fetched head is rolled back and reported as a hard failure (`Branch continuity check failed`), with the remote unchanged; without the stray commit the push succeeds"
    requirement: SYNC-06
    verification:
      - kind: integration
        ref: "src/sync/mod.rs#sync::tests::test_push_continuity_violation_is_hard_failure"
        status: pass
    human_judgment: false
  - id: D3
    description: "Porcelain classification: `!` + `[rejected]` (fetch first / non-fast-forward) is RejectedNonFastForward, `[remote rejected]` and other `!` lines are Other, no `!` line is Accepted; `[rejected]` outside a `!`-flagged summary never counts"
    requirement: SYNC-06
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_classify_push_porcelain"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_classify_push_porcelain_prop"
        status: pass
      - kind: other
        ref: "cargo mutants --file src/sync/mod.rs --re classify_push_porcelain (2 mutants: 1 caught, 1 unviable, 0 missed) plus 5/5 hand mutants caught"
        status: pass
    human_judgment: false
  - id: D4
    description: "From the CLI, `ude sync push --force` against an upstream that moved after the fetch exits 1 with `upstream changed, run ude sync pull`, publishes nothing and leaves ancestor state unchanged; a remote pre-receive rejection exits 2; UDE never passes a force or lease option to git"
    requirement: SYNC-06
    verification:
      - kind: e2e
        ref: "tests/e2e_lifecycle.rs#test_force_does_not_bypass_upstream_moved"
        status: pass
      - kind: e2e
        ref: "tests/e2e_lifecycle.rs#test_sync_push_remote_failure_does_not_advance_ancestor_state"
        status: pass
      - kind: other
        ref: "bash -c '! rg -n \"force-with-lease|\\\"--force\\\"|\\\"-f\\\"\" src/sync/mod.rs && rg -q \"\\\"push\\\", \\\"--porcelain\\\"\" src/sync/mod.rs'"
        status: pass
    human_judgment: false

duration: 12min
completed: 2026-10-19
status: complete
---

# Phase 01 Plan 11: Push Branch Continuity and Upstream-Race Abort Summary

**A direct `ude sync push` now builds only on the remote head it fetched and never force-pushes. If another machine published between UDE's fetch and its push, the remote's fast-forward check rejects the push, UDE undoes its cache commit, publishes nothing, prints `upstream changed, run ude sync pull` and exits 1, with `--force` or without. Every other push failure, including a remote hook rejection, is still a hard failure with exit 2. Neither path advances ancestor state.**

## Performance

- **Duration:** 12 min
- **Started:** 2026-10-19T12:05:55Z
- **Completed:** 2026-10-19T12:17:48Z
- **Tasks:** 3 (6 commits: RED + GREEN for Tasks 1 and 2, one extra test commit for Task 2, test-only for Task 3)
- **Files modified:** 3

## Accomplishments
- `ensure_remote_repo` returns `RemoteCache { templates_dir, fetched_sha, lock }`. `fetched_sha` is read with `git rev-parse --verify refs/remotes/origin/<branch>` after the clone or fetch path; a failure to read it is an error.
- `push_to_remote` takes `fetched_sha` and returns `Result<String, PushRemoteError>`. It is split into `stage_publication` (unchanged allowlist and index secret gate), `push_pr_branch` (unchanged PR flow, errors mapped to `Failed`) and `push_direct`.
- `push_direct`: commit, committed-tree gate, then `check_continuity` (cache commit parent must equal `fetched_sha`, otherwise rollback and `Failed("Branch continuity check failed: ...")`), then `git push --porcelain origin HEAD:refs/heads/<branch>`. A `[rejected]` porcelain line means rollback and `UpstreamMoved`. Any other non-zero push is `Failed("git push origin <branch> failed: <redacted stderr>")`. Porcelain stdout (which starts with `To <url>`) is never printed.
- `classify_push_porcelain` is a pure, documented function with fixed-case, property and mutation coverage.
- `main.rs`: `open_sync_templates` hands the `RemoteCache` to the action, which holds its lock until the action ends. The push arm passes `fetched_sha`. `UpstreamMoved` prints `Upstream branch <b> moved since UDE fetched it; nothing was published.` and `upstream changed, run ude sync pull`, then exits `EXIT_DRIFT` (1). `Failed` prints `Failed to push to remote: ...` and exits `EXIT_HARD_FAILURE` (2).

## Task Commits

1. **Task 1 (tracer) RED:** `09cf2c8` test(01-11): add failing test for direct push abort when upstream moved
2. **Task 1 GREEN:** `7f43bb7` feat(01-11): abort direct push cleanly when upstream moved (D-18)
3. **Task 2 RED:** `ac2726d` test(01-11): add failing tests for push porcelain classification
4. **Task 2 GREEN:** `9319e23` feat(01-11): classify git push porcelain output in a pure function
5. **Task 2 test gap from hand mutants:** `08841bb` test(01-11): pin order independence of push porcelain classification
6. **Task 3 (acceptance tests):** `8d5d6ea` test(01-11): prove --force cannot bypass the upstream-moved abort

Tracer gate (interactive, `end-of-phase`, verify is automated only): Task 1's verify was re-run on the committed tree at `7f43bb7` (1 passed) before Task 2 started.

## TDD Gate Compliance

Tasks 1 and 2 each have a `test(01-11)` commit before their `feat(01-11)` commit. Task 3 has no RED: both e2e targets passed when first run, because Tasks 1-2 already implemented the behaviour. The plan anticipated this. They are acceptance tests, and the product hand mutants below show that they fail when the behaviour breaks.

RED evidence came from nextest JUnit reports, classified with `gsd-tools check tdd-red-evidence` (the verdict field was read):

| Target | Verdict | Failed on | Semantic assessment |
|---|---|---|---|
| sync::tests::test_direct_push_aborts_when_upstream_moved | RED_EVIDENCE_OK | `left: Err(Failed("git push origin main failed: hint: Updates were rejected ..."))`, `right: Err(UpstreamMoved { branch: "main" })` | intended: the race was detected by git, but UDE reported it as a generic failure |
| sync::tests::test_classify_push_porcelain | RED_EVIDENCE_OK | `left: Other`, `right: RejectedNonFastForward` (stub body) | intended |
| sync::tests::test_classify_push_porcelain_prop | RED_EVIDENCE_OK | property 1, minimal input `noise = []`: `Other` vs `Accepted` | intended |
| sync::tests::test_push_continuity_violation_is_hard_failure | INVALID_RED / unexpected_green | passed | characterization: Task 1 already added the check. Hand mutants (check removed, rollback removed) both fail it |
| test_force_does_not_bypass_upstream_moved, test_sync_push_remote_failure_does_not_advance_ancestor_state | INVALID_RED / unexpected_green | passed | acceptance tests (see above) |

The Task 1 RED commit has scaffolding so the target fails on an assertion, not a compile error: the new types, the new signatures, and the split of push_to_remote with behaviour unchanged. The Task 2 RED commit has the classifier stub returning `Other`. It was not yet wired in, under a temporary `#[allow(dead_code)]` that the GREEN commit removed.

## Verification

| Check | Result | What it does not establish |
|---|---|---|
| Task 1 verify `cargo test --bin ude sync::tests::test_direct_push_aborts_when_upstream_moved` | 1 passed, including its negative control (no race: published commit's parent is `fetched_sha`) | The race is injected by pushing from a second clone after `ensure_remote_repo` returns. The engine test does not model a race during the push itself |
| Task 1 source check (no `force-with-lease`, `"--force"`, `"-f"`; `"push", "--porcelain"` present) | exit 0. Controls: the RED tree fails it (pair missing); a sample file containing `"--force"` fails it | Text check on src/sync/mod.rs only. A force option built by `format!` or from a variable would not be seen |
| Task 2 verify (3 targets) | 3 passed | — |
| cargo-mutants, scoped to `classify_push_porcelain`, classifier tests only | exit 0; **2 mutants: 1 caught (`!=` -> `==`), 1 unviable (`Default::default()`), 0 missed, 0 timeouts**; 41 s wall (37 s baseline build); plan check command exit 0 (baseline ran exactly the 2 classifier tests; caught.txt = outcomes.json = 1) | cargo-mutants generated only 2 mutants, so on its own it says little. It does not mutate `nth(1)`, the `starts_with("[rejected]")` rule, the `Other` assignment or the early return. Those were covered by hand mutants (next row) |
| Hand mutants, classifier | 5/5 caught on assertion failures: `nth(1)` -> `nth(0)`, `starts_with` -> `contains`, `Other` assignment dropped, flag prefix instead of exact `!`, `[rejected]` no longer returning early. The last one was missed at first and led to commit `08841bb` (order-independence case). The source was restored byte-identical (checked with `cmp`) | Hand-picked, not exhaustive |
| Hand mutants, push path | continuity check removed, continuity rollback removed, race rollback removed: each caught by its intended test | — |
| Hand mutants, product vs e2e | UpstreamMoved exits 2, Failed exits 1, rejection never classified, `--force` added to the push args: each caught by its intended e2e test | — |
| Task 3 verify (2 e2e targets) | 2 passed; the marker file proves the cache's post-checkout hook fired, which confirms the plan's [ASSUMED] point | The hook fires on the fetch path (`checkout -B`). A first-time clone path runs no hook, so a race during the initial clone is not exercised |
| Hermeticity gate (decoy `GIT_DIR`, `GIT_CONFIG_GLOBAL=/dev/null`, `GIT_CONFIG_NOSYSTEM=1`) | exit 0, 104 `sync::` tests; the decoy repo still had 0 objects and 0 refs afterwards | Covers unit tests only, not e2e |
| `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test` | exit 0 before every non-RED commit (RED commits: fmt + clippy, target failing). Final: 131 unit + 15 e2e (before: 127 + 14) | — |

## What the verification does NOT establish
- **Non-fast-forward wording** (Assumption A3): only the `(fetch first)` form was produced by a real race (in the shallow cache). The `(non-fast-forward)` form is covered only as a fixed string in the classifier test. Classification uses the `[rejected]` prefix, so the parenthetical does not matter if git keeps that prefix.
- **Multi-ref pushes:** UDE pushes one ref. The order-independence case pins the classifier's behaviour for several `!` lines, but no real multi-ref push is tested.
- **Unpublished cache commit after other failures** (T-01-31 accepted residual): after a timeout, a network error or a remote hook rejection, the cache commit is left in place until the next command's `ensure_remote_repo` reset discards it. Not tested in this plan.
- **Windows:** not compiled or run. The e2e race test is `cfg(unix)` (hook mode bits).

## Files Created/Modified
- `src/sync/mod.rs` - `RemoteCache`, `PushRemoteError`, `PushPorcelain`, `classify_push_porcelain`, `SYNC_COMMIT_MESSAGE`; `ensure_remote_repo` returns `RemoteCache` (new `fetched_head`); `push_to_remote` split into `stage_publication` / `push_pr_branch` / `push_direct`; new `check_continuity`; tests `test_direct_push_aborts_when_upstream_moved`, `test_classify_push_porcelain`, `test_classify_push_porcelain_prop`, `test_push_continuity_violation_is_hard_failure`, plus the helpers `push_failure`, `seeded_bare_remote` (now also used by `bare_remote_with_pre_receive`), `direct_push_edit`, `porcelain_noise`; existing callers updated to the new signatures
- `src/main.rs` - `open_sync_templates` returns the `RemoteCache`; the push arm passes `fetched_sha` and maps `UpstreamMoved` (exit 1, D-18 message) and `Failed` (exit 2)
- `tests/e2e_lifecycle.rs` - `test_force_does_not_bypass_upstream_moved`, helpers `remote_cache_checkout` and `mise_ancestor`; exit 2 assertion in `test_sync_push_remote_failure_does_not_advance_ancestor_state`; `sync_cli` sets `GIT_CONFIG_NOSYSTEM=1`

## Decisions Made
- **Refactor inside the RED commit:** push_to_remote was a single 200-line function. To change its error type without wrapping, it was split into three helpers, with behaviour unchanged (full suite green at GREEN). This kept the GREEN diff to the direct path only.
- **`[rejected]` decides regardless of line order.** This follows the plan's rule ("Other when any line has first field `!` otherwise"). It cannot occur with a single-ref push, but it is now pinned by a test instead of being left to a hand mutant.
- **GIT_CONFIG_NOSYSTEM goes into the shared `sync_cli`**, not only the new test. It makes every sync e2e test using that helper more hermetic, and the full suite still passes.
- **Continuity-check read failure:** if `git rev-parse HEAD~1` fails, the commit is rolled back and the push is a hard failure ("cache commit has no parent"), the same as a mismatch.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] The `"push", "--porcelain"` source check and the fmt gate**
- **Found during:** Task 1 verify
- **Issue:** rustfmt put the four-element push args array one element per line, so the pair `"push", "--porcelain"` was no longer on one line and the plan's `rg` source check failed.
- **Fix:** bound the refspec first (`let refspec = format!("HEAD:refs/heads/{branch}")`) so the array `["push", "--porcelain", "origin", &refspec]` fits on one line. It is still one args array.
- **Verification:** source check exit 0, with negative controls (see Verification)
- **Committed in:** 7f43bb7

**2. [Rule 2 - Test strength] Order-independence case added to the classifier test**
- **Found during:** Task 2 mutation gate (hand mutants)
- **Issue:** cargo-mutants generated only 2 mutants for the classifier. A hand mutant (a `[rejected]` line no longer returns early) survived the plan's tests.
- **Fix:** two mixed `!`-line inputs, one in each order, must both classify as `RejectedNonFastForward`. The mutant is now caught; nothing was excluded.
- **Committed in:** 08841bb

---

**Total deviations:** 2 auto-fixed (1 blocking, 1 test strength)
**Impact on plan:** No scope change. No `.cargo/mutants.toml` exclusion was needed, so that file was not created.

## Issues Encountered
- proptest wrote `proptest-regressions/sync/mod.txt` twice (on the RED run and during the hand-mutant runs). Each time it was removed before the next commit, and it was never committed.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- This was the last plan of phase 01. SYNC-06 (shared with 01-10) is complete. Next: phase verification (`/gsd-verify-work 01`).
- Logged in deferred-items.md (pre-existing, out of scope): `open_sync_templates` derives the checkout as `templates_dir.parent()`. For a remote with no `templates/` directory, that is the cache directory, not the checkout. This was read from code and not reproduced.

---
*Phase: 01-sync-engine-state-hermeticity*
*Completed: 2026-10-19*

## Self-Check: PASSED
- FOUND: src/sync/mod.rs, src/main.rs, tests/e2e_lifecycle.rs (modified; no files created)
- FOUND commits on this branch: 09cf2c8, 7f43bb7, ac2726d, 9319e23, 08841bb, 8d5d6ea (`git rev-list --count 63575d7..HEAD` = 6)
- Acceptance greps: `pub struct RemoteCache` 1, `pub enum PushRemoteError` 1, `fetched_sha: &str` in the push_to_remote signature, `fetched_sha` in main.rs, `upstream changed, run ude sync pull` 1, `pub fn classify_push_porcelain(stdout: &str) -> PushPorcelain` 1
