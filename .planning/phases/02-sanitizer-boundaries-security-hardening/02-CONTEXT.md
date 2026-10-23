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

### Claude's Discretion
- **Entropy threshold calibration.** The remediation plan's fixed numbers (≥20 chars, entropy > 4.2)
  are *not* locked. They were measured on 2026-10-23 (Python, 2000 random samples per length):
  - random Base64 cut to 20 chars exceeds 4.2 only **18.6%** of the time (a 20-char string tops out at
    log2(20) ≈ 4.32); at 32 chars it is 99.4%, at 40 chars 100%;
  - the CamelCase English token `ThisIsAPerfectlyNormalSentenceWithWords` scores **4.26**, which would
    break the remediation plan's "natural English passes" negative control;
  - UUID ≈ 3.86, 40-char hex ≈ 3.70, and an ed25519 public key body ≈ 5.05 (so it needs D-02's shape
    exemption).
  The planner and researcher choose a threshold that is length-aware or charset-aware (or both).
  Whatever they choose must pass a fixture table with both directions: random Base64/URL-safe
  tokens at 20, 32 and 40+ chars must block, and CamelCase/snake_case English identifiers, UUIDs,
  hex digests, ssh pubkeys and the shipped `templates/` tree must pass. Running the scan over every
  file in `templates/` and `template/` is a required negative control.
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

- **TypeSafe System One hardening** (remediation plan 2.3: fail closed when confidence < 0.85 or the
  top-two margin < 0.20; pre-scan the upstream text as well as the local text). It sits in PROJECT.md's
  Track 2 but is not among the Phase 2 requirements SEC-01..04, so it stays out of this phase
  following the roadmap. Today the code ignores `confidence` entirely (`src/semantic/mod.rs`
  `evaluate_drift`). It needs a requirement and a phase, or an explicit fold-in by the operator.
  Moving the API key to stdin is already done.

### Reviewed Todos (not folded)
- *Home-path placeholders leak the pusher's home path into shared templates* (2026-10-19): this overlaps SEC-03 from the push side. The operator chose not to fold it in, so it stays a standalone todo. Fixing SEC-03 may make it moot; re-check the todo after this phase.
- *Make generated projects' secret-scan hook self-sufficient* (2026-10-20): this is template/hook work, a better fit for Phase 4. Not folded in.
- The other 9 todo matches were keyword noise (all scored 0.6) and are unrelated to sanitizer scope.

</deferred>

---

*Phase: 02-sanitizer-boundaries-security-hardening*
*Context gathered: 2026-10-23*
