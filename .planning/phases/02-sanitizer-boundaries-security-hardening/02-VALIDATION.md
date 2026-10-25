---
phase: "02"
slug: "sanitizer-boundaries-security-hardening"
# status lifecycle: draft (seeded by plan-phase) → validated (set by validate-phase §6)
# audit-milestone §5.5 distinguishes NOT-VALIDATED (draft) from PARTIAL (validated + nyquist_compliant: false) (#2117)
status: draft
nyquist_compliant: false
wave_0_complete: false
created: "2026-10-23"
---

# Phase 02 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.
> Source: `02-RESEARCH.md` § Validation Architecture.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in test harness via cargo-nextest 0.9.148; proptest 1.11.0 |
| **Config file** | `.config/nextest.toml` |
| **Quick run command** | `cargo nextest run --bin ude sanitizer::` |
| **Full suite command** | `cargo nextest run --workspace && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo deny check && cargo machete` |
| **Estimated runtime** | unmeasured for the full suite; the quick filter's baseline was 11 tests |

---

## Sampling Rate

- **After every task commit:** Run `cargo nextest run --bin ude sanitizer::` plus the touched sync test filter
- **After every plan wave:** Run the full suite command
- **Before `/gsd-verify-work`:** Full suite must be green; `cargo mutants -v --file src/sanitizer/entropy.rs` and `--file src/sanitizer/mod.rs` must leave no surviving mutants in the R11 rule or the boundary predicate (plans 02-04, 02-06 and 02-07 run their gates with distinct scratch paths: `target/mutants-entropy*`, `target/mutants-gitleaks*`, `target/mutants-boundaries*`)
- **Max feedback latency:** quick filter only (single module)

---

## Per-Task Verification Map

Filled in by the planner / executor from the PLAN.md task IDs. Requirement-to-test map (each row carries its negative control):

| Requirement | Behavior | Test Type | Proposed test | Negative control |
|-------------|----------|-----------|---------------|------------------|
| SEC-01 | Seeded Base64 block rates meet R11 thresholds | unit (sampled) | `entropy::tests::test_r11_block_rates_seeded` (02-03-T1) | hex-32 rate ≤ 0.01 and lowercase-32 rate == 0 (revised: hex-40 is an exempt shape, so it tested the exemption, not the harness) |
| SEC-01 | `.env`-style `NAME=value` lines meet the bare-value floors (OD-1, `=` is a delimiter) | unit (sampled) | `entropy::tests::test_r11_env_assignment_rates_seeded` (02-03-T1) | the same values scored as one glued token (pre-OD-1 tokenizer) block below 0.90 |
| SEC-01 | Tokenizer keeps 1-2 trailing padding `=`, splits on every other `=` (OD-1) | unit | `entropy::tests::test_r11_tokenizer_padding_and_delimiters` (02-03-T1) | `===` and interior `=` split the run |
| SEC-01 | CONTEXT negative fixtures pass (incl. JSON, unspaced, quoted and comma-led ssh key lines) | unit | `entropy::tests::test_r11_negative_fixtures` (02-03-T2) | random 32-char Base64 appended on the same line blocks |
| SEC-01 | Shape exemptions are whole-token and exact (SRI only at digest length; hex/UUID single-case) | unit | `entropy::tests::test_r11_shape_exemption_is_whole_token`, `entropy::tests::test_exempt_shape_predicate` (02-03-T2) | wrong-length SRI (`sha256-`+32, `sha256-`+43 unpadded, `sha384-`+63, `sha512-`+43+`=`) still blocks; mixed-case hex/UUID not exempt |
| SEC-01 | Marker silences entropy only | unit | `entropy::tests::test_marker_silences_entropy_only_on_its_line`, `test_marker_does_not_silence_named_rules`, `test_marker_near_misses_do_not_silence` (02-04-T1) | runtime-built `ghp_` token with marker still blocks |
| SEC-01 | `templates/` + `template/` corpus → 0 hits | unit | `entropy::tests::test_r11_templates_corpus_zero_hits` (02-03-T2) | files ≥ 33 and tokens ≥ 200 scanned (256 tokens under the OD-1 tokenizer at plan revision); injected token → ≥ 1 hit |
| SEC-02 | Vendored rule file loads 222 rules, SHA-256 pinned | unit | `gitleaks::tests::test_vendored_rules_load_all` (02-05-T1) | invalid-regex TOML returns `Err` |
| SEC-02 | Go brace literal compat | unit | `gitleaks::tests::test_go_brace_literal` (02-05-T1) | fails to compile without the transform |
| SEC-02 | Matcher entropy is −Σ p·log2 p over byte frequencies | unit | `gitleaks::tests::test_entropy_floor_is_strict` (02-05-T2) | `abcd` (exactly 2.0) not reported; fails under the distinct-count ratio |
| SEC-02 | One positive per category → expected rule id (12 of 15 rows entropy-gated) | unit (table) | `gitleaks::tests::test_category_positives` (02-06-T2) | benign twin per row → clean |
| SEC-02 | Vendored rules without a behavioural fixture are counted | unit | `gitleaks::tests::test_report_rules_without_positive_fixture` (02-06-T2) | a misspelled table id fails; one fewer row raises the count |
| SEC-02 | Gitleaks-only category blocked at staged-blob gate | integration | `sync::tests::test_publication_gate_blocks_gitleaks_only_category` (02-05-T1) | RED before change; index empty after the refusal |
| SEC-02 | Committed-tree gate rolls back gitleaks-only category | integration | `sync::tests::test_post_commit_gate_blocks_gitleaks_only_category` (02-05-T1) | clean commit passes; HEAD lacks token after rollback |
| SEC-02 | Non-UTF-8 file with ASCII token blocked | unit + integration | `sanitizer::tests::test_scan_bytes_non_utf8`, `sync::tests::test_push_blocks_secret_in_non_utf8_file` (02-01-T1) | invalid bytes alone → clean |
| SEC-02 | UTF-16LE/BE file with a token blocked at staging and both gates | unit + integration | `sanitizer::tests::test_scan_bytes_utf16`, `sync::tests::test_push_blocks_secret_in_utf16_file` (02-01-T1), `test_publication_gate_scans_utf16_blob`, `test_post_commit_gate_scans_utf16_blob` (02-01-T2) | benign UTF-16 text → clean; the raw-view named rule misses the UTF-16 token (the escape) |
| SEC-03 | Boundary table replace/untouched rows | unit | `sanitizer::tests::test_path_boundary_table` (02-07-T1) | untouched rows incl. `/backup-/home/alice/x` and `x-/home/alice`; old regex fails on `(/home/alice)` |
| SEC-03 | No partial-substring corruption (trailing and leading side) | proptest | `sanitizer::tests::prop_no_partial_substring_corruption` (02-07-T2) | delimiter-suffix and ` `/`(`/`=`/`"`/`:-`-prefix companions assert the placeholder appears |
| SEC-03 | Degenerate home → no change | unit | `sanitizer::tests::test_degenerate_home_not_parameterized` (02-07-T2) | `/home/alice` still parameterizes |
| SEC-04 | parameterize→render round-trip | proptest | `sanitizer::tests::prop_parameterize_render_roundtrip` (02-07-T2) | cross-machine render leaves no old home |
| SEC-04 | Push refuses unrenderable parameterized file | integration | `sync::tests::test_push_refuses_unrenderable_parameterized_file` (02-08-T1) | same hook without home path pushes |
| SEC-04 | Refusal names a fixed error category and line, never engine detail or file text | unit + e2e | `template::tests::test_render_str_checked_reports_category_and_line_only` (02-08-T1), `sync::tests::test_check_render_back_reason_has_no_template_text`, e2e `test_push_refusal_never_echoes_template_text` (02-08-T2) | render_str's error does contain the sentinel identifier (the leak the checked path avoids) |
| SEC-04 | Stored blobs bound to the fingerprint of the bytes push validated (a clean filter or commit hook cannot change what is published, even when it rewrites the working tree too) | integration | `sync::tests::test_publication_gate_refuses_blob_changed_by_clean_filter`, `sync::tests::test_publication_gate_refuses_rewritten_working_tree`, `sync::tests::test_post_commit_hook_rewrite_is_refused`, `sync::tests::test_push_updates_records_validated_digests` (02-08-T3) | pass-through (`cat`) filter → Ok; restored text → Ok; missing record → refused; CRLF-to-LF `text` normalization → Ok with the blob shown to differ (`test_publication_gate_allows_line_ending_normalization`) |
| SEC-04 | `ude init` renders `{{ user.home }}` in non-jinja templates | integration | `test_init_renders_placeholders_in_static_templates_in_sync` (02-02-T1) | `.repo` without `{{` byte-identical after init |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] Seeded fixture helper in `src/sanitizer` tests: uniform per-character draws over an explicit alphabet (`proptest::sample::select` under `TestRunner::deterministic()`, plan 02-03 `SeededChars`), pinned by a symbol-frequency test with a skewed-alphabet negative control. Not proptest regex-string strategies: they skew character frequencies and moved R11 block rates below every floor (RESEARCH "Fixture generator check")
- [ ] Corpus walker helper (lists `templates/`, `template/` under `CARGO_MANIFEST_DIR`)
- [ ] Vendored `src/sanitizer/rules/gitleaks.toml` + `LICENSE-gitleaks`, checked against SHA-256 `e163e53b9e7e8a8511e77271e2b323ed057759542a6d988258afe3a1fa329caf`
- [ ] Convert the literal JWT fixture in `src/sanitizer/mod.rs` tests to a runtime-built fixture

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Port vs real gitleaks parity | SEC-02 | needs the external `gitleaks` binary; one-off phase-gate check | Automated in plan 02-06 Task 3: the ignored dump test writes runtime positives to target/gitleaks-parity, `gitleaks dir --config <vendored>` must report each file's own rule id and nothing for benign.txt; negative control in the same command: the slack token is removed and gitleaks re-runs into a fresh report, which must no longer report it; the scratch dir is deleted |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency within quick-filter scope
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
