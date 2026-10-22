---
created: 2026-10-22T11:56:15.665Z
title: Close two push test gaps from the phase 01 verification
area: testing
severity: minor
files:
  - tests/e2e_lifecycle.rs
  - src/sync/mod.rs
  - src/main.rs
---

## Problem

The final phase 01 verification (753dbeb) left two test gaps in push. Neither is a known bug;
the operator chose to backlog them on 2026-10-22.

1. **Push's skip message is only pinned by where the cache happens to live.**
   `test_push_names_the_shared_template_in_remote_mode` checks that push names the template by
   its path in the template repo, not the local cache copy. It passes partly because the test's
   `XDG_CACHE_HOME` sits under the test temp dir; the suffix check alone would not catch a
   regression to printing the cache path if the cache moved elsewhere.
2. **A history-lookup failure inside push is never triggered at runtime.** In remote mode push
   runs `SyncEngine::with_deletion_history` → `never_deleted_upstream`, which shells out to
   `git rev-parse --is-shallow-repository`, `git fetch --unshallow` and `git log --diff-filter=D`.
   If that fails, push should exit 2 with "could not compare the project with upstream".
   `test_scan_failure_is_not_reported_as_a_state_file_error` covers push only in local mode;
   status and pull already cover the same function's failure in remote mode.

## Solution

1. Assert explicitly that the printed path does not contain the cache directory (or that it
   matches the remote repo's template path exactly), independent of where the cache lives.
2. Add a remote-mode push case that makes the history lookup fail (for example a cache whose
   `.git` is corrupted after the scan's clone, or a shallow cache with an unreachable origin for
   `fetch --unshallow`) and assert exit 2 plus the "could not compare" message. Prove it with a
   mutation: drop the push arm's error handling and watch the test fail.
