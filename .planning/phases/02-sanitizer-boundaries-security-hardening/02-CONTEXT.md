# Phase 2: Sanitizer, Boundaries & Security Hardening - Context

**Gathered:** 2026-10-23
**Status:** Ready for planning

<domain>
## Phase Boundary

Screen and redact credentials and user-specific paths before anything reaches the shared template
repo on `ude sync push`: Shannon-entropy and Base64 credential detection (SEC-01), gitleaks-category
coverage in the outbound screen and publication gate (SEC-02), strict word/punctuation boundaries for
path parameterization (SEC-03), and verified reversibility between placeholders and local paths
(SEC-04). Scope is the four SEC requirements and the three roadmap plans (02-01..02-03); nothing else.

</domain>

<decisions>
## Implementation Decisions

### Entropy hits & overrides (SEC-01) — discussed 2026-10-23
- **D-01:** A string that trips only the entropy check (no named rule matched) **blocks the push**,
  the same way a named-rule hit does, but can be overridden narrowly (D-02). It fails closed because
  the gitleaks baseline UDE ships allowlists `templates/.*` by path
  (`templates/universal/generic/security/gitleaks.toml`), so UDE's own sanitizer is the only secret
  scan on what push publishes.
- **D-02:** Known non-secret high-entropy values pass through two mechanisms:
  1. **Built-in shape exemptions**: ssh public key lines (`ssh-ed25519 AAAA…`, `ssh-rsa`, `ecdsa-sha2-*`,
     `sk-*@openssh.com`), hex digests (40/64/128 hex chars), UUIDs, SRI hashes (`sha256-`/`sha384-`/`sha512-`).
     *Amended (operator, 2026-10-23, during plan-phase):* the ssh exemption covers only the base64 key
     body that follows a key type, not the whole line; the rest of the line is still scanned.
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

### Entropy rule (SEC-01) — calibrated and chosen 2026-10-23
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

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Requirements & scope
- `.planning/ROADMAP.md` §Phase 2: goal, 4 success criteria, 3 plan stubs
- `.planning/REQUIREMENTS.md` (SEC-01..SEC-04)
- `docs/REMEDIATION_PLAN.md` §Track 2 (2.1 entropy and alphabet, 2.2 punctuation boundaries; 2.3 TypeSafe is deferred, see below)

### Existing scanner configuration
- `templates/universal/generic/security/gitleaks.toml`: shipped gitleaks baseline. It allowlists `templates/.*` by path, which is the reason for D-01.

### Prior phase decisions
- `.planning/phases/01-sync-engine-state-hermeticity/01-CONTEXT.md`: exit-code convention, publication rollback, PARAMETERIZED_PLACEHOLDERS / CR-02 push guard

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `Sanitizer` (`src/sanitizer/mod.rs`): `check_secrets` returns the `SanitizationResult::SecretDetected { reason, pattern }` enum; 6 named regex rules; `parameterize_paths` (boundary class `[/\\"'\s:]` only).
- `PARAMETERIZED_PLACEHOLDERS` / `template_has_logic` (`src/sync/mod.rs` ~L2262): the push guard that exempts placeholder-only templates.

### Established Patterns
- Test fixtures build secrets at runtime and never commit them as literals; tests run in `TempDir` with Git env scrubbed (PROJECT.md constraints).
- Every scan ships with a known-positive sample and a negative control.

### Integration Points (all `check_secrets` callers must get the new behaviour)
- Push staging: `src/sync/mod.rs` ~L1449 (scan, then `parameterize_paths` at ~L1462)
- Publication gate over staged index blobs: `src/sync/mod.rs` ~L1737 (direct push) and ~L2034 (PR mode). Both use `from_utf8_lossy`.
- Pre-API scan before TypeSafe: `src/main.rs` ~L1074 (scans the local text only)

</code_context>

<specifics>
## Specific Ideas

- The marker spelling is exactly `ude:allow-entropy`.
- Entropy scoring includes `+`, `/` and `=` in the alphabet: tokens that start with `+` or contain `/` must be blocked (remediation plan 2.1 verification).

</specifics>

<deferred>
## Deferred Ideas

- **TypeSafe System One hardening** (remediation plan 2.3) **moved to Phase 3 as SEC-05**
  (operator, 2026-10-23): plan stub 03-04 and success criterion 5. It is out of scope here. Today the
  code ignores `confidence` entirely (`src/semantic/mod.rs` `evaluate_drift`). Moving the API key to
  stdin is already done. Phase 2's new scanner still applies to the existing pre-API scan of the
  local text (`src/main.rs` ~L1074), because that call site goes through `check_secrets`.

### Reviewed Todos (not folded)
- *Home-path placeholders leak the pusher's home path into shared templates* (2026-10-19): this overlaps SEC-03 from the push side. The operator chose not to fold it in, so it stays a standalone todo. Fixing SEC-03 may make it moot; re-check the todo after this phase.
- *Make generated projects' secret-scan hook self-sufficient* (2026-10-20): this is template/hook work, a better fit for Phase 4. Not folded in.
- The other 9 todo matches were keyword noise (all scored 0.6) and are unrelated to sanitizer scope.

</deferred>

---

*Phase: 02-sanitizer-boundaries-security-hardening*
*Context gathered: 2026-10-23*
