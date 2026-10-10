## CONFIRMED DEFECTS

1. **P1 — The publication gate scans the wrong bytes; credentials can reach the remote.**

   [src/sync/mod.rs:667](/tmp/ude-snapshot/src/sync/mod.rs:667) claims “Check staged file bytes for secrets,” but line 669 executes `fs::read_to_string(&staged_full_path)` against the working tree.

   **Reproduced:** Configure a Git clean filter on the allowlisted `.repo` file that outputs a dummy GitHub PAT. Push benign local content. `ude sync push` exits **0**, and the remote commit contains the PAT while the working-tree file remains clean.

   **Controls:** Without the filter, the remote receives benign content. Putting the PAT directly in the local file causes rejection with exit **1**.

   The allowlist validates filenames, not the actual publication payload. Scan index blobs and fail closed on inspection errors.

2. **P1 — A second pull silently erases the local edit preserved by the first merge.**

   [src/sync/mod.rs:410](/tmp/ude-snapshot/src/sync/mod.rs:410) selects merged content; [line 463](/tmp/ude-snapshot/src/sync/mod.rs:463) records it as the ancestor: `project_state.record_file(&rel, new_hash, Some(content));`.

   **Reproduced:**
   
   - Ancestor: `a=1,b=1`; local: `a=2,b=1`; upstream: `a=1,b=2`.
   - First pull produces `a=2,b=2`.
   - Second pull, with upstream unchanged, produces `a=1,b=2`.

   Recording the merged local result as the ancestor makes the second scan interpret the upstream’s missing local edit as an upstream update.

   **Control:** With upstream unchanged from the original ancestor, pull preserves the local edit.

   This violates [README.md:37](/tmp/ude-snapshot/README.md:37), “Idempotently materializes and updates environment baselines.” The ancestor must represent the synchronized upstream base, not the combined local result.

3. **P1 — An unmerged PR-branch push advances canonical ancestor state and enables data loss.**

   [src/sync/mod.rs:701](/tmp/ude-snapshot/src/sync/mod.rs:701) pushes `&sync_branch`. [Line 731](/tmp/ude-snapshot/src/sync/mod.rs:731) returns `Ok` even when PR creation fails. [src/main.rs:517](/tmp/ude-snapshot/src/main.rs:517) then unconditionally executes `project_state.record_file(&rel, hash, content)`.

   **Reproduced:** Push a local edit using `--pr`, with PR creation unavailable. The sync branch receives the edit; `main` remains unchanged. Ancestor state nevertheless records the edit. A subsequent pull from `main` overwrites the local file with its old contents.

   **Control:** Direct push followed by pull retains the edit.

   Remote branch publication does not establish acceptance into the canonical upstream. Keep pending proposal state separate from the canonical ancestor.

4. **P1 — Doctor certifies inert security controls.**

   [src/audit/mod.rs:99](/tmp/ude-snapshot/src/audit/mod.rs:99) accepts `allowed_signers.exists()`. [Line 175](/tmp/ude-snapshot/src/audit/mod.rs:175) accepts `gitconfig.exists()`. [Line 189](/tmp/ude-snapshot/src/audit/mod.rs:189) accepts `hook_file.exists()`.

   **Reproduced:** Supply an empty signers file, garbage `.gitconfig`, empty untracked non-executable hooks, no local include, and effective `core.hooksPath=/dev/null`. With global signing settings enabled, doctor reports every repository check **Pass** and exits **0**.

   **Control:** Removing the signers file produces exit **2**.

   [docs/SPECIFICATION.md:69](/tmp/ude-snapshot/docs/SPECIFICATION.md:69) requires authorized signing fingerprints; [lines 74–76](/tmp/ude-snapshot/docs/SPECIFICATION.md:74) require the include, configured hook path, and indexed executable modes. File existence establishes none of those properties.

5. **P2 — Switching branches breaks the shared remote cache.**

   [src/sync/mod.rs:296](/tmp/ude-snapshot/src/sync/mod.rs:296) keys the cache solely on `remote_url`. [Line 302](/tmp/ude-snapshot/src/sync/mod.rs:302) clones with `--depth 1 -b <branch>`. Subsequent calls fetch the requested branch at [line 314](/tmp/ude-snapshot/src/sync/mod.rs:314), then reset to `origin/{branch}` at [line 324](/tmp/ude-snapshot/src/sync/mod.rs:324).

   **Reproduced:** Against a remote containing both branches, run status on `main`, then status on `develop` using the same URL. Fetch succeeds, but reset fails because the initial clone’s fetch configuration does not create `origin/develop`. CLI exits **1**.

   **Control:** Repeating status on `main` exits **0**.

   Include branch identity in cache ownership, or explicitly fetch and check out the requested target ref.

## SUSPECTED DEFECTS

- **Speculation — Concurrent sync commands can destroy each other’s pending writes.** The URL-only cache above is shared; [src/sync/mod.rs:324](/tmp/ude-snapshot/src/sync/mod.rs:324) performs `reset --hard`, while [line 571](/tmp/ude-snapshot/src/sync/mod.rs:571) writes candidate templates before staging. No operation-level lock is present. A concurrent status/pull can reset another push’s candidates. This interleaving was **not reproduced**.

## RESIDUAL RISKS & TEST GAPS

- **The publication regression assertion accepts committed secrets.** [src/sync/mod.rs:1408](/tmp/ude-snapshot/src/sync/mod.rs:1408) allows `|| !status_out.contains("templates/unvalidated-secret.txt")`. I tested that exact predicate: it rejects a staged-secret fixture, then accepts the same secret after commit because clean status omits the path. Assert against committed trees and blobs.
- **The “failed push” test never exercises a failed remote push.** [src/sync/mod.rs:1324](/tmp/ude-snapshot/src/sync/mod.rs:1324) only invokes `push_updates`; [line 1343](/tmp/ude-snapshot/src/sync/mod.rs:1343) manually restores upstream content. It cannot establish the CLI’s remote-push or PR-state contract.
- **Existing checks miss these failures:** all **48 tests** and Clippy with warnings denied passed. That establishes those checks’ results, not publication safety or lifecycle correctness.

Reproductions are in the [probe harness](/tmp/ude-adversarial-z4cdn5__/probe.py). They used isolated local Git remotes and dummy credentials; live GitHub behavior and concurrency reliability remain unverified.