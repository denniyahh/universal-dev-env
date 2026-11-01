---
phase: 02-sanitizer-boundaries-security-hardening
plan: 11
subsystem: sync
tags: [rust, sync, push, git-hooks, publication-gate, replace-refs, gap-closure]

requires:
  - phase: 02-sanitizer-boundaries-security-hardening (02-08)
    provides: "ValidatedBlobs and the staged and committed publication gates"
  - phase: 02-sanitizer-boundaries-security-hardening (02-10)
    provides: "push gate 2 on the shared render rule (unchanged here)"
provides:
  - "validate_committed_tree binds the commit it validates to the fetched head in both push modes: exactly one parent, read from the raw commit object; changes listed from the fetched head"
  - "The gate's cat-file, diff-tree and show reads pass --no-replace-objects, each pinned by its own hook test"
  - "push_direct and push_pr_branch send <validated id>:refs/heads/<name> with --no-follow-tags"
  - "Hook test helpers for plan 02-12: push_with_post_commit_hook, HookedPush, remote_heads and the HOOK_* tails"
affects: [sync push, phase-02 verification gap 1, review WR-102, SEC-04, plan 02-12]

actuals:
  tokens: 11529     # chars/4 over the realized src/ diff (46115 chars, git diff 78cfece..6a88d0d)
  tasks: 3
  commits: 6
plan_head_before: 78cfecebceb691764ee60b3fb31a2c0ad0669856
plan_head_after: 6a88d0dd61972d34a909e6ecb0cc03426c628d9e

tech-stack:
  added: []
  patterns:
    - "Hook tests assert a marker the hook wrote, so neither a refusal nor a publish can come from a hook that never ran"
    - "A replace-ref test shows the default read fooled and the raw read correct in the same checkout"

key-files:
  created: []
  modified:
    - src/sync/mod.rs

key-decisions:
  - "02-11: the committed gate requires exactly one parent equal to the fetched head in both push modes, read from the raw commit object, and lists changes with a two-tree diff-tree from that head; check_continuity is removed and its message text moved into the gate"
  - "02-11: the gate's cat-file, diff-tree and show reads pass --no-replace-objects (the environment form is stripped by process::prepare_command); resolve_commit's rev-parse does not, because its id is what the gate reads raw and the push sends"
  - "02-11: both pushes send the validated commit id with --no-follow-tags; the PR push no longer sets an upstream"

patterns-established:
  - "A git shim on the engine's PATH can reach the point between two engine git commands where no hook runs"

requirements-completed: [SEC-04]

coverage:
  - id: D1
    description: "A post-commit hook that wraps an amended commit in a merge is refused in direct and PR mode, with no publish"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_direct_push_refuses_hook_merge_over_amended_commit"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_pr_push_refuses_hook_merge_over_amended_commit"
        status: pass
    human_judgment: false
  - id: D2
    description: "Parent-count boundary (0, 1, 2) and parent mismatch refused in both modes; object ids compare as full strings; parent parsing is header-only; an added file is refused"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_pr_push_refuses_hook_parentless_commit"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_pr_push_refuses_hook_added_commit"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_direct_push_refuses_hook_added_commit"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_committed_gate_compares_full_object_ids"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_raw_commit_parents"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_push_refuses_hook_added_file"
        status: pass
    human_judgment: false
  - id: D3
    description: "A benign post-commit hook still publishes the exact validated bytes in direct and PR mode; amend-only and a stray local commit stay refused"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_direct_push_publishes_with_benign_post_commit_hook"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_pr_push_publishes_with_benign_post_commit_hook"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_post_commit_hook_rewrite_is_refused"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_push_continuity_violation_is_hard_failure"
        status: pass
    human_judgment: false
  - id: D4
    description: "Each committed-gate read is raw, pinned by its own test: removing one read's option fails exactly its paired test"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_push_refuses_hook_replaced_blob"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_push_refuses_hook_merge_replaced_by_validated_commit"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_push_refuses_hook_replaced_tree"
        status: pass
    human_judgment: false
  - id: D5
    description: "Both pushes send the validated commit id, never a ref a hook or another process can move"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_pr_push_publishes_the_validated_commit_not_the_branch_tip"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_direct_push_publishes_the_validated_commit_after_head_moves"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_direct_push_aborts_when_upstream_moved"
        status: pass
    human_judgment: false
  - id: D6
    description: "No tag follows either push, whatever push.followTags a hook wrote; the same push without the option publishes the tag to a second bare repository"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_direct_push_does_not_publish_hook_tag"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_pr_push_does_not_publish_hook_tag"
        status: pass
    human_judgment: false

duration: 15min
completed: 2026-11-01
status: complete
---

# Phase 02 Plan 11: Committed Gate Bound to the Fetched Head Summary

**A post-commit hook can no longer publish through `ude sync push` by amending, merging, stacking or replacing commits, in direct or PR mode: the committed gate requires exactly one new commit on the fetched head, reads every object raw, and the push sends that commit id with no tags following.**

This is the first half of closing 02-VERIFICATION gap 1 (review WR-102). **Plan 02-08's rule in full is not yet true:** plan 02-12 still has to bind the commit message and close the channels through which a push sends objects outside the validated commit (grafts, commit-graphs, bitmaps). It also runs the phase's final gates.

## Performance

- **Duration:** 15 min
- **Started:** 2026-11-01T14:53:01Z
- **Completed:** 2026-11-01T15:08Z
- **Tasks:** 3
- **Files modified:** 1 (src/sync/mod.rs)

## Accomplishments

- The verifier's reproduction (amend, then a merge whose first parent is the fetched head) is now refused. Before this plan it published the injected line, in direct mode and in PR mode.
- PR mode is exercised by tests rather than inferred: the merge hook and the amend-plus-extra-commit hook are both refused, with no `ude-sync-*` branch on the remote.
- Replace refs cannot hide content from the gate: blob, commit and tree variants are each refused because of that read being raw.
- The push sends what the gate validated, in both modes, and no tag follows it.
- `check_continuity` is gone; its check lives in the committed gate for both modes. `test_push_continuity_violation_is_hard_failure` passes with its assertions unchanged.

## Task Commits

1. **Task 1 (tracer): merge and stacked-commit hooks refused, gate bound to the fetched head**
   - RED `24c0326` test(02-11): add failing tests for post-commit merge and stack hooks
   - GREEN `14c1db2` feat(02-11): bind the committed gate to the fetched head in both modes
2. **Task 2: boundaries and controls** - `4862e8b` test(02-11) (all seven passed on arrival)
3. **Task 3: raw reads, validated-id push, no tags**
   - RED `4091fc5` test(02-11): add failing tests for replace refs, branch tips and tags
   - GREEN `b612587` feat(02-11): read committed objects raw and push the validated id
   - Added test `6a88d0d` test(02-11): pin the direct push to the validated commit id (see Deviations)

**Plan metadata:** recorded in the docs(02-11) commit that adds this file.

## RED Evidence

Records are in `target/gsd-red/02-11-*.json` (gitignored), built from the nextest JUnit report. All nine were classified `RED_EVIDENCE_OK` / `target_test_failed` by `gsd-tools check tdd-red-evidence`.

**Semantic check, all nine:** each target ran and failed on its planned assertion, after its `hook_ran` assertion passed. Throwaway probe tests (removed before each RED commit, never committed) ran the same hooks on the same code and printed what the remote held, which is the column on the right.

| Test (Task) | Failure line | Probe: what the bare remote held |
|---|---|---|
| `test_direct_push_refuses_hook_merge_over_amended_commit` (1) | `the remote changed; push result: Ok("Directly pushed changes to remote branch 'main'.")` | `main` blob holds `INJECTED_BY_HOOK` |
| `test_pr_push_refuses_hook_merge_over_amended_commit` (1) | `the remote changed; push result: Ok("Pushed changes to branch 'ude-sync-...'...")` | `ude-sync-*` blob holds `INJECTED_BY_HOOK` |
| `test_pr_push_refuses_hook_added_commit` (1) | same `Ok("Pushed changes to branch ...")` | `ude-sync-*` blob holds `INJECTED_BY_HOOK` |
| `test_push_refuses_hook_replaced_blob` (3) | `the remote changed; push result: Ok("Directly pushed ...")` | `main` blob holds `INJECTED_BY_HOOK` |
| `test_push_refuses_hook_merge_replaced_by_validated_commit` (3) | same | `main^2` blob holds `INJECTED_BY_HOOK` |
| `test_push_refuses_hook_replaced_tree` (3) | same | `templates/hook-extra.txt` on `main` holds `INJECTED_BY_HOOK` |
| `test_pr_push_publishes_the_validated_commit_not_the_branch_tip` (3) | byte-exact blob assertion: left ends in the bytes of `INJECTED_BY_HOOK\n`, right is `[core]\n\tx = 1\n` | `ude-sync-*` blob holds `INJECTED_BY_HOOK` |
| `test_direct_push_does_not_publish_hook_tag` (3) | `left: "<id> tag\trefs/tags/ude-hook-tag"`, `right: ""` | `refs/tags/ude-hook-tag` |
| `test_pr_push_does_not_publish_hook_tag` (3) | same | `refs/tags/ude-hook-tag` |

## Task 2: Tests on Arrival

All seven passed on arrival, and no gate fix was needed: `test_raw_commit_parents`, `test_committed_gate_compares_full_object_ids`, `test_pr_push_refuses_hook_parentless_commit`, `test_direct_push_refuses_hook_added_commit`, `test_push_refuses_hook_added_file`, `test_direct_push_publishes_with_benign_post_commit_hook`, `test_pr_push_publishes_with_benign_post_commit_hook`.

Because passing on arrival proves little by itself, they were then run under temporary wrong rules, restored with a sha256 check each time:

| Temporary wrong rule | Failed |
|---|---|
| Parser ignores the end of the header (counts message lines) | `test_raw_commit_parents` |
| Parser counts indented continuation lines | `test_raw_commit_parents` |
| Parent compared by prefix | `test_committed_gate_compares_full_object_ids` |
| Any single parent accepted | `test_committed_gate_compares_full_object_ids`, `test_direct_push_refuses_hook_added_commit`, `test_pr_push_refuses_hook_added_commit`, `test_push_continuity_violation_is_hard_failure` |
| Zero parents accepted | `test_pr_push_refuses_hook_parentless_commit` |
| One-argument `diff-tree <commit>` instead of `<base> <commit>` | **none** (`test_push_refuses_hook_added_file` and the direct benign control pass) |

The last row is not a test gap, by reasoning rather than by a run: once the parent check has passed, the one-argument form compares the commit with that same parent, so the two forms list the same entries.

## Per-Read Control (Task 3)

Each row removed one read's `--no-replace-objects` and ran the three replace-ref tests with `--no-fail-fast`.

| Option removed from | Failed | Passed |
|---|---|---|
| `show` | `test_push_refuses_hook_replaced_blob` | the other two |
| `cat-file commit` | `test_push_refuses_hook_merge_replaced_by_validated_commit` | the other two |
| `diff-tree` | `test_push_refuses_hook_replaced_tree` | the other two |

`sha256sum src/sync/mod.rs` at GREEN and after every restore: `bf49c642666f62685e8f0893b08ba2372c5b3922df4ab9b6f0900af900a6772d`.

Further controls, same procedure:

| Temporary change | Failed | Passed |
|---|---|---|
| Direct push without `--no-follow-tags` | `test_direct_push_does_not_publish_hook_tag` | the PR tag test |
| PR push without `--no-follow-tags` | `test_pr_push_does_not_publish_hook_tag` | the direct tag test |
| PR push by branch name | `test_pr_push_publishes_the_validated_commit_not_the_branch_tip` | n/a |
| Direct push by `HEAD` refspec | `test_direct_push_publishes_the_validated_commit_after_head_moves` | n/a |

## Plan-End Gate

Run on the final commit `6a88d0d`, each exit code captured separately.

| Check | Exit |
|---|---|
| `cargo fmt --check` | 0 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 |
| `cargo nextest run --bin ude` over the plan's 25 named tests (25 `PASS` lines) | 0 |
| `git diff --exit-code -- Cargo.toml Cargo.lock` | 0 |

Each code commit was also gated on `cargo fmt --check && cargo clippy ... && cargo test`: 235 unit passed (1 ignored), 88 e2e passed on the last run. The two RED commits were gated on fmt and clippy only, because their tests fail by design.

## What These Checks Do Not Establish

- **Plan 02-08's rule in full.** A hook can still get objects outside the validated commit sent, or put unvalidated text in the commit message. Those channels are plan 02-12's.
- **The mutation run has not happened.** The scoped `cargo mutants` run is in plan 02-12, after the gate's last change. The controls above are hand-picked wrong rules, not an exhaustive set.
- **Only `.git/hooks/post-commit` was exercised.** A global `core.hooksPath`, a config-defined hook and `reference-transaction` were not run. The binding does not depend on which mechanism moved the refs (reasoned), and plan-time probes showed config-defined hooks do run.
- **File mode and type changes stay accepted** when the bytes match (operator-side scope decision recorded in the plan). No test covers this.
- **Hook leftovers in the cache checkout.** A refused or tag-following hook's replace refs, tag and `push.followTags` setting stay in the cache repository. Later pushes read raw and pass `--no-follow-tags`, but I did not test a second push after such a hook.
- **PR mode ran only against a local bare remote**, where `gh pr create` fails and the fallback message is returned. A real GitHub PR was not opened.
- **Strength:** one run of each test on git 2.56.0 on this host, plus the gated reruns. That is evidence for these scenarios on this git version, not a claim about other git versions.
- **Requirement status:** `requirements-completed` lists SEC-04 because the plan declares it. REQUIREMENTS.md was not marked: `requirements.ready-ids` reports SEC-04 blocked by plan 02-12, and the verifier decides after re-verification.

## Files Created/Modified

- `src/sync/mod.rs`: `raw_commit_parents`, `resolve_commit`, the bound and raw `validate_committed_tree`, `push_pr_branch` with `fetched_sha`, both pushes by validated id with `--no-follow-tags`, `check_continuity` removed, doc updates, and the test helpers and 17 tests.

## Decisions Made

- `validate_committed_tree` stayed one function, over the 40-line style limit, because the plan's structure check requires the parent check in its body and plan 02-12 extends it.
- `HOOK_*` tails that repeat the amend or add-file lines share them through two small `macro_rules!` literals, because `concat!` cannot take a `const`.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing test] Direct-mode validated-id push had no behavioural test**
- **Found during:** Task 3 controls
- **Issue:** Changing the direct push back to a `HEAD` refspec failed no test. The plan's validated-id test covers PR mode only, and no hook runs between the direct gate and its push, so prohibition 2 ("verification: test") rested on a source-text check for direct mode.
- **Fix:** Added `test_direct_push_publishes_the_validated_commit_after_head_moves`: a `git` shim on the engine's `PATH` amends HEAD right before the real push. The remote gets the validated bytes while local HEAD holds the injected line.
- **Files modified:** src/sync/mod.rs
- **Verification:** Passes on the code, fails under the `HEAD` refspec.
- **Committed in:** 6a88d0d

**2. [Rule 3 - Blocking] `HookedPush` fields added at first use**
- **Found during:** Task 1 RED
- **Issue:** The plan defines `checkout` and `managed_rel` in Task 1, but nothing reads them until later, and clippy `-D warnings` rejects unread fields.
- **Fix:** `managed_rel` arrives in Task 2 and `checkout` in Task 3. The final struct matches the plan.
- **Committed in:** 4862e8b, 4091fc5

**3. [Process - known GSD defect] `milestone: v8.30.1` in the state files**
- **Found during:** metadata commit
- **Issue:** The GSD state writers scrape `v8.30.1` (a gitleaks version in plan text) into STATE.md and state.json. It was already present in the orchestrator's uncommitted `state.begin-phase` update.
- **Fix:** Deleted the STATE.md line and set state.json `milestone` to null before the metadata commit, then logged the incident in `~/.config/agents/gsd-manual-fix-incidents.md`.

### Minor differences (no behaviour change)

- Added test helpers the plan does not name: `assert_remote_unchanged`, `remote_managed_blob`, `single_sync_branch`, `hook_recorded_id`, `assert_hook_tag_not_published`.
- Hook tests assert `hook_ran` and the remote state before `push_failure`, because `push_failure` consumes the result. This is also why RED fails on the remote assertion, whose message prints the `Ok`.

---

**Total deviations:** 2 auto-fixed (1 missing test, 1 blocking), 1 known GSD-defect correction.
**Impact on plan:** The plan adds 17 tests instead of 16; the plan-end gate's 25 named tests are unchanged. No other production file changed, and Cargo files are unchanged.

## Issues Encountered

- The first RED commit's subject exceeded the 72-character limit, so I amended it before any GREEN work. The hash above is the amended one.

## TDD Gate Compliance

RED `24c0326` precedes GREEN `14c1db2`, and RED `4091fc5` precedes GREEN `b612587` (`git merge-base --is-ancestor` exit 0 for both). Task 2 and `6a88d0d` are test-only. No REFACTOR commit.

## User Setup Required

None.

## Next Phase Readiness

- Ready for plan 02-12, which generalises `push_with_post_commit_hook` and extends `validate_committed_tree`.
- 02-REVIEW-DISPOSITION.md was not edited; the next code review records WR-102's closure.

## Self-Check: PASSED

- src/sync/mod.rs holds `raw_commit_parents` and `resolve_commit`, and no longer defines `check_continuity` (`rg` exit 1).
- Commits 24c0326, 14c1db2, 4862e8b, 4091fc5, b612587, 6a88d0d are ancestors of HEAD; `git rev-list --count 78cfece..6a88d0d` = 6.
- No stub markers in the added lines (`rg` exit 1).

---
*Phase: 02-sanitizer-boundaries-security-hardening*
*Completed: 2026-11-01*
