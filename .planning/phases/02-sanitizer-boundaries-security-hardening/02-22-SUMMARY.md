---
phase: 02-sanitizer-boundaries-security-hardening
plan: 22
subsystem: sync
tags: [git, publication-gate, object-integrity, hash-object, rust]

requires:
  - phase: 02-sanitizer-boundaries-security-hardening
    provides: "02-13 committed-route content check, 02-19 empty-status check, 02-21 control rows"
provides:
  - "process::run_bounded_with_input: a bounded run that feeds bytes to the child's stdin"
  - "SyncEngine::read_verified_object: every blob, and every tree below the root, the two push routes rely on is hashed with git hash-object and compared whole with its id"
  - "listed_tree_ids and verify_ancestor_trees: the trees above a recorded or deleted path are checked on both routes"
  - "A refusal text of its own for an id mismatch (OBJECT_ID_MISMATCH)"
affects: [02-23, push gate, docs/ARCHITECTURE.md section 3]

actuals:
  tokens: 14893
  tasks: 3
  commits: 3
plan_head_before: f0c7f659bdc8939cc6d3a62dc631a3e0ab65dcc6
plan_head_after: efe86b6d99284e0f98c07fba4f3a3e271b4555eb

tech-stack:
  added: []
  patterns:
    - "Object reads that count for a gate go through read_verified_object (cat-file -t, cat-file <kind>, hash-object -t <kind> --literally --no-filters --stdin, whole-id compare)"
    - "Tests forge a loose object by removing the target file before writing (local clones hardlink loose objects)"

key-files:
  created: []
  modified:
    - src/process.rs
    - src/sync/mod.rs

key-decisions:
  - "02-22: blobs and the trees above recorded or deleted paths are hashed with git's own hash-object in the cache checkout (its own object format, no write flag) and compared whole with the id; a mismatch is refused with its own text, naming only a path from the record, the staged list or the allowlisted changed entry"
  - "02-22: the root tree and the commit get pin rows, not code: git refuses a rewritten fetched root tree itself (git status exits 128, hash mismatch, measured in the rows)"

patterns-established:
  - "push_after_step: push_with_empty_status without the empty-status assertion, for rows that take the committed route"

requirements-completed: [SEC-04]

coverage:
  - id: D1
    description: "A blob the remote holds, rewritten in the cache checkout, is refused on the empty-status and committed routes in both modes, the remote untouched; controls refused for the existing reason"
    requirement: SEC-04
    verification:
      - kind: integration
        ref: "src/sync/mod.rs#test_direct_push_no_changes_refuses_blob_not_matching_its_id"
        status: pass
      - kind: integration
        ref: "src/sync/mod.rs#test_pr_push_no_changes_refuses_blob_not_matching_its_id"
        status: pass
      - kind: integration
        ref: "src/sync/mod.rs#test_direct_push_refuses_unchanged_blob_not_matching_its_id"
        status: pass
      - kind: integration
        ref: "src/sync/mod.rs#test_pr_push_refuses_unchanged_blob_not_matching_its_id"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_committed_gate_refuses_changed_blob_not_matching_its_id"
        status: pass
    human_judgment: false
  - id: D2
    description: "A subtree rewritten above a recorded or deleted path is refused on both routes in both modes, the remote untouched; controls refused for the existing reason"
    requirement: SEC-04
    verification:
      - kind: integration
        ref: "src/sync/mod.rs#test_direct_push_no_changes_refuses_subtree_not_matching_its_id"
        status: pass
      - kind: integration
        ref: "src/sync/mod.rs#test_pr_push_no_changes_refuses_subtree_not_matching_its_id"
        status: pass
      - kind: integration
        ref: "src/sync/mod.rs#test_direct_push_no_changes_refuses_subtree_hiding_a_deleted_path"
        status: pass
      - kind: integration
        ref: "src/sync/mod.rs#test_pr_push_no_changes_refuses_subtree_hiding_a_deleted_path"
        status: pass
      - kind: integration
        ref: "src/sync/mod.rs#test_direct_push_refuses_unchanged_subtree_not_matching_its_id"
        status: pass
      - kind: integration
        ref: "src/sync/mod.rs#test_pr_push_refuses_unchanged_subtree_not_matching_its_id"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_listed_tree_ids"
        status: pass
    human_judgment: false
  - id: D3
    description: "The id check: genuine blob and tree read Ok, other bytes under an id refused, unreadable ids refused, SHA-256 ids verify, an abbreviated id is refused, nothing is written"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_read_verified_object_checks_bytes_against_the_id"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_read_verified_object_refuses_unreadable_ids"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_read_verified_object_in_sha256_repository"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_read_verified_object_refuses_an_abbreviated_id"
        status: pass
    human_judgment: false
  - id: D4
    description: "process::run_bounded_with_input feeds stdin and returns when the child ignores it"
    verification:
      - kind: unit
        ref: "src/process.rs#test_run_bounded_with_input_feeds_stdin"
        status: pass
      - kind: unit
        ref: "src/process.rs#test_run_bounded_with_input_returns_when_the_child_ignores_stdin"
        status: pass
    human_judgment: false
  - id: D5
    description: "A rewritten fetched root tree is refused in both modes (git's own check, pinned) and the legitimate pushes still pass"
    requirement: SEC-04
    verification:
      - kind: integration
        ref: "src/sync/mod.rs#test_direct_push_refuses_fetched_root_tree_not_matching_its_id"
        status: pass
      - kind: integration
        ref: "src/sync/mod.rs#test_pr_push_refuses_fetched_root_tree_not_matching_its_id"
        status: pass
      - kind: integration
        ref: "cargo test (full suite: 437 bin tests passed, 1 ignored; 114 e2e passed)"
        status: pass
    human_judgment: false

duration: 19min
completed: 2026-11-04
status: complete
---

# Phase 02 Plan 22: Object-id check on both push routes Summary

**Every blob, and every tree above a recorded or deleted path, that the committed and empty-status routes read is now hashed with `git hash-object` in the cache checkout and compared whole with its id, so a rewritten loose object no longer reads as the record (review CR-400, verification gaps 1 and 2).**

## Performance

- **Duration:** 19 min
- **Started:** 2026-11-04T14:12:45Z
- **Completed:** 2026-11-04T14:32:00Z
- **Tasks:** 3
- **Files modified:** 2

## Accomplishments

- Gap 1 (the committed route, DEF-11) and gap 2 (the empty-status route, DEF-12) are closed in both modes. Each was first reproduced as Ok, as the verifier reproduced it, and is now refused with the remote untouched.
- `read_verified_object` is the single read both routes use. It runs `cat-file -t`, then `cat-file <kind>`, then `hash-object -t <kind> --literally --no-filters --stdin` in the checkout's own object format, and compares the whole id. It passes no write flag.
- The trees above each recorded path, and above each staged deletion, are checked once per id per call. A rewritten subtree can no longer stand in for the record or hide a deletion.
- A refusal text of its own, so an id mismatch is never reported as "does not hold the validated content".

## Task Commits

1. **Task 1 (tracer): blobs on both routes, both modes**: `91446c2` (feat)
2. **Task 2: the trees above recorded and deleted paths**: `3abb023` (feat)
3. **Task 3: root-tree pin rows and the plan's gates**: `efe86b6` (test)

The plan says "Do not commit RED", so the RED rows of Tasks 1 and 2 were run uncommitted and then committed together with their fix. See TDD Gate Compliance.

## Files Created/Modified

- `src/process.rs`:
  - `run_bounded_with_input`. `spawn_prepared` takes `input: Option<Vec<u8>>`; the writer runs on a detached thread.
  - Tests: `hash_object_stdin`, `test_run_bounded_with_input_feeds_stdin`, `test_run_bounded_with_input_returns_when_the_child_ignores_stdin`.
- `src/sync/mod.rs`, production:
  - New: `run_local_with_input`, `read_verified_object`, `enum ObjectDefect`, `enum NotHeld`, `const OBJECT_ID_MISMATCH`, `fn listed_tree_ids`, `verify_ancestor_trees`.
  - Changed: `read_blob_by_id` now returns a Result through the check. `first_path_not_held` returns `(path, NotHeld)` and takes `listed_trees`. `require_validated_content` takes `listed_trees`. The changed-blob arm and the staged-path loop gain the check. Doc comments are updated.
- `src/sync/mod.rs`, tests:
  - Helpers: `push_after_step` (`push_with_empty_status` now calls it), `ID_MISMATCH`, `ADDED_TEMPLATE`, `forge_loose_object`, `remote_object_matches_id`, `seed_blob_id`, `hide_hooked_template`, `hide_hooked_template_beside_added`, `forge_seed_blob`, `assert_push_refused`, `object_exists`, `GENERIC_REL`, `prime_cache_tree`, `forge_generic_tree`, `generic_listing_of_validated_blob`, `generic_tree_id`.
  - Tests: the 18 names listed in the plan's artifacts.
  - `test_committed_gate_requires_validated_content_refuses_tag_object` now passes an empty `listed_trees` map, as the plan prescribes.

## Refusal format strings (Task 1 acceptance)

- Changed-blob loop and `require_validated_content`: `"Post-commit publication gate violation: an object read for {} {OBJECT_ID_MISMATCH}"`, with `p.display()` (the changed entry, already on the allowlist) or the record's `path.display()`.
- `require_fetched_head_holds`: `"{PREFIX} an object read for {} {OBJECT_ID_MISMATCH}"`, with the record's path (from first_path_not_held) or the staged path `rel` (from the staged-path loop).
- `OBJECT_ID_MISMATCH` = `"does not match its object id in the cache checkout; an object file there was most likely rewritten"`.
- An Unreadable result in the staged-path loop reuses the existing `"{PREFIX} the fetched head's tree cannot be listed"`.

None of these prints a tree name, an object id or object bytes.

## RED Evidence

The records are `target/gsd-red/02-22-t{1,2}-*.json`, which is gitignored. Each was built from the nextest JUnit report of its run (`02-22-t1-red.junit.xml`, `02-22-t2-red.junit.xml`) and classified by `gsd-tools check tdd-red-evidence`. All 11 returned `RED_EVIDENCE_OK / target_test_failed`.

Semantic assessment: each target ran and failed on its own refusal assertion, with Ok where Err was expected. No row failed in a fixture step. In the `cat-file` and `ls-tree` assertions, which come before the refusal assertion, the forgery is read. So these runs show the forged object read as the record.

**Task 1**, on the unfixed code (f0c7f65 plus the test edits), exit 100, 5 of 5 failed:
- test_direct_push_no_changes_refuses_blob_not_matching_its_id: `expected a hard push failure, got Ok("No changes to push to remote repository.")`
- test_pr_push_no_changes_refuses_blob_not_matching_its_id: `expected a hard push failure, got Ok("No changes to push to remote repository.")`
- test_direct_push_refuses_unchanged_blob_not_matching_its_id: `expected a hard push failure, got Ok("Directly pushed changes to remote branch 'main'.")`
- test_pr_push_refuses_unchanged_blob_not_matching_its_id: `expected a hard push failure, got Ok("Pushed changes to branch 'ude-sync-14f11ae4-1793801773'. Create a PR on GitHub to merge into 'main'.")`
- test_committed_gate_refuses_changed_blob_not_matching_its_id: `a blob whose bytes do not hash to its id must be refused: ()`. The opposite direction, the genuine blob, had passed just before.

**Task 2**, on Task 1's code, exit 100, 6 of 6 failed:
- test_direct_push_no_changes_refuses_subtree_not_matching_its_id and test_pr_push_no_changes_refuses_subtree_not_matching_its_id: `expected a hard push failure, got Ok("No changes to push to remote repository.")`
- test_direct_push_no_changes_refuses_subtree_hiding_a_deleted_path and test_pr_push_no_changes_refuses_subtree_hiding_a_deleted_path: `expected a hard push failure, got Ok("No changes to push to remote repository.")`
- test_direct_push_refuses_unchanged_subtree_not_matching_its_id: `expected a hard push failure, got Ok("Directly pushed changes to remote branch 'main'.")`
- test_pr_push_refuses_unchanged_subtree_not_matching_its_id: `expected a hard push failure, got Ok("Pushed changes to branch 'ude-sync-14f11ae4-1793802252'. Create a PR on GitHub to merge into 'main'.")`

The staged gate did not refuse any of the tree rows first, so the `write-tree` priming worked as the plan measured. No fixture fix was needed.

**Passed on first run, as the plan intends.** These are tests of new functions or pins, not RED:
- The four `test_read_verified_object_*` tests and the two `test_run_bounded_with_input_*` tests, written beside the new functions.
- `test_listed_tree_ids`.
- The two root-tree rows (Task 3). They pin git's own check, they are not RED. A one-off probe printed the refusal (the probe line was removed before the commit): `git status failed in remote checkout ... exit status Some(128): error: hash mismatch <root id> | error: bad tree object HEAD`. So git refuses at `git status`, before either route's `ls-tree`. The plan's measurement named `ls-tree`; both are git's own check.

## Verification

- Task 1: verify 1 (11 names, each listed exactly once) passed 11/11. Verify 2 (`sync::tests::` and `process::tests::`) passed 332/332. Verify 3 (`"hash-object"` once in production code, no `"-w"` there while the whole file has one or more, `run_bounded_with_input` present, Cargo.toml and Cargo.lock unchanged from 4f9f043) exited 0.
- Task 2: verify 1 (18 names) passed 18/18. Verify 2 (each definition once, each call on exactly two lines) exited 0. Verify 3 (the modules) passed 339/339.
- Task 3: verify 1 (10 names) passed 10/10. Verify 2 (`cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && timeout -k 30 1800 cargo test && git diff --exit-code 4f9f043 -- Cargo.toml Cargo.lock`) exited 0, as the gate of commit efe86b6. Verify 3 (nothing uncommitted outside `.planning`; src/sanitizer, src/template and docs unchanged from 4f9f043) exited 0.
- Every commit ran behind `cargo fmt --check && cargo clippy ... -D warnings && timeout -k 30 1800 cargo test && git commit`. The last full suite: 437 bin tests passed (1 ignored) and 114 e2e tests passed.
- Hermeticity: after every run, `git status --porcelain` showed only the pre-existing `.planning/STATE.md` and `.planning/state.json` edits, and `git config --local --list | wc -l` stayed 22. TMPDIR pointed to the session scratchpad throughout.

What these checks do not establish:
- An object file swapped between two of the gate's reads (between `ls-tree` and the check). That is NOT-10's during-the-push class, which is pending the operator.
- A forged packed object. It was not run; the check is reasoned to hold because it hashes whatever `cat-file` returns.
- A forgery made by an installed hook through `ude sync push`. The rows drive the engine.
- Hosted remotes, https, ssh and Windows.
- The rows control the id check by running on the code without it (the RED runs). The controls on the final code, which remove each part of the fix, are plan 02-23's.

## Decisions Made

- `read_verified_object` compares hash-object's stdout, with one trailing LF stripped, byte for byte with the id. Output without that LF is compared as is, so it can only mismatch, never pass by trimming.
- The staged-path loop keeps one `verified` set for all staged paths, separate from first_path_not_held's set. A tree shared by several deletions is hashed once.
- `validate_committed_tree`'s doc now also says that each changed blob is checked against its id (Task 1's change), alongside the tree check the plan asked to add.

## Deviations from Plan

None. The plan was executed as written.

Observation, not a deviation: in the root-tree rows git refuses at `git status` (hash mismatch, exit 128), not at `ls-tree`. The rows assert no refusal text, as planned.

## TDD Gate Compliance

`git log` shows feat(02-22) 91446c2 and 3abb023 before the only test(02-22) commit, efe86b6. A log-order check will therefore report this plan as GREEN before RED. The cause is the plan itself: "Do not commit RED" (Task 1 action and the plan's verification). Every commit had to pass the full suite, so RED rows could not be committed alone. The RED discipline was kept by running each RED set uncommitted on the pre-fix code, with JUnit-backed records classified `RED_EVIDENCE_OK` (see RED Evidence). Plan 02-23 re-runs the controls on the final code.

## Threat Flags

| Flag | File | Description |
|------|------|-------------|
| T-02-89 (mitigate) | src/sync/mod.rs | A rewritten blob or subtree in the cache checkout read as the record. Mitigated: read_verified_object on each blob and each tree above a recorded or deleted path. Rows in both modes on both routes, each with a control, and RED before each part of the fix. 02-23's controls remove each part on the final code |
| T-02-90 (mitigate) | src/sync/mod.rs | The check refusing a legitimate push. Mitigated: stdin with `--no-filters` and no `--path`, `--literally`, the repository's own git, the SHA-256 unit test. The no-change, deletion, benign-hook and gc'd-remote rows still pass, and the full suite is green |
| T-02-91 (mitigate) | src/process.rs | A child that never reads stdin holding the push open. Mitigated: the writer is detached and the call stays under the deadline and group kill. test_run_bounded_with_input_returns_when_the_child_ignores_stdin (4 MiB to `git --version`) |
| T-02-92 (mitigate) | src/sync/mod.rs (tests) | A forgery written through a hardlink into the bare remote. Mitigated: forge_loose_object removes the target file first, and every forged row asserts remote_object_matches_id on the remote's object |
| T-02-93 (mitigate) | src/sync/mod.rs | A new refusal printing bytes a hook chose. Mitigated: the refusals name only a path from the record, the staged list or an allowlisted changed entry, plus fixed text |

No new security surface beyond the threat model: `hash-object` runs in the cache checkout with no write flag, and nothing is added to the network or the filesystem.

## Known Stubs

None.

## Issues Encountered

None.

## User Setup Required

None. No external service configuration is required.

## Next Phase Readiness

- Ready for 02-23, which owns docs/ARCHITECTURE.md (promise 2 and the DEF-11 and DEF-12 rows), the controls on the final code and the phase gates.
- Still open and put to the operator by the orchestrator: the base-side channel under the plan's `## Decisions and assumptions recorded here`. A base-side tree whose stored bytes do not match its id can hide an added file from the staged diff and from diff-tree. This plan does not close it; `read_verified_object` is the check that would apply to those reads.

## Self-Check: PASSED

- FOUND: src/process.rs, src/sync/mod.rs. `run_bounded_with_input`, `read_verified_object`, `listed_tree_ids` and `verify_ancestor_trees` are each defined once.
- FOUND: 91446c2, 3abb023, efe86b6 (all ancestors of HEAD). `check evaluation-scope --plan 02-22 --commits-only` resolved all three.

---
*Phase: 02-sanitizer-boundaries-security-hardening*
*Completed: 2026-11-04*
