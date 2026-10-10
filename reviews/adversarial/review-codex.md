## CONFIRMED DEFECTS

1. **Critical — the publication scan does not cover the commit actually pushed.**

   **Citation:** [src/sync/mod.rs:755](/tmp/ude-snapshot/src/sync/mod.rs:755), [src/sync/mod.rs:861](/tmp/ude-snapshot/src/sync/mod.rs:861).

   **Quoted code:** `"Inspect staged index: assert that ONLY allowed files are staged"` followed later by `.args(["commit", "-m", commit_msg])`.

   **Attack:** A pre-commit hook runs after validation and can change the index. Reproduced with a disposable global hook that staged `unvalidated-secret.txt` containing a dummy GitHub PAT. UDE exited **0** and published that file. The control without the hook published only the validated template.

   **Fix:** Build an immutable validated tree, or inspect the resulting commit’s complete tree/diff before pushing. Reject unexpected paths and scan the actual committed blobs.

2. **Critical — direct push after PR mode reports success without publishing the changes.**

   **Citation:** [src/sync/mod.rs:420](/tmp/ude-snapshot/src/sync/mod.rs:420), [src/sync/mod.rs:873](/tmp/ude-snapshot/src/sync/mod.rs:873), [src/main.rs:519](/tmp/ude-snapshot/src/main.rs:519).

   **Quoted code:** `.args(["reset", "--hard", &format!("origin/{branch}")])`; `.args(["push", "origin", branch])`; `project_state.record_file(&rel, hash, content)`.

   **Failure:** PR mode leaves the cache checked out on its sync branch. Resetting that branch does not switch back to `main`. A subsequent direct push commits on the sync branch but pushes the unchanged local `main` ref.

   **Reproduced:** Exit **0**, message `"Directly pushed changes to remote branch 'main'."`, remote `main` unchanged, ancestor advanced to unpublished content.

   **Fix:** Explicitly establish the intended checkout/ref, push `HEAD:refs/heads/<target>`, and verify the published commit before advancing ancestry.

3. **High — every PR attempt uses the same branch, and checkout failure is ignored.**

   **Citation:** [src/sync/mod.rs:805](/tmp/ude-snapshot/src/sync/mod.rs:805), [src/sync/mod.rs:808](/tmp/ude-snapshot/src/sync/mod.rs:808).

   **Quoted code:** `let sync_branch = format!("ude-sync-{}", &Self::compute_hash(commit_msg.as_bytes())[..8]);` and `let _ = Command::new("git") ... .args(["checkout", "-b", &sync_branch])`.

   **Failure:** The message is constant, so the branch is constant. Reproduced: first proposal succeeded; second hit `"a branch named 'ude-sync-14f11ae4' already exists"`, continued committing, then failed with a non-fast-forward push.

   **Fix:** Generate a unique proposal identifier, check checkout success, and define explicit retry/update behavior for an existing proposal.

4. **High — ordinary credential alphabets bypass the “strict” sanitizer.**

   **Citation:** [src/sanitizer/mod.rs:47](/tmp/ude-snapshot/src/sanitizer/mod.rs:47), [docs/ARCHITECTURE.md:76](/tmp/ude-snapshot/docs/ARCHITECTURE.md:76).

   **Quoted code:** `[A-Za-z0-9_\-\.]{20,}`.  
   **Quoted requirement:** `"rejecting any files containing high-entropy strings"`.

   **Attack:** `api_key = '+ABC/DEF/GHI/JKL/MNO/PQR/STU/VWX/YZ0123456789'` passes. An alphabetic API-key control is rejected. Values beginning with `+`, or split by `/`, never enter the permitted value alphabet. Assignment names outside four recognized families also escape generic screening.

   **Fix:** Parse supported configuration formats, inspect decoded values, and use credential detectors covering their actual alphabets. Implement a defined entropy policy or remove the entropy claim.

5. **High — pull writes through symlinks outside the project.**

   **Citation:** [src/sync/mod.rs:503](/tmp/ude-snapshot/src/sync/mod.rs:503), [src/sync/mod.rs:556](/tmp/ude-snapshot/src/sync/mod.rs:556).

   **Quoted code:** `let target_path = local_root.join(&s.relative_path);` and `fs::write(&target, &content)`.

   **Attack:** Point project `.gitconfig` at an external file containing the recorded baseline. Pulling an upstream update overwrites that external file.

   **Reproduced:** External victim changed from `"base\n"` to `"new\n"`; pull returned `Success { applied_files: 1 }`.

   **Fix:** Enforce destination containment, including symlinked parent directories. Use operations that prevent link substitution between checking and opening.

6. **High — unreadable content is interpreted as an absent file and can be overwritten.**

   **Citation:** [src/sync/mod.rs:332](/tmp/ude-snapshot/src/sync/mod.rs:332).

   **Quoted code:** `let local_content = fs::read_to_string(&local_path).ok();`

   **Failure:** Without ancestor state, a readable divergent file is correctly conflicted. Replace its content with invalid UTF-8 and the same path becomes “absent,” so pull replaces it.

   **Reproduced:** Local `[0xff]` became upstream `"base\n"` with a successful pull. The valid UTF-8 divergence control was conflicted.

   **Fix:** Treat only `NotFound` as absence. Return explicit errors for invalid encoding, permissions, directories, and other read failures.

7. **High — materialization can fail halfway through while leaving old ancestry.**

   **Citation:** [src/sync/mod.rs:546](/tmp/ude-snapshot/src/sync/mod.rs:546), [src/sync/mod.rs:579](/tmp/ude-snapshot/src/sync/mod.rs:579), [src/sync/mod.rs:669](/tmp/ude-snapshot/src/sync/mod.rs:669).

   **Quoted code:** `"Pass 2: Transactional materialization of validated files"`; writes occur individually, while `project_state.save_to_dir(local_root)` occurs afterward.

   **Failure:** First destination succeeds, second fails. Earlier writes remain, but ancestry is not saved. Reproduced with an updated `.gitconfig` followed by a `.gitignore` destination that was a directory: pull returned an I/O failure after changing `.gitconfig`.

   **Fix:** Stage output, validate destination types, and use a recoverable transaction/journal. Document crash recovery as well as validation atomicity.

8. **High — deletions are classified but cannot be synchronized correctly.**

   **Citation:** [src/sync/mod.rs:477](/tmp/ude-snapshot/src/sync/mod.rs:477), [src/sync/mod.rs:636](/tmp/ude-snapshot/src/sync/mod.rs:636), [src/sync/mod.rs:252](/tmp/ude-snapshot/src/sync/mod.rs:252).

   **Quoted code:** `fs::read_to_string(&upstream_path)`; `fs::read_to_string(&local_path)`; `local_content.unwrap_or("")`.

   **Failure:** Upstream deletion becomes `UpstreamModified`, but pull tries to read the deleted file and fails. Local deletion similarly reaches a mandatory read during push. When both sides delete an existing ancestor, comparison produces `Merged, Some("")`, allowing resurrection as an empty file.

   **Reproduced:** Upstream deletion failed with `No such file or directory`; simultaneous deletion produced `(Merged, Some(""))`.

   **Fix:** Represent deletion explicitly, propagate it through materialization/staging, and distinguish absent content from empty content.

9. **High — push silently discards clean three-way merge candidates.**

   **Citation:** [src/sync/mod.rs:623](/tmp/ude-snapshot/src/sync/mod.rs:623), [docs/ARCHITECTURE.md:60](/tmp/ude-snapshot/docs/ARCHITECTURE.md:60).

   **Quoted code:** `if state.status == SyncStatus::LocallyModified`.  
   **Quoted requirement:** `"When ude sync push or ude sync pull detects divergence"` and `"changes merge cleanly"`.

   **Failure:** Orthogonal edits are classified `Merged`, but push processes only `LocallyModified`. Reproduced: two independent line edits yielded `Success { synced_files: 0, ... }`.

   **Fix:** Publish validated merged content, or explicitly reject push with instructions to pull first. Silent success with zero published changes is unacceptable.

10. **High — convergence leaves stale ancestry and creates false conflicts later.**

    **Citation:** [src/sync/mod.rs:229](/tmp/ude-snapshot/src/sync/mod.rs:229), [src/sync/mod.rs:538](/tmp/ude-snapshot/src/sync/mod.rs:538).

    **Quoted code:** `if local_content == upstream_content && local_content.is_some()`; `return PullResult::Success { applied_files: 0 };`.

    **Failure:** Local and upstream converge on an accepted change, including after a PR merge. Pull recognizes equality but never records the new common ancestor. A later upstream-only edit is compared against the obsolete ancestor.

    **Reproduced:** `base → accepted` on both sides, then upstream `accepted → new`, produced `Conflicted` instead of `UpstreamModified`.

    **Fix:** Reconcile ancestor records for converged files, even when no file write is needed.

11. **High — manifest policy is silently ignored, and malformed manifests silently change routing.**

    **Citation:** [src/main.rs:358](/tmp/ude-snapshot/src/main.rs:358), [src/main.rs:364](/tmp/ude-snapshot/src/main.rs:364), [src/manifest/mod.rs:40](/tmp/ude-snapshot/src/manifest/mod.rs:40), [src/main.rs:516](/tmp/ude-snapshot/src/main.rs:516).

    **Quoted code:** `.and_then(|c| manifest::UdeManifest::parse_str(&c).ok())`; `push_to_remote(remote_checkout, &branch, pr, &staged_files)`.

    **Failure:** Only manifest remote and branch affect execution. `exclude`, `allow_overrides`, `strategy`, `auto_pr`, and matrix selections do not govern the pipeline. Setting `auto_pr = true` still permits direct push unless the CLI flag is supplied. A parse error erases the configured remote and selects local templates without reporting the error.

    **Additional reproduced inconsistency:** Omitting `[sync]` supplies default exclusions; adding `[sync] branch='main'` changes them to `[]`.

    **Fix:** Reject invalid manifests, apply supported policies, reject unsupported options, and make field defaults consistent with `SyncConfig::default()`.

12. **High — zero-confidence semantic answers authorize publication.**

    **Citation:** [src/semantic/mod.rs:170](/tmp/ude-snapshot/src/semantic/mod.rs:170), [src/main.rs:483](/tmp/ude-snapshot/src/main.rs:483).

    **Quoted code:** `Answer::Choice { choice, .. }`; `allow_agent_rules = true`.

    **Failure:** Confidence and probabilities are discarded. `Uncertain` handles unknown labels, not uncertain classifications.

    **Reproduced with a local transport mock:** `benign_rephrase` probability `0.34`, competing probabilities `0.33/0.33`, confidence `0.0`, authorized a rule removing verification restrictions. Without the semantic answer, the boundary control rejected publication.

    **Fix:** Apply explicit confidence/ambiguity thresholds and validate the distribution. Route uncertain answers to review. The [TypeSafe API reference](https://docs.typesafe.ai/api) exposes confidence separately from the selected label.

13. **High — overridden private rules are transmitted before candidate selection.**

    **Citation:** [src/main.rs:455](/tmp/ude-snapshot/src/main.rs:455), [src/main.rs:469](/tmp/ude-snapshot/src/main.rs:469), [src/sync/mod.rs:618](/tmp/ude-snapshot/src/sync/mod.rs:618).

    **Quoted code:** `typesafe.evaluate_drift("AGENTS.md", &up_txt, &local_txt)` precedes `"Overridden files are never pushed upstream"`.

    **Attack:** Add a local override and private text to the base rule. UDE sends the text for semantic evaluation before discovering that no file should be published.

    **Reproduced with a local transport capture:** Request contained the private marker; push subsequently synchronized **0 files**.

    **Fix:** Select eligible candidates and enforce privacy boundaries before external evaluation. Scan both transmitted texts and evaluate only the actual candidate diff.

14. **High — doctor accepts garbage as an authorized SSH signer.**

    **Citation:** [src/audit/mod.rs:101](/tmp/ude-snapshot/src/audit/mod.rs:101), [docs/SPECIFICATION.md:68](/tmp/ude-snapshot/docs/SPECIFICATION.md:68).

    **Quoted code:** `trimmed.contains("ssh-ed25519")`.  
    **Quoted requirement:** `"Evaluates dual SSH key presence"`.

    **Reproduced:** `garbage ssh-ed25519 garbage` received `Pass`; doctor exited **0**, with no `.ssh` directory at all. Removing the algorithm substring triggered the critical-failure control.

    The implementation does not verify key encoding, authorized principals/fingerprints, dual-key separation, or private-key encryption.

    **Fix:** Parse and validate keys, compare against configured identities, verify the effective allowed-signers setting, and implement the documented key checks.

15. **High — doctor passes hooks that Git refuses to execute.**

    **Citation:** [src/audit/mod.rs:262](/tmp/ude-snapshot/src/audit/mod.rs:262).

    **Quoted code:** `if first_word == "100755" { CheckStatus::Pass }`.

    **Reproduced:** Hooks tracked as `100755` but changed to `0644` on disk all passed doctor. Git then committed successfully while warning that pre-commit and commit-msg were ignored as non-executable.

    Conversely, an executable untracked hook also receives `Pass`, contradicting the tracked-mode requirement.

    **Fix:** Check index mode and actual executable permissions independently. Require tracked hooks when claiming portability.

16. **Medium — doctor accepts an include path that does not include the root configuration.**

    **Citation:** [src/audit/mod.rs:208](/tmp/ude-snapshot/src/audit/mod.rs:208).

    **Quoted code:** `l.trim() == "../.gitconfig" || l.trim() == ".gitconfig"`.

    **Failure:** Relative includes resolve relative to the containing configuration file. In ordinary `.git/config`, `.gitconfig` points inside `.git`, not to the repository root.

    **Reproduced:** Doctor reported `Pass`; querying a value present only in root `.gitconfig` exited **1**.

    **Fix:** Resolve the actual include target using Git’s configuration origins. Verify effective inclusion rather than matching strings.

17. **Medium — warnings violate the documented exit-code contract.**

    **Citation:** [src/audit/mod.rs:22](/tmp/ude-snapshot/src/audit/mod.rs:22), [src/main.rs:111](/tmp/ude-snapshot/src/main.rs:111), [docs/SPECIFICATION.md:79](/tmp/ude-snapshot/docs/SPECIFICATION.md:79).

    **Quoted code:** `matches!(self.status, CheckStatus::Fail(_) | CheckStatus::Critical(_))`.  
    **Quoted requirement:** `"1: One or more drift warnings or missing non-critical items."`

    **Failure:** `Warning` never contributes to `has_failures`. Missing includes, missing hooks, or a missing canonical rules file can therefore leave exit status **0**, accompanied by `"All required baselines verified."`

    **Fix:** Include warnings in exit-code aggregation, or change the specification and stop claiming all baselines were verified.

18. **High — initialization reports completion with no template assets.**

    **Citation:** [src/main.rs:166](/tmp/ude-snapshot/src/main.rs:166), [src/main.rs:603](/tmp/ude-snapshot/src/main.rs:603), [src/main.rs:621](/tmp/ude-snapshot/src/main.rs:621).

    **Quoted code:** `if let Some(ref tdir) = root_template_dir`; `if src.exists()` followed by unconditional `Ok(())`.

    **Reproduced:** Hiding assets in the disposable build copy produced exit **0**, `"Project initialization complete"`, no `mise.toml`, and an empty ancestor map. The assets-present control generated the file.

    **Fix:** Resolve and validate the complete required asset set before mutation. Missing required assets must fail.

19. **High — repeated initialization destroys existing managed customizations.**

    **Citation:** [src/main.rs:609](/tmp/ude-snapshot/src/main.rs:609), [src/main.rs:631](/tmp/ude-snapshot/src/main.rs:631), [src/main.rs:305](/tmp/ude-snapshot/src/main.rs:305).

    **Quoted code:** `fs::copy(src, dst)`; `fs::write(dst, rendered)`; `..Default::default()`.

    **Reproduced:** Initialize, replace `mise.toml` with custom content, initialize again: exit **0**, customization erased. Hooks and agent rules use the same overwrite paths. Initialization also rebuilds ancestry rather than preserving reconciliation history.

    **Fix:** Separate first installation from reconciliation. Refuse overwrite of modified managed files or use the existing ancestor-aware merge.

20. **High — the portable baseline installs another repository’s personal workflow, including missing executables.**

    **Citation:** [templates/projects/agents/agents.md.jinja:3](/tmp/ude-snapshot/templates/projects/agents/agents.md.jinja:3), [templates/projects/agents/mise.toml.jinja:29](/tmp/ude-snapshot/templates/projects/agents/mise.toml.jinja:29), [src/main.rs:220](/tmp/ude-snapshot/src/main.rs:220).

    **Quoted text:** `"Personal fork (denniyahh/gsd-core) of OpenGSD"`; `run = "./scratch/ci-mac.sh"`.

    **Failure:** Arbitrary projects receive instructions to branch from `upstream/next`, submit to `open-gsd/gsd-core`, and use a personal Mac runner. Initialization copies only three scratch scripts; `ci-mac.sh` is absent. The generated worktree helper also requires missing `check-workflow-budgets.sh`.

    **Reproduced:** A new Node project had the check task but no check runner. The container runner is independently hardcoded to Cargo at [templates/projects/generic/container/check-in-container.sh:22](/tmp/ude-snapshot/templates/projects/generic/container/check-in-container.sh:22).

    **Fix:** Move repository-specific policy into an explicit archetype. Generate ecosystem-appropriate tasks and validate every referenced executable.

21. **Medium — “layered overrides” are existence checks, not applied layers.**

    **Citation:** [src/sync/mod.rs:909](/tmp/ude-snapshot/src/sync/mod.rs:909), [docs/ARCHITECTURE.md:66](/tmp/ude-snapshot/docs/ARCHITECTURE.md:66).

    **Quoted code:** `local_variant.exists()` and `override_dir_variant.exists()`.  
    **Quoted requirement:** `"automatically applied on top of the base template during rendering"`.

    **Failure:** Override contents are never read or composed. `.override` suffixes are not recognized. Some tools independently consume `.local` files; that does not implement UDE’s promised generic layering.

    Conflict guidance at [src/main.rs:438](/tmp/ude-snapshot/src/main.rs:438) recommends creating an override, but simultaneous divergence still enters conflict comparison.

    **Fix:** Implement format-specific layering with defined precedence, or explicitly limit support to external tools’ native override mechanisms.

22. **Medium — path parameterization misses ordinary punctuation boundaries.**

    **Citation:** [src/sanitizer/mod.rs:92](/tmp/ude-snapshot/src/sanitizer/mod.rs:92).

    **Quoted code:** `([/\\"'\s:]|$)`.

    **Reproduced:** `/home/alice/data` parameterized correctly; `(/home/alice)` remained literal. Closing parentheses, commas, semicolons, and several other normal text boundaries are excluded.

    The implementation also emits `{{ user.xdg_config }}`, while [docs/ARCHITECTURE.md:80](/tmp/ude-snapshot/docs/ARCHITECTURE.md:80) specifies `{{ xdg.config }}`; the documented nested `xdg` and `git` objects are absent from `TemplateContext`.

    **Fix:** Define one supported variable schema and tokenize paths according to the containing format, including escaped Windows paths.

23. **Medium — valid slash-containing branch names break cache acquisition.**

    **Citation:** [src/sync/mod.rs:390](/tmp/ude-snapshot/src/sync/mod.rs:390).

    **Quoted code:** `cache_dir.join(format!("remote-{repo_hash}-{branch}.lock"))`.

    **Reproduced:** Remote branch `feature/test` existed and was pushable, but UDE failed opening `remote-…-feature/test.lock` because the embedded slash created an uncreated directory.

    **Fix:** Use the hash alone for filesystem names. Validate branch syntax separately.

24. **Medium — the cross-platform lock is a no-op on Windows and uses a Linux errno on other Unix systems.**

    **Citation:** [src/sync/mod.rs:100](/tmp/ude-snapshot/src/sync/mod.rs:100), [src/sync/mod.rs:120](/tmp/ude-snapshot/src/sync/mod.rs:120), [README.md:3](/tmp/ude-snapshot/README.md:3).

    **Quoted code:** `if errno != 11`; `#[cfg(not(unix))] { Ok(Self { ... }) }`.

    **Failure:** Windows returns a guard without acquiring any lock. Unix contention assumes numeric errno `11`; that is not a portable `EWOULDBLOCK` check.

    **Fix:** Use a supported cross-platform locking implementation and error kinds/platform constants. These defects are confirmed from the platform branches; Windows/macOS execution was not performed.

25. **Medium — clean merge silently rewrites line endings.**

    **Citation:** [src/sync/merge.rs:89](/tmp/ude-snapshot/src/sync/merge.rs:89), [src/sync/merge.rs:180](/tmp/ude-snapshot/src/sync/merge.rs:180).

    **Quoted code:** `base.lines().collect()`; `result_lines.join("\n")`.

    **Reproduced:** Orthogonal edits to CRLF input returned LF-only output. The LF control merged correctly. The subsequent “any input ends with newline” rule also cannot preserve an intentional final-newline removal.

    **Fix:** Preserve line terminators in diff tokens and define explicit final-newline merge semantics.

26. **Medium — missing template variables succeed silently, and scan suppresses real rendering failures.**

    **Citation:** [src/template/mod.rs:138](/tmp/ude-snapshot/src/template/mod.rs:138), [src/sync/mod.rs:339](/tmp/ude-snapshot/src/sync/mod.rs:339).

    **Quoted code:** `let mut env = Environment::new();`; `renderer.render_str(&raw, c).ok().or(Some(raw))`.

    **Reproduced:** Rendering `key={{ absent_required_key }}` succeeded instead of rejecting the missing required value. Rendering errors during scanning are replaced with raw template text, which then participates in comparison/merging.

    **Fix:** Enable strict undefined-variable handling and propagate render failures. Never treat an unrenderable template as usable upstream content.

27. **Medium — promised conflict artifacts do not exist.**

    **Citation:** [src/sync/mod.rs:455](/tmp/ude-snapshot/src/sync/mod.rs:455), [docs/SPECIFICATION.md:104](/tmp/ude-snapshot/docs/SPECIFICATION.md:104).

    **Quoted code:** `return PullResult::ConflictBlocked { path: s.relative_path.clone() };`.  
    **Quoted requirement:** `"writes .conflict report for operator decision"`.

    **Failure:** Pull returns only the first conflicting path. No conflict report or structured ancestor/local/upstream diff is written.

    **Fix:** Generate the specified artifact for every conflict, or remove that contract and provide an equivalent inspectable resolution interface.

## SUSPECTED / ARCHITECTURAL RISKS

1. **High — network operations can hold the cache lock indefinitely.**

   **Citation:** [src/sync/mod.rs:391](/tmp/ude-snapshot/src/sync/mod.rs:391), [src/sync/mod.rs:397](/tmp/ude-snapshot/src/sync/mod.rs:397), [src/sync/mod.rs:835](/tmp/ude-snapshot/src/sync/mod.rs:835).

   **Quoted code:** `RemoteLockGuard::acquire(&lock_path, 10)?`; Git `.status()` and GitHub CLI `.output()` without execution deadlines.

   **Risk:** The ten-second timeout bounds acquisition only. A stalled Git transport, credential prompt, hook, or `gh` invocation can retain the lock indefinitely. Subsequent clients merely time out.

   **Remediation:** Bound subprocess lifetimes, disable interactive prompting, and terminate/reap process groups. Actual network-stall behavior remains untested.

2. **High — inherited Git environment can redirect destructive cache commands into another checkout.**

   **Citation:** [src/sync/mod.rs:418](/tmp/ude-snapshot/src/sync/mod.rs:418).

   **Quoted code:** `.current_dir(&target_checkout).args(["reset", "--hard", ...])`.

   **Risk:** `current_dir` does not neutralize inherited `GIT_DIR`, `GIT_WORK_TREE`, or `GIT_INDEX_FILE`. Running from a hook or another contaminated environment can make fetch/reset/index operations target the caller’s repository.

   **Remediation:** Construct a scrubbed Git subprocess environment. Add a fixture proving that an external victim checkout remains unchanged.

3. **High — cache existence is mistaken for cache integrity and provenance.**

   **Citation:** [src/sync/mod.rs:395](/tmp/ude-snapshot/src/sync/mod.rs:395), [src/sync/mod.rs:434](/tmp/ude-snapshot/src/sync/mod.rs:434), [src/main.rs:377](/tmp/ude-snapshot/src/main.rs:377).

   **Quoted code:** `if !target_checkout.join(".git").exists()`; `Ok((target_checkout, lock_guard))`; `tmpl_dir.parent()`.

   **Risk:** No validation checks that the cache is a usable repository with the requested origin and ref. Interrupted clones or corrupt caches have no recovery path. The no-`templates/` fallback returns the checkout itself, but main then selects its parent as the Git checkout.

   **Remediation:** Return typed checkout/template roots separately; validate origin, repository identity, refs, and layout; quarantine failed caches.

4. **High — remote locks do not serialize local project state.**

   **Citation:** [src/sync/mod.rs:388](/tmp/ude-snapshot/src/sync/mod.rs:388), [src/sync/state.rs:57](/tmp/ude-snapshot/src/sync/state.rs:57).

   **Quoted code:** `format!("{remote_url}#{branch}")`; `fs::write(&state_path, json)`.

   **Risk:** Different remotes/branches acquire different locks while modifying the same project. Local-template sync acquires none. Concurrent read/modify/write operations can lose ancestor records or interleave file updates.

   **Remediation:** Add a project-level lock and atomic state replacement. Bind ancestry to upstream identity; the current state records do not identify the remote or branch.

5. **High — corrupted state and subprocess failure both collapse into benign-looking results.**

   **Citation:** [src/sync/state.rs:38](/tmp/ude-snapshot/src/sync/state.rs:38), [src/sync/mod.rs:725](/tmp/ude-snapshot/src/sync/mod.rs:725), [src/sync/mod.rs:762](/tmp/ude-snapshot/src/sync/mod.rs:762).

   **Quoted code:** `Self::default()` after failed state loading; `if status.stdout.is_empty()`; parsing `diff_cached.stdout` without checking its exit status.

   **Risk:** Corruption becomes “no ancestry”; failed status becomes “no changes”; failed staged-diff inspection can become an empty inspection. Missing `ancestor_content` also causes stored hashes to be ignored by content-based comparison.

   **Remediation:** Distinguish missing from corrupt state, validate schema/hash consistency, and require successful subprocess exits before interpreting stdout.

6. **High — semantic transport exposes more information than the safety model acknowledges.**

   **Citation:** [src/semantic/mod.rs:120](/tmp/ude-snapshot/src/semantic/mod.rs:120), [src/semantic/mod.rs:141](/tmp/ude-snapshot/src/semantic/mod.rs:141), [src/semantic/mod.rs:168](/tmp/ude-snapshot/src/semantic/mod.rs:168).

   **Quoted code:** `"original_rule": original_text`; `&format!("Authorization: Bearer {api_key}")`; `"body: {resp_str}"`.

   **Risk:** The upstream original is transmitted without the CLI’s candidate secret scan. The API key is put in process arguments. Invalid-response bodies are copied into diagnostic output. Exposure depends on process visibility and response contents; no real credential leak was exercised.

   **Remediation:** Use an HTTP client with protected credential handling, scan both inputs, and redact response diagnostics.

7. **Medium — input size is unbounded across diffing, rendering, and state retention.**

   **Citation:** [src/sync/merge.rs:99](/tmp/ude-snapshot/src/sync/merge.rs:99), [src/sync/state.rs:13](/tmp/ude-snapshot/src/sync/state.rs:13).

   **Quoted code:** Nested `for l in &local_edits` / `for u in &upstream_edits`; `ancestor_content: Option<String>`.

   **Risk:** Large alternating edits multiply overlap comparisons. Whole-file reads, duplicated content, persisted ancestors, and unrestricted template rendering add memory/CPU pressure. This is a load-risk hypothesis, not a measured exhaustion result.

   **Remediation:** Establish file/render limits and benchmark adversarial edit patterns before claiming safe edge-load behavior.

## RESIDUAL RISKS & TEST GAPS

- **The existing green suite misses demonstrated failures.** The unmodified snapshot passed **47 unit tests, 8 CLI tests, and Clippy**. Additional temporary Rust probes produced **13 failing regression assertions**. Those results establish specific counterexamples, not failure rates or comprehensive platform coverage. Probe source: [review_probe_extended.rs](/tmp/ude-adversarial-wna89x52/review_probe_extended.rs).

- **Deletion tests stop at classification.** [src/sync/mod.rs:1010](/tmp/ude-snapshot/src/sync/mod.rs:1010) verifies statuses without executing deletion propagation. Add upstream deletion, local deletion, simultaneous deletion, and delete-versus-edit controls.

- **PR tests exercise one proposal only.** [tests/e2e_lifecycle.rs:286](/tmp/ude-snapshot/tests/e2e_lifecycle.rs:286) invokes a single `push --pr`. Add second proposal, PR-to-direct transition, merged/rejected proposal, missing `gh`, and a post-push disconnect. Inspect remote trees and ancestry, not just exit status.

- **Atomicity tests exercise rejection before writes.** [src/sync/mod.rs:1125](/tmp/ude-snapshot/src/sync/mod.rs:1125) covers conflict prevalidation. Add second-write failure, state-save failure, permission failure, crash interruption, and recovery controls.

- **Doctor’s “fixed” signer fixture is itself invalid.** [tests/e2e_lifecycle.rs:96](/tmp/ude-snapshot/tests/e2e_lifecycle.rs:96) uses `"AAAAC3NzaC1lZDI1NTE5AAAAI..."`. Add real generated public keys, malformed encoding, wrong identities, identical maintainer/agent keys, ineffective includes, and index/disk permission disagreement.

- **Semantic tests validate self-authored JSON, not operational decisions.** [src/semantic/mod.rs:188](/tmp/ude-snapshot/src/semantic/mod.rs:188) and [src/semantic/mod.rs:217](/tmp/ude-snapshot/src/semantic/mod.rs:217) omit missing answers, wrong types, zero confidence, invalid probabilities, HTTP errors, and private-override exclusion. The runtime semantic reproductions above used local mocks, not live Jev judgments.

- **Mandatory hook controls are explicitly deferred.** [docs/SPECIFICATION.md:155](/tmp/ude-snapshot/docs/SPECIFICATION.md:155) requires automated negative controls; [docs/BACKLOG.md:24](/tmp/ude-snapshot/docs/BACKLOG.md:24) describes manual behavior testing. Also, backlog expects `master` protection, while [templates/projects/generic/hooks/pre-commit:13](/tmp/ude-snapshot/templates/projects/generic/hooks/pre-commit:13) protects `main` and `develop`.

- **The claimed 2×2 sync surface has no host-sync control.** [src/sync/mod.rs:314](/tmp/ude-snapshot/src/sync/mod.rs:314) enumerates project files only; [README.md:36](/tmp/ude-snapshot/README.md:36) claims host and project reconciliation. Add a test that changes a universal template and verifies its intended host destination—or narrow the advertised scope.

- **Acceptance language exceeds any falsifiable contract.** [docs/ARCHITECTURE.md:73](/tmp/ude-snapshot/docs/ARCHITECTURE.md:73) promises synchronization `"without risk of credential leakage or machine path corruption"`. Supported encodings, credential classes, entropy thresholds, path grammar, size limits, and platform behavior are unspecified. Define those boundaries and require reject/accept pairs for each.

- **Verification limits:** No live GitHub publication, authenticated TypeSafe request, Windows/macOS execution, network-partition test, or multi-process local-state race was performed. Git publication probes used disposable local remotes and a failing local `gh` stub. The supplied snapshot has no Git metadata, so findings are pinned to these files and lines, not a commit SHA.