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
- **Before `/gsd-verify-work`:** Full suite must be green; `cargo mutants -v --file src/sanitizer/entropy.rs` and `--file src/sanitizer/mod.rs` must leave no surviving mutants in the R11 rule or the boundary predicate
- **Max feedback latency:** quick filter only (single module)

---

## Per-Task Verification Map

Filled in by the planner / executor from the PLAN.md task IDs. Requirement-to-test map (each row carries its negative control):

| Requirement | Behavior | Test Type | Proposed test | Negative control |
|-------------|----------|-----------|---------------|------------------|
| SEC-01 | Seeded Base64 block rates meet R11 thresholds | unit (sampled) | `entropy::tests::r11_block_rates_seeded` | hex-40 rate == 0 |
| SEC-01 | CONTEXT negative fixtures pass | unit | `entropy::tests::r11_negative_fixtures` | random 32-char Base64 on same line blocks |
| SEC-01 | Marker silences entropy only | unit | `tests::marker_silences_entropy_not_named_rules` | runtime-built `ghp_` token with marker still blocks |
| SEC-01 | `templates/` + `template/` corpus → 0 hits | unit | `tests::corpus_templates_zero_hits` | files ≥ 33 and tokens ≥ 200 scanned; injected token → ≥ 1 hit |
| SEC-02 | Vendored rule file loads 222 rules, SHA-256 pinned | unit | `gitleaks::tests::rules_load_all` | invalid-regex TOML returns `Err` |
| SEC-02 | Go brace literal compat | unit | `gitleaks::tests::go_brace_literal` | fails to compile without the transform |
| SEC-02 | One positive per category → expected rule id | unit (table) | `gitleaks::tests::category_positives` | benign twin per row → clean |
| SEC-02 | Gitleaks-only category blocked at staged-blob gate | integration | `sync::tests::gate_blocks_gitleaks_only_category` | RED before change; benign filter output → Ok |
| SEC-02 | Committed-tree gate rolls back gitleaks-only category | integration | `sync::tests::committed_gate_rolls_back_gitleaks_only_category` | clean commit passes; HEAD lacks token after rollback |
| SEC-02 | Non-UTF-8 file with ASCII token blocked | unit + integration | `tests::scan_bytes_non_utf8`, `sync::tests::push_blocks_secret_in_non_utf8_file` | invalid bytes alone → clean |
| SEC-03 | Boundary table replace/untouched rows | unit | `tests::path_boundary_table` | untouched rows; old regex fails on `(/home/alice)` |
| SEC-03 | No partial-substring corruption | proptest | `tests::prop_no_partial_substring_corruption` | delimiter-suffix companion asserts placeholder appears |
| SEC-03 | Degenerate home → no change | unit | `tests::degenerate_home_not_parameterized` | `/home/alice` still parameterizes |
| SEC-04 | parameterize→render round-trip | proptest | `tests::prop_parameterize_render_roundtrip` | cross-machine render leaves no old home |
| SEC-04 | Push refuses unrenderable parameterized file | integration | `sync::tests::push_refuses_unrenderable_parameterized_file` | same hook without home path pushes |
| SEC-04 | `ude init` renders `{{ user.home }}` in non-jinja templates | integration | `init_renders_placeholders_in_static_templates` | `.repo` without `{{` byte-identical after init |

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
| Port vs real gitleaks parity | SEC-02 | needs the external `gitleaks` binary; one-off phase-gate check | Generate positives at runtime in a scratch dir, run the port and `gitleaks dir --config <vendored>`, diff rule ids (expect generic-dedupe difference only), delete scratch dir |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency within quick-filter scope
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
