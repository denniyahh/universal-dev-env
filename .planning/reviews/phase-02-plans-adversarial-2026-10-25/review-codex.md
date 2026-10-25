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
