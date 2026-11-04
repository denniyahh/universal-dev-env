---
phase: 02-sanitizer-boundaries-security-hardening
plan: 21
subsystem: sync
tags: [rust, sync, push, publication-gate, controls, mutation-testing, coverage, gap-closure]

requires:
  - phase: 02-sanitizer-boundaries-security-hardening (02-16 to 02-19)
    provides: "the push source no ref can name, the base checked against git ls-remote, the push to the caller's URL with its read-back, the empty-status check and the parent-free continuity refusal"
  - phase: 02-sanitizer-boundaries-security-hardening (02-20)
    provides: "the closed list in docs/ARCHITECTURE.md section 3"
provides:
  - "A control table on the gate's final code: 46 edits, each removed one at a time, 35 same and 11 corrected (names added only, none dropped)"
  - "A scoped mutation result over 23 gate functions: 122 mutants, 103 caught, 0 missed, 0 timeouts, 19 unviable"
  - "Two tests pinning the empty-status deletion check's path comparison inside the mutation run's name filter"
  - "The phase's final gates and a coverage record on the final state"
  - "The hand-edits todo with the 2026-11-02 and 2026-11-03 additions"
affects: [02-re-verification, 02-SECURITY, 02-VALIDATION, 02-REVIEW-DISPOSITION]

actuals:
  tokens: 2145    # chars/4: test diff 623 + todo diff 1522; this SUMMARY is not counted
  tasks: 3
  commits: 1      # git rev-list --count plan_head_before..plan_head_after, measured before this SUMMARY's commit
plan_head_before: 5ce1beec235a0307694405cbf7fa24367d8146cf
plan_head_after: 8570971756c87056993a956d7f623e00bf408a0f

tech-stack:
  added: []
  patterns:
    - "Each control edit is a scripted exact-once replacement confined to a named region before `mod tests {`, applied, tested, restored from a copy and checked with cmp"
    - "A mutant missed under a name-listed filter is killed by a test whose name contains a listed name, so the plan's command stays as written"

key-files:
  created:
    - .planning/phases/02-sanitizer-boundaries-security-hardening/02-21-SUMMARY.md
  modified:
    - src/sync/mod.rs
    - .planning/todos/pending/2026-11-01-hand-edits-owed-for-phase-02-after-re-verification.md

key-decisions:
  - "02-21: 11 control rows recorded corrected, each adding names to the table's set and dropping none; every added test fails on its own assertion and the code was judged right, so no restart and no T-02-88"
  - "02-21: the one missed mutant (== to != in require_fetched_head_holds' staged-deletion comparison) was killed by two tests whose names contain the filter's no-change names, not by changing the plan's command; test_require_fetched_head_holds_edges already killed it but is outside the filter"

patterns-established:
  - "A function added after a mutation filter's name list was written needs its unit test in the filter or a filter-selected sibling; re-check the list against the SUMMARYs before trusting a clean run"

requirements-completed: [SEC-04, SEC-02]

coverage:
  - id: D1
    description: "Each of the 46 control edits fails at least one test, exactly the table's set (35 rows) or the table's set plus recorded additions (11 rows), with exit 100 and no compile error"
    requirement: SEC-04
    verification:
      - kind: other
        ref: "Task 1's first verify over target/controls-02-21.tsv and target/controls-02-21-rows (same=35 corrected=11, exit 0)"
        status: pass
    human_judgment: false
  - id: D2
    description: "No mutant survives in the plan's 23 gate functions under the plan's test list"
    requirement: SEC-04
    verification:
      - kind: other
        ref: "cargo mutants -o target/mutants-tree-02-21 (Task 2's first verify), target/mutants-tree-02-21/mutants.out/missed.txt empty"
        status: pass
    human_judgment: false
  - id: D3
    description: "The phase's final gates pass on the final state, with the code before each mod tests line byte-identical to the controls' start"
    requirement: SEC-02
    verification:
      - kind: other
        ref: "Task 2's verify commands 2 to 8 and Task 3's two verifies"
        status: pass
    human_judgment: false
  - id: D4
    description: "Each corrected row's reason is true (the code is right and the table's set was incomplete)"
    requirement: SEC-04
    verification: []
    human_judgment: true
    rationale: "The reasons come from reading each added test's panic site and what the test asserts; no run can show a reason is true (T-02-88's class, though no name was dropped). Re-verification re-runs these rows"

duration: 2h 40m
completed: 2026-11-04
status: complete
---

# Phase 02 Plan 21: Controls, mutation run and final gates on the gate's final state Summary

**All 46 control edits fail tests on the final gate code (35 exactly as the table states, 11 with recorded extra tests and none dropped), the scoped mutation run ends with none missed after one survivor forced two new tests, the final gates pass, and hand edits are owed after re-verification (see the todo).**

## Performance

- **Duration:** 2h 40m (control rows 56 min, two mutation runs 40 min each)
- **Started:** 2026-11-04T03:40:02Z
- **Completed:** 2026-11-04T06:20Z
- **Tasks:** 3
- **Files modified:** 2, plus this SUMMARY

## Accomplishments

- 46 of 46 control rows fail at least one test, each run exiting 100. 35 are `same`. 11 are `corrected`: each failed every test the table names plus one to three more, each on its own assertion.
- The first mutation run missed one mutant, in the comparison inside require_fetched_head_holds' staged-deletion check. Two new tests kill it; the re-run has 0 missed.
- fmt, clippy, full `cargo test`, `cargo deny`, `cargo machete`, the closed list's verify and the hermeticity gate all exit 0 after the last commit. Cargo.toml, Cargo.lock, src/sanitizer and src/template are unchanged.
- The hand edits owed are in `.planning/todos/pending/2026-11-01-hand-edits-owed-for-phase-02-after-re-verification.md`, now with the 2026-11-02 and 2026-11-03 additions.

## Control table

**Counts: 35 `same`, 11 `corrected`, 0 `different`.** Task 1's first verify printed `same=35 corrected=11` and exited 0, after the last restore and again after every Task 2 gate. The rows ran on commit `5ce1bee`, from 03:46 to 04:42 UTC. The later test-only commit `8570971` left the code before each file's `mod tests {` byte-identical, which the hash comparison confirms.

Before the rows, an unedited run of both commands exited 0: 319 unit and 29 e2e tests passed, with no line matching the failure pattern. Counted a second way, the PASS lines were 319 and 29. So the pattern reads no failures on a green suite. Negative control on the verify: the same command over a results file with row 10 relabelled `same` exited 1.

| # | Edit | State | Failing tests |
|---|------|-------|---------------|
| 1 | validate_committed_tree: remove the comparison with the recorded tree id | same | test_committed_gate_requires_the_staged_tree, test_direct_push_refuses_hook_exec_flip, test_pr_push_refuses_hook_exec_flip |
| 2 | Changed-blob loop: pass the bytes of `format!("{commit}:{}", p.display())` to read_blob_by_id in place of the entry's id | same | test_direct_push_refuses_index_hook_hidden_blob, test_direct_push_refuses_index_hook_hidden_token, test_pr_push_refuses_index_hook_hidden_blob, test_pr_push_refuses_index_hook_hidden_token |
| 3 | validate_committed_tree: remove the held-content call | same | test_committed_gate_requires_validated_content, test_direct_push_refuses_index_hook_removed_file, test_pr_push_refuses_index_hook_removed_file, test_push_refuses_index_hook_symlink |
| 4 | tree_listing_blobs: remove the repeated-path refusal | same | prop_tree_listing_blobs_round_trips, test_direct_push_refuses_index_hook_repeated_same_blob, test_pr_push_refuses_index_hook_repeated_same_blob, test_tree_listing_blobs |
| 5 | tree_listing_blobs: remove the path-component refusal | same | prop_tree_listing_blobs_round_trips, test_committed_gate_refuses_bad_path_components, test_tree_listing_blobs |
| 6 | tree_listing_blobs: put every record in the map, not only regular files | same | test_committed_gate_requires_validated_content, test_push_refuses_index_hook_symlink, test_tree_listing_blobs |
| 7 | validate_committed_tree: treat count_empty_directories' result as zero | same | test_push_refuses_hook_empty_directory, test_push_refuses_hook_replaced_tree_hiding_empty_directory, test_push_refuses_index_hook_empty_directory |
| 8 | read_blob_by_id: drop `--no-replace-objects` | same | test_committed_gate_reads_blob_past_replace_ref, test_push_refuses_hook_replaced_blob |
| 9 | Gate's `cat-file commit` read: drop `--no-replace-objects` | same | test_direct_push_ignores_replace_ref_to_a_tag, test_direct_push_publishes_validated_commit_past_replace_ref, test_pr_push_ignores_replace_ref_to_a_tag, test_pr_push_publishes_validated_commit_past_replace_ref, test_push_refuses_hook_merge_replaced_by_validated_commit, test_push_refuses_hook_replaced_message |
| 10 | Gate's `diff-tree` read: drop `--no-replace-objects` | corrected | test_direct_push_ignores_replace_ref_to_a_tag, test_pr_push_ignores_replace_ref_to_a_tag, test_push_refuses_hook_replaced_tree |
| 11 | Gate's `ls-tree` read: drop `--no-replace-objects` | corrected | test_direct_push_ignores_replace_ref_to_a_tag, test_pr_push_ignores_replace_ref_to_a_tag, test_push_refuses_hook_replaced_tree_hiding_empty_directory |
| 12 | push_direct: drop `-c core.commitGraph=false` | same | test_direct_push_ignores_inherited_git_test_commit_graph, test_direct_push_sends_no_object_from_forged_commit_graph, test_push_sends_no_object_from_forged_alternate_commit_graph, test_push_sends_no_object_from_forged_split_commit_graph |
| 13 | push_pr_branch: drop `-c core.commitGraph=false` | same | test_pr_push_ignores_inherited_git_test_commit_graph, test_pr_push_sends_no_object_from_forged_commit_graph |
| 14 | push_direct: drop `-c pack.useBitmaps=false` | same | test_direct_push_sends_no_object_from_graft_built_bitmap, test_push_sends_no_object_from_graft_built_multi_pack_index_bitmap |
| 15 | push_pr_branch: drop `-c pack.useBitmaps=false` | same | test_pr_push_sends_no_object_from_graft_built_bitmap |
| 16 | push_direct: drop the remove_git_test_env call | same | test_direct_push_ignores_inherited_git_test_commit_graph |
| 17 | push_pr_branch: drop the remove_git_test_env call | same | test_pr_push_ignores_inherited_git_test_commit_graph |
| 18 | push_direct: run the push through self.run_network | same | test_cli_direct_push_alone_gets_the_null_grafts_file, test_direct_push_sends_no_object_from_grafts_written_during_push, test_direct_push_sends_no_object_from_pre_push_grafts |
| 19 | push_pr_branch: run the push through self.run_network | same | test_cli_pr_push_alone_gets_the_null_grafts_file, test_pr_push_sends_no_object_from_grafts_written_during_push, test_pr_push_sends_no_object_from_pre_push_grafts |
| 20 | push_direct: drop `-c advice.graftFileDeprecated=false` | same | test_direct_push_failure_excerpt_has_no_grafts_hint |
| 21 | push_pr_branch: drop `-c advice.graftFileDeprecated=false` | same | test_pr_push_failure_excerpt_has_no_grafts_hint |
| 22 | run_bounded_ignoring_grafts: drop the line that sets the variable | same | test_cli_direct_push_alone_gets_the_null_grafts_file, test_cli_pr_push_alone_gets_the_null_grafts_file, test_direct_push_sends_no_object_from_grafts_written_during_push, test_direct_push_sends_no_object_from_pre_push_grafts, test_pr_push_sends_no_object_from_grafts_written_during_push, test_pr_push_sends_no_object_from_pre_push_grafts, test_run_bounded_ignoring_grafts_sets_the_null_device |
| 23 | push_direct's non-fast-forward arm: remove the remote_head comparison, so it always returns UpstreamMoved | same | test_direct_push_shallow_file_refusal_does_not_say_upstream_moved |
| 24 | push_direct: source `{commit}` in place of `{commit}^{{commit}}` | same | test_cli_direct_push_ignores_tag_named_after_the_commit, test_direct_push_ignores_amended_message_behind_shadow_ref, test_direct_push_ignores_branch_named_after_the_commit, test_direct_push_ignores_deleted_file_behind_shadow_ref, test_direct_push_ignores_dot_tree_behind_shadow_ref, test_direct_push_ignores_duplicate_tree_behind_shadow_ref, test_direct_push_ignores_remote_ref_named_after_the_commit, test_direct_push_ignores_reverted_file_behind_shadow_ref, test_direct_push_ignores_shadow_ref_left_by_clean_filter, test_direct_push_ignores_tag_named_after_the_commit, test_direct_push_ignores_token_behind_shadow_ref |
| 25 | push_pr_branch: source `{commit}` in place of `{commit}^{{commit}}` | same | test_cli_pr_push_ignores_tag_named_after_the_commit, test_pr_push_ignores_amended_message_behind_shadow_ref, test_pr_push_ignores_branch_named_after_the_commit, test_pr_push_ignores_deleted_file_behind_shadow_ref, test_pr_push_ignores_dot_tree_behind_shadow_ref, test_pr_push_ignores_duplicate_tree_behind_shadow_ref, test_pr_push_ignores_remote_ref_named_after_the_commit, test_pr_push_ignores_reverted_file_behind_shadow_ref, test_pr_push_ignores_shadow_ref_left_by_clean_filter, test_pr_push_ignores_tag_named_after_the_commit, test_pr_push_ignores_token_behind_shadow_ref |
| 26 | push_direct: drop `--no-replace-objects` | same | test_direct_push_ignores_replace_ref_to_a_tag, test_direct_push_publishes_validated_commit_past_replace_ref |
| 27 | push_pr_branch: drop `--no-replace-objects` | same | test_pr_push_ignores_replace_ref_to_a_tag |
| 28 | resolve_commit: drop `--no-replace-objects` | same | test_direct_push_ignores_replace_ref_to_a_tag, test_pr_push_ignores_replace_ref_to_a_tag |
| 29 | ensure_remote_repo_for_push: return the cache without comparing | corrected | test_cli_direct_push_refuses_tracking_ref_moved_by_post_checkout_hook, test_cli_direct_push_refuses_tracking_ref_moved_on_clone_path, test_cli_direct_push_reports_upstream_moved_during_fetch, test_cli_pr_push_refuses_tracking_ref_moved_by_post_checkout_hook, test_cli_pr_push_refuses_tracking_ref_moved_on_clone_path, test_direct_push_refuses_fetch_redirected_by_hook_written_instead_of, test_direct_push_refuses_fetch_redirected_by_hook_written_origin_url, test_direct_push_refuses_token_on_moved_tracking_ref, test_direct_push_refuses_tracking_ref_kept_by_hook_written_fetch_config, test_direct_push_refuses_tracking_ref_moved_by_clean_filter, test_direct_push_refuses_tracking_ref_moved_by_post_checkout_hook, test_direct_push_refuses_tracking_ref_moved_by_reference_transaction_hook, test_direct_push_refuses_tracking_ref_moved_by_smudge_filter, test_direct_push_refuses_tracking_ref_moved_on_clone_path, test_ensure_for_push_reports_real_upstream_move_as_upstream_moved, test_pr_push_refuses_fetch_redirected_by_hook_written_instead_of, test_pr_push_refuses_fetch_redirected_by_hook_written_origin_url, test_pr_push_refuses_token_on_moved_tracking_ref, test_pr_push_refuses_tracking_ref_kept_by_hook_written_fetch_config, test_pr_push_refuses_tracking_ref_moved_by_clean_filter, test_pr_push_refuses_tracking_ref_moved_by_post_checkout_hook, test_pr_push_refuses_tracking_ref_moved_by_reference_transaction_hook, test_pr_push_refuses_tracking_ref_moved_by_smudge_filter, test_pr_push_refuses_tracking_ref_moved_on_clone_path |
| 30 | ensure_remote_repo_for_push: on a difference always return Failed, with no second read | same | test_cli_direct_push_reports_upstream_moved_during_fetch, test_ensure_for_push_reports_real_upstream_move_as_upstream_moved |
| 31 | remote_head: take the first line's id whatever its refname | same | test_remote_head |
| 32 | absolute_remote_url: return its input unchanged | same | test_absolute_remote_url, test_cli_push_with_relative_remote_path_publishes |
| 33 | src/main.rs Push arm: call open_sync_templates in place of open_push_templates | same | test_cli_direct_push_reads_the_remote_twice_from_outside_the_checkout, test_cli_direct_push_refuses_tracking_ref_moved_by_post_checkout_hook, test_cli_direct_push_refuses_tracking_ref_moved_on_clone_path, test_cli_direct_push_reports_upstream_moved_during_fetch, test_cli_pr_push_reads_the_remote_twice_from_outside_the_checkout, test_cli_pr_push_refuses_tracking_ref_moved_by_post_checkout_hook, test_cli_pr_push_refuses_tracking_ref_moved_on_clone_path |
| 34 | push_direct: name `origin` in place of the URL | corrected | test_cli_direct_push_ignores_hook_written_origin_pushurl, test_cli_direct_push_reads_the_remote_twice_from_outside_the_checkout, test_direct_push_ignores_hook_written_origin_pushurl, test_direct_push_refuses_fetch_redirected_by_hook_written_origin_url, test_direct_push_refuses_hook_written_url_keyed_pushurl, test_direct_push_sends_no_object_from_grafts_written_during_push |
| 35 | push_pr_branch: name `origin` in place of the URL | corrected | test_cli_pr_push_ignores_hook_written_origin_pushurl, test_cli_pr_push_reads_the_remote_twice_from_outside_the_checkout, test_pr_push_ignores_hook_written_origin_pushurl, test_pr_push_refuses_fetch_redirected_by_hook_written_origin_url, test_pr_push_refuses_hook_written_url_keyed_pushurl, test_pr_push_sends_no_object_from_grafts_written_during_push |
| 36 | push_direct: remove the confirm_published call | corrected | test_cli_direct_push_reads_the_remote_twice_from_outside_the_checkout, test_cli_direct_push_refuses_hook_written_push_instead_of, test_direct_push_refuses_fetch_redirected_by_hook_written_instead_of, test_direct_push_refuses_global_push_instead_of, test_direct_push_refuses_hook_written_instead_of_after_the_fetch, test_direct_push_refuses_hook_written_push_instead_of, test_direct_push_refuses_hook_written_url_keyed_pushurl, test_direct_push_refuses_redirect_written_by_clean_filter, test_direct_push_refuses_when_remote_branch_moved_after_push |
| 37 | push_pr_branch: remove the confirm_published call | corrected | test_cli_pr_push_reads_the_remote_twice_from_outside_the_checkout, test_cli_pr_push_refuses_hook_written_push_instead_of, test_pr_push_refuses_fetch_redirected_by_hook_written_instead_of, test_pr_push_refuses_global_push_instead_of, test_pr_push_refuses_hook_written_instead_of_after_the_fetch, test_pr_push_refuses_hook_written_push_instead_of, test_pr_push_refuses_hook_written_url_keyed_pushurl, test_pr_push_refuses_redirect_written_by_clean_filter, test_pr_push_refuses_when_remote_branch_moved_after_push |
| 38 | stage_publication: remove both require_fetched_head_holds calls | corrected | test_cli_direct_push_refuses_index_flag_and_keeps_state, test_cli_pr_push_refuses_index_flag_and_keeps_state, test_direct_push_no_changes_refuses_deleted_path_still_held, test_direct_push_no_changes_refuses_head_off_the_fetched_head, test_direct_push_no_changes_refuses_replace_ref_on_the_fetched_tree, test_direct_push_refuses_assume_unchanged_set_after_ensure, test_direct_push_refuses_assume_unchanged_set_during_ensure, test_pr_push_no_changes_refuses_deleted_path_still_held, test_pr_push_no_changes_refuses_head_off_the_fetched_head, test_pr_push_no_changes_refuses_replace_ref_on_the_fetched_tree, test_pr_push_refuses_assume_unchanged_set_after_ensure, test_pr_push_refuses_assume_unchanged_set_during_ensure |
| 39 | require_fetched_head_holds: remove the HEAD comparison | corrected | test_direct_push_no_changes_refuses_head_off_the_fetched_head, test_pr_push_no_changes_refuses_head_off_the_fetched_head, test_require_fetched_head_holds_edges |
| 40 | require_fetched_head_holds: remove the staged-path loop | corrected | test_direct_push_no_changes_refuses_deleted_path_still_held, test_pr_push_no_changes_refuses_deleted_path_still_held, test_require_fetched_head_holds_edges |
| 41 | The continuity refusal: print the parent text again | same | test_cli_direct_push_refusal_prints_no_hook_bytes, test_cli_pr_push_refusal_prints_no_hook_bytes, test_direct_push_refusal_omits_hook_text_in_parent_line, test_direct_push_refusal_omits_token_in_parent_line, test_pr_push_refusal_omits_hook_text_in_parent_line, test_pr_push_refusal_omits_token_in_parent_line |
| 42 | require_fetched_head_holds: drop `--no-replace-objects` from its own `ls-tree` | same | test_direct_push_no_changes_refuses_replace_ref_on_the_fetched_tree, test_pr_push_no_changes_refuses_replace_ref_on_the_fetched_tree |
| 43 | require_fetched_head_holds: remove the refusal on first_path_not_held's path (the held-content check) | corrected | test_cli_direct_push_refuses_index_flag_and_keeps_state, test_cli_pr_push_refuses_index_flag_and_keeps_state, test_direct_push_no_changes_refuses_replace_ref_on_the_fetched_tree, test_direct_push_refuses_assume_unchanged_set_after_ensure, test_direct_push_refuses_assume_unchanged_set_during_ensure, test_pr_push_no_changes_refuses_replace_ref_on_the_fetched_tree, test_pr_push_refuses_assume_unchanged_set_after_ensure, test_pr_push_refuses_assume_unchanged_set_during_ensure, test_require_fetched_head_holds_edges |
| 44 | remote_head: run with `self.git(None)`, the process's working directory, in place of the filesystem root | same | test_cli_direct_push_reads_the_remote_twice_from_outside_the_checkout, test_cli_pr_push_reads_the_remote_twice_from_outside_the_checkout |
| 45 | src/main.rs open_push_templates: handle UpstreamMoved as the Failed arm does (its message, exit EXIT_HARD_FAILURE) | same | test_cli_direct_push_reports_upstream_moved_during_fetch |
| 46 | src/main.rs open_push_templates: handle Failed as the UpstreamMoved arm does (the two pull-first lines, exit EXIT_DRIFT) | same | test_cli_direct_push_refuses_tracking_ref_moved_by_post_checkout_hook, test_cli_direct_push_refuses_tracking_ref_moved_on_clone_path, test_cli_pr_push_refuses_tracking_ref_moved_by_post_checkout_hook, test_cli_pr_push_refuses_tracking_ref_moved_on_clone_path |

### Corrected rows

Every corrected row kept all of the table's names. **No name was dropped, so T-02-88 does not arise.** Each added test's failure line is quoted below. The reason is also the fourth field of the row's line in target/controls-02-21.tsv.

| Row | Name added | Failure line | Why the table was wrong |
|-----|------------|--------------|-------------------------|
| 10 | test_direct_push_ignores_replace_ref_to_a_tag, test_pr_push_ignores_replace_ref_to_a_tag | `Err(Failed("Post-commit publication gate violation: committed HEAD blob templates/projects/generic/gitconfig.repo differs from the validated content"))`, at the tests' `assert!(pushed.res.is_ok(), ...)` | Without the option, diff-tree reads the replaced commit, which is the tag's commit, so the gate refuses a validated push. The table listed only the bypass direction, not this fail-closed one |
| 11 | the same two | `Err(Failed("Post-commit publication gate violation: the commit to publish does not hold the validated content of templates/projects/generic/gitconfig.repo"))`, same assertion | Without the option, ls-tree lists the replaced commit's tree, so the held-content check refuses a validated push. Same omission as row 10 |
| 29 | test_direct_push_refuses_tracking_ref_moved_by_smudge_filter, test_pr_push_refuses_tracking_ref_moved_by_smudge_filter | `Branch continuity check failed: the cache commit's parent is not the fetched head <id>`, the tests' check that the head check is what refused | The push is still refused, but by the later continuity check, as 02-17-SUMMARY recorded for its own mutation |
| 34 | test_direct_push_refuses_fetch_redirected_by_hook_written_origin_url | `Err(Failed("git push reported success, but the remote branch main is not at the validated commit; ..."))`, in the delivering control's `assert!(pushed.res.is_ok(), ...)` | The control expects a push to the caller's URL to publish (Ok). A push naming `origin` lands in the redirected repository, and the read-back refuses it |
| 35 | test_pr_push_refuses_fetch_redirected_by_hook_written_origin_url | the same refusal for the `ude-sync-*` branch, same control assertion | Row 34 in PR mode |
| 36 | test_direct_push_refuses_fetch_redirected_by_hook_written_instead_of | `expected the read-back's refusal, got Ok("Directly pushed changes to remote branch 'main'.")` | 02-18 made this control expect the read-back's refusal, because insteadOf also rewrites the push URL. Without confirm_published the control gets Ok |
| 37 | test_pr_push_refuses_fetch_redirected_by_hook_written_instead_of | `expected the read-back's refusal, got Ok("Pushed changes to branch 'ude-sync-…'. …")` | Row 36 in PR mode |
| 38 | test_direct_push_refuses_assume_unchanged_set_after_ensure, test_pr_push_refuses_assume_unchanged_set_after_ensure | `expected a hard push failure, got Ok("No changes to push to remote repository.")` | The after-ensure rows also reach the empty-status route. The table listed only the during-ensure rows |
| 39 | test_require_fetched_head_holds_edges | `HEAD is not the fetched head: ()` | The edge table calls the function directly. 02-19-SUMMARY recorded it failing under its `nohead` mutation |
| 40 | test_require_fetched_head_holds_edges | `a deleted path still in HEAD: ()` | Same; 02-19-SUMMARY's `nodelete` mutation |
| 43 | test_direct_push_refuses_assume_unchanged_set_after_ensure, test_pr_push_refuses_assume_unchanged_set_after_ensure, test_require_fetched_head_holds_edges | `expected a hard push failure, got Ok("No changes to push to remote repository.")` (both rows); `one path is not held: ()` (edge table) | The after-ensure rows hide the change the same way as the during-ensure rows, and the edge table asserts the not-held refusal |

**How each row was read.** I read each failing test's panic site and its first message line. All 35 `same` rows and every added name fail inside the tests, on the row's behaviour. The two panic sites that print a bare `Err(...)` (src/sync/mod.rs 11544 and 14853 at `8570971`) are the tests' own `assert!(pushed.res.is_ok(), "{:?}", pushed.res)`. They are not fixture `unwrap`s. No log holds a compile error; every log ends in `exit=100`.

**What this does not establish.**
- A row shows that its listed tests fail when that one edit is made. It does not show they are the only tests pinning the check, or that the check is sufficient for the threat.
- That the edit made was the row's edit is shown only by the scripted replacement. Each replacement was required to match exactly once inside its named function, and its diff size was printed before the run.
- The correction reasons are readings of what each test asserts. No run can show a reason is true.
- Rows 10 and 11 show that those two options are needed for a validated push to publish past a replace ref. That is the gate failing closed. The bypass direction rests on the table's own names (test_push_refuses_hook_replaced_tree and test_push_refuses_hook_replaced_tree_hiding_empty_directory).

Re-run at re-verification: 10, 11, 29, 34, 35, 36, 37, 38, 39, 40, 43

## Mutation result

| Run | Scope | Listed | Caught | Missed | Timeouts | Unviable | Wall time | Exit |
|-----|-------|--------|--------|--------|----------|----------|-----------|------|
| First | Plan's command, at `5ce1bee` | 122 | 102 | 1 | 0 | 19 | 40 min | 2 |
| **Final** | **Plan's command, at `8570971`** | **122** | **103** | **0** | **0** | **19** | **40 min** | **0** |

The final run's output files are `target/mutants-tree-02-21/mutants.out/{caught,missed,timeout,unviable}.txt`, and its log is target/mutants-tree-02-21-run.log. `missed.txt` is empty. Plan 02-15 listed 96 mutants, 83 of them caught.

**The missed mutant.** `src/sync/mod.rs:2858:48: replace == with != in SyncEngine::require_fetched_head_holds`. This is the comparison inside the staged-deletion `listed` closure. Under the mutant, a staged deletion with no record counts as present whenever the fetched head lists any other path, so a legitimate no-change push would be refused. That is the fail-closed direction. `test_require_fetched_head_holds_edges` kills it, through its "absent passes" row. But that test is not in the plan's name list, which was written before 02-19 added it. So this was a gap in the filter, not in the suite, as in 02-15.

**The tests added** (commit `8570971`):
- `test_direct_push_no_changes_publishes_nothing_when_content_is_held_and_deletion_is_gone`
- `test_pr_push_no_changes_publishes_nothing_when_content_is_held_and_deletion_is_gone`

`assert_no_change_publishes_nothing` gained a `gone_deletion` flag. When it is set, the staged list also holds a deletion with no record whose path the fetched head no longer lists, while that head still lists other paths. The result must still be `Ok("No changes...")`, with no remote head moved and no sync branch. The filter's existing names select the new tests as substrings, so the plan's command runs unchanged.

Both directions were run:
- Without the mutant, the five related tests pass.
- With it applied (`sed` on line 2858, then restored and checked with `cmp`), both new tests and the edge table fail on `assertion left == right failed`. The two original no-change tests still pass, which is why the mutant survived.

**Unviable, so the mutation run says nothing about these:**
- `run_bounded_ignoring_grafts`, `run_push`, `ensure_remote_repo_for_push` and `publish_target` returning `Default::default()`. Rows 18, 19, 22, 29 and 30 cover the first three.
- `first_path_not_held` returning `Some(leaked default)`.
- `raw_diff_entries` returning a one-entry vec.
- Nine `tree_listing_blobs` returns with a made-up map, and deleting one of its match arms.
- Three `&&` to `||` in `absolute_remote_url`'s let-chain, which do not compile. Row 32 covers that function.

**What this does not establish.** Mutation testing checks the operators cargo mutants generates in these functions, under a filter of 163 listed names plus the two new tests. It does not exercise string arguments or deleted statements; the control table covers those. src/main.rs is not mutated. Its arms are pinned by rows 33, 45 and 46. Its no-URL fall-through is covered by test_reinit_keeps_ancestor_of_unedited_gitignore and test_push_never_deletes_a_template_with_logic, which is a reading of what those tests assert; they ran and passed in the full suite.

## Gates

Each exit code was captured directly. The gates ran in the plan's order after the last commit, `8570971`:

| Gate | Exit | Result |
|------|------|--------|
| Mutation command (final) | 0 | see above |
| caught.txt non-empty, missed.txt empty, temp dir gone, nothing staged, local config unchanged | 0 | |
| Hermeticity, GIT_DIR inherited from a decoy | 0 | 336 passed; decoy: 0 object files, 0 ref files, no packed-refs |
| Coverage report has both rows | 0 | see below |
| `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test && cargo deny check advisories bans sources && git diff --exit-code -- Cargo.toml Cargo.lock && git diff --quiet b8c54e9 HEAD -- src/sanitizer src/template` | 0 | 417 unit tests passed (1 ignored), 114 e2e passed; advisories, bans and sources ok |
| `cargo machete` | 0 | |
| Closed list's verify (02-20's DEF and NOT rows against docs/ARCHITECTURE.md, each name exactly one test, then run) | 0 | 216 tests run, 216 passed. The DEF rows name 214 tests; nextest filters by substring, so it also runs the two new tests |
| Generated dirs absent, code before `mod tests {` matches the controls' start, nothing outside `.planning` in `git status` | 0 | run last in Task 2 and again in Task 3 |
| Task 1's first verify, re-run after the gates | 0 | same=35 corrected=11 |
| Task 3's two verifies | 0 | |

**What this does not establish:**
- The hermeticity gate shows that no sync test wrote to the decoy, not that none read it.
- The e2e pushes use local bare remotes, not a hosted remote. Nothing was run on Windows.
- A single run of each gate is a weak bound on flakiness. It shows that these runs passed, not that the suite is stable.

## Coverage

The command was `cargo +nightly llvm-cov --bin ude --branch --summary-only --show-missing-lines`. It exited 0, and branches were measured. It covers `--bin ude` only, so nothing run through the e2e suite counts. No threshold.

| File | Lines at 02-15 | Lines now | Branches at 02-15 | Branches now |
|------|----------------|-----------|-------------------|--------------|
| src/process.rs | 372, 96.77% (12 missed) | 372, 96.77% (12 missed) | 22, 77.27% (5 missed) | 22, 81.82% (4 missed) |
| src/sync/mod.rs | 8903, 94.99% (446 missed) | 10607, 95.67% (459 missed) | 568, 75.70% (138 missed) | 628, 77.07% (144 missed) |

The report's uncovered-line list for src/sync/mod.rs has 428 entries, while its summary row says 459 missed lines. The two counts come from different parts of llvm-cov's output, and I did not reconcile them. The table below comes from the list.

Uncovered lines in `push_direct`, `push_pr_branch` and the functions in the mutation regex, at `8570971`:

| Function | Lines | What they are |
|----------|-------|---------------|
| record_staged_tree | 2082 | spawn failure of `git write-tree` |
| push_pr_branch | 2121, 2124-2127 | `git checkout -b` failing to spawn or exiting non-zero |
| push_pr_branch | 2135, 2138-2142 | `git commit` failing to spawn or exiting non-zero |
| push_pr_branch | 2148-2150 | resolve_commit failing after the commit |
| push_pr_branch | 2214 | spawn failure of the push |
| push_pr_branch | 2249-2250 | `gh pr create` succeeding: no test has a working `gh` |
| push_direct | 2291, 2294-2297 | `git commit` failing to spawn or exiting non-zero |
| push_direct | 2303-2304 | resolve_commit failing after the commit |
| resolve_commit | 2418, 2420 | `git rev-parse` failing to spawn or exiting non-zero |
| refuse_grafts_file | 2482 | spawn failure of `git rev-parse` |
| validate_committed_tree | 2544-2545, 2547-2550 | `cat-file commit` failing to spawn or exiting non-zero |
| validate_committed_tree | 2571-2573 | a commit object with no empty line |
| validate_committed_tree | 2609-2610 | spawn failure of `diff-tree` |
| validate_committed_tree | 2621-2623 | an unparseable `diff-tree` entry |
| validate_committed_tree | 2659-2662 | a listed path with no validated record |
| validate_committed_tree | 2687-2688, 2690, 2692-2697 | `ls-tree` failing to spawn or exiting non-zero |
| validate_committed_tree | 2705-2707, 2724-2726 | an unparseable `ls-tree` entry |
| require_fetched_head_holds | 2833, 2835-2838 | the fetched head's `ls-tree` failing to spawn or exiting non-zero |
| require_fetched_head_holds | 2841-2843 | a TreeDefect in the fetched head's listing |
| raw_commit_tree | 2922 | `.next()?` on a split that always has a first item |

The other named functions have no uncovered lines: run_push, read_blob_by_id, raw_commit_parents, raw_commit_parts, raw_diff_entries, tree_listing_blobs, git_path_bytes, count_empty_directories, remove_git_test_env, run_bounded_ignoring_grafts, require_validated_content, first_path_not_held, remote_head, ensure_remote_repo_for_push, confirm_published, absolute_remote_url and publish_target. Nearly every uncovered line is a refusal path for a git command that fails or prints what git does not print. No test drives those refusals, so their messages and their rollbacks have not been run.

## Task Commits

1. **Task 1: Control table.** No commit: the checkout ended byte-identical.
2. **Task 2: Mutation run and gates.** `8570971` (test), forced by the missed mutant.
3. **Task 3: Todo.** Committed with this SUMMARY.

## Files Created/Modified

- `src/sync/mod.rs`: `assert_no_change_publishes_nothing` gained `gone_deletion`, plus two new tests, all after `mod tests {`. Nothing else changed.
- `.planning/todos/pending/2026-11-01-hand-edits-owed-for-phase-02-after-re-verification.md`: a section `## Added by plan 02-21`, and 02-21-PLAN.md under `files:`. Every existing line was kept. The T-02-77, T-02-80 and T-02-81 Status cells are byte copies, checked by script.

## Decisions Made

- **How the corrected rows were judged.** Each row met the plan's conditions for `corrected`: every run exited 100, at least one test failed, every added test failed on its own assertion, and all of the table's names were among the failures. For each added name I read what the test asserts and why the edit reaches it (reasons above). None points to a code defect, so no restart.
- **How the missed mutant was killed.** It was killed the same way as in 02-15: new tests under names the filter already selects, so the plan's command stays as written. Adding `test_require_fetched_head_holds_edges` to the list would also work, but it changes the command.
- **Edit forms the plan leaves open.**
  - Row 4 keeps `seen.insert(path);` without the refusal, because removing the statement leaves `seen` with no type.
  - Row 7 maps the result with `.map(|_| 0usize)`, so `None` is still refused.
  - Row 29 compares with `true || …`, so the head is still read but no longer decides.
  - Row 34 and row 35 keep the `--` before `origin`.
  - Row 41 restores 02-19's prior text, `cache commit parent {parent} is not the fetched head {base}`.
  - Row 45 prints the Failed arm's message with the branch name as its detail.

## Deviations from Plan

1. **[Process] The rows ran as one scripted batch, with every `different` resolved afterwards, not one at a time with a stop at each.** No `different` row was a defect or a build failure, so no row needed a redo and no restart was needed. The result is the same as a stop-and-resolve run. The edits, commands and records are the plan's. Each nextest run had `timeout --kill-after=30 1800`; none timed out.
2. **[Process] `PROPTEST_DISABLE_FAILURE_PERSISTENCE=1` was also set for the coverage run and for the plan's fmt/clippy/test/deny gate.** The plan sets it only on its nextest commands. It stops proptest from writing regression files into the checkout and does not change which tests run. No proptest-regressions directory exists.
3. **[Process] Before the table started I added an unedited baseline run and a dry run of all 46 edits on scratch copies.** Neither is in the plan. Both are controls on the method.

**Total deviations:** 3, all process; none touches production code. **Impact:** none on what the plan's verifies check.

## Issues Encountered

- One re-run of Task 1's verify was piped into `head`, which made it report 141 (SIGPIPE). I re-ran it without the pipe, and it exited 0.
- Dispatch named `.agents/GEMINI.md` as the project instructions in config, and it does not exist (as 02-15 found). Nothing depended on it.

## Threat Flags

| Flag | File | Description |
|------|------|-------------|
| threat_flag: T-02-60 | src/sync/mod.rs | pending the operator, not accepted |
| threat_flag: T-02-61 | src/sync/mod.rs | pending the operator, not accepted |
| threat_flag: T-02-64 | src/sync/mod.rs | pending the operator, not accepted |
| threat_flag: T-02-65 | src/sync/mod.rs | pending the operator, not accepted |
| threat_flag: T-02-67 | src/process.rs | pending the operator, not accepted |
| threat_flag: T-02-77 | src/sync/mod.rs | accepted by the operator 2026-11-02 for global `url.*.insteadOf` (the option recorded as "fix all four causes", whose text included "Global `url.*.insteadOf` stays open and would be recorded as a residual."); widened 2026-11-03 to every global redirect key. Question: "Two git settings in your GLOBAL config (~/.gitconfig), not the cache checkout's, can redirect UDE's push. Global `url.*.pushInsteadOf` sends the validated commit to another repository before the read-back refuses; this was measured. Global `remote.<url>.url` makes the remote-head check read another repository; this was measured with plain git, and whether UDE then returns Ok is not known. You already accepted global `url.*.insteadOf` as a residual. Anything that can write your global config can also run git itself. How should these two be handled?" Selected "Accept with insteadOf", whose option text read "Widen the accepted global-config residual to cover all global redirect keys: same writer, same privilege. The plans record it as accepted, and the gate's promise stays scoped to the cache checkout and its hooks." |
| threat_flag: T-02-80 | src/sync/mod.rs | accepted by the operator 2026-11-03. Question: "New risk from pushing to the caller's URL and reading the ref back: a hook-written `url.*.pushInsteadOf` in the cache checkout still sends the validated commit to another repository before the read-back refuses. `ude sync push` returns no Ok, and only validated bytes travel. How should this be handled? (Planner's finding, severity low, not a defect.)" Selected "Accept (Recommended)", whose option text read "Record it as an accepted residual. A refusal before the push would need a list of config keys and would still miss a process that writes the key later." Scope, the same day, a question ending "Does your acceptance cover all of them?" over every repository-level redirect key in NOT-04 (`url.*.pushInsteadOf`, `url.*.insteadOf`, `remote.<url>.pushurl`, and the reasoned `core.sshCommand` and `http.proxy`): selected "All redirect keys (Recommended)", whose option text read "Same effect and the same protection (no Ok, only validated bytes travel). A refusal for each key would need a key list and would still miss a process writing later." |
| threat_flag: T-02-81 | src/sync/mod.rs | accepted by the operator 2026-11-03. Question: "Second new risk: pushing to the caller's URL puts that URL on the git command line for every push and read, where before it appeared only at first clone. If the URL embeds a token, other local users can see it in the process list. How should this be handled? (Planner's finding, severity low, not a defect.)" Selected "Accept (Recommended)", whose option text read "Record it as an accepted risk. Safe if you don't use token-bearing URLs on a shared host; please say so under notes if you do." No notes were given. |

T-02-88 does not apply: no corrected row dropped a name.

Status when this SUMMARY was written; the ## Operator answers tables hold the current one.

## Cleanup

Removed and confirmed absent:
- target/hermetic-gate
- target/llvm-cov-target
- /var/home/denniyahh/.cache/ude-mutants-02-21-tmp, after each run
- the controls' copies under the session scratch directory

Kept, as the plan says:
- target/mutants-tree-02-21, target/mutants-tree-02-21-run.log and target/mutants-tree-02-21-config.before
- target/controls-02-21.tsv and target/controls-02-21-rows
- target/controls-02-21.nontest.sha256 and target/controls-02-21.table.sha256
- target/llvm-cov-02-21.txt and target/llvm-cov-02-21.log

No proptest-regressions directory exists. The repository's `git config --local --list` hash was unchanged from before the first run to after the last.

## User Setup Required

None.

## Next Phase Readiness

- Re-verification of phase 02 is next. Name the closed list in 02-20-PLAN.md and docs/ARCHITECTURE.md to the verifier.
- Re-run rows 10, 11, 29, 34, 35, 36, 37, 38, 39, 40 and 43 at re-verification.
- `requirements-completed` above copies the plan's frontmatter and is not a verdict. `phase.complete` must wait for re-verification.
- Not claimed:
  - re-verification's verdict;
  - that the closed list is exhaustive (it is closed by decision, not proven);
  - any hosted remote, or Windows;
  - that an edit outside the table would fail a test;
  - mutation of src/main.rs;
  - the operator's answers on T-02-60, T-02-61, T-02-64, T-02-65 and T-02-67, which are still pending.

## Self-Check: PASSED

`8570971` is an ancestor of HEAD, and `git rev-list --count 5ce1bee..8570971` is 1. src/sync/mod.rs and the todo exist. Checked before the commit that adds this file.
