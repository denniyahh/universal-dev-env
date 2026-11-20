# Phase 3: Doctor & CLI Hardening - Context

**Gathered:** 2026-11-20
**Status:** Ready for planning

<domain>
## Phase Boundary

Make `ude doctor` (host and repo) strict and trustworthy, give every subcommand one exit-code
contract, and make the TypeSafe semantic drift check on `ude sync push` fail closed and never
transmit unscanned or local-only content. Requirements: CLI-01, CLI-02, CLI-03, CLI-04, SEC-05.
No new subcommands. Init behaviour in monorepos is out of scope (see Deferred).

</domain>

<decisions>
## Implementation Decisions

The operator delegated every gray area and the todo triage to Claude ("you decide", 2026-11-20).
Every decision below is therefore Claude's call. The operator can overturn any of them before
planning.

### Exit-code contract (CLI-04)
- **D-01:** Every subcommand uses the existing constants `EXIT_DRIFT = 1` and `EXIT_HARD_FAILURE = 2`
  in `src/main.rs`. Bare `std::process::exit(1)` / `exit(2)` literals are replaced by the constants.
  Exit 0 only when nothing needs attention.
- **D-02:** Meaning: **1** = the operator should look but nothing is broken or unsafe (drift, a
  skipped or refused file, a doctor warning). **2** = a security invariant is violated, or UDE could
  not do its job (missing required tool, I/O or git failure, unparseable state). Phase 1/2 carry
  forward: refusals (secret, boundary, reversibility, policy shift) stay exit 1, with nothing
  published.
- **D-03:** Doctor: `Critical` → 2, `Fail` → 2 (it is a system failure: required tool missing, not a
  git repo), `Warning` → 1. Today a warning exits 0 and `Fail` exits 1.
  — **Reversibility:** costly — scripts and CI will branch on doctor's exit code.
- **D-04:** So that a warning stays meaningful, a missing *optional* tool (cargo, zig, gitleaks, mise,
  podman) is no longer a warning. Add an informational status (e.g. `CheckStatus::Info`) that prints
  and serialises but never affects the exit code. Without this, doctor would exit 1 on almost every
  machine. JSON output gains a variant, which is an additive change.
- **D-05:** `ude sync status` exits 1 when any managed file is not in sync (locally modified, upstream
  modified, conflict, deleted), and 0 only when everything is in sync. This reverses Phase 1's
  "status exits 0 on drift", as CLI-04 requires. — **Reversibility:** costly — a published CLI
  contract that scripts will rely on.
- **D-06:** A test table drives the contract: one row per subcommand outcome (clean → 0, drift or
  refusal → 1, hard failure → 2), asserting the exit code *and* the stderr text. The suite includes at
  least one row per exit value per command, so a mapping that collapses to one code fails.

### SSH allowed_signers parsing (CLI-01)
- **D-07:** Parse each non-blank, non-`#` line of `allowed_signers` in OpenSSH format:
  `principals [options] keytype base64 [comment]`. Base64-decode the body and check the wire format:
  a length-prefixed type string equal to the declared keytype, then the type's fields with no
  trailing bytes. For `ssh-ed25519`, exactly one 32-byte key string.
- **D-08:** Other OpenSSH key types (`ssh-rsa`, `ecdsa-sha2-*`, `sk-*@openssh.com`) are accepted when
  their wire structure is well-formed: type string matches, every length prefix is in bounds, no
  trailing bytes. Only ed25519 gets full field validation. Rejecting valid RSA/ECDSA signers would
  break working setups for no security gain.
- **D-09:** Any malformed key line → `Critical`, naming the file and line number, never the key bytes.
  A file with no valid key line → `Critical` (as today). A missing principal or an unknown key type
  is also malformed.
- **D-10:** The parser is a pure function over `&str` with unit tests: a valid ed25519 line; a corrupted
  base64 body; a truncated key; a type-string mismatch (body says `ssh-rsa`, line says `ssh-ed25519`);
  trailing bytes; a valid RSA line. Fixtures are built at runtime from known bytes, not pasted real
  keys. A `proptest` checks that arbitrary input never panics.

### Hook mode audit (CLI-02)
- **D-11:** For each hook in `scripts/hooks/`, check both the disk mode (any `0o111` bit, on Unix) and the
  git index mode. Pass only when the disk mode is executable *and* (the index says `100755`, or the
  hook is untracked). Each mismatch is a `Warning` that names which side is wrong and its remedy
  (`chmod +x` or `git update-index --chmod=+x`). Today the disk mode is read only for untracked hooks.
- **D-12:** If the `git ls-files --stage` call itself fails, that is a `Fail` (exit 2), not a silent
  empty string. Today `.ok()…unwrap_or_default()` turns the failure into "untracked".

### Include resolution and repo detection (CLI-03)
- **D-13:** Doctor finds the repository with `git rev-parse --show-toplevel` and `--git-dir`, not
  `repo_path.join(".git").exists()`, so it works from a subdirectory and in worktrees.
- **D-14:** Each `include.path` value is resolved the way git resolves it: relative paths against the
  directory of the config file that holds them (`.git/` for the local config), `~/` expanded. The
  result is canonicalised and compared with the canonical `<toplevel>/.gitconfig`. String-comparing
  `../.gitconfig` / `.gitconfig` is dropped. (The roadmap's "`.gitconfig.repo`" is the upstream
  template `templates/projects/generic/gitconfig.repo`; the project-side file is `.gitconfig`.)
- **D-15:** Tests: run doctor from a subdirectory of a clean repo → no include warning, as a control
  against the string match it replaces; an include that points at a different file → warning; a
  worktree → passes.

### TypeSafe fail-closed (SEC-05)
- **D-16:** Only a `benign_rephrase` verdict with confidence ≥ 0.85 **and** a top-two margin ≥ 0.20 may
  authorise publishing agent rules (`allow_agent_rules = true`). Everything else is fail-closed: a
  lower score, a missing margin data, `project_exception`, `Uncertain`, a parse error, a timeout or
  a missing key. Fail-closed means the agent rules are not published, the other files still are, the
  reason goes to stderr, and push exits 1, following the D-10 skip pattern from Phase 1. This
  replaces today's "bypassed" notice, which then pushes without the rules and exits 0.
- **D-17:** Thresholds are named constants (`0.85`, `0.20`). The margin is top choice minus runner-up.
  If the API returns no per-choice scores, so no margin can be computed, the verdict fails closed.
  The researcher must confirm what the System One response returns; do not assume a field exists.
- **D-18:** `policy_shift` without `--force` keeps refusing (exit 1). `--force` keeps its current
  meaning: the operator authorises the agent rules and TypeSafe is advisory only.
- **D-19:** Scan before sending: run `check_secrets` on **both** the local and the upstream text. A hit
  in the local text refuses the push (exit 1, rule name and file, never the value; as today). A hit in
  the upstream text means no API call; the check fails closed (D-16), and stderr names the upstream
  template path. Neither text is sent if either has a hit.
- **D-20:** Never send local-only or overridden content: if `.agents/AGENTS.md` is local-only
  (`is_local_only_path`) or in `SyncStatus::Overridden`, skip the API call. It is not publishable
  anyway. The symlink refusal stays.
- **D-21:** Tests use a fake TypeSafe endpoint (an injectable URL or transport), with no network. They
  cover: benign at 0.90/0.30 → published (the positive control); benign at 0.84 → skipped, exit 1;
  benign at 0.90/0.19 → skipped, exit 1; timeout → skipped, exit 1; an upstream secret → the fake
  endpoint records **zero** requests; local-only or overridden → zero requests. Assert on the request
  count, not only on the exit code.

### Claude's Discretion
- Exact names (`CheckStatus::Info`, helper functions), the parser module layout, and whether the
  key parser lives in `src/audit/` or a new `src/audit/ssh.rs`.
- How the TypeSafe transport is made injectable for tests, without printing or logging the API key.
- Whether `curl` stays the transport. `curl`'s `--max-time 15` bound must stay, or be replaced by an
  equivalent bound.

### Folded Todos
- **"Register hooks and gitconfig when init runs in a monorepo subdirectory"**, the **doctor half
  only**: doctor uses `rev-parse`-based repo detection (D-13), so `ude doctor --repo <subdir>` no
  longer says "not a git repository". The init half stays in the todo (see Deferred).

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Requirements and roadmap
- `.planning/ROADMAP.md` — Phase 3 goal, success criteria, plans 03-01…03-04
- `.planning/REQUIREMENTS.md` — CLI-01…CLI-04, SEC-05

### Prior decisions carried forward
- `.planning/phases/01-sync-engine-state-hermeticity/01-CONTEXT.md` — exit 1 for drift/refusal, exit 2 for hard failure; the D-10 skip-and-report pattern
- `.planning/phases/02-sanitizer-boundaries-security-hardening/02-CONTEXT.md` — refusal output names the rule, never the value; `check_secrets` is the scanner to reuse

### Pending todos
- `.planning/todos/pending/2026-10-21-register-hooks-and-gitconfig-when-init-runs-in-a-monorepo-su.md` — doctor half folded (D-13)

### External formats
- OpenSSH `ssh-keygen(1)` "ALLOWED SIGNERS" section and RFC 8709 (ed25519 public key wire format) — the researcher should verify against these
- git-config(1) "Includes" — how relative `include.path` values resolve

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `crate::process::run_bounded` with `LOCAL_TIMEOUT` — every git call in doctor already uses it
- `sanitizer::Sanitizer::check_secrets` and `is_local_only_path` (`src/sanitizer/mod.rs:300`)
- `EXIT_DRIFT` / `EXIT_HARD_FAILURE` constants (`src/main.rs:17-19`)
- `sync::refuse_symlink_under` — the existing guard before the TypeSafe read

### Established Patterns
- `AuditReport { category, name, status }` with `CheckStatus::{Pass, Warning, Fail, Critical}` (`src/audit/mod.rs:7-30`); `--format json` serialises it
- Push skips are collected and turn into exit 1 at the end (`src/main.rs:~1318-1326`)

### Integration Points
- allowed_signers check: `src/audit/mod.rs:~95-125` (`contains("ssh-ed25519")` today)
- Include check: `src/audit/mod.rs:~212-250`; repo detection: `src/audit/mod.rs:199`
- Hook modes: `src/audit/mod.rs:~280-330`
- Doctor exit mapping: `src/main.rs:~40-150`
- TypeSafe call site: `src/main.rs:~1052-1147`; client: `src/semantic/mod.rs:81-183` (ignores `confidence` today)
- Bare exit literals left for CLI-04: `src/main.rs:63,65,133,141,1097,1110,1335`

</code_context>

<specifics>
## Specific Ideas

- Negative controls are required throughout: each guard test keeps a known-positive sample that must
  still trip it, and asserts on the failure text, not just the exit code.

</specifics>

<deferred>
## Deferred Ideas

- Check that `user.signingkey`'s public key is present in allowed_signers: a new doctor check, beyond CLI-01.

### Reviewed Todos (not folded)
- **Register hooks and gitconfig in a monorepo subdirectory (init half):** a design choice with
  repo-wide effects (`core.hooksPath` is shared across subprojects); it belongs with init work, not doctor.
- **Never publish files that init kept over the shared template:** a push-classification change (a new
  state class), not a TypeSafe transmission rule. Phase 3 covers only "overridden files never go to the API" (D-20).
- The other 14 matches were keyword-only overlaps (sanitizer, push, init and tooling items) with no
  Phase 3 requirement behind them.

</deferred>

---

*Phase: 03-doctor-cli-hardening*
*Context gathered: 2026-11-20*
