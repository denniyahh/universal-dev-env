---
phase: 2
reviewers: [codex, antigravity]
reviewed_at: 2026-10-25T00:00:00Z
plans_reviewed: [02-01-PLAN.md, 02-02-PLAN.md, 02-03-PLAN.md, 02-04-PLAN.md, 02-05-PLAN.md, 02-06-PLAN.md, 02-07-PLAN.md, 02-08-PLAN.md]
models:
  codex: "gpt-6.1-sol (reasoning=high)"
  antigravity: "gemini-3.8-flash-high (effort=high)"
model_sources:
  codex: "command line"
  antigravity: "command line"
---

# Cross-AI Plan Review — Phase 2

Produced by the operator-requested `external-review` skill (lens: adversarial, lanes codex and agy
only), not by `/gsd-review`; raw outputs and prompt are in
`.planning/reviews/phase-02-plans-adversarial-2026-10-25/`. The host agent verified every finding
against plan text and source; dispositions are in the Consensus Summary and are binding for the
`--reviews` replan.

<!-- gsd:plan-revision-conflicts:begin -->
## Plan-Revision Conflicts
<!-- gsd:plan-revision-conflicts:end -->

## Codex Review

## CONFIRMED

All `02-*` citations below refer to `.planning/phases/02-sanitizer-boundaries-security-hardening/`.

### 1. BLOCKER — UTF-16 is an explicitly accepted escape from every scanner

**Citation:** `02-01-PLAN.md:245`:

> “A UTF-16 file's ASCII tokens are interleaved with NUL bytes and would not match. Accepted; no managed file type is UTF-16.”

Nothing enforces that assumption. The existing binary-file test explicitly permits arbitrary bytes (`src/sync/mod.rs:7166–7174`), and push copies non-UTF-8 content unchanged (`src/sync/mod.rs:1473–1474`). Plan 08 also excludes it from render-back validation.

**Failure scenario:** Save a managed file containing a PAT as UTF-16LE. NUL bytes break every ASCII credential pattern and every ≥20-byte entropy candidate. The prescribed scanner passes the credential through unchanged.

**Verified control:** A runtime-generated PAT produced `github-pat`, exit 1, through gitleaks 8.30.1’s plain-text scanner. Its UTF-16LE equivalent produced no findings, exit 0. The same encoding eliminated all candidate runs under the proposed tokenizer. This verifies the encoding escape, not the future Rust implementation.

**Suggested fix:** Reject unsupported encodings before publication, or decode supported encodings and scan their text alongside raw bytes. Require UTF-16LE/BE credential fixtures and benign controls at staging and both publication gates.

### 2. MAJOR — The SRI exemption accepts malformed hashes containing credentials

**Citation:** `02-03-PLAN.md:174` exempts:

> `^sha(256|384|512)-[A-Za-z0-9+/]+={0,2}$`

Locked D-02 exempts **SRI hashes** (`02-CONTEXT.md:26–28`), not arbitrary strings beginning with a hash algorithm’s name.

**Failure scenario:** Prefix a 32-character, otherwise-blocking credential with `sha256-`. The resulting 32-character body cannot represent a SHA-256 digest, but this regex exempts it. Put it in a neutral comment: no named assignment rule applies, and render-back validation has nothing to render.

**Verified control:** An in-memory reproduction of the specified R11 predicate blocked both the bare token and its prefixed form before exemption. The planned exemption accepted the prefixed form. Gitleaks 8.30.1 reported neither form, so it supplies no fallback for this case.

**Suggested fix:** Validate algorithm-specific decoded digest lengths and valid Base64 encoding. Add malformed-length, padding and truncated-hash controls. Keep the bare credential blocked.

### 3. MAJOR — Render-back validation protects bytes that Git may subsequently replace

**Citations:**

- `02-08-PLAN.md:121`: render-back validation runs “inside pass 1.”
- `02-01-PLAN.md:159`: the later gates only replace their existing scanner calls with `scan_bytes`.
- `src/sync/mod.rs:1660–1665`: Git clean filters run during `git add`.
- `src/sync/mod.rs:1720–1745,2019–2041`: index and HEAD validation inspect secrets, without checking reversibility.

**Failure scenario:** Adapt the existing clean-filter injection harness (`src/sync/mod.rs:6264–6290`) to append:

```text
{# unterminated
{{ user.home }}
```

The local candidate passes render-back validation before the filter runs. The resulting index and HEAD blobs contain no credential, so both publication gates pass them. Consumers subsequently fail to render the published template.

This directly defeats `02-08-PLAN.md:36`:

> “MUST NOT publish a template that does not render back to the pusher's local file.”

**Suggested fix:** Bind index and committed blobs to the validated candidate bytes, or repeat render-back validation on those actual blobs. Extend the existing filter harness with an irreversible-template case and an unchanged-content control.

### 4. MAJOR — Renderer errors can quote file content despite the plan claiming otherwise

**Citations:**

- `02-08-PLAN.md:93`: “The engine's errors carry a position, not file content.”
- `02-08-PLAN.md:120`: return `parameterized text does not render back: {e}`.
- `src/template/mod.rs:167–168`: renderer errors are converted directly into strings.

The pinned MiniJinja 2.24.0 source contradicts the assumption: `src/vm/mod.rs:623–625` constructs:

```rust
format!("filter {} is unknown", normalized_name.as_ref())
```

Its `src/error.rs:183–191` includes that detail in ordinary `Display` output.

**Failure scenario:** A managed file contains `{{ 1 | private_label }}`. The scanner has no reason to flag that short identifier. Rendering fails, and the proposed refusal echoes `private_label`. The promise that reasons never quote file content is false; content that escapes scanning can also escape through diagnostics.

**Suggested fix:** Emit a controlled error category and line number instead of forwarding the engine’s detail string. Add a sentinel identifier inside an invalid filter/function expression and assert it is absent from both CLI streams.

### 5. MAJOR — The new leading-boundary rule corrupts foreign absolute paths

**Citations:**

- `02-07-PLAN.md:124`: accept any preceding character that is neither alphanumeric, `_` nor `.`.
- `02-07-PLAN.md:271`: a directory ending in `-` causing replacement “is not a realistic path.”
- `02-07-PLAN.md:43`: “MUST NOT rewrite any path that is not the pusher's own home or XDG config path.”

**Failure scenario:** With home `/home/alice`:

```text
/backup-/home/alice/file
```

becomes:

```text
/backup-{{ user.home }}/file
```

That is a valid foreign path. On another machine it changes to `/backup-/home/bob/file`.

**Verified control:** The specified predicate accepts the occurrence after `/backup-` and rejects it after `/backup`. The distinction comes entirely from allowing `-`. The round-trip property still passes on Alice’s machine, so reversibility does not establish correct path identity.

**Suggested fix:** Recognize the intended shell-default context, such as `${W:-/home/alice}`, without treating every dash inside an absolute path as a boundary. Add foreign-prefix controls alongside the shell-default positive.

### 6. MAJOR — Plan 01’s acceptance check rejects a required, unchanged dependency

**Citations:**

- `02-01-PLAN.md:160`: “`is_local_only_path` and `parameterize_paths` are unchanged.”
- `02-01-PLAN.md:179`: `rg -n 'regex::Regex|use regex::Regex' src/sanitizer/mod.rs` must print nothing.
- `src/sanitizer/mod.rs:1,84,101`: the unchanged parameterizer requires the `regex::Regex` import.

**Failure scenario:** Correctly convert the named secret rules to byte regexes while preserving the path parameterizer. The acceptance check still fails on its necessary import. Delete the import to satisfy the check and the unchanged function stops compiling.

Changing the import’s spelling could fool the check, but would establish nothing about scanner correctness.

**Suggested fix:** Restrict the assertion to the named-rule implementation. Retain the path regex dependency until plan 07 removes it.

### 7. MAJOR — The parity negative control reuses evidence from before the mutation

**Citation:** `02-06-PLAN.md:169`:

> “delete the token from one category file and re-run only the comparison loop; it must fail.”

But the comparison loop at `02-06-PLAN.md:178` reads filenames and the previously generated `report.json`. It never reads the changed file contents or reruns gitleaks.

**Failure scenario:** Generate a passing report, remove any token without renaming its file, then execute the prescribed comparison loop. Its inputs remain unchanged, so it passes again.

This is a mechanically ineffective negative control.

**Suggested fix:** After editing the corpus, rerun gitleaks into a fresh report before comparing. Require the previously expected category finding to disappear and the original completeness assertion to fail. Do not rerun the corpus generator, which would restore the token.

### 8. MAJOR — “Full categories” is reduced to fifteen asserted rule IDs

**Citations:**

- `02-CONTEXT.md:102–103`: “Each rule category needs a known-positive fixture.”
- `02-05-PLAN.md:29–30`: vendor and load 222 rules.
- `02-06-PLAN.md:139,151`: the positive table must contain **exactly 15 rows**.
- `02-06-PLAN.md:169`: parity reuses those same fifteen categories.

**Failure scenario:** A vendored rule outside those fifteen never produces findings. The hash and loader-count checks still pass; the required category table and parity corpus never exercise it. For example, the real `pkcs12-file` rule receives no required positive fixture, although `02-05-PLAN.md:91` explicitly identifies it. A synthetic `.p12` rule test checks the matcher mechanism, not that vendored rule.

The acceptance criteria establish selected-category coverage, not the locked requirement’s coverage.

**Suggested fix:** Require a positive fixture and benign control for every vendored rule ID, including real path-scoped rules. Keep loader counts separate from behavioral coverage and report any unexercised rules explicitly.

## SUSPECTED

### MAJOR — SPECULATION: render-back validation introduces an unbounded computation path

**Citations:** `02-08-PLAN.md:120–121` requires rendering arbitrary candidate text. `src/template/mod.rs:153–168` configures no execution or output budget; `Cargo.toml:14` enables only the loader feature explicitly.

**Scenario:** A short candidate containing nested `range(100000)` loops can demand billions of iterations or output bytes before the gate can refuse it. The pinned MiniJinja source permits an individual range of that size. Git subprocess timeouts do not bound in-process rendering.

**Suggested fix:** Bound render execution and output size, with a small-budget exhaustion test and a normal-template control.

**Limit:** I verified the missing budget and individual range allowance from source. I did not execute an exhaustion payload or measure its resource consumption.

## COVERAGE

Read:

- All eight Phase 2 plans and `02-CONTEXT.md`, `02-RESEARCH.md`, `02-VALIDATION.md`, `02-PATTERNS.md`.
- `.planning/REQUIREMENTS.md`, `.planning/ROADMAP.md` Phase 2 section, `.planning/config.json`.
- Entire `src/sanitizer/mod.rs` and `src/template/mod.rs`.
- Relevant portions of `src/sync/mod.rs`, `src/main.rs`, `src/process.rs`, `src/manifest/mod.rs`, `src/sync/merge.rs`, and `tests/e2e_lifecycle.rs`.
- `Cargo.toml`, targeted dependency versions in `Cargo.lock`, `.config/nextest.toml`.
- The shipped gitleaks configuration, commit-msg template, pre-commit hook, supply-chain workflow, and relevant architecture documentation.
- Cached MiniJinja 2.24.0 error, VM and range implementations; relevant GSD execution-isolation instructions.

Ran memory-only predicate controls and bounded gitleaks stdin probes. No Cargo build/tests ran; the proposed entropy and gitleaks Rust modules do not yet exist. No files or temporary paths were created, and all probe subprocesses completed.

---

## Antigravity Review

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

---

## Consensus Summary

### Operator decisions (2026-10-25, binding)

- **OD-1 (amends locked D-04):** `=` is a delimiter in the R11 tokenizer, except a trailing run of
  one or two `=` at the end of a candidate (base64 padding), which stays part of that candidate.
  `NAME=<value>` is scored as `<value>`. Required tests: `.env`-style lines with long uppercase
  names (`CONFIG_PARAMETER_KEY=`, `MY_SERVICE_CLIENT_SECRET_VALUE=`) followed by seeded B64 values
  meet the same block-rate floors as the bare value; negative control: the old glued tokenization
  falls below the floor (host measurement with a Python re-implementation: bare 32-char 0.936,
  `API_TOKEN=` 0.702, `CONFIG_PARAMETER_KEY=` 0.130, `MY_SERVICE_CLIENT_SECRET_VALUE=` 0.003).
  Padded base64 (`...==`) must still be one candidate.
- **OD-2:** keep 02-08's refusal of files whose published text does not render back (fail closed).
  Automatic escaping is deferred (todo 2026-10-25-auto-escape-literal-template-syntax-on-push).

### Agreed Concerns (verified; must be fixed in the plans)

1. Agy-5 / OD-1: `=` gluing (BLOCKER) — see OD-1.
2. Codex-2: SRI exemption (02-03-PLAN.md:174) must check the base64 body decodes to the digest
   length of the named algorithm (sha256 = 44 chars incl. padding, sha384 = 64, sha512 = 88);
   add a wrong-length control that still blocks.
3. Codex-3: render-back must cover the bytes actually staged/committed, not only the pre-`git add`
   candidate (clean filters run at add, src/sync/mod.rs:1660; harness at src/sync/mod.rs:6264).
   Extend the filter harness with an irreversible-template case and an unchanged-content control.
4. Codex-4: refusal text must not forward MiniJinja's error string (minijinja 2.24.0
   src/vm/mod.rs:625 `filter {name} is unknown`); emit a fixed category + line number. Sentinel
   identifier test asserting absence from stdout and stderr. Correct 02-08-PLAN.md:93.
5. Codex-6: 02-01-PLAN.md:179 acceptance check conflicts with keeping `parameterize_paths`
   (src/sanitizer/mod.rs:1,84,101 use `regex::Regex`); scope the check to the named-rule code.
6. Codex-7: parity negative control (02-06-PLAN.md:169) must re-run gitleaks into a fresh report
   after editing the corpus, not only the comparison loop.
7. Codex-1: UTF-16 — enforce instead of assume: detect UTF-16 (BOM or NUL-interleaved ASCII) and
   either scan the decoded text too or refuse; UTF-16LE/BE credential fixtures plus benign control.
8. Agy-3: ssh body exemption must also accept the key type after `"`, `'`, `=`, `:`, `,` (JSON and
   unspaced gitconfig), still body-only per the D-02 amendment; keep the appended-token control.
9. Agy-6: plans 02-06 and 02-07 (both wave 4) must use distinct cargo-mutants output, tmp, log and
   exit-file paths.
10. Codex-5 (minor): 02-07 boundary set must not treat `-` inside a path as a leading boundary;
    recognise the shell-default form (`:-`) instead; add `/backup-/home/alice/x` unchanged control.
11. Agy-2 (minor): state Shannon entropy as -Σ p·log2 p over byte frequencies in 02-05-PLAN.md:141
    and 02-03-PLAN.md:130; add one entropy-gated gitleaks rule to the category table if none is
    present, so a wrong formula fails a test.
12. Agy-7 (minor): hex/UUID exemptions cannot change an R11 outcome; drop them from mutation
    expectations or justify keeping them as documented no-ops (D-02 lists them).
13. Codex-8 (host decision, CONTEXT "Claude's Discretion"): keep 15 behavioural category rows; add
    a test that prints the number of vendored rules without a positive fixture, and record it in
    the SUMMARY as residual risk.

### Divergent Views / Rejected

- Agy-8 (`git check-ignore` inverted): rejected — the idiom passes only on exit 1 (not ignored),
  which is correct.
- Agy-4 (`dummy_ctx` host home in `scan_project`): pre-existing Phase 1 behaviour; todo
  2026-10-25-push-scan-uses-host-home-not-user-home-argument. 02-08 may build the context from the
  arguments once and reuse it, if that is the natural change.
- Codex suspected render DoS and agy suspected binary-scan cost: speculative, not carried.
