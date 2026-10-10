## CONFIRMED DEFECTS

**1. [High] A rejected remote push can cause the next pull to erase your local edit.**

[ src/sync/mod.rs:472 ](/tmp/ude-snapshot/src/sync/mod.rs:472) records the local edit as the ancestor: `"project_state.record_file(&rel_path, local_hash)"`. Line 477 saves it **before** [src/main.rs:421](/tmp/ude-snapshot/src/main.rs:421) calls `push_to_remote`. The next remote refresh executes `"reset", "--hard"` at [src/sync/mod.rs:236](/tmp/ude-snapshot/src/sync/mod.rs:236).

**Reproduced:** initialize both sides with `base`; edit locally; reject the push using a remote `pre-receive` hook; refresh the cache; pull. The rejected edit becomes `UpstreamModified` and is overwritten with `base`.

**Control:** an accepting disposable remote received the pushed commit. This is a broken ancestor transition on failure, not merely a Git error.

Persist the ancestor only after the intended remote synchronization succeeds.

**2. [High] The publication gate validates selected files, then commits additional, unvalidated files.**

[src/sync/mod.rs:504](/tmp/ude-snapshot/src/sync/mod.rs:504) runs `"git", "add", "templates"`. The subsequent check at [src/sync/mod.rs:522](/tmp/ude-snapshot/src/sync/mod.rs:522) accepts anything whose name `"starts_with(\"templates/\")"`. Neither operation restricts staging to the files sanitized by `push_updates`.

**Reproduced:** place a dummy GitHub token in cached `templates/unvalidated-secret.txt`; make one clean managed-file edit; push. The remote receives the token file.

**Control:** the identical token in managed `.gitconfig` produces `SecretBlocked`.

A directory prefix is not proof of validation. This defeats the “every upstream sync” sanitization requirement in [docs/ARCHITECTURE.md:73](/tmp/ude-snapshot/docs/ARCHITECTURE.md:73). Stage an explicit validated-file allowlist and inspect those staged bytes.

**3. [High] Path parameterization breaks the push/pull round trip for static files.**

[src/sanitizer/mod.rs:88](/tmp/ude-snapshot/src/sanitizer/mod.rs:88) inserts `"{{ user.home }}"`. But [src/sync/mod.rs:161](/tmp/ude-snapshot/src/sync/mod.rs:161) and [src/sync/mod.rs:303](/tmp/ude-snapshot/src/sync/mod.rs:303) render only files whose extension is `"jinja"`.

`.gitconfig` maps to `projects/generic/gitconfig.repo` at [src/sync/mod.rs:613](/tmp/ude-snapshot/src/sync/mod.rs:613); hooks similarly remain extensionless.

**Reproduced:** push `.gitconfig` containing `/home/fixture/.gitignore`. Immediately afterward, status reports `UpstreamModified`; pull writes literal `{{ user.home }}/.gitignore` into the live configuration.

**Control:** the same round trip without a home path remains `InSync`.

The parameterizer also misses repeated occurrences: `/home/user /home/user` becomes `{{ user.home }} /home/user`. The regex at [src/sanitizer/mod.rs:86](/tmp/ude-snapshot/src/sanitizer/mod.rs:86) consumes the separator needed by the next match. Neighboring-home and embedded-backup-path controls remained unchanged.

**4. [Medium] Both advertised reconciliation strategies have materially different implementations.**

[docs/ARCHITECTURE.md:62](/tmp/ude-snapshot/docs/ARCHITECTURE.md:62) promises: “changes merge cleanly” for orthogonal modifications. Actual code at [src/sync/mod.rs:88](/tmp/ude-snapshot/src/sync/mod.rs:88) unconditionally returns `Conflicted` for `"(false, false)"`.

**Reproduced:** ancestor `a=1,b=1`; local changes only `a`; upstream changes only `b`. Result: `Conflicted`. An overlapping-edit control also conflicts; an upstream-only control correctly returns `UpstreamModified`. Hash classification cannot implement the promised text merge.

The fallback is also misrepresented. [docs/ARCHITECTURE.md:66](/tmp/ude-snapshot/docs/ARCHITECTURE.md:66) says overrides are “automatically applied on top of the base template during rendering.” Instead, [src/sync/mod.rs:71](/tmp/ude-snapshot/src/sync/mod.rs:71) immediately returns `Overridden`, and pull processes only `UpstreamModified` at [src/sync/mod.rs:282](/tmp/ude-snapshot/src/sync/mod.rs:282).

**Reproduced:** adding `mise.local.toml` prevents an upstream base update entirely; removing it permits the update. The implementation freezes the base rather than rendering a layered configuration.

**5. [High] Successful CLI exits conceal failed initialization and failed security requirements.**

For initialization, [src/main.rs:493](/tmp/ude-snapshot/src/main.rs:493) ignores copy failures, and [src/main.rs:506](/tmp/ude-snapshot/src/main.rs:506) ignores write failures. Nevertheless, [src/main.rs:257](/tmp/ude-snapshot/src/main.rs:257) prints `"Project initialization complete."`

**Reproduced:** use an existing regular file as the destination. No `.gitconfig` can be created, yet `ude project init` exits **0** and claims completion. A directory destination creates the config as the opposite control. Re-running initialization also overwrites an existing `.gitignore` customization.

For doctor, [docs/SPECIFICATION.md:80](/tmp/ude-snapshot/docs/SPECIFICATION.md:80) requires **2** for missing allowed signers. [src/audit/mod.rs:93](/tmp/ude-snapshot/src/audit/mod.rs:93) classifies that condition as `Warning`; [src/main.rs:30](/tmp/ude-snapshot/src/main.rs:30) counts only failures.

**Reproduced:** isolated configuration with missing allowed signers returns **0**. Removing Git from `PATH` produces **1**, confirming the exit-status probe distinguishes failures.

Worse, [tests/e2e_lifecycle.rs:69](/tmp/ude-snapshot/tests/e2e_lifecycle.rs:69) explicitly asserts doctor must **not** exit 2, without distinguishing a parsing error from the documented security failure.

## SUSPECTED DEFECTS / SPECULATION

- **Concurrent sync may overwrite edits or publish another process’s changes.** Pull scans before writing at [src/sync/mod.rs:266](/tmp/ude-snapshot/src/sync/mod.rs:266), then calls `fs::write` at line 341 without checking whether the local bytes changed. The remote cache key includes only the URL at [src/sync/mod.rs:208](/tmp/ude-snapshot/src/sync/mod.rs:208), so concurrent branch operations share a checkout. No concurrent execution was reproduced.

- **The semantic transport’s advertised bound may exclude payload transmission.** [src/semantic/mod.rs:155](/tmp/ude-snapshot/src/semantic/mod.rs:155) performs blocking `stdin.write_all` and discards its result; line 159 waits without a parent-enforced deadline. Curl’s `--max-time` is not an explicit bound on these parent operations. Large-payload/backpressure behavior remains untested.

## RESIDUAL RISKS & TEST GAPS

- **“Atomic” means prevalidation, not rollback.** Writes happen sequentially at [src/sync/mod.rs:341](/tmp/ude-snapshot/src/sync/mod.rs:341) and [src/sync/mod.rs:465](/tmp/ude-snapshot/src/sync/mod.rs:465). State persistence directly overwrites JSON at [src/sync/state.rs:55](/tmp/ude-snapshot/src/sync/state.rs:55); malformed state becomes an empty default at line 44. Missing controls: failure on the second write, interrupted state persistence, and concurrent writers. Permission-setting errors are also discarded at [src/sync/mod.rs:356](/tmp/ude-snapshot/src/sync/mod.rs:356).

- **The universal bootstrap contains one repository’s personal workflow.** [templates/projects/agents/agents.md.jinja:3](/tmp/ude-snapshot/templates/projects/agents/agents.md.jinja:3) hardcodes `denniyahh/gsd-core`; line 8 directs PRs to `open-gsd/gsd-core`. [mise.toml.jinja:29](/tmp/ude-snapshot/templates/projects/agents/mise.toml.jinja:29) invokes `./scratch/ci-mac.sh`, which initialization does not install; its absence was reproduced. These project-specific policies leak into the generic bootstrap abstraction.

Evidence: the isolated [probe](/tmp/ude-adversarial-1x53u04l/src/bin/adversarial_probe.rs) exercised byte-identical copies of the original Rust sources; [results](/tmp/ude-adversarial-1x53u04l/probe.stdout) include the controls. These establish the specific Linux fixture failures above. They do not establish production frequency, cross-platform behavior, or full-suite coverage.