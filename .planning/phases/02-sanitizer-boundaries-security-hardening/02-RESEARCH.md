# Phase 2: Sanitizer, Boundaries & Security Hardening - Research

**Researched:** 2026-10-23
**Domain:** Rust secret scanning (entropy + gitleaks-rule port), path parameterization boundaries, template round-trip
**Confidence:** HIGH for the code-path findings and probe measurements; MEDIUM for the boundary heuristics (they are judgment calls on what real files contain)

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

#### Entropy hits & overrides (SEC-01) — discussed 2026-10-23
- **D-01:** A string that trips only the entropy check (no named rule matched) **blocks the push**,
  the same way a named-rule hit does, but can be overridden narrowly (D-02). It fails closed because
  the gitleaks baseline UDE ships allowlists `templates/.*` by path
  (`templates/universal/generic/security/gitleaks.toml`), so UDE's own sanitizer is the only secret
  scan on what push publishes.
- **D-02:** Known non-secret high-entropy values pass through two mechanisms:
  1. **Built-in shape exemptions**: ssh public key lines (`ssh-ed25519 AAAA…`, `ssh-rsa`, `ecdsa-sha2-*`,
     `sk-*@openssh.com`), hex digests (40/64/128 hex chars), UUIDs, SRI hashes (`sha256-`/`sha384-`/`sha512-`).
  2. **A per-line inline marker `ude:allow-entropy`**: an entropy-only hit on a line carrying the
     marker is not reported.
  No config file is used (`.ude.toml` policy loading is Phase 4). — **Reversibility:** costly — the
  marker string becomes a contract in users' managed files; renaming it later unmasks every marked line.
- **D-03:** The `ude:allow-entropy` marker silences **entropy-only hits only**. A named-rule hit
  (GitHub/GitLab PAT, AWS key, private-key header, Bearer, generic API-key assignment, and the
  gitleaks-category rules added by SEC-02) on a marked line still blocks. `gitleaks:allow` is not
  honoured by UDE.
- **D-04:** The entropy check scores **every token**: split all text on whitespace, quotes and
  delimiters, then score every ≥20-character run of the credential alphabet, including `+ / = - _ .`.
  It does not score assignment values only. Bare tokens in prose (AGENTS.md), scripts and comments
  are in scope. The built-in shapes (D-02) and the marker absorb the false positives.

#### Entropy rule (SEC-01) — calibrated and chosen 2026-10-23
- **D-05:** The remediation plan's fixed "≥20 chars, entropy > 4.2" rule is **replaced** by rule "R11".
  A token blocks only if it passes **both** tests:
  1. **Tokenize:** take maximal runs of `[A-Za-z0-9+/=_\-.]` that are at least 20 characters long,
     after removing the D-02 shapes (ssh public key lines, 40/64/128-char hex digests, UUIDs, SRI hashes).
  2. **Entropy test:** Shannon entropy over the token's characters ≥ `0.80 × log2(min(len, 64))`.
     This is length-aware, because a string of length n can never score above log2(n).
  3. **Randomness test:** over the token's alphanumeric characters only (symbols are skipped),
     classify each as upper, lower or digit, then
     `observed = (adjacent class changes) / (alnum_count − 1)`. Expected for random text over the
     classes present is `E = 1 − Σ(size_c / Σsize)²`, with sizes upper = 26, lower = 26, digit = 10.
     It requires `E > 0` (at least two classes present) and `observed ≥ 0.75 × E`.
  — **Reversibility:** reversible — two constants and one function; the fixture table (below) is
  what makes a change safe.
- **Measured results**, from a Python prototype on 2026-10-23. Each positive row is 1000 random
  tokens, so read it as ±~1.5 points.

  | Input | Remediation rule (4.2) | **R11 (chosen)** |
  |---|---|---|
  | random mixed-case Base64 / URL-safe / alnum, 20–24 chars | 18–70% blocked | **93–94% blocked** |
  | same, 32 chars | 99% | **93–97%** |
  | same, 40–88 chars, AWS-secret-shaped, JWT-shaped | 100% | **96–100%** |
  | random lowercase+digit or uppercase+digit (base36), 20–40 chars | 2–96% | **77–82%** |
  | random hex-only (32 chars) | 0% | **0%** (see limits) |
  | this repo, every git-tracked file (148 files, 4421 tokens ≥20 chars) | 300 false positives | **1** (a JWT test fixture in `src/sanitizer/mod.rs` tests) |
  | `templates/` + `template/` only (34 files, 281 tokens) | 72 | **0** |
  | operator's host agent-rule files (6 files) | 2 | **0** |

  Of the candidates tried, two came close. "R7" (a fixed switch-rate ≥ 0.40) blocked 97–99% of
  mixed-case tokens but only about 48% of base36 tokens. "R10/R12" (looser class-relative factors)
  produced 5–26 false positives on CamelCase `A/B` slash lists.
- **What this calibration does not establish** (the planner must carry these forward):
  - Hex-only and single-case-only secrets are **never** caught by entropy, because a 16- or 26-symbol
    alphabet can't clear the bar without also flagging English. Only named rules (SEC-02) can catch
    them.
  - About 3–7% of random mixed-case keys and about 18–23% of base36 keys get through, by chance
    switching patterns. Entropy is a backstop, not the primary guard.
  - The false-positive corpus is this repo plus 6 host files. That is mostly planning prose, not
    other users' managed files. The real false-positive rate on other machines is unmeasured.
  - These are prototype numbers. The Rust implementation must reproduce them, and is verified only
    by the fixture table below.
- **Required tests** (two directions, and the corpus negative control is mandatory):
  - **Positive fixtures, built at runtime and never committed as literals:** random Base64 and
    URL-safe tokens at 20, 32 and 64 chars, an AWS-secret-shaped 40-char token, a JWT-shaped token,
    and a token starting with `+` or containing `/`. Assert a block rate over a seeded sample, not a
    single lucky draw.
  - **Negative fixtures:** CamelCase and snake_case identifiers, file paths, URLs, target triples
    (`x86_64-pc-windows-msvc`), UUIDs, hex digests, an ssh-ed25519 public key line, and
    `ThisIsAPerfectlyNormalSentenceWithWords`.
  - **Corpus control:** scanning every file under `templates/` and `template/` yields **zero** hits.

#### Specific ideas (locked)
- The marker spelling is exactly `ude:allow-entropy`.
- Entropy scoring includes `+`, `/` and `=` in the alphabet: tokens that start with `+` or contain `/` must be blocked (remediation plan 2.1 verification).

### Claude's Discretion
- **Refusal output.** Name the rule (or `high-entropy`) with file:line. Never print the matched value
  (pattern carried over from devflow T-30-26). Exit code follows Phase 1: 1 for drift or refusal,
  2 for hard failure. The gate keeps Phase 1's rollback.
- **Secret rule source (SEC-02).** The operator did not select this area for discussion. Claude picks:
  port the gitleaks default categories to Rust `regex`, embed a pinned rule file, or run the pinned
  gitleaks binary. Whichever it is, a missing scanner is a hard failure, never a skip (supply-chain
  policy item 6). Each rule category needs a known-positive fixture, built at runtime and never
  committed as a literal.
- **Non-UTF-8 blobs at the gate.** Today the gate decodes blobs with `String::from_utf8_lossy`.
  Claude decides how binary blobs are screened. A lossy decode must not quietly turn a hit into a miss.
- **Placeholder round-trip (SEC-03/04).** The operator did not select this area for discussion.
  Claude decides: whether reversibility is checked at push time or only by tests, the boundary set
  (remediation plan 2.2: `[/\\"'\s:,;()\[\]<>]|$`), and whether to add a `{{ xdg.config }}` alias.
  Any choice must keep the existing round-trip test
  (`test_defect3_path_parameterization_roundtrip_static_template`) green.

### Deferred Ideas (OUT OF SCOPE)
- **TypeSafe System One hardening** (remediation plan 2.3) **moved to Phase 3 as SEC-05**
  (operator, 2026-10-23): plan stub 03-04 and success criterion 5. It is out of scope here. Today the
  code ignores `confidence` entirely (`src/semantic/mod.rs` `evaluate_drift`). Moving the API key to
  stdin is already done. Phase 2's new scanner still applies to the existing pre-API scan of the
  local text (`src/main.rs` ~L1074), because that call site goes through `check_secrets`.
- *Home-path placeholders leak the pusher's home path into shared templates* (2026-10-19): this overlaps SEC-03 from the push side. The operator chose not to fold it in, so it stays a standalone todo. Fixing SEC-03 may make it moot; re-check the todo after this phase.
- *Make generated projects' secret-scan hook self-sufficient* (2026-10-20): this is template/hook work, a better fit for Phase 4. Not folded in.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| SEC-01 | `Sanitizer` calculates Shannon entropy to detect high-entropy tokens and raw Base64 credentials lacking obvious prefixes. | R11 reproduced in Rust this session (probe numbers in "Entropy (R11) — Rust reproduction"); exact interpretation choices to lock; bytes-level tokenizer; marker and shape handling; seeded fixtures via `proptest::test_runner::TestRunner::deterministic()`. |
| SEC-02 | Outbound content screening expands regex token patterns to cover full Gitleaks secret categories before git staging. | Embedded, pinned gitleaks v8.30.1 rule file, compiled with `regex::bytes` in ASCII mode plus a Go-compatible `{` escape: all 222 rules load, 48 ms compile in release. A probe matched the real gitleaks binary on 13 generated categories, and both found 0 hits on the repo. Gate call sites and the non-UTF-8 hole are listed below. |
| SEC-03 | Path parameterization enforces strict word and punctuation boundaries (`\b` and path delimiters) to prevent partial substring corruption. | Probe shows a literal regex `\b` is **wrong** for paths that start with `/`. Recommended boundary predicate and scanner; real template evidence (`${VAR:-/var/home/...}`, `file:///home/...`). |
| SEC-04 | Sanitizer verifies token substitution reversibility between templated placeholders and local machine environments. | **Defect found:** `ude init` copies non-`.jinja` templates verbatim, so placeholders are never substituted on init. Push-time render-back check via the real `TemplateRenderer`; minijinja probe shows shipped `commit-msg` hook (`${#FIRST_LINE}`) would become an unrenderable template once a placeholder is inserted. |
</phase_requirements>

## Project Constraints (from CLAUDE.md / GEMINI.md / operator rules)

No `./CLAUDE.md` in the repo; `GEMINI.md` (project section from PROJECT.md) and the operator's global rules apply:

- Rust edition 2024, zero clippy warnings: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`.
- `cargo deny check`, `cargo machete` must stay green. No new crates are needed (see Standard Stack).
- Tests: hermetic `tempfile::TempDir`; scrub `GIT_DIR`, `GIT_WORK_TREE`, `GIT_CONFIG`; **never** `std::env::set_var` in tests; set env only on spawned `Command`s.
- Tests must assert non-trivial properties and negative cases; `proptest` for parsing and round-trip properties; `cargo mutants` for critical algorithms (R11, boundary predicate, rule loader).
- Secret fixtures are built at runtime, never committed as literals (the repo's own CI runs `gitleaks git --redact`).
- Never print a matched secret value; never print environment variables in scripts or verification steps.
- "Never emit raw credentials or bypass publication validation gates on automated remote pushes" (PROJECT.md).
- Prefer Rust dependencies; use `rg` not `grep` in any proposed commands.
- TDD mode is on (`workflow.tdd_mode: true`): RED evidence is nextest JUnit (`cargo nextest run --bin ude <filter>` → `target/nextest/default/junit.xml`, per `.config/nextest.toml`).

## Summary

The phase has four parts. Three are well-bounded code changes. The fourth, SEC-04, turned out to cover a real defect that the roadmap plan stubs do not mention:

1. **SEC-01 (entropy).** The CONTEXT's R11 rule reproduces exactly in Rust. A probe this session with a Rust R11, a seeded generator and 1000 samples per row found: mixed-case Base64, URL-safe and alnum tokens 92–100% blocked; base36 at 20–40 chars 78–84% blocked; repo **1** false positive (the JWT literal at `src/sanitizer/mod.rs:231`); `templates/` + `template/` **0**; every negative fixture passes; the marker control works. One limit the CONTEXT did not measure: **base36 tokens of exactly 64 chars are blocked only 16–20%** (88 chars: 68%). The entropy bar caps at `0.8 × log2(64) = 4.8` bits, and random 64-char base36 sits right at it.
2. **SEC-02 (gitleaks categories).** Embed gitleaks' own default config (`config/gitleaks.toml` at tag v8.30.1, 222 rules, MIT) and port its detection semantics, about 150 lines. Under the default Unicode-mode `regex::Regex`, **7 patterns fail**: 4 use a bare `{` that Go treats as a literal, and 3 exceed the 10 MiB compiled-size limit. Unicode-mode compilation also takes **3.4 s** in release. Use `regex::bytes::RegexBuilder` with `.unicode(false)` (Go's `\w \d \s \b` are ASCII-only, so this is also the *faithful* port) plus a 15-line Go-compat escape for an unparseable `{`. Then all 222 rules load in 48 ms in release (≈0.45 s debug). Byte-mode scanning also settles the non-UTF-8 question: no decoding, nothing lossy. The port matched the real `gitleaks` 8.30.1 binary rule-for-rule on 13 generated positive categories, and both reported 0 on the repo.
3. **SEC-03 (boundaries).** A literal regex `\b` is wrong here. Home paths start with `/`, so `\b/home/alice` only matches when a *word character* precedes the slash: the probe matched `/home/alice/home/alice` (second occurrence) and **missed** `cd /home/alice now`. The `regex` crate also has no look-around. Implement the boundary as a hand-written predicate over `str::match_indices`; it also removes the replace-until-fixpoint loop. The remediation set `[/\\"'\s:,;()\[\]<>]|$` still leaks `}` (`${H:-/home/alice}`), a sentence-final `.`, and anything after a leading `-` (`${W:-/home/alice/x}`). The operator's own shipped template has exactly that shape: `template/scratch/sync-personal-env.sh:12`.
4. **SEC-04 (reversibility).** `ude init` materializes `.gitconfig`, `.gitignore`, hooks and `check-in-container.sh` with `fs::copy` (`src/main.rs:319-330`, `1641-1655`). Only `.jinja` templates are rendered (`kept_ancestor`, `src/main.rs:1712`). A `{{ user.home }}` that push writes into `gitconfig.repo` therefore reaches a freshly initialized project **literally**, while sync scan and pull render it (`src/sync/mod.rs:642-645`, `1065-1067`). Success criterion 4 ("substitute back cleanly … on initialization") is false today for 6 of the 8 publishable managed files (`.gitconfig`, `.gitignore`, the 3 hooks, `check-in-container.sh`; only `mise.toml` and `.agents/AGENTS.md` map to `.jinja`). Separately, a push-time render-back check is needed. minijinja 2.24.0 fails to render the shipped `commit-msg` hook (`${#FIRST_LINE}`, a Jinja comment opener) as soon as a placeholder is added to it (probe output below). Without the check, one push would publish a template that breaks `ude sync pull` for every consumer.

**Primary recommendation:** Build one byte-level scanner, `Sanitizer::scan_bytes`. It runs UDE's 6 named rules, then the embedded gitleaks v8.30.1 rules, then R11. Call it on the *published* bytes at staging (including the non-UTF-8 branch that is unscanned today) and on raw blob bytes at both gates. Replace the path regex with a predicate scanner. Add a push-time render-back check through the real `TemplateRenderer`, and make `ude init` render non-`.jinja` templates that contain `{{` (the rule scan and pull already use).

## Architectural Responsibility Map

Single-binary CLI; the "tiers" are modules.

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Entropy (R11), shapes, marker | `src/sanitizer` (new `entropy.rs`) | — | Pure function over bytes; no I/O. |
| Gitleaks rule loading + matching | `src/sanitizer` (new `gitleaks.rs` + vendored `rules/gitleaks.toml`) | — | Compile once per process (`LazyLock`); pure matching. |
| Scan orchestration and refusal reason (rule + line) | `src/sanitizer/mod.rs` (`Sanitizer`) | — | One entry point keeps staging and gate verdicts identical. |
| Pre-staging scan of published bytes | `src/sync/mod.rs` `push_updates` (L1446-1475) | — | Scans exactly the bytes that will be staged. |
| Publication gates (index + committed HEAD) + rollback | `src/sync/mod.rs` `stage_publication` (L1736), `validate_committed_tree` (L2033), `rollback_commit` (L1959) | — | Already exist; swap the decode-and-scan call only. |
| Path parameterization (boundaries) | `src/sanitizer/mod.rs` `parameterize_paths` | — | Pure string transform. |
| Render-back reversibility check at push | `src/sync/mod.rs` `push_updates` | `src/template` (`TemplateRenderer`) | Must use the same renderer and render rule as scan/pull/init, not a string-replace proxy. |
| Placeholder substitution on init | `src/main.rs` init (`copy_template_file`, `kept_ancestor`) | `src/template` | The defect lives here; share one "should render" helper with sync. |
| Pre-API scan (TypeSafe) | `src/main.rs` ~L1074 | — | Unchanged call; inherits new scanner automatically. |

## Standard Stack

### Core (all already dependencies — no additions)
| Library | Version (Cargo.lock) | Purpose | Why |
|---------|---------|---------|-----|
| `regex` | 1.13.1 | `regex::bytes::RegexBuilder` with `.unicode(false)` for the gitleaks port and UDE rules | Linear-time, no look-around; ASCII mode equals Go RE2's ASCII `\w \d \s \b` [VERIFIED: probe this session + CITED: pkg.go.dev/regexp/syntax] |
| `toml` | 0.8.23 | Parse the embedded gitleaks rule file into serde structs | Parsed the 3209-line v8.30.1 file in 4 ms release / 44 ms debug [VERIFIED: probe] |
| `serde` | 1.0 (derive) | Rule structs | existing |
| `sha2` | 0.10 | Test that pins the vendored rule file's SHA-256 | existing |
| `minijinja` | 2.24.0 | Render-back check reuses `crate::template::TemplateRenderer` | existing [VERIFIED: Cargo.lock] |
| `proptest` (dev) | 1.11.0 | `TestRunner::deterministic()` + regex-string strategies for seeded fixtures; round-trip properties | [CITED: Context7 /proptest-rs/proptest — `TestRunner::deterministic()`, `"<regex>".new_tree(&mut runner)`] |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Embedded rule port | Shell out to the pinned `gitleaks` binary | Exact fidelity (incl. its base64/hex decoding), but adds a hard runtime dependency (Go, not Rust). Under supply-chain item 6 every machine without gitleaks could not push. It also adds version skew and a process per blob. Rejected. |
| Embedded rule port | Hand-curated subset of ~30 rules in Rust code | Cannot claim "full Gitleaks categories"; drifts silently. Rejected. |
| `regex::Regex` (Unicode) | — | 7 of 222 patterns fail and compile takes 3.4 s release; not Go-faithful. Rejected. |
| An entropy/base64 crate | — | R11 is ~40 lines, fully specified by D-05; gitleaks itself hand-writes `shannonEntropy` (detect/utils.go:117). No Base64 *decoding* is needed. Do not add a crate. |
| `{{ xdg.config }}` alias (remediation 2.2 item 2) | — | Not recommended this phase: a second spelling must be added to `PARAMETERIZED_PLACEHOLDERS`, `template_has_logic` and `TemplateContext`, and gains nothing for SEC-03/04. `docs/ARCHITECTURE.md:80` lists `{{ xdg.config }}`; fixing that doc line is cheaper than a second spelling. |

**Installation:** none.

## Package Legitimacy Audit

No external packages are installed by this phase. Every crate used is already in `Cargo.lock` (`regex` 1.13.1, `toml` 0.8.23, `serde`, `sha2`, `minijinja` 2.24.0, `proptest` 1.11.0). The one new third-party artifact is a **data file**, not a package:

| Artifact | Source | Pin | License | Disposition |
|---------|--------|-----|---------|-------------|
| gitleaks default config `config/gitleaks.toml` | `https://raw.githubusercontent.com/gitleaks/gitleaks/v8.30.1/config/gitleaks.toml` (latest release v8.30.1, published 2026-03-21 per GitHub API) | SHA-256 `e163e53b9e7e8a8511e77271e2b323ed057759542a6d988258afe3a1fa329caf`, 3209 lines, 222 `[[rules]]` | MIT ("Copyright (c) 2019 Zachary Rice") | Vendor verbatim; ship the MIT notice beside it. `cargo deny` does not see vendored data, so the notice is a manual obligation. [VERIFIED: downloaded + sha256sum this session] |

**Packages removed due to [SLOP] verdict:** none. **Packages flagged [SUS]:** none.

## Architecture Patterns

### System Architecture Diagram

```
 ude sync push
   │
   ▼
 push_updates (src/sync/mod.rs)
   │  for each LocallyModified managed file
   ├─► read local bytes
   │     ├─ UTF-8 ──► parameterize_paths ──► apply EOL style ──► published bytes
   │     └─ non-UTF-8 ─────────────────────────────────────────► published bytes (raw)
   │
   ├─► Sanitizer::scan_bytes(published)  ── hit ──► PushResult::SecretBlocked (exit 1)
   │        1. UDE named rules (6)                    reason = "<rule> at line N" (no value)
   │        2. gitleaks v8.30.1 rules (keyword prefilter → regex → secretGroup → entropy → allowlists)
   │        3. R11 entropy (skip lines with `ude:allow-entropy`)
   │
   ├─► reversibility check (UTF-8 only): render(published, ctx{home,xdg}) == local text?
   │        ── mismatch / render error ──► refusal (exit 1), file named
   ▼
 StagedTransaction writes upstream templates
   ▼
 push_to_remote → stage_publication: git add allowlist → git diff --cached
   │      └─► for each staged blob: git show :path → scan_bytes(raw stdout) ── hit ──► reset index, Err (exit 2)
   ▼
 commit (--no-verify) → validate_committed_tree: git show HEAD:path → scan_bytes(raw stdout)
   │      └─ hit ──► rollback_commit (+ abandon_sync_branch in PR mode), Err (exit 2)
   ▼
 git push (direct, never forced) / PR branch

 ude init / sync scan / sync pull
   └─► template bytes → should_render(path, text) [".jinja" ext OR contains "{{"] → TemplateRenderer::render_str(ctx)
        (today init skips this for non-.jinja: the SEC-04 defect)
```

### Recommended Project Structure
```
src/sanitizer/
├── mod.rs            # Sanitizer facade: scan_bytes, check_secrets (wrapper), parameterize_paths, is_local_only_path, UDE named rules
├── entropy.rs        # R11: tokenizer, shapes, marker, shannon, randomness test
├── gitleaks.rs       # serde structs, go_compat_braces, LazyLock<RuleSet>, match semantics
└── rules/
    ├── gitleaks.toml          # vendored verbatim, v8.30.1 (keep this exact file name, see Pitfall 6)
    └── LICENSE-gitleaks       # MIT notice
```

### Pattern 1: One byte-level scan entry point used by every caller
**What:** `pub fn scan_bytes(&self, bytes: &[u8]) -> Option<Finding>`, where `Finding { rule: String, line: usize }` never holds the value. Keep `check_secrets(&str) -> SanitizationResult` as a thin wrapper so `main.rs` ~L1075 and existing tests keep compiling. Put the line number in `reason`, which avoids changing the enum.
Current enum, verbatim [VERIFIED: src/sanitizer/mod.rs:6-9]:
```rust
pub enum SanitizationResult {
    Clean(String),
    SecretDetected { reason: String, pattern: String },
}
```
**Order:** named UDE rules → gitleaks rules → R11, first hit wins. D-03 then follows from the order: the marker is consulted only inside R11.
**Call sites to switch** [VERIFIED: read this session]:
- `src/sync/mod.rs:1446-1475`: today it scans raw `content` *before* parameterization and **does not scan the non-UTF-8 branch at all** (`(raw_bytes.clone(), None)` at L1474). Scan the final `published` bytes in both branches instead.
- `src/sync/mod.rs:1736-1738` (`stage_publication`) and `2033-2035` (`validate_committed_tree`): replace `String::from_utf8_lossy(&blob_res.stdout)` + `check_secrets` with `scan_bytes(&blob_res.stdout)`.
- `src/main.rs:1074-1086`: unchanged; it gets the new behaviour through `check_secrets`.

### Pattern 2: Faithful gitleaks port
Semantics read from `detect/detect.go` at v8.30.1 [VERIFIED: fetched this session, lines 371-560]:
1. Keyword prefilter: skip the rule unless a lower-cased keyword occurs in the lower-cased fragment (rules with no keywords always run).
2. For each regex match: secret = capture group `secretGroup` if > 0; else the **first non-empty** capture group; else the whole match.
3. If `entropy` is set and `shannonEntropy(secret) <= entropy`, skip. Gitleaks divides rune counts by the **byte** length (`utils.go:117-133`).
4. Skip if the global allowlist or any rule allowlist matches. Each allowlist has a `regexTarget`: `secret` by default, or `match` / `line`. Stopwords are substring-matched against the lower-cased secret. `condition = "AND"` requires every defined check to pass.
5. Rules with `path`: 5 in v8.30.1 (`freemius-secret-key`, `hashicorp-tf-password`, `kubernetes-secret-yaml`, `nuget-config-password`, `pkcs12-file`; `pkcs12-file` is path-only). Pass the publish-relative path and honour the `path` regex. A path-only rule produces a finding on path match.
6. Do **not** implement `gitleaks:allow` (D-03), the global `paths` allowlist (managed files are a fixed list of 11, none matching), or gitleaks' encoded-segment decoding (document it as a gap; R11 covers most encoded blobs).

### Pattern 3: Boundary predicate instead of regex
**What:** iterate `content.match_indices(needle)`. Accept a hit only if both boundary predicates hold; copy the text between accepted hits unchanged. This runs in one pass, needs no fixpoint loop (the current loop exists because `replace_all` consumes the shared delimiter: probe one-pass output `H:/home/alice`), and needs no regex built from user input.
- **Leading:** start of text, or previous char is not alphanumeric (Unicode) and not `_` / `.`. Unlike today's class, `-` **counts as** a boundary, so `${W:-/home/alice}` parameterizes. A dir name ending in `-` immediately followed by `home/<user>` is not a realistic path.
- **Trailing:** end of text, or next char is not alphanumeric / `_` / `-`; and if the next char is `.`, the char after it must be end-of-text or not alphanumeric / `_` / `-`. Under this rule `/home/alice.` (end of sentence) parameterizes, while `/home/alice.bak` and `/home/alice-old` do not.
- Apply `xdg_config` before `user_home` (as today), and only when `xdg_config != user_home`.
- **Guard degenerate homes:** skip parameterization unless `user_home` is absolute with at least one normal component (not `/`, not `.`). `src/main.rs:715` and `src/template/mod.rs:47` fall back to `"."` when no home is found, and a container may run with `HOME=/`.
This set is a strict superset of the remediation set `[/\\"'\s:,;()\[\]<>]|$`, so remediation plan 2.2's verification cases still hold.

### Pattern 4: Reversibility = the real pipeline, not an inverse function
Push-time (UTF-8 candidates only): build a context whose `user.home` / `user.xdg_config` (and the flat `user_home` / `xdg_config`) equal the **parameters** passed to `push_updates`. Do not use the `detect_for_project` values: `push_updates` today builds `dummy_ctx` from the real host home (`src/sync/mod.rs:1330-1331`), which differs from the `user_home` argument in tests. Render `published` with `TemplateRenderer::render_str` under the same should-render rule as scan/pull. Require `Ok(rendered)` with `normalize_eol(rendered) == normalize_eol(content)`; otherwise refuse that file. A string-replace inverse would report "reversible" for exactly the cases minijinja breaks (proxy measurement).

### Anti-Patterns to Avoid
- **Literal `\b` around a path:** inverted semantics for a leading `/` (probe table below).
- **`String::from_utf8_lossy` before scanning:** replace with byte scanning; there is then no lossy decode anywhere to reason about.
- **Compiling rules in `Sanitizer::new()`:** `SyncEngine::new()`/`Sanitizer::new()` appear 71 times in `src/` (57 in `src/sync/mod.rs`, mostly tests); at ≈0.45 s debug each that is minutes of test CPU. Use a `static RULES: LazyLock<…>`.
- **Unicode-mode `regex::Regex` for gitleaks patterns:** compile failures (size limit) and a 3.4 s startup.
- **Scanning raw local text at staging while the gate scans parameterized blobs:** two different byte strings → verdicts can disagree (staging passes, gate blocks with exit 2). Scan the published bytes.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Secret rule catalogue | Your own list of provider regexes | Vendored gitleaks v8.30.1 `config/gitleaks.toml` | 222 maintained rules with keywords, entropy floors and allowlists; regenerated upstream |
| TOML parsing of rules | Line-based parser | `toml` 0.8 + serde | Rule file uses `'''` literal strings and nested `[[rules.allowlists]]` |
| Seeded random fixtures | Ad-hoc RNG in tests | `proptest::test_runner::TestRunner::deterministic()` + `"[A-Za-z0-9+/]{20}".new_tree(&mut runner)` | Deterministic, already a dev-dependency |
| Placeholder inverse | `str::replace("{{ user.home }}", home)` | `crate::template::TemplateRenderer::render_str` | The real renderer is what init/scan/pull use; minijinja parses `{#`, `{%`, `{{` |

Hand-writing is right for: Shannon entropy (D-05 fully specifies it; gitleaks does the same), the R11 randomness test, the path boundary predicate (no look-around in `regex`), and the Go `{` escape (15 lines).

## Runtime State Inventory

Not a rename/migration phase. One related item applies:

| Category | Items Found | Action Required |
|----------|-------------|------------------|
| Stored data | Upstream template repos may already hold templates published by earlier pushes. Code reading suggests these would contain `{{ user.home }}` in non-jinja files that `init` copies verbatim. No such file is in this repo's `templates/` (rg: only `mise.toml.jinja` holds Jinja syntax under `templates/projects`). | None in-repo; the init fix makes existing ones render. |
| Live service config | None — verified: no service config involved. | — |
| OS-registered state | None. | — |
| Secrets/env vars | None renamed. | — |
| Build artifacts | None. | — |

## Common Pitfalls

### Pitfall 1: Go-vs-Rust regex dialect
**What goes wrong:** 7 of the 222 gitleaks patterns fail in `regex::Regex::new` [VERIFIED: probe]:
`global` allowlist ×2 (`^\$(?:\d+|{\d+})$`, `^\${(?:[A-Z_]+|[a-z_]+)}$`), `curl-auth-user` allowlist, `generic-api-key` allowlist (`import[ \t]+{…`): "repetition quantifier expects a valid decimal". `generic-api-key`, `pypi-upload-token`, `vault-batch-token`: "Compiled regex exceeds size limit of 10485760 bytes".
**Why:** Go's parser treats a `{` that cannot start a repeat as a literal ("If the repeat cannot be parsed, { is a literal." [CITED: golang/go src/regexp/syntax/parse.go]); Rust errors. Rust `\w`/`\b` are Unicode; Go's are ASCII-only ("all ASCII-only" [CITED: pkg.go.dev/regexp/syntax]), and Unicode `\w` inside `[\w.-]{0,50}?` inflates the automaton.
**How to avoid:** escape non-repeat `{` (outside escapes) before compiling; build with `regex::bytes::RegexBuilder::new(p).unicode(false)`. Probe result: 0 errors, 48 ms release.
**Warning signs:** a loader that `filter_map`s errors away; a rule count below 222 with no failing test.

### Pitfall 2: A load failure that becomes a skip
**How to avoid:** the loader returns `Result`. A unit test asserts exactly 222 rules (221 regex, 1 path-only) and 0 compile errors. If `LazyLock` initialization fails in a shipped build it panics. That cannot happen once the test passes, and it is a failure, never a silent skip (supply-chain item 6).

### Pitfall 3: R11 interpretation drift
The Rust probe reproduced the CONTEXT numbers only under these readings. Lock them in the plan:
- Entropy counts **characters**; threshold compare is `>=`; length is the token's char count.
- Tokens: maximal ASCII runs of `[A-Za-z0-9+/=_\-.]`, len ≥ 20; any other byte (including non-ASCII) is a delimiter.
- Hex (exactly 40/64/128), UUID (8-4-4-4-12) and SRI (`^sha(256|384|512)-[A-Za-z0-9+/]+={0,2}$`) are exempt as **whole tokens**.
- An ssh key body (`AAAA[A-Za-z0-9+/]+={0,3}`) is exempt only when it follows a key type (`ssh-ed25519|ssh-rsa|ssh-dss|ecdsa-sha2-nistp(256|384|521)|sk-ssh-ed25519@openssh.com|sk-ecdsa-sha2-nistp256@openssh.com`) and whitespace on the same line.
- Marker: the line contains `ude:allow-entropy`, so all entropy-only hits on that line are suppressed.

### Pitfall 4: The non-UTF-8 staging hole
Today a non-UTF-8 managed file is staged **without any secret scan** (`src/sync/mod.rs:1473-1475`). The gate then scans a lossy decode. ASCII secrets inside binary junk survive a lossy decode, so the gate probably catches them. But staging-time refusal (exit 1, nothing staged) is skipped. Write a RED test: invalid bytes plus an ASCII token at staging must return `SecretBlocked`.

### Pitfall 5: Init does not render non-jinja templates (SEC-04 defect)
`copy_template_file` (`src/main.rs:1641-1655`) is `fs::copy`; `kept_ancestor` renders only `.jinja` (`src/main.rs:1712`). Scan/pull render when ".jinja OR contains `{{`" (`src/sync/mod.rs:642-645`, `1065-1067`). Extract one `should_render(path, text)` helper and use it in init (`render_and_write` for those files), `kept_ancestor`, scan and pull. Shipped non-jinja templates contain no `{{` [VERIFIED: rg this session], so this changes nothing for current templates.

### Pitfall 6: The repo's own gitleaks and the vendored rule file
CI runs `gitleaks git --redact` with the default config. Its global `paths` allowlist contains `gitleaks\.toml`, so the vendored file **must be named `gitleaks.toml`** (e.g. `src/sanitizer/rules/gitleaks.toml`). Any other name makes CI scan its stopword hex strings and examples. Also keep every new secret fixture runtime-built.

### Pitfall 7: Placeholder insertion makes a hook unrenderable
`templates/projects/generic/hooks/commit-msg:42-43` contains `${#FIRST_LINE}`. minijinja 2.24.0 probe:
`"if [ \"${#FIRST_LINE}\" -gt 72 ]; then\n  {{ user.home }}/bin/x\nfi\n" -> Err("syntax error: unexpected end of comment (in t:4)")`. Same for `{{{{ user.home }}` (from `{{/home/alice`) and `${{ secrets.TOKEN }}` (`undefined value`). Only the push-time render-back check catches these before publication.

### Pitfall 8: Alias home paths
On this host `/home -> var/home` [VERIFIED: `ls -ld /home`]. A file referencing `/home/<user>/…` while `$HOME` is `/var/home/<user>` is not parameterized, and the render-back check passes it (it renders back unchanged), so the username is published. Shipped `templates/projects/agents/agents.md.jinja:53-54` already contains `file:///home/denniyahh/...`, and `template/scratch/sync-personal-env.sh:12` contains `${GSD_PERSONAL_WORKSPACE:-/var/home/denniyahh/...}`. Neither came from push. Out of scope per the deferred todo; see Open Questions.

## Code Examples

### Go-compatible brace escape + ASCII byte regex (probe-verified)
```rust
// Source: probe this session (scratch gl.rs); semantics per golang regexp/syntax parse.go
fn go_compat_braces(p: &str) -> String {
    let b = p.as_bytes();
    let mut out = String::with_capacity(p.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 1 < b.len() {
            out.push_str(&p[i..i + 2]); // keep escapes; ASCII-only escapes in the rule file
            i += 2;
            continue;
        }
        if b[i] == b'{' && !starts_valid_repeat(&p[i + 1..]) {
            out.push_str("\\{");
            i += 1;
            continue;
        }
        let ch = p[i..].chars().next().unwrap_or('\0'); // planner: avoid unwrap in non-test code
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}
// starts_valid_repeat: `\d+}` or `\d+,\d*}`
fn compile(p: &str) -> Result<regex::bytes::Regex, regex::Error> {
    regex::bytes::RegexBuilder::new(&go_compat_braces(p)).unicode(false).build()
}
```

### R11 core (probe-verified against CONTEXT numbers)
```rust
const ENTROPY_FACTOR: f64 = 0.80;
const ENTROPY_LEN_CAP: usize = 64;
const SWITCH_FACTOR: f64 = 0.75;
fn passes_r11(tok: &[u8]) -> bool {
    let n = tok.len(); // ASCII-only tokens: bytes == chars
    if shannon(tok) < ENTROPY_FACTOR * (n.min(ENTROPY_LEN_CAP) as f64).log2() { return false; }
    let classes: Vec<u8> = tok.iter().filter_map(|c| match c {
        b'A'..=b'Z' => Some(0), b'a'..=b'z' => Some(1), b'0'..=b'9' => Some(2), _ => None }).collect();
    if classes.len() < 2 { return false; }
    let sizes = [26.0, 26.0, 10.0];
    let present: Vec<f64> = (0..3u8).filter(|c| classes.contains(c)).map(|c| sizes[c as usize]).collect();
    let total: f64 = present.iter().sum();
    let expected = 1.0 - present.iter().map(|s| (s / total).powi(2)).sum::<f64>();
    if expected <= 0.0 { return false; }
    let changes = classes.windows(2).filter(|w| w[0] != w[1]).count();
    changes as f64 / (classes.len() - 1) as f64 >= SWITCH_FACTOR * expected
}
```

### Placeholders that must stay in sync
[VERIFIED: src/sync/mod.rs:2264] `const PARAMETERIZED_PLACEHOLDERS: [&str; 2] = ["{{ user.home }}", "{{ user.xdg_config }}"];`. The parameterizer must emit exactly these two strings.

## Probe Evidence (this session; scratch removed afterwards)

**Gitleaks port vs real binary** (generated positives, 13 categories; values never printed):
port hits = binary hits for `anthropic-api-key, aws-access-token, gcp-api-key, generic-api-key, github-pat, gitlab-pat, jwt, npm-access-token, private-key, pypi-upload-token, slack-bot-token, stripe-access-token, vault-batch-token`. The port also reports `generic-api-key` on 5 files where gitleaks dedupes generic findings that overlap a specific rule. That does not matter for blocking.
Repo (149 tracked files): port 0, `gitleaks dir` on a `git archive HEAD` snapshot 0.
Negative control for the allowlist logic: with allowlists disabled the port reports 1 hit (`generic-api-key`, a `.planning` summary), so the allowlist code is exercised. This is weak coverage: only one case on this corpus.
Timing: ASCII mode release compile 48 ms and repo scan 88 ms; debug compile 406 ms and repo scan 1.7 s; Unicode mode release compile 3.25–3.47 s.

**R11 in Rust** (xorshift seed, 1000 samples each; % blocked):
```
       base64: 20=93.1% 24=91.9% 32=94.1% 40=97.8% 64=99.1% 88=100.0%
      urlsafe: 20=92.0% 24=94.1% 32=95.4% 40=97.3% 64=98.7% 88=99.8%
        alnum: 20=92.8% 24=94.4% 32=94.9% 40=96.9% 64=99.0% 88=99.7%
 base36-lower: 20=80.7% 24=81.8% 32=78.2% 40=82.0% 64=16.5% 88=68.4%
 base36-upper: 20=81.0% 24=84.2% 32=79.7% 40=80.9% 64=20.2% 88=68.8%
          hex: 20=28.1% 24=9.3% 32=0.0% 40=0.0% 64=0.0% 88=0.0%
jwt-shaped: 100.0%   aws-secret-40: 97.4%   leading-plus-24: 91.8%
all 11 negative fixtures pass; marker control: bare_hits=1 marked_hits=0
repo: files=149 tokens>=20=4695 hits=1 (src/sanitizer/mod.rs:231, the literal JWT test fixture)
templates/ + template/: files=33 tokens>=20=260 hits=0
```
What this does not establish: the RNG differs from the Python prototype, so rows agree within the stated ±1.5 points, not exactly. The corpus counts are still this repo only. Hex 20–24 chars is *sometimes* blocked (28% / 9%), which the CONTEXT table did not show.

**Boundary semantics** (`regex` 1.13.1):
```
case                       naive_b  current  remediation
cd /home/alice now         false    true     true
/home/alice.bak            false    false    false
(/home/alice)              false    false    true
${H:-/home/alice}          false    false    false
see /home/alice.           false    false    false
a;/home/alice;b            false    false    true
/home/alice/home/alice     true     true     true
${W:-/home/alice/Github/x} false    false    false
file:///home/alice/x       false    true     true
one-pass replace_all("/home/alice:/home/alice") = "H:/home/alice"
```

## State of the Art

| Old Approach | Current Approach | Impact |
|--------------|------------------|--------|
| Remediation plan "≥20 chars, entropy > 4.2" | R11 (D-05) | 300 → 1 repo FP; 19% → 93% at 20 chars |
| Remediation plan 2.1 item 4 "parse TOML/YAML/JSON/INI scalar values" | D-04 scores every token in all text | Superseded; do not build format parsers |
| gitleaks v8 `[rules.allowlist]` (single) | `[[rules.allowlists]]` (array) with `regexTarget`, `condition` | v8.30.1 file uses the array form (13 entries); the global one is still `[allowlist]` ("TODO: change to [[allowlists]]") |

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | Managed files are never UTF-16; byte scanning without decoding is sufficient | Pattern 1 | A UTF-16 file with a secret passes all scans. Mitigation option: refuse a UTF-16 BOM. |
| A2 | Treating a leading `-` as a boundary never corrupts a real path | Pattern 3 | A dir name ending in `-` followed by `/home/<user>` would be rewritten. |
| A3 | Not porting gitleaks' encoded-segment decoding is acceptable (R11 catches most encoded blobs) | Pattern 2 | A base64-wrapped short secret that fails R11 is missed. |
| A4 | Not honouring gitleaks' global `paths` allowlist is correct for UDE's 11 managed files | Pattern 2 | None foreseen; the list is fixed in code. |
| A5 | A refusal on reversibility failure should be exit 1 (refusal), matching `SecretBlocked` | Pattern 4 | Scripts branching on exit 2 would misclassify. |
| A6 | The gate's existing exit 2 (`PushRemoteError::Failed` → `EXIT_HARD_FAILURE`, `src/main.rs:1290-1295`) stays as-is for gate-level secret hits: such a hit means content mutated after the staging scan | Discretion: refusal output | If the operator wants all secret refusals at exit 1, the gate needs its own error variant. |

## Open Questions

1. **Alias and unparameterizable home paths (`/home/<u>` vs `/var/home/<u>`, `-I/home/<u>/include`).**
   - Known: these publish the username today and still will after SEC-03 (the render-back check passes them).
   - Unclear: whether the operator wants push to refuse a residual occurrence of the home path, or of its `/home` alias.
   - Recommendation: keep it out of this phase (the deferred todo already covers it). Mention it in the phase summary so the todo gets re-checked.
2. **Should a gate-level secret hit exit 1 or 2?** Today it exits 2 (see A6). Recommendation: keep 2. Decided within Claude's discretion unless the operator objects.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| cargo / rustc | build, tests | ✓ | 1.99.1 | — |
| cargo-nextest | TDD RED evidence | ✓ | 0.9.148 | `cargo test` (not accepted as RED evidence) |
| cargo-deny / cargo-machete / cargo-mutants / cargo-llvm-cov | gates | ✓ | present on PATH | — |
| git | gate integration tests | ✓ | 2.56.0 | — |
| gitleaks | one-off parity check only (not a runtime or test dependency) | ✓ | 8.30.1 | skip parity step |
| network (raw.githubusercontent.com) | fetching the pinned rule file once | ✓ (this session) | — | none; required once (SHA-256 check makes the source irrelevant) |

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in test harness run via cargo-nextest 0.9.148; proptest 1.11.0 |
| Config file | `.config/nextest.toml` (JUnit at `target/nextest/default/junit.xml`) |
| Quick run command | `cargo nextest run --bin ude sanitizer::` |
| Full suite command | `cargo nextest run --workspace && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo deny check && cargo machete` |
| Baseline (this session) | `cargo nextest run --bin ude sanitizer:: test_defect3` → 11 passed, 129 skipped |

### Phase Requirements → Test Map
Every row carries its negative control and its failing direction.

| Req | Behavior | Type | Test (proposed name) | Negative control / failing direction |
|-----|----------|------|----------------------|--------------------------------------|
| SEC-01 | Seeded random Base64/URL-safe at 20/32/64 blocked ≥ 0.88/0.90/0.95; AWS-40 ≥ 0.95; JWT-shaped ≥ 0.99; leading `+` ≥ 0.88 | unit (sampled) | `entropy::tests::r11_block_rates_seeded` | Same harness on hex-40 asserts rate == 0 (proves the harness can report a miss). Fails if a factor is mutated (e.g. 0.80→0.90 drops the 20-char rate below 0.88); the message prints the rate. |
| SEC-01 | Each CONTEXT negative fixture passes | unit | `entropy::tests::r11_negative_fixtures` | Paired positive: a random 32-char Base64 on the same line format must block. |
| SEC-01 | Marker silences entropy only | unit | `tests::marker_silences_entropy_not_named_rules` | Same token without the marker blocks; a runtime-built `ghp_` token **with** the marker still blocks (D-03). |
| SEC-01 | Corpus control: `templates/` + `template/` → 0 hits | unit (reads repo via `env!("CARGO_MANIFEST_DIR")`) | `tests::corpus_templates_zero_hits` | Assert files scanned ≥ 33 and tokens ≥ 200 (guards against "0 because nothing read"); copy one corpus file into a TempDir with a runtime random token appended → ≥ 1 hit. |
| SEC-02 | Embedded rule file loads: 222 rules, 0 errors, SHA-256 pinned | unit | `gitleaks::tests::rules_load_all` | A TOML string with one invalid regex returns `Err` (errors are not filtered away). Fails if a rule is dropped. |
| SEC-02 | Go brace compat | unit | `gitleaks::tests::go_brace_literal` | `^\$(?:\d+|{\d+})$` errors without the transform; with it, compiles and matches `${12}`, does not match `${x}`. |
| SEC-02 | One runtime-built positive per category (private key, cloud, VCS, registries, chat, payment, AI APIs, JWT, generic assignment, observability, secret managers, curl auth) → expected rule id | unit (table) | `gitleaks::tests::category_positives` | Benign twin per row (e.g. `api_key = "{{ user.home }}"`, all-`a` value) → clean. |
| SEC-02 | Gitleaks-only category (e.g. Slack bot token, not caught by the 6 UDE rules today) injected by clean filter → staged-blob gate rejects; index reset | integration (reuse `test_publication_gate_scans_staged_blob_rejecting_clean_filter_secrets` harness) | `sync::tests::gate_blocks_gitleaks_only_category` | RED first: passes push before the change. Benign filter output → `Ok`. |
| SEC-02 | Committed-tree gate + rollback for a gitleaks-only category | integration | `sync::tests::committed_gate_rolls_back_gitleaks_only_category` | Clean commit passes `validate_committed_tree`; after rollback, HEAD blob lacks the token. |
| SEC-02 | Non-UTF-8: invalid bytes + ASCII token → blocked at staging and gate | unit + integration | `tests::scan_bytes_non_utf8`, `sync::tests::push_blocks_secret_in_non_utf8_file` | Invalid bytes alone → clean; existing `test_binary_non_utf8_files_scanned_and_tracked_by_hash` stays green. |
| SEC-03 | Boundary table: probe cases with expected outputs, plus `${W:-/home/alice/x}`, `/home/alice}`, `/home/alice.`, `/home/alice,`, `(/home/alice)` → replaced; `/home/alice2`, `/home/alice.bak`, `/home/alice-old`, `/backup/home/alice` → untouched | unit | `tests::path_boundary_table` | The untouched rows are the controls; running the table against the old regex must fail on `(/home/alice)`. |
| SEC-03 | Property: for generated home names and suffixes starting with `[A-Za-z0-9_-]` or `.`+word char, output == input | proptest | `tests::prop_no_partial_substring_corruption` | Companion property with delimiter suffix asserts the placeholder appears. |
| SEC-03 | Degenerate home (`/`, `.`, relative) → no change | unit | `tests::degenerate_home_not_parameterized` | `/home/alice` home still parameterizes. |
| SEC-04 | Property: `render(parameterize(x, h, c), ctx{h,c}) == x` for `x` free of `{`/`}` | proptest | `tests::prop_parameterize_render_roundtrip` | Cross-machine: rendering with home `h2` leaves no boundary-delimited occurrence of `h`. |
| SEC-04 | Push refuses irreversible content (hook with `${#X}` + home path) | integration | `sync::tests::push_refuses_unrenderable_parameterized_file` | Same hook without a home path pushes (no `{{` inserted). |
| SEC-04 | `ude init` renders a non-jinja template containing `{{ user.home }}` | integration (`tests/e2e_lifecycle.rs` or main tests) | `init_renders_placeholders_in_static_templates` | A `.repo` without `{{` is byte-identical after init (including `${#…}`). Existing `test_defect3_path_parameterization_roundtrip_static_template` stays green. |

### Sampling Rate
- **Per task commit:** `cargo nextest run --bin ude sanitizer::` plus the touched sync test filter.
- **Per wave merge:** full suite command above.
- **Phase gate:** full suite green; `cargo mutants -v --file src/sanitizer/entropy.rs` and `--file src/sanitizer/mod.rs` (boundary predicate), with no surviving mutants in R11 or the boundary predicate. Plus a one-off parity step: generate positives at runtime in a scratch dir, run both the port and `gitleaks dir --config <vendored>`, and diff rule ids (expect the generic-dedupe difference only). Delete the scratch dir afterwards.

### Wave 0 Gaps
- [ ] Seeded fixture helper (proptest `TestRunner::deterministic()`) in `src/sanitizer` tests.
- [ ] Corpus walker helper (lists `templates/`, `template/` under `CARGO_MANIFEST_DIR`).
- [ ] Vendored `src/sanitizer/rules/gitleaks.toml` + `LICENSE-gitleaks`, fetched by the executor and checked against SHA-256 `e163e53b9e7e8a8511e77271e2b323ed057759542a6d988258afe3a1fa329caf`.
- [ ] Convert the literal JWT at `src/sanitizer/mod.rs:231` to a runtime-built fixture (it is the 1 repo FP and breaks the fixture policy).

## Security Domain

### Applicable ASVS Categories (Level 1)
| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | — |
| V3 Session Management | no | — |
| V4 Access Control | yes (publish boundary) | Existing `is_local_only_path` + staged allowlist; unchanged |
| V5 Input Validation | yes | Byte-level scanning of untrusted file content; linear-time regex (no ReDoS: "worst case O(m * n)" [CITED: Context7 /rust-lang/regex README]) |
| V6 Cryptography | no (sha2 used for pinning only) | — |
| V7 Error Handling & Logging | yes | Refusals name rule + file:line, never the value |
| V8 Data Protection | yes | Fail-closed secret gates at staging, index and committed HEAD; rollback |
| V14 Configuration | yes | Vendored rules pinned by SHA-256 test; MIT notice shipped |

### Known Threat Patterns
| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Secret slips via encoding change (lossy decode, binary file) | Information disclosure | Scan raw bytes everywhere; scan the non-UTF-8 staging branch |
| Content mutated between scan and commit (clean filter, hooks) | Tampering | Existing index + HEAD gates, now with the full rule set |
| Marker abuse (`ude:allow-entropy` on a real key) | Information disclosure | D-03: named rules ignore the marker; residual risk for prefix-less keys is accepted by D-02 |
| Regex blowup / slow startup | Denial of service | ASCII byte regex, `LazyLock` compile once, linear-time engine |
| Published template that breaks consumers' pull | Denial of service / Tampering | Push-time render-back check |
| Home path / username leak | Information disclosure | Boundary predicate; residual alias leak tracked in todo |

## Sources

### Primary (HIGH)
- Codebase, read this session: `src/sanitizer/mod.rs:1-343`; `src/sync/mod.rs:125-175, 207-219, 620-660, 1321-1500, 1597-2057, 2262-2330, 6127-6185, 6246-6310, 6966-7030, 7158-7208`; `src/main.rs:14-20, 1060-1095, 1130-1150, 1240-1350, 1641-1730`; `src/template/mod.rs:1-175`; `docs/REMEDIATION_PLAN.md:93-130`; `docs/ARCHITECTURE.md:70-90`; `templates/universal/generic/security/gitleaks.toml`.
- gitleaks v8.30.1 `config/gitleaks.toml`, `detect/detect.go`, `detect/utils.go`, `LICENSE` (raw.githubusercontent.com, fetched this session); GitHub API latest release.
- Context7 `/rust-lang/regex`: no look-around; `\b` semantics; linear time.
- Context7 `/proptest-rs/proptest`: `TestRunner::deterministic()`, regex strategies.
- golang/go `src/regexp/syntax/parse.go` (literal `{`); pkg.go.dev/regexp/syntax (ASCII-only classes).
- Probes (Rust, offline, crate versions pinned to this repo's Cargo.lock) and `gitleaks` 8.30.1 binary cross-check.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH. No new crates; every API used was compiled and run in a probe.
- Architecture: HIGH for call sites and defects (read and, where possible, executed); MEDIUM for the boundary set (heuristic, A2).
- Pitfalls: HIGH. Each one is backed by probe output or a code citation.

**Research date:** 2026-10-23
**Valid until:** 2026-11-22. Re-check the gitleaks release if planning slips; the pin is deliberate either way.
