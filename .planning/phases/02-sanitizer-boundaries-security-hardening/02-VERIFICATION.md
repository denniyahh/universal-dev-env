---
phase: 02-sanitizer-boundaries-security-hardening
verified: 2026-10-27T09:40:00Z
status: gaps_found
score: 23/26 must-haves verified
covered_files:
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-01-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-01-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-02-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-02-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-03-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-03-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-04-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-04-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-05-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-05-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-06-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-06-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-07-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-07-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-08-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-08-SUMMARY.md"
  - "docs/ARCHITECTURE.md"
  - "src/main.rs"
  - "src/sanitizer/entropy.rs"
  - "src/sanitizer/gitleaks.rs"
  - "src/sanitizer/mod.rs"
  - "src/sanitizer/rules/LICENSE-gitleaks"
  - "src/sanitizer/rules/gitleaks.toml"
  - "src/sync/mod.rs"
  - "src/template/mod.rs"
  - "tests/e2e_lifecycle.rs"
covered_digest: "v4:sha256:e8b1ba8ccb414955d478fbb6a73b7e96a7ea60eac3e76eaadbdc898106c9eb91"
behavior_unverified: 0
overrides_applied: 0
gaps:
  - truth: "02-07 prohibition (SEC-03): MUST NOT publish a boundary-delimited occurrence of the pusher's home or XDG config path in a shared template"
    status: failed
    reason: "When HOME ends in `/`, home-path parameterization rewrites nothing and `ude sync push` publishes the raw home path, exits 0 and prints 'parameters applied' (review CR-01, reproduced end to end through the CLI)."
    artifacts:
      - path: "src/main.rs"
        issue: "lines 724-727: user_home is BaseDirs::home_dir() as given, trailing `/` kept"
      - path: "src/template/mod.rs"
        issue: "lines 43-50: TemplateContext::detect_for_project takes the same untrimmed home, so render and parameterize must be normalized together"
      - path: "src/sanitizer/mod.rs"
        issue: "parameterize_paths (line 256) / is_parameterizable_root (line 135) accept `/home/alice/` as a root; replace_bounded (line 149) then never matches `/home/alice/x` (next char alphanumeric) or `(/home/alice)` (no trailing slash)"
    missing:
      - "Normalize trailing separators off user_home and xdg_config (keep a lone `/` so the degenerate-root guard still rejects it), in parameterize_paths' caller and in TemplateContext::detect_for_project"
      - "Boundary-table row and push-level test with HOME=`/home/alice/`, plus the negative control that `/` is still rejected"
  - truth: "ROADMAP SC4: Sanitizer round-tripping verifies that templated tokens substitute back cleanly to local machine environments on initialization"
    status: failed
    reason: "A non-path placeholder in a non-.jinja managed template (e.g. `email = {{ git_email }}` in gitconfig.repo) is rendered by init (new in 02-02), then overwritten by push with the pusher's value; the next machine's init gets the pusher's email instead of its own. Push gate 2 (template_has_logic) still only protects `.jinja` files, and gate 5 passes because the published text has no `{{` (review CR-02, reproduced end to end). Pre-existing (Phase 01 re-review W3, pending todo 2026-10-19 'template-logic push guard only checks .jinja files'); 02-02 widened it from pull-only to init. Latent: no shipped sync-managed non-.jinja template carries a non-path placeholder today."
    artifacts:
      - path: "src/sync/mod.rs"
        issue: "template_has_logic (line 2445) returns Ok(false) for any non-.jinja path, while template::should_render renders any text containing `{{`"
    missing:
      - "Gate 2 decides 'has logic' with template::should_render (the shared render rule), not the .jinja extension"
      - "Regression test: non-.jinja template with `{{ git_email }}`, local edit, push -> file lands in skipped_templates and the email is never staged; control: placeholder-only template still publishes"
behavior_unverified_items: []
---

# Phase 2: Sanitizer, Boundaries & Security Hardening Verification Report

**Phase Goal:** Screen and redact all confidential tokens, raw credentials, and user-specific paths before any remote git staging or push.
**Verified:** 2026-10-27T09:40:00Z
**Status:** gaps_found
**Re-verification:** No (initial verification)

## Bottom line

The secret-screening half of the goal is achieved and holds up under independent probing: entropy-only
tokens, gitleaks-only tokens (including three rules outside the phase's own fixture table, checked
against the real gitleaks 8.30.1 binary) and `--force` are all refused at the CLI without echoing the
value. The path-redaction and round-trip half has two reproduced holes:

1. **HOME with a trailing `/` disables home-path redaction entirely** (review CR-01). Push publishes
   the raw home path and reports success. Pre-existing in the old regex too, but it violates this
   phase's own SEC-03 prohibition. Trigger is an uncommon (valid) environment.
2. **A non-path placeholder in a non-`.jinja` managed template is erased by push and the pusher's
   value is published to every consumer** (review CR-02). Pre-existing and already tracked as a
   major todo; this phase widened it by making init render such templates. Latent today: no shipped
   managed template triggers it.

## Goal Achievement

### Roadmap Success Criteria

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| SC1 | High-entropy strings and raw Base64 tokens without prefixes are flagged before staging | VERIFIED | CLI probe: runtime random 32-char Base64 line in `.gitconfig` -> `ude sync push` exit 1, `high-entropy at line 29`, template unchanged, value absent from output. Negative control: 38-char lowercase run pushes, exit 0. Seeded rate tests `test_r11_block_rates_seeded`, `test_r11_env_assignment_rates_seeded`, corpus control `test_r11_templates_corpus_zero_hits` pass. |
| SC2 | Outbound commit inspection catches gitleaks categories across committed blobs and rolls back | VERIFIED | Vendored file SHA-256 matches the plan pin (e163e53b...), 222 `[[rules]]`. CLI: Slack bot token refused (`slack-bot-token at line 29`), also with `--force`; benign twin pushes. Three rules outside the 15-row fixture table (digitalocean-pat, shopify-access-token, huggingface-access-token; hex / single-case, so R11 cannot catch them) each refused at the CLI with their own rule id, and the gitleaks 8.30.1 binary reports the same ids on the same files and nothing on a benign file. Commit gate: `test_post_commit_gate_blocks_gitleaks_only_category`, `test_post_commit_hook_rewrite_is_refused` (real post-commit hook through `push_to_remote`, remote main unchanged, control push succeeds). |
| SC3 | Path parameterization matches only full word / punctuation boundaries | VERIFIED | CLI probe (synthetic home `<scratch>/home/alice`): replaced in `${X:-H}`, `a,H,b`, `see H.`, `file://H/x`, `"H"`, `H:H`, `(H)`, `H/x`; untouched in `/backup-H/x`, `xH`, `H..bak`, `H./x`, `H_x`, `H-old`, `.H`, `H2`, `H.bak`; file then reads In Sync. `test_path_boundary_table` (29 rows) and both properties pass. Over-matching (the SC's wording) is what this proves; under-matching is the CR-01 gap below. |
| SC4 | Templated tokens substitute back cleanly to local environments on initialization | FAILED | Path placeholders round-trip (`test_push_then_init_substitutes_home_on_new_project` with a different-home control; my control probe kept `{{ user.home }}/inc.local` through push). But a `{{ git_email }}` placeholder added to `gitconfig.repo` was rendered by init, replaced by `pusher@example.com` on push (exit 0), and a consumer init with `consumer@example.com` got `pusher@example.com`. See gap 2. |

### Plan must-have groups

| Plan | Truths (summary) | Status | Evidence |
|------|------------------|--------|----------|
| 02-01 | single `scan_bytes` entry; raw + NUL-stripped views; non-UTF-8/UTF-16 scanned at staging and both gates; rule+line refusals | VERIFIED | `scan_bytes(&published` (1 call, src/sync/mod.rs:1569), `scan_bytes(&blob_res.stdout` (2 calls, 1869, 2193); `check_secrets` delegates (src/sanitizer/mod.rs). Tests `test_push_blocks_secret_in_non_utf8_file`, `..._utf16_file`, `test_publication_gate_scans_utf16_blob`, `test_post_commit_gate_scans_utf16_blob`, `test_binary_non_utf8_files_scanned_and_tracked_by_hash` pass. |
| 02-02 | shared `should_render`; init renders `{{` templates, byte-copies the rest; kept ancestor rendered | VERIFIED | `materialize_template` (src/main.rs:1675, 6 call sites), `should_render` used in main/scan/pull. e2e `test_push_then_init_substitutes_home_on_new_project` (with other-home control) and kept-ancestor tests pass. |
| 02-03 | R11 tokenizer/entropy/randomness, D-02 exemptions, seeded rates, corpus control | VERIFIED | `entropy::first_high_entropy_line` wired in `scan_view`; 15 `sanitizer::entropy::tests` pass; CLI probe above. |
| 02-04 | `ude:allow-entropy` per-line, entropy-only | VERIFIED | `test_marker_silences_entropy_only_on_its_line`, `test_marker_near_misses_do_not_silence`, `test_marker_does_not_silence_named_rules`, `test_marker_does_not_silence_gitleaks_rules` pass. Mutation-gate claim not re-run (see limits). |
| 02-05 | vendored pinned rules, 0 compile errors, gitleaks semantics, load failure is an error | VERIFIED | Hashes match pins; `test_vendored_rules_load_all`, `test_invalid_rule_regex_is_error`, `test_unknown_key_is_error` pass; `gitleaks::vendored().first_finding` wired between named rules and R11. |
| 02-06 | per-category CLI refusal, `--force`, category table, D-03, corpus control, parity | VERIFIED | e2e `test_push_refuses_gitleaks_categories_without_printing_value`, `test_category_positives` pass; my independent CLI + binary parity on 3 extra rules. |
| 02-07 | boundary predicates, degenerate-home guard, properties, doc placeholders | VERIFIED (as stated) | Rules behave as the truths state (CLI probe, table, properties). `docs/ARCHITECTURE.md:79-80` list exactly `{{ user.home }}`, `{{ user.xdg_config }}`; no `{{ xdg.config }}` in src/docs/templates. The truths do not cover a trailing-slash home; that is gap 1 under the prohibition. |
| 02-08 | render-back gate 5, no engine detail, should_render for scan/pull, validated-blob binding | VERIFIED | CLI: `{{ 1 \| zqx_sentinel_label }}` refused, exit 1, `unknown filter at line 29`, identifier absent from output; control without `{{` pushes. `test_push_refuses_irreversible_shapes`, `test_push_secret_scan_precedes_reversibility`, `test_publication_gate_refuses_blob_changed_by_clean_filter`, `test_post_commit_hook_rewrite_is_refused` pass. |

### Prohibitions (all test-tier)

| Plan | Prohibition | Status | Evidence |
|------|-------------|--------|----------|
| 02-01 | No lossy decode / skipped non-UTF-8 / UTF-16 assumption before the scan | VERIFIED | raw `git show` bytes scanned; UTF-16 and non-UTF-8 tests pass |
| 02-01 | No matched value printed | VERIFIED | every CLI probe: value count in stdout+stderr = 0 |
| 02-02 | Init never changes bytes of a no-`{{` or non-UTF-8 template | VERIFIED | e2e byte-identical controls pass |
| 02-03 | Refusal names rule, file, line | VERIFIED | CLI: `Push rejected on .gitconfig: ... high-entropy at line 29` |
| 02-04 | Marker never silences named or gitleaks rules | VERIFIED | two marker tests pass |
| 02-05 | Unloadable rule set is never treated as clean | VERIFIED | load-error tests pass; LazyLock `.expect` |
| 02-06 | No inline suppression except `ude:allow-entropy` | VERIFIED | `test_inline_gitleaks_allow_not_honoured` passes |
| 02-06 | `--force` never skips the scan | VERIFIED | CLI: `--force` with Slack token exit 1, template unchanged |
| 02-07 | Never publish a boundary-delimited occurrence of the pusher's home/XDG path | **FAILED** | HOME=`<home>/`: template gained `(<home>)`, `<home>/x`, `path = <home>/notes.local` raw; exit 0; 0 placeholders. Control HOME=`<home>`: same lines become `{{ user.home }}` |
| 02-07 | Never rewrite a path that is not the pusher's own | VERIFIED | CLI rows u1-u7, `H2`, `H.bak` byte-identical |
| 02-08 | Never publish a template that does not render back to the pusher's local file | VERIFIED (letter) | gate 5 tests + CLI sentinel probe. Its stated rationale ("silently changes init for every consumer") is what gap 2 shows happening by another route |
| 02-08 | Never publish a blob differing from the validated bytes | UNCERTAIN (WARNING) | Single-commit HEAD: verified (clean-filter, working-tree rewrite and amending post-commit hook tests). Merge HEAD / extra commit on top: see WR-02 below; reproduced at the git level only |
| 02-08 | Never print engine detail or file content in a reversibility refusal | VERIFIED | CLI sentinel probe; `test_check_render_back_reason_has_no_template_text` |

**Score:** 23/26 verified (4 SC + 8 plan groups + 14 prohibitions); 2 FAILED, 1 UNCERTAIN; 0 present-but-behavior-unverified.

### Required Artifacts

| Artifact | Status | Details |
|----------|--------|---------|
| `src/sanitizer/mod.rs` | VERIFIED | `pub fn scan_bytes`, `fn replace_bounded`, `entropy::first_high_entropy_line`, `gitleaks::vendored().first_finding` |
| `src/sanitizer/entropy.rs` | VERIFIED | `pub fn first_high_entropy_line`, `ALLOW_MARKER` |
| `src/sanitizer/gitleaks.rs` | VERIFIED | `pub fn first_finding`, `include_str!("rules/gitleaks.toml")` |
| `src/sanitizer/rules/gitleaks.toml`, `LICENSE-gitleaks` | VERIFIED | SHA-256 equal to the plan pins |
| `src/sync/mod.rs` | VERIFIED | `ReversibilityBlocked`, `check_render_back`, `ValidatedBlobs` |
| `src/template/mod.rs` | VERIFIED | `pub fn should_render`, `pub fn render_str_checked` |
| `src/main.rs` | VERIFIED | `fn materialize_template`, `PushResult::ReversibilityBlocked` arm, `validated_blobs` passed to `push_to_remote` |
| `tests/e2e_lifecycle.rs` | VERIFIED | named e2e tests present and passing |
| `docs/ARCHITECTURE.md` | VERIFIED | placeholder list correct |

`verify.artifacts`: 20/20 passed across the 8 plans (2+3+2+1+3+2+3+4).

### Key Link Verification

`verify.key-links` could not evaluate any link: every plan's `from:` field is a description, not a
file path (tool error "from: must be a relative file path"). Each link pattern was checked by hand
instead; all present (counts): `scan_bytes(&published` 1, `scan_bytes(&blob_res.stdout` 2,
`materialize_template(` 6, `should_render(` in main 3, `entropy::first_high_entropy_line` 1,
`ALLOW_MARKER` 5, `gitleaks::vendored().first_finding` 1, `include_str!("rules/gitleaks.toml")` 1,
`parameterize_paths(` in sync 1, `check_render_back(` 3, `ReversibilityBlocked` in main 1,
`ValidatedBlobs` in sync 34. Negative control: a nonexistent pattern counts 0. Grep presence does
not prove call order; order is covered by `test_push_secret_scan_precedes_reversibility` and by
reading `scan_view` / `push_updates`.

### Behavioral Spot-Checks (built `target/debug/ude`, current with HEAD; synthetic homes in a scratch dir)

| Behavior | Result | Control | Status |
|----------|--------|---------|--------|
| Random Base64 token in `.gitconfig` blocks push | exit 1, `high-entropy at line 29`, no echo | lowercase run: exit 0 | PASS |
| Slack bot token blocks push, also with `--force` | exit 1 both, `slack-bot-token`, no echo | benign line: exit 0 | PASS |
| 3 gitleaks rules outside the fixture table | each exit 1 with its rule id; gitleaks binary agrees | binary reports nothing on benign file | PASS |
| Boundary rows at the CLI | as listed under SC3; file In Sync after push | untouched rows | PASS |
| Unknown-filter sentinel refused, not echoed | exit 1, `unknown filter at line 29` | no `{{`: exit 0 | PASS |
| HOME with trailing `/` | raw home published, exit 0 | HOME without `/`: placeholders | FAIL (gap 1) |
| `{{ git_email }}` in non-.jinja template survives push | overwritten with pusher's email; consumer init gets it | `{{ user.home }}` placeholder survives | FAIL (gap 2) |
| `git diff-tree -r HEAD` on merge / empty commit | 0 lines each | parent commit: 1 line | confirms WR-02 primitive |

Full suite run once: `cargo test` exit 0, 210 unit passed (1 ignored: the parity-corpus dump), 84 e2e passed.

### Requirements Coverage

| Requirement | Source Plans | Status | Evidence |
|-------------|--------------|--------|----------|
| SEC-01 entropy / raw Base64 | 02-03, 02-04, 02-06 | SATISFIED | SC1 |
| SEC-02 gitleaks categories before staging | 02-01, 02-05, 02-06 | SATISFIED | SC2 |
| SEC-03 strict boundaries | 02-07 | PARTIAL | boundaries correct; trailing-slash HOME disables redaction (gap 1) |
| SEC-04 placeholder reversibility | 02-02, 02-07, 02-08 | PARTIAL | sanitizer placeholders round-trip; non-path placeholders are erased by push (gap 2) |

All four IDs are claimed by plans; none orphaned. REQUIREMENTS.md already marks SEC-03 and SEC-04
Complete; that contradicts gaps 1 and 2.

### Anti-Patterns Found

No `TBD`/`FIXME`/`XXX`/`TODO`/`HACK`/`todo!`/`unimplemented!` in any file changed since `2070f2d`
(negative control: a planted `FIXME` is counted).

### Review findings, judged against the goal

| Review item | Reproduced here | Verdict |
|-------------|-----------------|---------|
| CR-01 trailing-slash HOME | Yes, end to end through the CLI | Gap 1 (blocker) |
| CR-02 gate 2 `.jinja`-only | Yes, end to end incl. consumer init | Gap 2 (blocker on SC4; pre-existing, widened here) |
| WR-01 raw home after parameterization (`-I<home>`, non-UTF-8 files, escaped JSON) | Not re-run | Not a gap: `-I`, alias spellings, colon-less defaults and non-UTF-8/UTF-16 files are accepted risks T-02-21 / T-02-25, from the operator's choice not to fold the home-path todo (02-CONTEXT "Reviewed Todos (not folded)"). Escaped-slash JSON is the same class but not named in T-02-21 |
| WR-02 merge HEAD / extra commit passes the commit gate | git primitive only | WARNING. Direct mode: an amend plus an empty commit on top fails `check_continuity` (HEAD~1 is not the fetched head). A merge HEAD whose first parent is the base, and PR mode (no continuity check), are not covered. Needs a hook in the user's own git config to trigger |
| IN-01 silent skip of unparameterizable home | by code reading | Info: no diagnostic when the home is `.`, relative or `/` |
| IN-02 two init render paths | not re-run | Info |
| IN-03 rule pin only under `cargo test`; no `.gitattributes` | no `.gitattributes` in repo root | Info |
| IN-04 ARCHITECTURE.md overstates ("rejecting any files containing high-entropy strings") | line 76 confirmed | Info |
| IN-05 TypeSafe egress of unparameterized AGENTS.md | not re-run | Out of scope (goal is git staging/push); Phase 3 SC5 covers TypeSafe hardening |

### What these checks do not establish

- One random token per probe: a single PASS shows that token was caught, not the rate (the seeded tests carry the rates; 3-7% of mixed-case and 18-23% of base36 keys pass R11 by design).
- Gitleaks coverage: 15 + 3 rules have positive evidence; the other 204 are loaded and compiled but unexercised (accepted T-02-29).
- Mutation-gate results in the SUMMARYs (entropy.rs, gitleaks.rs, boundary functions) were not re-run here.
- Gap 2 being phase-widened rests on code reading (`git show 2070f2d:src/main.rs` copies `gitconfig.repo` with `copy_template_file`); the old binary was not built and run.
- WR-02 was reproduced only at the `git diff-tree` level, not through `push_to_remote`.
- Windows path behaviour is unverified (Phase 999.1).

### Gaps Summary

Two blockers, independent root causes:

1. **Home normalization (SEC-03, goal "redact user-specific paths").** Strip trailing separators from
   the home and XDG roots before use, in both `push_updates`' caller and
   `TemplateContext::detect_for_project`. Small, one function plus tests.
2. **Gate 2 render-rule mismatch (SC4 / SEC-04).** `template_has_logic` must use
   `template::should_render` instead of the `.jinja` extension. Already specified in the pending todo
   "Template-logic push guard only checks .jinja files". If the operator reads SC4 as covering only the
   sanitizer's own placeholders, this can be accepted instead with an override:

```yaml
overrides:
  - must_have: "Sanitizer round-tripping verifies that templated tokens substitute back cleanly to local machine environments on initialization"
    reason: "Non-path placeholders in non-.jinja templates are pre-existing (Phase 01 W3) and tracked as a pending todo; SC4 covers the sanitizer's own placeholders"
    accepted_by: "<operator>"
    accepted_at: "<ISO timestamp>"
```

No gap is deferred: no later phase (3 or 4) names either item in its goal or success criteria.

---

_Verified: 2026-10-27T09:40:00Z_
_Verifier: Claude (gsd-verifier)_
