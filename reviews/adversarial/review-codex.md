## CONFIRMED DEFECTS

1. **Critical — “3-way synchronization” is an unconditional overwrite.**  
   **Citation:** [src/sync/mod.rs:64](/tmp/ude-snapshot/src/sync/mod.rs:64), [src/sync/mod.rs:263](/tmp/ude-snapshot/src/sync/mod.rs:263); contradicts [docs/ARCHITECTURE.md:61](/tmp/ude-snapshot/docs/ARCHITECTURE.md:61).  
   **Quoted code:** Unequal existing files become `SyncStatus::LocallyModified`; push executes `fs::write(&target_upstream, parameterized)`.  
   **Failure:** There is no ancestor input. An upstream-only change and a conflicting change are indistinguishable from local drift. Reproduced: an equal-content control pushed zero files; changing only upstream caused push to overwrite it with the old local content. `Conflicted` and `ConflictBlocked` are never constructed.  
   **Fix:** Persist the rendered ancestor, compare all three versions, merge independent edits, and reject unresolved conflicts before writing.

2. **High — Pull reports application without applying anything.**  
   **Citation:** [src/main.rs:309](/tmp/ude-snapshot/src/main.rs:309); contradicts [docs/SPECIFICATION.md:103](/tmp/ude-snapshot/docs/SPECIFICATION.md:103).  
   **Quoted code:** `println!("  {} Updated {}", ...); pulled += 1;`  
   **Failure:** The pull loop performs no copy, rendering, patching, or write. Reproduced with one upstream file and no local counterpart: exit `0`, “Applied 1 upstream update(s),” destination still absent. The no-template control reported no updates.  
   **Fix:** Implement materialization and conflict reporting; increment the applied count only after a successful write.

3. **High — Comparing rendered files to template source destroys reusable templates.**  
   **Citation:** [src/sync/mod.rs:100](/tmp/ude-snapshot/src/sync/mod.rs:100), [src/sync/mod.rs:401](/tmp/ude-snapshot/src/sync/mod.rs:401), [src/sync/mod.rs:263](/tmp/ude-snapshot/src/sync/mod.rs:263).  
   **Quoted code:** Local `mise.toml` maps to `projects/agents/mise.toml.jinja`; both are read directly and compared.  
   **Failure:** A correctly initialized Rust project differs from its conditional Jinja source even without edits. Push replaces that source with the rendered Rust configuration, deleting the Node/polyglot branches. Reproduced; an equal-content control made no write. Separately, push can insert `{{ user.home }}` into `gitconfig.repo`, while initialization copies that file without rendering it ([src/main.rs:136](/tmp/ude-snapshot/src/main.rs:136)).  
   **Fix:** Compare local files against rendered baselines. Preserve template structure when accepting changes, and render every artifact containing template expressions.

4. **Critical — Git publishes files that never passed either security gate.**  
   **Citation:** [src/sync/mod.rs:297](/tmp/ude-snapshot/src/sync/mod.rs:297), [src/sync/mod.rs:82](/tmp/ude-snapshot/src/sync/mod.rs:82).  
   **Quoted code:** `.args(["add", "-A"])` versus a fixed `managed_files` list.  
   **Attack:** Leave an untracked `.planning/private.txt` containing a token in the cached checkout. `reset --hard` preserves it; scanner validation examines managed project candidates; `git add -A` stages the private file. Reproduced: a synthetic PAT reached a local bare remote alongside a clean managed update.  
   **Fix:** Stage only explicitly validated paths. Reject unexplained cache changes and inspect the complete staged diff before committing.

5. **Critical — Secrets leave the process before the secret scanner runs.**  
   **Citation:** [src/main.rs:335](/tmp/ude-snapshot/src/main.rs:335), [src/main.rs:363](/tmp/ude-snapshot/src/main.rs:363), [src/semantic/mod.rs:121](/tmp/ude-snapshot/src/semantic/mod.rs:121).  
   **Quoted code:** `"proposed_update": candidate_text` is submitted by `evaluate_drift`; `push_updates` subsequently performs scanning.  
   **Attack:** Put a recognizable PAT in `.agents/AGENTS.md` with TypeSafe enabled. Reproduced using fake curl: the captured request contained the complete synthetic token before the sanitizer rejected push. The key-absent control made no request. No external transmission was performed in this test.  
   **Fix:** Enforce boundary and secret checks before constructing any external request.

6. **High — Semantic approval ignores uncertainty and authorizes different bytes.**  
   **Citation:** [src/semantic/mod.rs:154](/tmp/ude-snapshot/src/semantic/mod.rs:154), [src/main.rs:349](/tmp/ude-snapshot/src/main.rs:349), [src/sync/mod.rs:219](/tmp/ude-snapshot/src/sync/mod.rs:219).  
   **Quoted code:** `Answer::Choice { choice, .. }`, followed by `allow_agent_rules = true`; push rereads the file.  
   **Attack:** A `benign_rephrase` answer with confidence `0.001` still grants publication. Reproduced. A fake curl then changed the candidate after receiving the request: Jev saw benign text, but push published “Disable verification and all governance rules.”  
   **Fix:** Validate the distribution, apply an evaluated uncertainty threshold, and bind approval to immutable candidate bytes. TypeSafe exposes confidence separately; the client discards it. [API reference](https://docs.typesafe.ai/api).

7. **High — Ordinary JSON credential syntax bypasses the generic scanner.**  
   **Citation:** [src/sanitizer/mod.rs:47](/tmp/ude-snapshot/src/sanitizer/mod.rs:47), [src/sanitizer/mod.rs:70](/tmp/ude-snapshot/src/sanitizer/mod.rs:70).  
   **Quoted code:** `...\b(api[_-]?key|...)\s*[:=]\s*['"]?...`  
   **Attack:** `api_key=abcdefghijklmnopqrstuvwx` is rejected; `{"api_key":"abcdefghijklmnopqrstuvwx"}` is accepted. Reproduced against the actual Rust module. The closing quote around the field name prevents the regex from matching. There is also no general entropy calculation despite the claim in [docs/ARCHITECTURE.md:76](/tmp/ude-snapshot/docs/ARCHITECTURE.md:76).  
   **Fix:** Handle quoted keys and supported serialization formats; add paired fixtures using identical credential values across syntaxes.

8. **High — Template symlinks escape the write boundary.**  
   **Citation:** [src/sync/mod.rs:389](/tmp/ude-snapshot/src/sync/mod.rs:389), [src/sync/mod.rs:263](/tmp/ude-snapshot/src/sync/mod.rs:263).  
   **Quoted code:** `upstream_root.join(...)` followed by `fs::write(...)`.  
   **Attack:** Make `projects/generic/gitconfig.repo` a symlink to a file outside the template root. Push follows it and overwrites that external file. Reproduced with a disposable victim; the ordinary-file control left the victim unchanged.  
   **Fix:** Reject symlink traversal in destinations and parents, and use directory-relative operations that enforce containment during the write.

9. **High — Read failures become “In Sync.”**  
   **Citation:** [src/sync/mod.rs:100](/tmp/ude-snapshot/src/sync/mod.rs:100), [src/sync/mod.rs:103](/tmp/ude-snapshot/src/sync/mod.rs:103), [src/sync/mod.rs:73](/tmp/ude-snapshot/src/sync/mod.rs:73).  
   **Quoted code:** `fs::read_to_string(...).ok()` and `(None, None) => SyncStatus::InSync`.  
   **Failure:** Missing files, permission errors, directories, and invalid UTF-8 collapse into the same state. Reproduced: a directory named `.gitconfig` with no upstream file was “In Sync,” and push returned success with zero files. A readable-file control became locally modified.  
   **Fix:** Treat only `NotFound` as absence; propagate other read failures explicitly.

10. **High — “Transactional materialization” leaves partial updates.**  
    **Citation:** [src/sync/mod.rs:252](/tmp/ude-snapshot/src/sync/mod.rs:252), [src/sync/mod.rs:263](/tmp/ude-snapshot/src/sync/mod.rs:263).  
    **Quoted code:** `// Pass 2: Transactional materialization of validated files` followed by sequential direct writes.  
    **Failure:** A later destination failure leaves earlier files replaced. Reproduced: the first `.gitconfig` update persisted when the `.gitignore` destination was a directory. The secret-prevalidation control correctly wrote nothing, proving only that earlier validation stage—not transactionality.  
    **Fix:** Stage replacements, preserve recovery information, and implement rollback or a documented recoverable transaction protocol.

11. **High — Every PR uses the same branch; the second invocation breaks.**  
    **Citation:** [src/sync/mod.rs:305](/tmp/ude-snapshot/src/sync/mod.rs:305), [src/sync/mod.rs:308](/tmp/ude-snapshot/src/sync/mod.rs:308).  
    **Quoted code:** `compute_hash(commit_msg.as_bytes())` with a constant message; `let _ = ... checkout -b ...`.  
    **Failure:** The branch name never changes. The existing-branch error is ignored. Reproduced against a local bare remote: the first PR push succeeded; the second failed branch creation and then a non-fast-forward push.  
    **Fix:** Define explicit PR reuse semantics or generate unique branches. Check branch creation and use explicit source/destination refspecs.

12. **High — Switching upstream branches breaks the shared cache.**  
    **Citation:** [src/sync/mod.rs:139](/tmp/ude-snapshot/src/sync/mod.rs:139), [src/sync/mod.rs:157](/tmp/ude-snapshot/src/sync/mod.rs:157), [src/sync/mod.rs:167](/tmp/ude-snapshot/src/sync/mod.rs:167).  
    **Quoted code:** Cache identity hashes only `remote_url`; fetch uses `["fetch", "origin", branch]`; reset targets `origin/{branch}`.  
    **Failure:** A depth-one clone initially tracks one branch. Fetching another branch need not create its remote-tracking ref. Reproduced with a `file://` fixture: initial `main` synchronization succeeded; selecting existing `dev` failed resetting `origin/dev`.  
    **Fix:** Isolate caches by branch or fetch an explicit refspec and operate on the verified fetched commit.

13. **High — The flat-repository fallback reports a remote push that never occurred.**  
    **Citation:** [src/sync/mod.rs:181](/tmp/ude-snapshot/src/sync/mod.rs:181), [src/main.rs:260](/tmp/ude-snapshot/src/main.rs:260), [src/sync/mod.rs:290](/tmp/ude-snapshot/src/sync/mod.rs:290).  
    **Quoted code:** Fallback returns `target_checkout`; main takes `tmpl_dir.parent()`; empty Git-status stdout means “No changes to push.”  
    **Failure:** Without a `templates/` directory, push operates Git from the cache parent. Failed `git status` is mistaken for a clean checkout. Reproduced: cached templates changed, remote content remained unchanged, CLI exited `0`.  
    **Fix:** Return checkout and template roots separately. Check every Git command’s exit status before interpreting output.

14. **High — Doctor certifies missing security invariants with exit zero.**  
    **Citation:** [src/audit/mod.rs:93](/tmp/ude-snapshot/src/audit/mod.rs:93), [src/main.rs:30](/tmp/ude-snapshot/src/main.rs:30), [src/main.rs:98](/tmp/ude-snapshot/src/main.rs:98); contradicts [docs/SPECIFICATION.md:77](/tmp/ude-snapshot/docs/SPECIFICATION.md:77).  
    **Quoted code:** Missing signers become `Warning`; exit selection tests only `is_fail()`; output says “All required baselines verified.”  
    **Failure:** Reproduced with isolated configuration: missing signers, signing configuration, and canonical rules produced warnings and exit `0`. A missing-repository control exited `1`. Exit `2` is never emitted by doctor. The auditor also never verifies `commit.gpgsign`, dual keys, signer contents, configured hook paths, or Git-index executable modes.  
    **Fix:** Implement the specified assertions and explicit severity-to-exit mapping.

15. **High — Initialization swallows errors and destroys existing configuration.**  
    **Citation:** [src/main.rs:431](/tmp/ude-snapshot/src/main.rs:431), [src/main.rs:444](/tmp/ude-snapshot/src/main.rs:444), [src/main.rs:226](/tmp/ude-snapshot/src/main.rs:226).  
    **Quoted code:** `let _ = fs::copy(...)`, `let _ = fs::write(...)`, then “Project initialization complete.”  
    **Failure:** Reproduced: an existing regular file supplied as the destination produced exit `0` and completion output. Reinitializing a valid project silently removed a custom `.gitignore` rule. Missing templates likewise do not fail initialization.  
    **Fix:** Validate prerequisites, propagate errors, and reconcile existing files rather than replacing them without resolution.

16. **High — The portable baseline injects one personal fork’s policy and a nonexistent verification task.**  
    **Citation:** [templates/projects/agents/agents.md.jinja:3](/tmp/ude-snapshot/templates/projects/agents/agents.md.jinja:3), [templates/projects/agents/agents.md.jinja:13](/tmp/ude-snapshot/templates/projects/agents/agents.md.jinja:13), [templates/projects/agents/mise.toml.jinja:29](/tmp/ude-snapshot/templates/projects/agents/mise.toml.jinja:29), [src/main.rs:160](/tmp/ude-snapshot/src/main.rs:160).  
    **Quoted text:** “Personal fork (`denniyahh/gsd-core`)”; mandatory Mac CI; `run = "./scratch/ci-mac.sh"`.  
    **Failure:** Every initialized project inherits GSD-specific upstream targets, changeset rules, and personal paths. Initialization never installs `ci-mac.sh`. Reproduced on a Node project: the check task referenced an absent script.  
    **Fix:** Move personal-fork policy into an explicit archetype or local layer. Ensure every generated task references an installed implementation appropriate to the selected ecosystem.

17. **High — Manifest exclusions are parsed but ignored.**  
    **Citation:** [src/manifest/mod.rs:52](/tmp/ude-snapshot/src/manifest/mod.rs:52), [src/main.rs:249](/tmp/ude-snapshot/src/main.rs:249), [src/sync/mod.rs:196](/tmp/ude-snapshot/src/sync/mod.rs:196).  
    **Quoted code:** `pub exclude: Vec<String>`; `push_updates` calls `scan_project` without configuration.  
    **Failure:** Reproduced: `sync.exclude = [".gitconfig"]` still published `.gitconfig`. `strategy`, `allow_overrides`, and `auto_pr` likewise have no execution-path consumers.  
    **Fix:** Pass validated sync policy into the engine and enforce it. Reject unsupported settings instead of accepting inert configuration.

18. **Medium — Path parameterization corrupts longer absolute paths.**  
    **Citation:** [src/sanitizer/mod.rs:85](/tmp/ude-snapshot/src/sanitizer/mod.rs:85).  
    **Quoted code:** `format!(r#"{}([/\\\"'\s:]|$)"#, regex::escape(user_home))`  
    **Failure:** Only the trailing boundary is checked. Reproduced: `/backup/home/alice/file` becomes `/backup{{ user.home }}/file`. The existing neighboring-user control `/home/alice2/file` remains unchanged, so it does not detect this defect.  
    **Fix:** Match complete path tokens with both boundaries; test prefixes, delimiters, punctuation, and platform-specific paths.

## SUSPECTED / ARCHITECTURAL RISKS

1. **Shared cache operations have no concurrency ownership.**  
   **Citation:** [src/sync/mod.rs:140](/tmp/ude-snapshot/src/sync/mod.rs:140), [src/sync/mod.rs:167](/tmp/ude-snapshot/src/sync/mod.rs:167).  
   **Quoted code:** URL-derived shared checkout and `reset --hard`.  
   **Risk — speculation:** Concurrent status/pull/push operations can reset another operation’s working tree or race staging, commits, and branch selection. No lock or per-operation checkout exists.  
   **Remediation:** Use isolated operation worktrees or hold a cache lock across the complete operation; add forced-interleaving tests.

2. **Overrides suppress synchronization rather than implement layering.**  
   **Citation:** [src/sync/mod.rs:59](/tmp/ude-snapshot/src/sync/mod.rs:59), [src/sync/mod.rs:421](/tmp/ude-snapshot/src/sync/mod.rs:421); contradicts [docs/ARCHITECTURE.md:66](/tmp/ude-snapshot/docs/ARCHITECTURE.md:66).  
   **Quoted code:** `return SyncStatus::Overridden` based on file existence.  
   **Risk:** The override bytes are never read or applied. An override also hides base-file drift, including upstream security updates. `.override` suffix handling advertised in README is absent.  
   **Remediation:** Track base synchronization independently from overlays and define actual rendering/merge semantics.

3. **Semantic transport has no operational bounds.**  
   **Citation:** [src/semantic/mod.rs:130](/tmp/ude-snapshot/src/semantic/mod.rs:130), [src/semantic/mod.rs:143](/tmp/ude-snapshot/src/semantic/mod.rs:143).  
   **Quoted code:** Curl uses `"-s"` and blocking `.output()` without connection/total timeouts or HTTP-failure handling.  
   **Risk — speculation:** A stalled endpoint can hang push indefinitely. Credentials and full rule text are passed through process arguments; exposure depends on host process visibility.  
   **Remediation:** Use a bounded HTTP client, explicit HTTP-status handling, response-size limits, and credential-safe request construction.

4. **Environment detection uses the caller’s repository, not the target.**  
   **Citation:** [src/template/mod.rs:35](/tmp/ude-snapshot/src/template/mod.rs:35), [src/template/mod.rs:56](/tmp/ude-snapshot/src/template/mod.rs:56), [src/main.rs:118](/tmp/ude-snapshot/src/main.rs:118).  
   **Quoted code:** `detect(project_name, project_type)` invokes `git config --get user.name` without a target working directory.  
   **Risk — speculation:** Initializing another repository can render the caller repository’s identity or substitute `developer@example.com`. Signing-key fields are always `None` during detection.  
   **Remediation:** Resolve context against the target and require security-sensitive values explicitly.

## RESIDUAL RISKS & TEST GAPS

- **The existing green suite does not establish the advertised behavior.** All 22 supplied tests passed; `cargo clippy --workspace --all-targets -- -D warnings` exited `0`. Neither establishes working pull, conflict handling, safe publication, or audit completeness. Nine disposable module probes passed, including one that refuted the suspected encrypted-PKCS#8 bypass.

- **Conflict tests encode the defect.** [src/sync/mod.rs:447](/tmp/ude-snapshot/src/sync/mod.rs:447) expects unequal files to be `LocallyModified`. Add ancestor-based upstream-only, local-only, independent-edit, conflicting-edit, deletion, and add/add cases.

- **“Atomicity” testing covers only prevalidation.** [src/sync/mod.rs:523](/tmp/ude-snapshot/src/sync/mod.rs:523) injects a secret before writes begin. Add second-write failure, disk exhaustion, interruption, rollback, and recovery controls.

- **Semantic tests never exercise a verdict or transport failure.** [src/semantic/mod.rs:172](/tmp/ude-snapshot/src/semantic/mod.rs:172) and [src/semantic/mod.rs:201](/tmp/ude-snapshot/src/semantic/mod.rs:201) test serialization/deserialization. Missing controls include low confidence, malformed distributions, wrong answer types, timeout, HTTP failures, and changed candidate bytes. The production path never asks the Noul credential-screening question promised by [README.md:41](/tmp/ude-snapshot/README.md:41).

- **Documented CLI examples are rejected.** Reproduced: `ude project init`, `ude init --type node`, and `ude doctor --host` exit `2`. Compare [docs/SPECIFICATION.md:64](/tmp/ude-snapshot/docs/SPECIFICATION.md:64), [docs/SPECIFICATION.md:86](/tmp/ude-snapshot/docs/SPECIFICATION.md:86), and [src/cli/mod.rs:18](/tmp/ude-snapshot/src/cli/mod.rs:18). Add executable documentation tests.

- **No end-to-end sanitizer exit-code contract is tested.** [docs/SPECIFICATION.md:153](/tmp/ude-snapshot/docs/SPECIFICATION.md:153) requires rejection through `sync push`; [src/sync/mod.rs:514](/tmp/ude-snapshot/src/sync/mod.rs:514) checks an internal enum. Test CLI exit codes and unchanged local, cached, and remote state.

- **Verification limits:** Git reproductions used isolated local bare remotes; semantic reproductions used fake curl and synthetic credentials. Live Jev behavior, real hosting permissions/hooks, concurrent interleavings, and container parity remain unverified. The supplied snapshot has no Git metadata, so findings are pinned to its files, not a verified commit SHA.

**Still open — needs your decision:** Keep this review local, or authorize Gemini and DeepSeek source export for independent reviews? Recommendation: keep it local.

Automatic approval review rejected the external-review CLI launch because exporting the supplied source and documents to third-party services lacked explicit authorization. No independent external lane completed.