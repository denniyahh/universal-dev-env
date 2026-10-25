## CONFIRMED

### 1. Reversibility Gate Refuses Valid Managed Shell Hooks and Prose
- **Severity:** BLOCKER
- **Citation:**
  - [.planning/phases/02-sanitizer-boundaries-security-hardening/02-08-PLAN.md](file:///.planning/phases/02-sanitizer-boundaries-security-hardening/02-08-PLAN.md#L28-L29):
    > "The shipped commit-msg hook shape (`${#FIRST_LINE}`) plus a home path is refused (render error), and the same hook without a home path is published byte-identical (no `{{` inserted, nothing rendered)"
    > "Other irreversible shapes are refused: `{{` directly before a home path (`x{{/home/alice/y` becomes `{{{{`), `${{ secrets.TOKEN }}` with no home path (undefined value), a `.jinja`-mapped file whose text contains `{#`, and a literal `{{ git_email }}` that renders to different text"
  - [.planning/phases/02-sanitizer-boundaries-security-hardening/02-08-PLAN.md](file:///.planning/phases/02-sanitizer-boundaries-security-hardening/02-08-PLAN.md#L241):
    > "Planner decision: the check also applies to UTF-8 files that gained no placeholder but already contain `{{` (for example `${{ secrets.TOKEN }}` in a hook, or a literal `{{ git_email }}`). Today such a push publishes a template that pull refuses to render (IoFailed) or renders to different text for every consumer, so refusing at push is the fail-closed reading of SEC-04. Expected user-visible change: a managed file holding literal `{{ ... }}` text, for example in AGENTS.md prose, can no longer be pushed until the text is changed; the refusal names the file and line."
  - Shipped codebase templates:
    - [templates/projects/generic/hooks/commit-msg](file:///templates/projects/generic/hooks/commit-msg#L42): `if [ ${#FIRST_LINE} -gt 72 ]; then`
    - [templates/projects/agents/scratch/check-publish-boundary.sh](file:///templates/projects/agents/scratch/check-publish-boundary.sh#L37): `if [ ${#SECRETS_FOUND[@]} -gt 0 ]; then`
- **Failure Scenario:**
  A developer manages standard Git hooks or shell scripts with UDE. The script uses standard POSIX variable length expansion (`${#VAR}`) and references any path under the user's home directory (e.g. `export PATH="/home/user/.cargo/bin:$PATH"` or `REPO_ROOT="/home/user/code"`).
  1. `parameterize_text` replaces `/home/user` with `{{ user.home }}`.
  2. Because the file now contains `{{`, `template::should_render` returns `true`.
  3. Gate 5 routes the published text into MiniJinja for render-back verification.
  4. MiniJinja parses `${#VAR}` as an unclosed Jinja comment start (`{#`), fails with `syntax error: unexpected end of comment`, and Gate 5 returns `PushResult::ReversibilityBlocked`.
  5. The push is aborted with exit 1 (`EXIT_DRIFT`). The user is permanently blocked from syncing valid Git hooks.
  Furthermore, the planner explicitly acknowledges at line 241 that markdown files containing template syntax explanations (such as `.agents/AGENTS.md`) or GitHub Actions workflows (`${{ secrets.GITHUB_TOKEN }}`) can no longer be pushed at all until the developer modifies their documentation or workflows.
- **Suggested Fix:**
  SEC-04 requires verifying that token substitution is reversible, not that arbitrary shell scripts and markdown files are valid MiniJinja templates. Separate path parameterization reversibility (replacing known tokens `{{ user.home }}` back to `user_home` via string replacement or dedicated delimiter handling) from full template evaluation. For non-`.jinja` files, do not route the entire file through MiniJinja, or escape/configure MiniJinja comment delimiters so POSIX `${#...}` parameter expansions are not parsed as Jinja comments.

---

### 2. Mangled Shannon Entropy Formula in Gitleaks Engine Disables 130 Rules
- **Severity:** BLOCKER
- **Citation:**
  - [.planning/phases/02-sanitizer-boundaries-security-hardening/02-05-PLAN.md](file:///.planning/phases/02-sanitizer-boundaries-security-hardening/02-05-PLAN.md#L141):
    > "compute the secret by re-running the regex on that slice (`captures`) and taking `secretGroup` when set (skip when that group is absent), else the first non-empty group, else the trimmed match; skip when the rule's entropy is set and the secret's Shannon entropy (count each distinct byte, divide by the byte length; equal to gitleaks for ASCII secrets) is <= that entropy."
  - [.planning/phases/02-sanitizer-boundaries-security-hardening/02-RESEARCH.md](file:///.planning/phases/02-sanitizer-boundaries-security-hardening/02-RESEARCH.md#L268):
    > "3. If `entropy` is set and `shannonEntropy(secret) <= entropy`, skip. Gitleaks divides rune counts by the **byte** length (`utils.go:117-133`)."
- **Failure Scenario:**
  In Gitleaks v8.30.1 (`config/gitleaks.toml`), exactly 130 out of 222 rules (58.5% of the entire ruleset, including `1password-secret-key`, `1password-service-account-token`, `adobe-client-id`, `adobe-client-secret`, `alibaba-access-key-id`, etc.) specify an `entropy` threshold. These threshold values range from `2.0` to `4.3`.
  In Gitleaks Go source (`detect/utils.go:117-133`), `shannonEntropy` calculates standard Shannon entropy: for each distinct character, probability $p = \frac{\text{count}}{\text{length}}$, accumulating $- \sum p \log_2(p)$.
  Line 141 of `02-05-PLAN.md` misinterprets "divides rune counts by byte length" and instructs the implementer to compute: `(count each distinct byte, divide by the byte length)`.
  The ratio of distinct bytes to total length is bounded in $[0.0, 1.0]$.
  Because this ratio can never exceed `1.0`, it is mathematically impossible for it to exceed any rule threshold in `gitleaks.toml` ($\ge 2.0$).
  Consequently, `secret_entropy <= rule.entropy` evaluates to `true` on every single candidate match, causing all 130 rules to skip matching secrets. The ported scanner silently misses 130 secret categories while reporting zero findings.
- **Suggested Fix:**
  Replace the cardinality ratio in `02-05-PLAN.md:141` with the exact Shannon entropy calculation: count byte frequencies $f_b$, compute $p_b = f_b / L$, and evaluate $H = - \sum p_b \log_2(p_b)$.

---

### 3. SSH Public Key Exemption Broken by Surrounding Characters, Quotes, and Delimiters
- **Severity:** MAJOR
- **Citation:**
  - [.planning/phases/02-sanitizer-boundaries-security-hardening/02-03-PLAN.md](file:///.planning/phases/02-sanitizer-boundaries-security-hardening/02-03-PLAN.md#L175):
    > "Private `fn ssh_key_body_spans(line: &[u8]) -> Vec<std::ops::Range<usize>>`, from one compile-once `LazyLock<regex::bytes::Regex>` (ASCII mode, `.expect` with a reason) matching `(?:^|[ \t])(?:key::)?(?:ssh-ed25519|ssh-rsa|ssh-dss|ecdsa-sha2-nistp(?:256|384|521)|sk-ssh-ed25519@openssh\.com|sk-ecdsa-sha2-nistp256@openssh\.com)[ \t]+(AAAA[A-Za-z0-9+/]+={0,3})`... Forms with another byte before the key type or `key::` (for example `signingkey=key::ssh-ed25519` with no spaces, or a quoted `"ssh-ed25519 AAAA..."`) stay scored: they fail closed and the plan 02-04 marker covers them."
  - [.planning/phases/02-sanitizer-boundaries-security-hardening/02-CONTEXT.md](file:///.planning/phases/02-sanitizer-boundaries-security-hardening/02-CONTEXT.md#L26-L30):
    > "D-02: Known non-secret high-entropy values pass through two mechanisms:
    > 1. Built-in shape exemptions: ssh public key lines (`ssh-ed25519 AAAA…`, `ssh-rsa`, `ecdsa-sha2-*`, `sk-*@openssh.com`), hex digests (40/64/128 hex chars), UUIDs, SRI hashes (`sha256-`/`sha384-`/`sha512-`).
    > Amended (operator, 2026-10-23, during plan-phase): the ssh exemption covers only the base64 key body that follows a key type, not the whole line; the rest of the line is still scanned."
  - [.planning/phases/02-sanitizer-boundaries-security-hardening/02-CONTEXT.md](file:///.planning/phases/02-sanitizer-boundaries-security-hardening/02-CONTEXT.md#L39-L42):
    > "D-04: The entropy check scores every token: split all text on whitespace, quotes and delimiters, then score every ≥20-character run of the credential alphabet, including `+ / = - _ .`... The built-in shapes (D-02) and the marker absorb the false positives."
- **Failure Scenario:**
  Git configurations frequently omit spaces around `=` (e.g. `signingkey=key::ssh-ed25519 AAAAC3...` or `signingkey="ssh-ed25519 AAAAC3..."`), and JSON configurations format values with quotes (`"key": "ssh-ed25519 AAAAC3..."`).
  Because `ssh_key_body_spans` enforces `(?:^|[ \t])` directly before `key::` or `ssh-`, any preceding quote, delimiter, or unspaced `=` prevents the regex from matching.
  The plan author concedes this in line 175, stating they "stay scored: they fail closed and the plan 02-04 marker covers them."
  93.6% of random Ed25519 public key bodies exceed R11 entropy thresholds.
  This violates D-02 and D-04: D-02 mandated built-in shape exemptions specifically so users would not need manual `ude:allow-entropy` overrides for public keys. Furthermore, in JSON files containing SSH keys, inserting a `ude:allow-entropy` comment breaks JSON syntax, leaving users with no possible workaround.
- **Suggested Fix:**
  Update `ssh_key_body_spans` regex to permit delimiters, quotes, or `=` before the key type: `(?:^|[\s"'=:,])(?:key::)?(?:ssh-...)[ \t]+(AAAA[A-Za-z0-9+/]+={0,3})`.

---

### 4. `push_updates` Leaks Host Environment into `scan_project` via Unpatched `dummy_ctx`
- **Severity:** MAJOR
- **Citation:**
  - [src/sync/mod.rs](file:///src/sync/mod.rs#L1330-L1335):
    ```rust
    let dummy_ctx =
        crate::template::TemplateContext::detect_for_project(local_root, "project", "generic");
    let states = match self.scan_project(local_root, upstream_root, Some(&dummy_ctx)) {
        Ok(states) => states,
        Err(reason) => return PushResult::ScanFailed { reason },
    };
    ```
  - [.planning/phases/02-sanitizer-boundaries-security-hardening/02-RESEARCH.md](file:///.planning/phases/02-sanitizer-boundaries-security-hardening/02-RESEARCH.md#L282):
    > "Do not use the detect_for_project values: push_updates today builds dummy_ctx from the real host home (src/sync/mod.rs:1330-1331), which differs from the user_home argument in tests."
  - [.planning/phases/02-sanitizer-boundaries-security-hardening/02-08-PLAN.md](file:///.planning/phases/02-sanitizer-boundaries-security-hardening/02-08-PLAN.md#L121):
    > "In push_updates, before pass 1, build `render_ctx` from `dummy_ctx.clone()` with user.home and user_home set to the `user_home` argument and user.xdg_config and xdg_config set to the `xdg_config` argument, plus one `TemplateRenderer::new()`. In pass 1, right after gate 4... run gate 5..."
- **Failure Scenario:**
  `push_updates` accepts explicit `user_home: &str` and `xdg_config: &str` arguments to guarantee hermetic execution in test fixtures, container runners, and multi-user environments.
  `02-RESEARCH.md:282` explicitly documented that `push_updates` leaks the real host home through `dummy_ctx`.
  However, `02-08-PLAN.md:121` only clones and patches `render_ctx` for gate 5 inside pass 1, leaving `scan_project` at line 1332 called with the unpatched `dummy_ctx`.
  `TemplateContext::detect_for_project` reads the host machine's home directory via `directories::BaseDirs::new()`.
  When `scan_project` reads upstream templates (lines 645-651), it renders them using `dummy_ctx` containing the host user's actual `$HOME`.
  When `scan_project` compares the local files (rendered with `/home/alice`) against upstream templates (rendered with the host's `/var/home/denniyahh`), it falsely detects drift and conflicts due to host environment contamination.
- **Suggested Fix:**
  Build the patched `render_ctx` immediately upon entering `push_updates` before line 1332, and pass `Some(&render_ctx)` into `self.scan_project(local_root, upstream_root, Some(&render_ctx))`.

---

### 5. Inclusion of `=` in Candidate Alphabet Glues Key-Value Pairs and Bypasses Entropy Detection
- **Severity:** MAJOR
- **Citation:**
  - [.planning/phases/02-sanitizer-boundaries-security-hardening/02-03-PLAN.md](file:///.planning/phases/02-sanitizer-boundaries-security-hardening/02-03-PLAN.md#L24):
    > "Per D-04 and D-05, a candidate token is a maximal run of bytes from `[A-Za-z0-9+/=_\-.]` at least 20 bytes long; every other byte, including any non-ASCII byte, is a delimiter..."
  - [.planning/phases/02-sanitizer-boundaries-security-hardening/02-03-PLAN.md](file:///.planning/phases/02-sanitizer-boundaries-security-hardening/02-03-PLAN.md#L129):
    > "Private `fn candidate_tokens(line: &[u8]) -> impl Iterator<Item = (usize, &[u8])>` yields (start offset, token) for maximal runs of `A-Z a-z 0-9 + / = _ - .` bytes with length >= MIN_TOKEN_LEN. Every other byte is a delimiter."
  - [.planning/phases/02-sanitizer-boundaries-security-hardening/02-CONTEXT.md](file:///.planning/phases/02-sanitizer-boundaries-security-hardening/02-CONTEXT.md#L39-L40):
    > "D-04: The entropy check scores every token: split all text on whitespace, quotes and delimiters, then score every ≥20-character run of the credential alphabet, including `+ / = - _ .`."
- **Failure Scenario:**
  In unquoted configurations, shell scripts, and `.env` files (e.g. `CONFIG_PARAMETER_KEY=<random-28-char-value, redacted by host>`), `=` functions as an assignment operator.
  Because `=` is included in the candidate token alphabet, `candidate_tokens` extracts `CONFIG_PARAMETER_KEY=<random-28-char-value, redacted by host>` as a single candidate token.
  The 20-character uppercase variable name prefix contains zero class switches.
  When the alphanumeric class switch rate $\frac{c}{n - 1}$ is calculated, the denominator $n - 1$ increases by 20 while $c$ remains unchanged.
  This dilutes the class switch rate significantly: a high-entropy secret whose switch rate was ~0.80 drops to ~0.40, falling below the required $0.75 \times E$ threshold.
  `passes_randomness_test` evaluates to `false`, and the high-entropy credential silently bypasses the R11 scanner and is published.
- **Suggested Fix:**
  Treat `=` as a token delimiter during tokenization, allowing `=` only at the trailing end of a token to accommodate Base64 padding (`={1,2}$`), rather than allowing interior `=` to concatenate variable names with values.

---

### 6. Concurrent Wave 4 Mutation Gate Path Collisions
- **Severity:** MINOR
- **Citation:**
  - [.planning/phases/02-sanitizer-boundaries-security-hardening/02-06-PLAN.md](file:///.planning/phases/02-sanitizer-boundaries-security-hardening/02-06-PLAN.md#L2-L5): `wave: 4`, `autonomous: true`
  - [.planning/phases/02-sanitizer-boundaries-security-hardening/02-06-PLAN.md](file:///.planning/phases/02-sanitizer-boundaries-security-hardening/02-06-PLAN.md#L174):
    > `<automated>bash -c 'set -uo pipefail; mkdir -p target/mutants-tmp; echo running > target/mutants-run.exit; TMPDIR="$PWD/target/mutants-tmp" GIT_CEILING_DIRECTORIES="$PWD/target" cargo mutants --file src/sanitizer/gitleaks.rs -o target/mutants -- --bin ude -- sanitizer::gitleaks:: > target/mutants-run.log 2>&1; ec=$?; echo "$ec" > target/mutants-run.exit; exit "$ec"'</automated>`
  - [.planning/phases/02-sanitizer-boundaries-security-hardening/02-07-PLAN.md](file:///.planning/phases/02-sanitizer-boundaries-security-hardening/02-07-PLAN.md#L2-L5): `wave: 4`, `autonomous: true`
  - [.planning/phases/02-sanitizer-boundaries-security-hardening/02-07-PLAN.md](file:///.planning/phases/02-sanitizer-boundaries-security-hardening/02-07-PLAN.md#L197):
    > `<automated>bash -c 'set -uo pipefail; mkdir -p target/mutants-tmp; echo running > target/mutants-run.exit; TMPDIR="$PWD/target/mutants-tmp" GIT_CEILING_DIRECTORIES="$PWD/target" cargo mutants --file src/sanitizer/mod.rs --re "parameterize_paths|replace_bounded|is_leading_boundary|is_trailing_boundary|is_parameterizable_root" -o target/mutants -- --bin ude -- sanitizer::tests:: > target/mutants-run.log 2>&1; ec=$?; echo "$ec" > target/mutants-run.exit; exit "$ec"'</automated>`
- **Failure Scenario:**
  In GSD execution, plans in the same wave marked `autonomous: true` run in parallel in separate subagents.
  Both `02-06` and `02-07` hardcode identical artifact paths: `target/mutants`, `target/mutants-tmp`, `target/mutants-run.log`, and `target/mutants-run.exit`.
  Concurrent execution in Wave 4 causes simultaneous write, read, and deletion races on these paths, resulting in intermittent verification failures.
- **Suggested Fix:**
  Assign distinct target and log paths to each plan: `-o target/mutants-gitleaks` and `target/mutants-gitleaks-run.log` for `02-06`, and `-o target/mutants-boundaries` and `target/mutants-boundaries-run.log` for `02-07`.

---

### 7. Non-Falsifiable Hex and UUID Negative Controls
- **Severity:** MINOR
- **Citation:**
  - [.planning/phases/02-sanitizer-boundaries-security-hardening/02-03-PLAN.md](file:///.planning/phases/02-sanitizer-boundaries-security-hardening/02-03-PLAN.md#L167):
    > "The hex digest and UUID fixtures are outside this rule: the digests are drawn over `0-9 a-f` and the UUID is written lowercase (canonical form), and single-case hex (at most log2(16) = 4.0 bits per character) or a single-case UUID (17 symbols, at most log2(17) = 4.09 against 0.80 × log2(36) = 4.14) cannot clear the entropy bar, so they scan clean with or without their exemption, and no test claims otherwise."
  - [.planning/phases/02-sanitizer-boundaries-security-hardening/02-VALIDATION.md](file:///.planning/phases/02-sanitizer-boundaries-security-hardening/02-VALIDATION.md#L46):
    > "Each CONTEXT negative fixture scans clean (CamelCase, snake_case, paths, URLs, x86_64-pc-windows-msvc, UUID, 40/64/128 hex digests, ssh-ed25519 line, signingkey = key:: line, sentence)"
- **Failure Scenario:**
  Single-case hex strings (at most $\log_2(16) = 4.0$ bits) and canonical UUIDs (at most $\log_2(17) \approx 4.09$ bits) mathematically cannot reach R11's entropy threshold ($4.14$ and $4.80$ bits).
  Testing that single-case hex digests and UUIDs pass without triggering `first_high_entropy_line` passes 100% of the time even if `is_exempt_shape` is completely omitted or deleted. The proposed negative controls have zero discriminating power over the exemption logic.
- **Suggested Fix:**
  Add direct unit tests for `is_exempt_shape` asserting `true` for valid hex digests and UUIDs, or construct high-entropy mixed-case hexadecimal tokens that trip R11 when the shape exemption is removed and pass when it is present.

---

### 8. Inverted `git check-ignore` Logic in Rule Verification Command
- **Severity:** MINOR
- **Citation:**
  - [.planning/phases/02-sanitizer-boundaries-security-hardening/02-05-PLAN.md](file:///.planning/phases/02-sanitizer-boundaries-security-hardening/02-05-PLAN.md#L146-L147):
    > `<automated>bash -c '... rc=0; git check-ignore -q src/sanitizer/rules/gitleaks.toml || rc=$?; test "$rc" -eq 1'</automated>`
    > `<fails_when>... The ignore check passes only on git check-ignore's exit 1 (not ignored); 0 (ignored) and 128 (git error) both fail. Checked at plan revision: the idiom exits 0 for a tracked file (Cargo.toml), 1 for an ignored path (target/x) and 1 outside a repository (git exit 128).'</fails_when>`
- **Failure Scenario:**
  The plan narrative explains git's exit codes in reverse: `git check-ignore -q` exits 0 when a file *is* ignored, and 1 when it *is not* ignored.
  Additionally, when executed in snapshot archives or CI environments lacking a `.git` directory, `git check-ignore` exits with code 128 (`fatal: not a git repository`).
  The command captures `rc=128`, and `test "$rc" -eq 1` fails.
- **Suggested Fix:**
  Check git status safely: `if git rev-parse --is-inside-work-tree >/dev/null 2>&1; then ! git check-ignore -q src/sanitizer/rules/gitleaks.toml; fi`.

---

## ## SUSPECTED

### 1. Sequential Regex Scanning Overhead on Binary Buffers
- **Status:** SPECULATION
- **Citation:** [.planning/phases/02-sanitizer-boundaries-security-hardening/02-01-PLAN.md](file:///.planning/phases/02-sanitizer-boundaries-security-hardening/02-01-PLAN.md#L23), [.planning/phases/02-sanitizer-boundaries-security-hardening/02-05-PLAN.md](file:///.planning/phases/02-sanitizer-boundaries-security-hardening/02-05-PLAN.md#L141)
- **Hypothesis:**
  `02-01-PLAN.md` expands secret scanning to non-UTF-8 and binary blobs in both staging and publication gates (`scan_bytes(&blob_res.stdout)`).
  In `02-05-PLAN.md:141`, `first_finding` iterates through all rules without keywords (or matching keywords) using `find_iter` over the entire buffer.
  If a repository tracks large binary artifacts or compiled binaries (e.g., 20–50 MiB), evaluating 221 compiled regexes sequentially without size caps or streaming chunks could induce noticeable CPU latency or timeouts during `ude sync push`, even though `ude` primarily targets dotfiles.

---

## ## COVERAGE

The following files were read and verified against the live codebase:
- `.planning/REQUIREMENTS.md`
- `.planning/ROADMAP.md`
- `.planning/phases/02-sanitizer-boundaries-security-hardening/02-CONTEXT.md`
- `.planning/phases/02-sanitizer-boundaries-security-hardening/02-RESEARCH.md`
- `.planning/phases/02-sanitizer-boundaries-security-hardening/02-VALIDATION.md`
- `.planning/phases/02-sanitizer-boundaries-security-hardening/02-PATTERNS.md`
- `.planning/phases/02-sanitizer-boundaries-security-hardening/02-01-PLAN.md`
- `.planning/phases/02-sanitizer-boundaries-security-hardening/02-02-PLAN.md`
- `.planning/phases/02-sanitizer-boundaries-security-hardening/02-03-PLAN.md`
- `.planning/phases/02-sanitizer-boundaries-security-hardening/02-04-PLAN.md`
- `.planning/phases/02-sanitizer-boundaries-security-hardening/02-05-PLAN.md`
- `.planning/phases/02-sanitizer-boundaries-security-hardening/02-06-PLAN.md`
- `.planning/phases/02-sanitizer-boundaries-security-hardening/02-07-PLAN.md`
- `.planning/phases/02-sanitizer-boundaries-security-hardening/02-08-PLAN.md`
- `Cargo.toml`
- `src/main.rs`
- `src/sanitizer/mod.rs`
- `src/sync/mod.rs`
- `src/template/mod.rs`
- `templates/universal/generic/security/gitleaks.toml`
- `templates/projects/generic/hooks/commit-msg`
- `templates/projects/agents/mise.toml.jinja`
- `templates/projects/agents/scratch/check-publish-boundary.sh`
- `templates/universal/generic/allowed_signers.jinja`
