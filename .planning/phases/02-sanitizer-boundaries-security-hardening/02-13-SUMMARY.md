---
phase: 02-sanitizer-boundaries-security-hardening
plan: 13
subsystem: sync
tags: [rust, sync, push, git-hooks, publication-gate, tree-binding, gap-closure]

requires:
  - phase: 02-sanitizer-boundaries-security-hardening (02-12)
    provides: "validate_committed_tree with raw reads, the empty-directory listing, push_with_hook and the hook test helpers"
provides:
  - "The committed gate requires the raw commit's tree line to equal the staged tree id byte for byte, in direct and PR mode"
  - "Each listed blob is read by its own object id, and a listed id that is not a blob is refused"
  - "Repeated paths and path components that are empty, `.` or `..` are refused from the raw listing"
  - "Every path in the validated record must hold its recorded content as a regular file in the commit"
  - "The committed gate's refusals name no hook-chosen text"
affects: [sync push, phase-02 verification gaps 1 2 4 5, review CR-200 CR-201 WR-200, SEC-04, SEC-02, plan 02-14, plan 02-15]

actuals:
  tokens: 23689     # chars/4 over the realized src/ diff (94755 chars, git diff 47738c7..a44ae05)
  tasks: 2
  commits: 6
plan_head_before: 47738c721e751567d41e702d17942bf596bb587f
plan_head_after: a44ae0594f5225d348209247ab21f31eab728fc6

tech-stack:
  added: []
  patterns:
    - "A tree id recorded after the staged gate and compared with the raw commit binds everything a hook does after that point, with one comparison"
    - "A core.fsmonitor script reading a prepared tree into the index during `git show :<path>` reaches the window between the staged gate's read and `git write-tree` deterministically"

key-files:
  created: []
  modified:
    - src/sync/mod.rs

key-decisions:
  - "02-13: stage_publication records the staged tree id with git write-tree, before PR mode's checkout, and the committed gate requires the raw commit's first line to hold that id byte for byte, as its last check"
  - "02-13: the committed gate reads each listed blob by its own id, refuses a repeated path or a bad path component from the raw listing, and requires every recorded path to hold its recorded content; no separate refusal of a D entry, so a deletion with no record still publishes"
  - "02-13: read_blob_by_id refuses an object that is not a blob, because cat-file blob peels a tag; added during execution after a probe published a tag object through the real push in both modes"

patterns-established:
  - "A refusal in the committed gate prints a path only after it passed the allowlist"

requirements-completed: [SEC-04, SEC-02]

coverage:
  - id: D1
    description: "A hook that changes the tree after its id is recorded is refused in direct and PR mode; a hook that keeps the content still publishes"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_direct_push_refuses_hook_exec_flip"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_pr_push_refuses_hook_exec_flip"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_pr_push_refuses_post_checkout_index_change"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_committed_gate_requires_the_staged_tree"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_direct_push_publishes_with_content_kept_amend"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_pr_push_publishes_with_benign_post_checkout_hook"
        status: pass
    human_judgment: false
  - id: D2
    description: "A repeated path or directory never publishes and no blob reaches the remote unread, through a post-commit hook and through a post-index-change hook whose tree id matches the commit"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_direct_push_refuses_hook_duplicate_entry"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_pr_push_refuses_hook_duplicate_directory"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_direct_push_refuses_index_hook_hidden_blob"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_pr_push_refuses_index_hook_hidden_blob"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_tree_listing_blobs"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#prop_raw_diff_entries_round_trips"
        status: pass
    human_judgment: false
  - id: D3
    description: "A token-shaped second entry is refused and rolled back, and its body is not printed (ROADMAP SC2's committed-blob channel)"
    requirement: SEC-02
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_direct_push_refuses_hook_duplicate_token_entry"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_pr_push_refuses_hook_duplicate_token_entry"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_direct_push_refuses_index_hook_hidden_token"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_pr_push_refuses_index_hook_hidden_token"
        status: pass
    human_judgment: false
  - id: D4
    description: "A push returning Ok means every validated path holds its recorded content; a deletion nothing validated still publishes"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_committed_gate_requires_validated_content"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_direct_push_refuses_hook_deleted_file"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_pr_push_refuses_hook_reverted_file"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_pr_push_refuses_index_hook_removed_file"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_pr_push_publishes_deletion"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_push_to_remote_deletion"
        status: pass
    human_judgment: false
  - id: D5
    description: "A path component that is empty, `.` or `..` never publishes, and `git clone` of the same commit fails with `invalid path`"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_direct_push_refuses_hook_dot_subtree"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_pr_push_refuses_hook_dot_subtree"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_committed_gate_refuses_bad_path_components"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#prop_tree_listing_blobs_round_trips"
        status: pass
    human_judgment: false
  - id: D6
    description: "A tag object listed at a validated path is refused instead of reading as the bytes it tags, including through an index swap after the staged gate's read"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_committed_gate_refuses_tag_object_at_validated_path"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_direct_push_refuses_index_swap_to_tag_object"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_pr_push_refuses_index_swap_to_tag_object"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_push_publishes_with_index_swap_to_the_staged_tree"
        status: pass
    human_judgment: false

duration: 22min
completed: 2026-11-02
status: complete
---

# Phase 2 Plan 13: Staged-tree binding and tree reads by object id Summary

**The committed gate now binds the published commit to the tree UDE staged and reads that tree properly: each listed object by its own id and only if it is a blob, no repeated path or bad component, and every validated path holding its recorded content. Gaps 1, 2, 4 and 5 are closed pending plan 02-15's controls and re-verification.**

## Read this first

**A prediction did not cover everything: the plan's design as written still published an unread object.** `git cat-file blob <id>` peels an annotated tag, so a tree listing a tag at a validated path read as the validated bytes while the push sent the tag object with its text. A throwaway probe showed the gate returning Ok and the remote holding the tag object, through the real `push_to_remote`, in both modes (a `core.fsmonitor` script swaps the index after the staged gate's last read and before `git write-tree`, so the recorded id matches). This is outside the plan's threat register. Fixed in this plan with RED first: `read_blob_by_id` checks the object type (commits `eb2fd06`, `a44ae05`). See Deviations.

**What this does not establish.**

- The tag channel was found by a probe aimed at `cat-file blob`, not by any test the plan listed; every listed test was green while it was open. So green hook tests here are not evidence that the object channels are exhausted.
- Every hook test is one run on git 2.56.0 on Linux against a local bare remote. No hosted remote and no Windows run. Whether a hosted remote's fsck refuses these trees was not tested and is not relied on.
- The `test_*_index_swap_*` tests read `/proc/$PPID/cmdline`, so they are `#[cfg(target_os = "linux")]`.
- A hand-crafted binary index was not tried. The `core.fsmonitor` swap reaches the same window, but only through trees `git read-tree` accepts.
- A process that keeps running and swaps the index at another moment was not run; only the deterministic `git show` moment was.
- The Gitleaks category coverage in ROADMAP SC2 comes from plans 02-05 and 02-06 and was not re-tested beyond a GitHub PAT shape.
- Per-check controls and the mutation run are plan 02-15's. No mutation run was done here.
- Grafts (gap 3) is plan 02-14's. T-02-52 stays the recorded exception until plan 02-14's tests hold.

## Performance

- **Duration:** 22 min
- **Started:** 2026-11-02T16:22:39Z
- **Completed:** 2026-11-02T16:45:03Z
- **Tasks:** 2
- **Files modified:** 1

## Accomplishments

- The tree id is recorded once, in `stage_publication`, before PR mode's `git checkout -b`, and compared last in the gate. A failed record resets the index and refuses.
- The gate lists changes with `diff-tree -r -z --raw` and reads each entry by id, so a repeated name cannot hide a blob; a token-shaped second entry is refused with its body unprinted and the checkout back at the fetched head.
- `tree_listing_blobs` refuses repeated paths and empty, `.` or `..` components as raw bytes; `require_validated_content` covers deletion, revert, symlink and gitlink with one check.
- The gate's two name-bearing refusals are fixed text; `test_push_refuses_hook_added_file` and `test_push_refuses_hook_replaced_tree` assert `hook-extra` is absent.
- Benign hook, content-kept amend, benign post-checkout, CRLF under a `text` attribute, pass-through filter and no-record deletion still publish in both modes.

## Task Commits

1. **Task 1: Tracer - bind the published commit to the tree UDE staged**
   - RED `063897c` test(02-13): add failing tests for commits a hook changed after staging
   - GREEN `b9c03ce` feat(02-13): bind the published commit to the tree UDE staged
2. **Task 2: Validate the staged tree itself**
   - RED `8c9e8b0` test(02-13): add failing tests for what reaches the staged tree
   - GREEN `a11d126` feat(02-13): read the staged tree by object id and require held content
3. **Deviation: tag object at a validated path**
   - RED `eb2fd06` test(02-13): add failing tests for a tag object at a validated path
   - GREEN `a44ae05` fix(02-13): refuse a tag object listed at a file's place

The tracer gate ran after Task 1 (`human_verify_mode` end-of-phase, automated-only verify): the 36-name command re-ran green before Task 2 started.

## RED Evidence

Records are in `target/gsd-red/02-13-t1-*.json`, `02-13-t2-*.json` and `02-13-t3-*.json` (gitignored), built from the nextest JUnit report. All 33 were classified `RED_EVIDENCE_OK` / `target_test_failed` by `gsd-tools check tdd-red-evidence`. No hook needed a fix: no refuse-test passed at RED.

**Task 1, 27 tests: 17 failed, 10 passed, as predicted.** Every failure is at `assert_remote_unchanged`, after the `hook_ran` assertion passed. The failure line for each direct test is `assertion left == right failed: the remote changed; push result: Ok("Directly pushed changes to remote branch 'main'.")`, and for each PR test the same with `Ok("Pushed changes to branch 'ude-sync-…'. Create a PR on GitHub to merge into 'main'.")`:

`test_direct_push_refuses_hook_duplicate_entry`, `test_pr_push_refuses_hook_duplicate_entry`, `test_direct_push_refuses_hook_duplicate_token_entry`, `test_pr_push_refuses_hook_duplicate_token_entry`, `test_direct_push_refuses_hook_duplicate_directory`, `test_pr_push_refuses_hook_duplicate_directory`, `test_direct_push_refuses_hook_repeated_same_blob`, `test_pr_push_refuses_hook_repeated_same_blob`, `test_direct_push_refuses_hook_dot_subtree`, `test_pr_push_refuses_hook_dot_subtree`, `test_direct_push_refuses_hook_deleted_file`, `test_pr_push_refuses_hook_deleted_file`, `test_direct_push_refuses_hook_reverted_file`, `test_pr_push_refuses_hook_reverted_file`, `test_direct_push_refuses_hook_exec_flip`, `test_pr_push_refuses_hook_exec_flip`, `test_pr_push_refuses_post_checkout_index_change`.

Passed at RED as expected: `test_direct_push_publishes_with_content_kept_amend`, `test_pr_push_publishes_with_content_kept_amend`, `test_pr_push_publishes_with_benign_post_checkout_hook`, `test_direct_push_publishes_crlf_file_under_text_attribute`, `test_pr_push_publishes_crlf_file_under_text_attribute`, `test_direct_push_publishes_with_pass_through_clean_filter`, `test_pr_push_publishes_with_pass_through_clean_filter`, `test_direct_push_refuses_rewriting_clean_filter`, `test_pr_push_refuses_rewriting_clean_filter`, `test_pr_push_publishes_deletion`.

**Task 2, 17 tests run (the filter `test_push_refuses_hook_replaced_tree` also matches `..._hiding_empty_directory`): 13 failed, 4 passed, as predicted.** This RED is what shows the binding alone is not enough: the recorded id equals the commit's tree in the index-hook tests and the push still returned Ok.

| Test | Failure line |
|------|--------------|
| `test_direct_push_refuses_index_hook_hidden_blob`, `..._hidden_token`, `..._repeated_same_blob`, `..._removed_file`, `test_push_refuses_index_hook_symlink` | `the remote changed; push result: Ok("Directly pushed changes to remote branch 'main'.")` |
| `test_pr_push_refuses_index_hook_hidden_blob`, `..._hidden_token`, `..._repeated_same_blob`, `..._removed_file` | `the remote changed; push result: Ok("Pushed changes to branch 'ude-sync-…'…")` |
| `test_push_refuses_hook_added_file`, `test_push_refuses_hook_replaced_tree` | `Post-commit publication gate violation: unvalidated file committed in HEAD: templates/hook-extra.txt` (the name was still printed) |
| `test_committed_gate_requires_validated_content` | `g deleted: Ok(())`, `g reverted to the base's blob: Ok(())`, `g a symlink entry holding the validated blob: Ok(())`, `g a gitlink: Err("…committed HEAD blob templates/g differs from the validated content")` |
| `test_committed_gate_refuses_bad_path_components` | `subtree named ".": Ok(()), expected "path component"` |

Passed at RED as expected: `test_push_refuses_index_hook_empty_directory`, `test_index_hook_exec_flip_reaches_the_remote`, `test_committed_gate_reads_blob_past_replace_ref`, and the prefix match `test_push_refuses_hook_replaced_tree_hiding_empty_directory`.

**Deviation, 5 tests: 3 failed, 2 passed.**

| Test | Failure line |
|------|--------------|
| `test_committed_gate_refuses_tag_object_at_validated_path` | `a tag object at a validated path must be refused: ()` |
| `test_direct_push_refuses_index_swap_to_tag_object` | `the remote changed; push result: Ok("Directly pushed changes to remote branch 'main'.")` |
| `test_pr_push_refuses_index_swap_to_tag_object` | `the remote changed; push result: Ok("Pushed changes to branch 'ude-sync-…'…")` |

Passed at RED as expected: `test_push_refuses_index_swap_to_injected_blob` (the same route with a plain blob is refused by the read by id) and `test_push_publishes_with_index_swap_to_the_staged_tree` (the route alone refuses nothing).

## GREEN Evidence

After `a44ae05`: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and full `cargo test` exit 0 (319 unit passed, 88 e2e passed, 1 ignored: `test_dump_parity_corpus`, which this plan did not touch). `Cargo.toml` and `Cargo.lock` unchanged. Each GREEN commit was gated on that chain's exit code in one command; RED commits on fmt and clippy, because their tests fail by design. The repository's `.git/config` hash and its empty index were unchanged after each e2e run, and TMPDIR was never set.

## Plan predictions checked

A throwaway probe test ran each row of the plan's engine table in both modes, and each odd fetched head from fresh and existing caches, through the real push. Removed before commit with a sha256 check.

**Matched, every row:** post-commit benign and kept amend Ok; duplicate V, I and repeated directory `differs from the validated content`; duplicate V, V `repeats a path`; duplicate token `secret detected in committed HEAD blob`; subtree `.` `path component`; delete and revert `does not hold the validated content`; exec flip `is not the tree UDE staged`; post-index-change V, I, V `differs`; V, V `repeats a path`; V, I at the staged gate; token `secret detected`; empty directory `contains an empty directory`; `git rm --cached` and symlink `does not hold`; exec flip **Ok, mode 100755**; prepare-commit-msg exec flip Ok; post-checkout `git rm --cached` `does not hold`. Odd heads: repeated path in an unchanged sibling refused at the staged gate from a fresh cache and with `repeats a path` from an existing one; managed file listed twice Ok from both.

**Did not hold or needs a qualifier:**

1. **Tag objects.** "Each listed blob is read by its own object id" with `cat-file blob` alone left the tag channel open. Fixed. Tag at a *tree* position does not have this problem: `ls-tree` exits 1 (`Object … not a tree`), so the gate refuses, and a plain push fails too (probed once).
2. **`core.fsmonitor` is real, not only reasoned.** T-02-58 lists it as "reasoned, not run". It ran: a script swaps the index between the staged gate's last read and `git write-tree`, and the recorded id then matches. A plain injected blob by that route is refused by the read by id, and the tests above pin that.
3. **Gitlink row at RED.** The plan named the deleted, reverted and symlink rows as failing "at least". At RED the gitlink row failed too, with `differs from the validated content`, because `git show <commit>:<path>` printed the commit object the gitlink named. At GREEN it gives `failed to inspect committed blob` as written.
4. **Empty directory in an unchanged sibling, qualifier.** The refusal from an existing cache holds when the sibling also has a file (probed: Ok from fresh, `contains an empty directory` from existing). When the sibling directory is wholly empty, both caches publish a repaired tree (probed: Ok, Ok). The plan's text does not separate the two.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug / Rule 2 - Missing critical] A tag object at a validated path read as the bytes it tags**
- **Found during:** a probe after Task 2's GREEN, aimed at `cat-file blob` peeling.
- **Issue:** `cat-file blob <tag>` returns the tagged blob's bytes and `ls-tree` reports the entry as `blob` from its mode. So the secret scan, the record comparison and the held-content check all passed while the push sent the tag object with text nothing read. Reproduced through `push_to_remote` in both modes.
- **Fix:** `read_blob_by_id` runs `git --no-replace-objects cat-file -t <id>` first and returns `None` for anything but `blob`, which gives the existing `failed to inspect committed blob` refusal. The must-have's `cat-file blob` read is kept.
- **Tests:** `test_committed_gate_refuses_tag_object_at_validated_path` (with the opposite direction: the plain read peels, a plain push delivers the tag), `test_direct_push_refuses_index_swap_to_tag_object`, `test_pr_push_refuses_index_swap_to_tag_object`, `test_push_refuses_index_swap_to_injected_blob`, `test_push_publishes_with_index_swap_to_the_staged_tree`.
- **Files modified:** src/sync/mod.rs
- **Committed in:** eb2fd06, a44ae05

**2. [Plan shape] Helpers beyond the plan's artifact table**
- `require_validated_content` (the plan allowed a private method), `split_at_nul`, and test helpers `post_commit_hook_refusal`, `index_hook_refusal`, `git_with_input`, `write_blob`, `commit_of_templates`, `templates_repo`, `templates_record`, `raw_diff_record`, `tree_listing_record`, `listed_path_strategy`, `literal_tree`, `tag_on_blob`, `push_with_index_swap`, `root_over_generic`, and the constant `HOOK_TOKEN_BODY`.
- `raw_diff_entries` also returns `None` for an empty path and for an extra field in a record, and `tree_listing_blobs` returns `Unparseable` for an extra metadata field; both are fail-closed additions with table rows.

**Total deviations:** 1 auto-fixed security bug, 1 note on helper shape.
**Impact on plan:** the fix is one type check and five tests; no test was weakened and no scope was removed.

## Issues Encountered

- `clippy::type_complexity` on a test table's annotation at the first gate; the annotation was removed, and the commit was gated on the rerun.
- A first version of my scratch evidence script mis-parsed self-closed JUnit cases and named four records after passing tests; I deleted those records and rebuilt all of them after fixing the parser.

## For plan 02-15

- `read_blob_by_id` now has two `--no-replace-objects` reads and a new type check. Control row 8 ("drop `--no-replace-objects`") names the function, not which read: it must edit the `cat-file blob` line for its expected tests to fail.
- The type check has no control row in plan 02-15's list of 23. The tag tests above fail if it is removed (they failed before it existed), and the mutation run's scope already includes `read_blob_by_id`. Whether to add a row is that plan's call.

## Threat Flags

| Flag | File | Description |
|------|------|-------------|
| threat_flag: T-02-60 | src/sync/mod.rs | pending the operator, not accepted |
| threat_flag: T-02-61 | src/sync/mod.rs | pending the operator, not accepted |
| threat_flag: tag-object-at-blob-position | src/sync/mod.rs | Not in this plan's register. Found by probe, closed in this plan with tests in both modes. T-02-58's `core.fsmonitor` route also ran for the first time here |

Status when this SUMMARY was written; ## Operator answers in 02-15-PLAN.md holds the current one.

## TDD Gate Compliance

RED `063897c` precedes GREEN `b9c03ce`; RED `8c9e8b0` precedes GREEN `a11d126`; RED `eb2fd06` precedes GREEN `a44ae05`. No REFACTOR commit.

## User Setup Required

None.

## Next Phase Readiness

Plan 02-14 (grafts file written after the gate) and plan 02-15 (controls and mutation run) are next. Gaps 1, 2, 4 and 5 are closed pending plan 02-15's controls and re-verification; `requirements-completed` above copies the plan's frontmatter as the template asks and is not a verdict.

## Self-Check: PASSED

src/sync/mod.rs exists; 063897c, b9c03ce, 8c9e8b0, a11d126, eb2fd06 and a44ae05 are ancestors of HEAD (`git merge-base --is-ancestor`, checked before writing this file). `git rev-list --count 47738c7..a44ae05` is 6.

---
*Phase: 02-sanitizer-boundaries-security-hardening*
*Completed: 2026-11-02*
