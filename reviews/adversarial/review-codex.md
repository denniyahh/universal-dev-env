## CONFIRMED DEFECTS

1. **[Critical] `sync pull` reports updates it never applies.**  
   **Citation:** [src/main.rs:295](/tmp/ude-snapshot/src/main.rs:295): `if s.status == sync::SyncStatus::UpstreamModified`; [src/main.rs:297](/tmp/ude-snapshot/src/main.rs:297): `pulled += 1;`; [src/main.rs:303](/tmp/ude-snapshot/src/main.rs:303): `"Applied {} upstream update(s)."`  
   **Failure:** The handler scans and prints; it never writes, renders, patches, or merges. A CLI probe reported **11 applied updates**, exited `0`, and left `.gitconfig` absent. This violates [docs/SPECIFICATION.md:103](/tmp/ude-snapshot/docs/SPECIFICATION.md:103): “Applies clean 3-way updates.”  
   **Fix:** Implement application and count successful writes. Return failure when application fails.

2. **[Critical] The advertised three-way reconciler cannot distinguish upstream edits from local edits.**  
   **Citation:** [src/sync/mod.rs:64](/tmp/ude-snapshot/src/sync/mod.rs:64): `if local == upstream`; [src/sync/mod.rs:67](/tmp/ude-snapshot/src/sync/mod.rs:67): `SyncStatus::LocallyModified`.  
   **Failure:** With ancestor `A`, unchanged local `A`, and updated upstream `B`, the engine calls the local file modified and allows it to overwrite upstream. There is no ancestor input or persisted ancestor state. `Conflicted` and `ConflictBlocked` have no producing path. This contradicts [docs/ARCHITECTURE.md:61](/tmp/ude-snapshot/docs/ARCHITECTURE.md:61): “Ancestor Snapshot vs. Central Upstream vs. Local State.”  
   **Fix:** Persist the last applied revision and rendered contents; implement actual three-way comparison, merge, and conflict rejection.

3. **[Critical] The secret gate accepts ordinary credential formats.**  
   **Citation:** [src/sanitizer/mod.rs:25](/tmp/ude-snapshot/src/sanitizer/mod.rs:25): `r"-----BEGIN [A-Z0-9_-]+ PRIVATE KEY-----"`; [src/sanitizer/mod.rs:27](/tmp/ude-snapshot/src/sanitizer/mod.rs:27): `r"gh[pousr]_[A-Za-z0-9_]{36,255}"`; [src/sanitizer/mod.rs:29](/tmp/ude-snapshot/src/sanitizer/mod.rs:29): the generic pattern requires a quoted value.  
   **Attack:** Executed probes returned `Clean` for:
   - `-----BEGIN PRIVATE KEY-----` — PKCS#8 private-key header.
   - `glpat-` plus a token body.
   - `github_pat_` plus a token body.
   - Unquoted `api_key=...`.
   - `Authorization: Bearer ...`.

   The same probe correctly rejected a classic `ghp_` fixture. These are demonstrated coverage gaps, not a broken test harness. [docs/ARCHITECTURE.md:76](/tmp/ude-snapshot/docs/ARCHITECTURE.md:76) explicitly promises GitLab and bearer-token rejection.  
   **Fix:** Cover supported credential formats, assignment/header syntax, and private-key encodings; scan the actual staged payload.

4. **[Critical] Git staging bypasses the sanitizer’s file boundary.**  
   **Citation:** [src/sync/mod.rs:269](/tmp/ude-snapshot/src/sync/mod.rs:269): `.args(["add", "-A"])`; [src/sync/mod.rs:166](/tmp/ude-snapshot/src/sync/mod.rs:166): `.args(["reset", "--hard", ...])`.  
   **Attack:** An untracked credential file in the cache survives `reset --hard`. `git add -A` then stages it, although `push_updates` inspected only its fixed managed-file list. A local Git probe confirmed both survival and staging.  
   **Fix:** Stage an explicit validated file set and validate the complete staged diff. Reject unexplained cache contents.

5. **[High] A rejected push has already modified upstream files.**  
   **Citation:** [src/sync/mod.rs:195](/tmp/ude-snapshot/src/sync/mod.rs:195): `for state in states`; [src/sync/mod.rs:219](/tmp/ude-snapshot/src/sync/mod.rs:219): `return PushResult::SecretBlocked`; [src/sync/mod.rs:238](/tmp/ude-snapshot/src/sync/mod.rs:238): `fs::write(target_upstream, parameterized)`.  
   **Failure:** A clean `.gitconfig` followed by a secret-bearing `.gitignore` writes the first template before rejecting the second. The executed probe returned `SecretBlocked` while the earlier upstream file existed. The comment at line 184 claims “Pre-validates,” but validation and mutation are interleaved.  
   **Fix:** Validate every candidate before mutation; stage writes transactionally and define rollback behavior.

6. **[High] Failed filesystem writes are reported as successful synchronization.**  
   **Citation:** [src/sync/mod.rs:236](/tmp/ude-snapshot/src/sync/mod.rs:236): `let _ = fs::create_dir_all(parent);`; [src/sync/mod.rs:238](/tmp/ude-snapshot/src/sync/mod.rs:238): `let _ = fs::write(...)`; [src/sync/mod.rs:239](/tmp/ude-snapshot/src/sync/mod.rs:239): `synced += 1;`.  
   **Failure:** Making `upstream/projects` a regular file prevents destination creation. The executed probe still returned `Success { synced_files: 1 }`, with no destination file. Permission errors, full disks, and failed mounts take the same ignored-error path.  
   **Fix:** Propagate I/O failures and increment the count only after successful materialization.

7. **[High] Cached repositories cannot reliably switch branches.**  
   **Citation:** [src/sync/mod.rs:138](/tmp/ude-snapshot/src/sync/mod.rs:138): the cache key hashes only `remote_url`; [src/sync/mod.rs:144](/tmp/ude-snapshot/src/sync/mod.rs:144): `["clone", "--depth", "1", "-b", branch, remote_url]`; [src/sync/mod.rs:166](/tmp/ude-snapshot/src/sync/mod.rs:166): reset to `origin/{branch}`.  
   **Failure:** After a shallow clone of `main`, fetching another branch can populate `FETCH_HEAD` without creating `origin/other`. Replaying these commands produced reset exit `128`: unknown revision `origin/other`. Resetting to `FETCH_HEAD` worked, but left the checkout attached to `main`.  
   **Fix:** Fetch an explicit destination refspec and explicitly establish the requested branch, preferably in an isolated worktree.

8. **[High] Every automated PR uses the same branch; subsequent pushes fail.**  
   **Citation:** [src/sync/mod.rs:277](/tmp/ude-snapshot/src/sync/mod.rs:277): the fixed `commit_msg`; [src/sync/mod.rs:280](/tmp/ude-snapshot/src/sync/mod.rs:280): `compute_hash(commit_msg.as_bytes())`; [src/sync/mod.rs:281](/tmp/ude-snapshot/src/sync/mod.rs:281): checkout result discarded.  
   **Failure:** The branch name never changes. After the first PR, cache refresh rewinds that branch. Creating it again fails silently; the next commit forms a competing history and its push is rejected. A local replay reproduced checkout exit `128` and push exit `1`.  
   **Fix:** Define whether each operation creates a new PR or updates an existing one. Check checkout results and preserve the corresponding branch history.

9. **[High] Direct push after PR mode can succeed without publishing the new commit.**  
   **Citation:** [src/sync/mod.rs:166](/tmp/ude-snapshot/src/sync/mod.rs:166): `reset --hard origin/{branch}`; [src/sync/mod.rs:345](/tmp/ude-snapshot/src/sync/mod.rs:345): `.args(["push", "origin", branch])`.  
   **Failure:** Reset changes the current PR branch’s commit, not its branch identity. The subsequent commit lands on that PR branch, while `git push origin main` pushes local `main`. A local replay returned success with remote `main` unchanged and unequal to the intended commit.  
   **Fix:** Establish the correct local branch and use an explicit, validated refspec such as `HEAD:refs/heads/<target>`.

10. **[High] Read failures become absence, and absence can become “In Sync.”**  
    **Citation:** [src/sync/mod.rs:99](/tmp/ude-snapshot/src/sync/mod.rs:99): `fs::read_to_string(&local_path).ok()`; [src/sync/mod.rs:72](/tmp/ude-snapshot/src/sync/mod.rs:72): `(None, None) => SyncStatus::InSync`; [src/sync/mod.rs:213](/tmp/ude-snapshot/src/sync/mod.rs:213): `Err(_) => continue`.  
    **Failure:** A managed file containing invalid UTF-8, with no upstream counterpart, was reported `InSync` with `local_hash=None`. Unreadable files and failed reads are similarly hidden.  
    **Fix:** Distinguish `NotFound` from read/decode failure. Hash bytes and expose failures explicitly.

11. **[High] A repository without `templates/` gets the wrong Git working directory and a false success.**  
    **Citation:** [src/sync/mod.rs:180](/tmp/ude-snapshot/src/sync/mod.rs:180): `Ok(target_checkout)`; [src/main.rs:246](/tmp/ude-snapshot/src/main.rs:246): `tmpl_dir.parent()`; [src/sync/mod.rs:262](/tmp/ude-snapshot/src/sync/mod.rs:262): `if status.stdout.is_empty()`.  
    **Failure:** The supported root-layout fallback causes `main` to pass the cache parent to Git. A fatal `git status` commonly has empty stdout; the code ignores its exit status and returns “No changes to push.” A disposable non-repository probe confirmed exit `128` with empty stdout.  
    **Fix:** Return checkout root and template root separately, and check Git’s exit status before interpreting stdout.

12. **[High] Initialization truncates existing configuration and conceals failures.**  
    **Citation:** [src/main.rs:404](/tmp/ude-snapshot/src/main.rs:404): `let _ = fs::copy(src, dst);`; [src/main.rs:417](/tmp/ude-snapshot/src/main.rs:417): `let _ = fs::write(dst, rendered);`; [src/main.rs:212](/tmp/ude-snapshot/src/main.rs:212): `"Project initialization complete."`.  
    **Failure:** Existing `.gitignore`, hooks, `mise.toml`, and agent instructions are overwritten without reconciliation. Missing templates, rendering failures, and failed writes still reach unconditional success output. An installed-layout probe without templates exited `0` and claimed configuration while creating no `.gitconfig`.  
    **Fix:** Fail before mutation when templates are unavailable; preserve existing content through explicit reconciliation and propagate errors.

13. **[High] Linked-worktree initialization configures shared repository state and includes the primary checkout’s configuration.**  
    **Citation:** [src/main.rs:191](/tmp/ude-snapshot/src/main.rs:191): `["config", "--local", "include.path", "../.gitconfig"]`; [src/main.rs:197](/tmp/ude-snapshot/src/main.rs:197): `["config", "--local", "core.hooksPath", "scripts/hooks"]`.  
    **Failure:** In linked worktrees, `--local` targets shared configuration. The relative include resolves against the common Git config location. A primary/linked fixture with different `.gitconfig` values returned the primary value from both worktrees.  
    **Fix:** Use worktree-specific configuration with correctly resolved includes; test isolation from the primary checkout.

14. **[High] Doctor’s security checks are existence checks, and its documented exit contract is false.**  
    **Citation:** [src/audit/mod.rs:84](/tmp/ude-snapshot/src/audit/mod.rs:84): `if allowed_signers.exists()`; [src/audit/mod.rs:156](/tmp/ude-snapshot/src/audit/mod.rs:156): `if hook_file.exists()`; [src/main.rs:44](/tmp/ude-snapshot/src/main.rs:44): `return;`.  
    **Failure:** Empty signer files, non-executable hooks, inactive hook configuration, and unsigned commits are not adequately checked. Doctor does not inspect tracked hook mode `100755`, dual signing keys, or the required rules symlink mesh. An executed JSON audit of a nonexistent repository contained `Fail` but exited `0`. This contradicts [docs/SPECIFICATION.md:77](/tmp/ude-snapshot/docs/SPECIFICATION.md:77), which assigns nonzero statuses to failures and warnings.  
    **Fix:** Inspect effective configuration, signer contents, filesystem/index permissions, and aggregate severity into the specified exit code.

15. **[Medium] Template comparison makes correctly initialized projects look modified; reverse sync does not preserve template semantics.**  
    **Citation:** [src/sync/mod.rs:102](/tmp/ude-snapshot/src/sync/mod.rs:102): upstream templates are read directly; [src/sync/mod.rs:64](/tmp/ude-snapshot/src/sync/mod.rs:64): raw equality comparison; [src/main.rs:154](/tmp/ude-snapshot/src/main.rs:154): initialization renders `mise.toml.jinja`.  
    **Failure:** A rendered `mise.toml` cannot equal its source containing `{% if project_type ... %}`. Push replaces that shared conditional template with one project’s rendered result, removing other ecosystems’ branches. Parameterized `.repo` files and scripts are later copied without rendering.  
    **Fix:** Compare against the rendered baseline and preserve source-template structure during upstream proposals. Test render–push–render round trips.

16. **[Medium] “Layered overrides” suppress synchronization rather than apply a layer.**  
    **Citation:** [src/sync/mod.rs:58](/tmp/ude-snapshot/src/sync/mod.rs:58): `return SyncStatus::Overridden`; [src/sync/mod.rs:197](/tmp/ude-snapshot/src/sync/mod.rs:197): overridden files are skipped.  
    **Failure:** Merely creating an override sibling hides all base-file drift and suppresses unrelated base improvements. The override contents are never merged by the renderer. Generic `.override` files promised by [docs/SPECIFICATION.md:105](/tmp/ude-snapshot/docs/SPECIFICATION.md:105) are not recognized.  
    **Fix:** Track base drift independently, define actual overlay application, and exclude only the private overlay from publication.

17. **[Medium] Path parameterization corrupts neighboring paths and disagrees with the documented variable schema.**  
    **Citation:** [src/sanitizer/mod.rs:59](/tmp/ude-snapshot/src/sanitizer/mod.rs:59): `result.replace(user_home, "{{ user_home }}")`; [docs/ARCHITECTURE.md:79](/tmp/ude-snapshot/docs/ARCHITECTURE.md:79): `{{ user.home }}`.  
    **Failure:** With home `/home/alice`, an executed probe transformed `/home/alice2/file` into `{{ user_home }}2/file`. The implementation exposes flat variables, whereas the documents promise nested variables; rendering `{{ user.home }}` failed in the probe. Missing flat variables silently rendered empty.  
    **Fix:** Replace complete path prefixes at valid boundaries, standardize the context schema, and enable strict undefined-variable handling.

18. **[Medium] The publishing boundary contradicts the advertised agent-rule synchronization path.**  
    **Citation:** [src/main.rs:313](/tmp/ude-snapshot/src/main.rs:313): candidate `.agents/AGENTS.md`; [src/sanitizer/mod.rs:70](/tmp/ude-snapshot/src/sanitizer/mod.rs:70): `path_norm.starts_with(".agents/")`; [src/sync/mod.rs:205](/tmp/ude-snapshot/src/sync/mod.rs:205): boundary rejection.  
    **Failure:** A benign semantic verdict cannot make this candidate publishable: the deterministic gate rejects it afterward. The same document promises local rule refinement upstream while prohibiting `.agents/` publication at [docs/ARCHITECTURE.md:84](/tmp/ude-snapshot/docs/ARCHITECTURE.md:84).  
    **Fix:** Separate public-project contribution boundaries from central-template contribution boundaries, with explicit ownership and allowlists.

19. **[Medium] Manifest policy fields are decorative, and defaults depend on table presence.**  
    **Citation:** [src/manifest/mod.rs:42](/tmp/ude-snapshot/src/manifest/mod.rs:42): `strategy`; [src/manifest/mod.rs:44](/tmp/ude-snapshot/src/manifest/mod.rs:44): `allow_overrides`; [src/manifest/mod.rs:50](/tmp/ude-snapshot/src/manifest/mod.rs:50): `auto_pr`; [src/main.rs:231](/tmp/ude-snapshot/src/main.rs:231): only `.ude.toml` is loaded, with errors discarded.  
    **Failure:** Strategy, exclusions, override policy, automatic PR policy, and matrix selection do not drive operations. Host configuration and JSON manifests are not loaded. A probe found that omitting `[sync]` supplies four exclusions, while adding empty `[sync]` supplies none.  
    **Fix:** Centralize configuration resolution, reject malformed/unsupported policy, honor fields, and make defaults consistent.

20. **[Medium] The documented CLI commands fail; default-enabled features cannot be disabled.**  
    **Citation:** [docs/SPECIFICATION.md:86](/tmp/ude-snapshot/docs/SPECIFICATION.md:86): `ude project init`; [docs/SPECIFICATION.md:64](/tmp/ude-snapshot/docs/SPECIFICATION.md:64): `--host`; [src/cli/mod.rs:53](/tmp/ude-snapshot/src/cli/mod.rs:53): `#[arg(long, default_value_t = true)]`.  
    **Failure:** Executed commands rejected `ude project init`, `ude doctor --host`, and `ude init --mempalace=false`, each with exit `2`. Boolean flags default to enabled without a supported disabling argument.  
    **Fix:** Implement the documented command hierarchy/aliases and explicit true/false or disable switches.

21. **[High] Repository-specific workflow requirements leaked into the universal project baseline.**  
    **Citation:** [templates/projects/agents/agents.md.jinja:3](/tmp/ude-snapshot/templates/projects/agents/agents.md.jinja:3): `Personal fork (denniyahh/gsd-core)`; [templates/projects/agents/mise.toml.jinja:29](/tmp/ude-snapshot/templates/projects/agents/mise.toml.jinja:29): `run = "./scratch/ci-mac.sh"`; [templates/projects/agents/scratch/new-worktree.sh:43](/tmp/ude-snapshot/templates/projects/agents/scratch/new-worktree.sh:43): `git fetch upstream next`.  
    **Failure:** Arbitrary Rust/Node repositories inherit GSD-specific upstream rules, a mandatory SSH Mac runner, and references to scripts initialization never supplies. Fresh repositories typically lack `upstream/next`; the worktree helper also requires missing `ci-mac.sh` and budget scripts.  
    **Fix:** Move these requirements into an explicit GSD fork profile. Generic scaffolding must reference only supplied capabilities.

22. **[Medium] The implemented lifecycle does not cover the documented 2×2 matrix.**  
    **Citation:** [README.md:3](/tmp/ude-snapshot/README.md:3): “Configures host-wide developer baselines”; [src/sync/mod.rs:81](/tmp/ude-snapshot/src/sync/mod.rs:81): the managed-file list contains only project paths; [src/main.rs:120](/tmp/ude-snapshot/src/main.rs:120): initialization materializes project templates.  
    **Failure:** Universal Git/signing and agent templates exist, but no CLI application/synchronization path manages those quadrants. Doctor only samples their presence. README’s completed-phase claims therefore exceed the implemented lifecycle.  
    **Fix:** Implement explicit host operations and quadrant selection, or narrow the documented supported scope.

## SUSPECTED / ARCHITECTURAL RISKS

1. **Concurrent commands can corrupt shared cache state — speculation about scheduling, confirmed absence of operation locking.**  
   **Citation:** [src/sync/mod.rs:139](/tmp/ude-snapshot/src/sync/mod.rs:139): shared `target_checkout`; [src/sync/mod.rs:166](/tmp/ude-snapshot/src/sync/mod.rs:166): `reset --hard`; [src/sync/mod.rs:269](/tmp/ude-snapshot/src/sync/mod.rs:269): `add -A`.  
   **Risk:** One process can reset while another writes, stages, or commits. Git’s individual lock files do not make this multi-command sequence atomic. Even `sync status` refreshes and resets the cache.  
   **Remediation:** Lock the entire cache operation or allocate per-operation worktrees; test controlled interleavings.

2. **Cache refresh can erase recovery evidence after failed publication.**  
   **Citation:** [src/sync/mod.rs:166](/tmp/ude-snapshot/src/sync/mod.rs:166): `reset --hard`; [src/sync/mod.rs:350](/tmp/ude-snapshot/src/sync/mod.rs:350): return failure after push rejection.  
   **Risk:** A committed but unpublished update is rewound on the next invocation. Recovery depends on Git reflogs rather than an explicit operation record. Partial clones and malformed `.git` directories also have no repair protocol.  
   **Remediation:** Preserve pending commits and operation metadata; validate repository identity and recover damaged caches without deleting unreviewed work.

3. **Network and credential-helper stalls have no application deadline.**  
   **Citation:** [src/sync/mod.rs:146](/tmp/ude-snapshot/src/sync/mod.rs:146): clone `.status()`; [src/sync/mod.rs:321](/tmp/ude-snapshot/src/sync/mod.rs:321): PR `.output()`; [src/semantic/mod.rs:143](/tmp/ude-snapshot/src/semantic/mod.rs:143): curl `.output()`.  
   **Risk:** No child-process deadline, curl connection/total timeout, or noninteractive Git policy is supplied. Dropped connections and credential prompts can block automation. This was not tested against a live stalled remote.  
   **Remediation:** Bound process trees, disable prompting, classify retryable failures, and preserve pending changes.

4. **Semantic uncertainty and service failure do not form a meaningful governance gate.**  
   **Citation:** [src/semantic/mod.rs:154](/tmp/ude-snapshot/src/semantic/mod.rs:154): `Answer::Choice { choice, .. }`; [src/main.rs:340](/tmp/ude-snapshot/src/main.rs:340): `"semantic evaluation bypassed"`; [src/semantic/mod.rs:132](/tmp/ude-snapshot/src/semantic/mod.rs:132): curl uses `-s` without HTTP failure handling.  
   **Risk:** A mocked answer with confidence `0.0` returned `BenignRephrase`. Unknown labels and service errors only produce notices. No production secret-screening `Noul` request exists, despite the promised screening. The current API documents probabilities/confidence and explicit HTTP error statuses. [TypeSafe API reference](https://docs.typesafe.ai/api).  
   **Remediation:** Define uncertainty and outage policy, validate responses, handle HTTP status separately, and test governance decisions. The mock proves local decision behavior; it does not measure Jev’s accuracy.

5. **Filesystem and Git environment boundaries are not enforced.**  
   **Citation:** [src/sync/mod.rs:238](/tmp/ude-snapshot/src/sync/mod.rs:238): unchecked destination write; [src/main.rs:427](/tmp/ude-snapshot/src/main.rs:427): `permissions.set_mode(0o755)`; [src/sync/mod.rs:257](/tmp/ude-snapshot/src/sync/mod.rs:257): `.current_dir(remote_checkout)`.  
   **Risk:** Symlinked destinations can redirect writes and permission changes outside the intended root. Inherited `GIT_DIR`/`GIT_WORK_TREE` can redirect Git despite `current_dir`; a disposable Git probe demonstrated that redirection. Symlink exploitation through the complete CLI was not exercised.  
   **Remediation:** Validate containment and symlinks, preserve intended modes, and scrub repository-local Git variables on every subprocess.

6. **The boundary helper is lexical rather than a filesystem boundary.**  
   **Citation:** [src/sanitizer/mod.rs:67](/tmp/ude-snapshot/src/sanitizer/mod.rs:67): `trim_start_matches("./")`.  
   **Risk:** Executed helper probes rejected `.agents/AGENTS.md` but accepted `x/../.agents/AGENTS.md` and `.local/credentials`. The current fixed managed-file list limits direct exploitability; expanding manifest-driven paths would expose this flaw.  
   **Remediation:** Validate path components and destination containment; define private directories consistently across Rust and shell guards.

## RESIDUAL RISKS & TEST GAPS

- **Passing checks do not establish the advertised feature contracts.** The existing **17 tests passed**, independently matched by a source inventory; Clippy passed with `--workspace --all-targets -- -D warnings`. Nevertheless, targeted probes reproduced the defects above. These checks were local/offline, not container-parity or live-remote verification.

- **The remote-cache test does not test refresh or shallow-clone behavior.** [src/sync/mod.rs:578](/tmp/ude-snapshot/src/sync/mod.rs:578) calls `ensure_remote_repo` once; [src/sync/mod.rs:583](/tmp/ude-snapshot/src/sync/mod.rs:583) checks only file existence. Its local-path clone printed that `--depth` was ignored. Add second-fetch, changed-content, alternate-branch, damaged-cache, interrupted-clone, and concurrent-operation controls.

- **Secret tests omit the bypass formats and end-to-end exit behavior.** [docs/SPECIFICATION.md:153](/tmp/ude-snapshot/docs/SPECIFICATION.md:153) requires rejection with exit code `> 0`; [src/sync/mod.rs:485](/tmp/ude-snapshot/src/sync/mod.rs:485) only matches a Rust enum. Add CLI rejection tests, clean counterparts, harmless credential-like examples, and a staged file outside the managed list.

- **No auditor or CLI tests enforce their contracts.** Negative controls must include empty signer files, disabled signing, inactive hooks, hook mode `100644`, broken symlinks, documented CLI invocations, and doctor’s exact exit statuses. Assert resulting files and effective Git configuration, not success messages.

- **Semantic tests establish serialization only.** [src/semantic/mod.rs:171](/tmp/ude-snapshot/src/semantic/mod.rs:171) and [src/semantic/mod.rs:200](/tmp/ude-snapshot/src/semantic/mod.rs:200) do not exercise HTTP errors, missing answers, unknown labels, low confidence, delayed responses, or policy-shift decisions.

- **The shipped test fixture leaks host configuration.** [src/sync/mod.rs:548](/tmp/ude-snapshot/src/sync/mod.rs:548) launches Git without environment/config isolation. During the test run, the host’s global Gitleaks hook executed. Scrub Git variables and isolate HOME/global/system configuration before treating fixtures as hermetic.

- **“Zero-drift” has no operational acceptance definition.** [README.md:3](/tmp/ude-snapshot/README.md:3) promises “zero-drift, high-assurance repositories”; [README.md:70](/tmp/ude-snapshot/README.md:70) marks reconciliation complete. Require initialized-state convergence, second-run idempotence, independent local/upstream edits, conflicting edits, override preservation, and failed-write controls.

- **Large-input behavior remains unmeasured.** [src/sanitizer/mod.rs:48](/tmp/ude-snapshot/src/sanitizer/mod.rs:48) clones clean contents; [src/sanitizer/mod.rs:53](/tmp/ude-snapshot/src/sanitizer/mod.rs:53) clones them again. No size limit exists. Classic catastrophic-backtracking ReDoS is not established here: Rust’s regex engine documents bounded search complexity. Memory exhaustion and whole-file duplication still need load controls. [Regex documentation](https://docs.rs/regex/latest/regex/).

- **Still open — approval required for external lanes:** Do you authorize sending this snapshot to `agy` and `pi` for independent review? The [external-review skill](/var/home/denniyahh/.codex/skills/external-review/SKILL.md) requires “Run at least two lanes.”

Automatic approval review rejected those external invocations because they could expose sensitive code to unverified destinations. Neither external lane ran; this report contains source analysis and local probes only.