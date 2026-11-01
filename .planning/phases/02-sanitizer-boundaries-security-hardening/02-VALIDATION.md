---
phase: "02"
slug: "sanitizer-boundaries-security-hardening"
# status lifecycle: draft (seeded by plan-phase) → validated (set by validate-phase §6)
# audit-milestone §5.5 distinguishes NOT-VALIDATED (draft) from PARTIAL (validated + nyquist_compliant: false) (#2117)
status: validated
nyquist_compliant: false
wave_0_complete: true
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
| SEC-03 | Trailing separators on the home and XDG roots do not change parameterization | unit + integration | `sanitizer::tests::test_normalize_root`, `sync::tests::test_push_parameterizes_home_with_trailing_separator` (02-09) | a root of only separators stays rejected: `sync::tests::test_push_does_not_parameterize_root_home`, `sanitizer::tests::test_degenerate_home_not_parameterized` |
| SEC-04 | Push never overwrites or deletes a template with logic, whatever its extension | unit + integration | `sync::tests::test_template_has_logic`, `test_push_skips_static_template_with_logic`, `test_push_never_deletes_static_template_with_logic`, `test_push_never_deletes_non_utf8_jinja_template_with_logic` (02-10) | placeholder-only template still publishes (`test_push_publishes_over_placeholder_only_static_template`); `.jinja` without logic can be deleted (`test_push_deletes_non_utf8_jinja_template_without_logic`) |
| SEC-04 | The published commit has exactly one parent, the fetched head, in both push modes | integration | `test_direct_push_refuses_hook_merge_over_amended_commit`, `test_pr_push_refuses_hook_merge_over_amended_commit`, `test_pr_push_refuses_hook_added_commit`, `test_direct_push_refuses_hook_added_commit`, `test_pr_push_refuses_hook_parentless_commit`, `test_committed_gate_compares_full_object_ids`, `test_raw_commit_parents` (02-11) | benign hook publishes in both modes (`test_direct_push_publishes_with_benign_post_commit_hook`, `test_pr_push_publishes_with_benign_post_commit_hook`); RED evidence in 02-11-SUMMARY |
| SEC-04 | The gate reads objects raw, so replace refs cannot hide what the push sends | integration | `test_push_refuses_hook_replaced_blob`, `test_push_refuses_hook_merge_replaced_by_validated_commit`, `test_push_refuses_hook_replaced_tree`, `test_push_refuses_hook_replaced_message`, `test_push_refuses_hook_replaced_tree_hiding_empty_directory` (02-11, 02-12) | per-read control: removing one read's option fails exactly its paired tests (02-11-SUMMARY, 02-12-SUMMARY) |
| SEC-04 | The push sends the validated commit id and no tags | integration | `test_pr_push_publishes_the_validated_commit_not_the_branch_tip`, `test_direct_push_publishes_the_validated_commit_after_head_moves`, `test_direct_push_does_not_publish_hook_tag`, `test_pr_push_does_not_publish_hook_tag` (02-11) | a push without `--no-follow-tags` delivers the tag to a second bare repository, in the same tests |
| SEC-04 | Commit text a hook writes is refused: message bound byte for byte, header scanned, no empty directory | unit + integration | `test_committed_gate_requires_exact_sync_message`, `test_direct_push_refuses_prepare_commit_msg_hook_text`, `test_pr_push_refuses_prepare_commit_msg_hook_text`, `test_direct_push_refuses_hook_amended_message`, `test_pr_push_refuses_hook_amended_message`, `test_push_refuses_hook_secret_in_commit_header`, `test_push_refuses_hook_empty_directory`, `test_raw_commit_parts`, `test_count_empty_directories` (02-12) | a prepare-commit-msg hook that leaves the message alone publishes (`test_direct_push_publishes_with_benign_prepare_commit_msg_hook`, `test_pr_push_publishes_with_benign_prepare_commit_msg_hook`) |
| SEC-04 | A grafts file present when the gate runs refuses the push | unit + integration | `test_direct_push_refuses_hook_grafts_file`, `test_pr_push_refuses_hook_grafts_file`, `test_refuse_grafts_file_outcomes` (02-12) | the same injected-commit hook with no grafts file publishes and sends no injected blob (`test_direct_push_publishes_with_hook_commit_and_no_grafts_file`) |
| SEC-04 | Commit-graphs, bitmaps and `GIT_TEST_*` cannot make the push send extra objects | unit + integration | `test_direct_push_sends_no_object_from_forged_commit_graph`, `test_pr_push_sends_no_object_from_forged_commit_graph`, `test_push_sends_no_object_from_forged_split_commit_graph`, `test_push_sends_no_object_from_forged_alternate_commit_graph`, `test_direct_push_sends_no_object_from_graft_built_bitmap`, `test_pr_push_sends_no_object_from_graft_built_bitmap`, `test_push_sends_no_object_from_graft_built_multi_pack_index_bitmap`, `test_direct_push_ignores_inherited_git_test_commit_graph`, `test_pr_push_ignores_inherited_git_test_commit_graph`, `test_remove_git_test_env` (02-12) | a push without the options delivers the injected blob to a second bare repository; a cache git itself maintains still publishes (`test_push_from_clone_of_gc_remote_publishes`) |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

All rows ✅ green on 2026-11-01 at e0b4f12 source state: `cargo nextest run --workspace` ran 351 tests, all passed, 1 skipped (`test_dump_parity_corpus`, the ignored parity dump). Evidence is git 2.56.0 on one Linux host. Every test name above was matched against `cargo nextest list`, with an invented name as the control that must be reported absent.

---

## Wave 0 Requirements

- [x] Seeded fixture helper in `src/sanitizer` tests: uniform per-character draws over an explicit alphabet (`proptest::sample::select` under `TestRunner::deterministic()`, plan 02-03 `SeededChars`), pinned by a symbol-frequency test with a skewed-alphabet negative control. Not proptest regex-string strategies: they skew character frequencies and moved R11 block rates below every floor (RESEARCH "Fixture generator check")
- [x] Corpus walker helper (lists `templates/`, `template/` under `CARGO_MANIFEST_DIR`)
- [x] Vendored `src/sanitizer/rules/gitleaks.toml` + `LICENSE-gitleaks`, checked against SHA-256 `e163e53b9e7e8a8511e77271e2b323ed057759542a6d988258afe3a1fa329caf`
- [x] Convert the literal JWT fixture in `src/sanitizer/mod.rs` tests to a runtime-built fixture

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Port vs real gitleaks parity | SEC-02 | needs the external `gitleaks` binary; one-off phase-gate check | Automated in plan 02-06 Task 3: the ignored dump test writes runtime positives to target/gitleaks-parity, `gitleaks dir --config <vendored>` must report each file's own rule id and nothing for benign.txt; negative control in the same command: the slack token is removed and gitleaks re-runs into a fresh report, which must no longer report it; the scratch dir is deleted |

Not tested, by operator decision of 2026-10-28 (T-02-52): a grafts file a pre-push hook writes during `git push` is not refused and makes the push send the grafted objects. This is an accepted residual, recorded in 02-12-PLAN, not a coverage gap.

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies (31 tasks across 12 plans, 0 without `<automated>`)
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers all MISSING references
- [x] No watch-mode flags
- [x] Feedback latency within quick-filter scope
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** PARTIAL. SEC-04 has channels no test covers (duplicate tree entries, deletion or revert of a validated file, a tree entry named `.`, grafts written after the gate; 02-VERIFICATION.md gaps), found after the audit below. Earlier audit, 2026-11-01: no gaps found against the test map. This audit establishes that each requirement has automated tests that run green. It does not judge whether the phase goal is met; the phase verifier does that.

## Validation Audit 2026-11-01

| Metric | Count |
|---|---|
| Gaps found | 0 |
| Resolved | 0 |
| Escalated | 0 |

## Validation Audit 2026-11-01

| Metric | Count |
|---|---|
| Gaps found | 4 |
| Resolved | 0 |
| Escalated | 0 |
