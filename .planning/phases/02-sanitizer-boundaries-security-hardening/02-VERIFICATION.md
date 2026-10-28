---
phase: 02-sanitizer-boundaries-security-hardening
verified: 2026-10-28T00:52:24Z
status: gaps_found
score: 32/33 must-haves verified
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
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-09-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-09-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-10-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-10-SUMMARY.md"
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
covered_digest: "v4:sha256:e40ef58b8e47a9e5267c4ce1f015832e8882ea96adc55404c00e46b66c19e6cb"
behavior_unverified: 0
overrides_applied: 0
re_verification:
  previous_status: gaps_found
  previous_score: 23/26
  gaps_closed:
    - "02-07 prohibition (SEC-03): MUST NOT publish a boundary-delimited occurrence of the pusher's home or XDG config path in a shared template (trailing-separator HOME, review CR-01)"
    - "ROADMAP SC4: Sanitizer round-tripping verifies that templated tokens substitute back cleanly to local machine environments on initialization (non-.jinja template with a non-path placeholder, review CR-02)"
  gaps_remaining: []
  regressions: []
gaps:
  - truth: "02-08 prohibition (SEC-04): MUST NOT publish a blob whose content differs, beyond CRLF/LF line endings of UTF-8 text, from the bytes push_updates validated, whatever git clean filter or commit hook produced it"
    status: failed
    reason: "Previously UNCERTAIN (reproduced only at the git level); now reproduced through SyncEngine::push_to_remote in direct mode. A post-commit hook in the cache checkout that amends ude's commit and then makes HEAD a merge whose first parent is the fetched head gets its content to the remote: push_to_remote returns Ok, remote main moves, and the remote blob holds the hook's injected line. `git diff-tree -r HEAD` lists nothing for a merge, so validate_committed_tree checks no blob (no binding check, no secret scan), and check_continuity passes because HEAD~1 is the fetched head. Control: the same hook without the merge step is refused (the existing test_post_commit_hook_rewrite_is_refused case). PR mode has no continuity check, so an amend plus an extra empty commit should pass the same way there (reasoned from code, not run). This is review WR-102 (first review WR-02), still open in the disposition ledger. Trigger requires a user-configured post-commit hook (global core.hooksPath or the cache checkout's .git/hooks) that rewrites commits; such a hook already runs with the user's privileges."
    artifacts:
      - path: "src/sync/mod.rs"
        issue: "validate_committed_tree (line 2137) inspects `git diff-tree --root -r HEAD` (line 2145), which is empty for a merge commit or an empty commit, so nothing is scanned or compared with ValidatedBlobs"
      - path: "src/sync/mod.rs"
        issue: "check_continuity (line 2070) checks only HEAD~1 (the first parent); push_pr_branch (line 1909) has no ancestry check at all"
    missing:
      - "Bind the committed tree to the pre-commit base, not to HEAD's own parent: refuse unless HEAD has exactly one parent equal to the base (fetched head in direct mode, the branch tip in PR mode), or diff the base against HEAD (`git diff-tree -r <base> HEAD`) so every changed blob is scanned and compared"
      - "Alternatively or additionally, run the cache-checkout commit with hooks disabled (`-c core.hooksPath=/dev/null`), which removes the whole post-commit hook class"
      - "Regression test: post-commit hook that amends and wraps HEAD in a merge (and a PR-mode variant with an extra empty commit) is refused and the remote ref is unchanged; control: the plain amend case stays refused and a hook-free push still publishes"
---

# Phase 2: Sanitizer, Boundaries & Security Hardening Verification Report

**Phase Goal:** Screen and redact all confidential tokens, raw credentials, and user-specific paths before any remote git staging or push.
**Verified:** 2026-10-28T00:52:24Z
**Status:** gaps_found
**Re-verification:** Yes, after gap closure (plans 02-09, 02-10)

## Bottom line

Both prior gaps are closed and hold up under probes the plans' own tests do not cover, each with a
negative control on the pre-fix binary (49591e4) that shows the probe detects the old failure.

One must-have that the previous report left UNCERTAIN is now FAILED on reproduced evidence: the
post-commit binding gate passes when a commit hook turns HEAD into a merge, and the hook's content
is pushed without being scanned. The code is unchanged since the first review (WR-102). It only fires
with a user-configured post-commit hook that rewrites commits, so it is a real hole in a stated
prohibition but an unusual trigger. Operator choice: fix it (small), or accept it with an override.

## Goal Achievement

### Roadmap Success Criteria

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| SC1 | High-entropy strings and raw Base64 tokens without prefixes are flagged before staging | VERIFIED | Regression: full `cargo test` exit 0; R11 seeded-rate, corpus and push tests pass. Previous CLI probe (random Base64 token refused, lowercase control pushes) not re-run; code under src/sanitizer/entropy.rs unchanged since then (`git diff 49591e4 HEAD` touches only sanitizer/mod.rs, sync, template, e2e). |
| SC2 | Outbound commit inspection catches gitleaks categories across committed blobs and rolls back | VERIFIED | Regression: `test_push_refuses_gitleaks_categories_without_printing_value`, `test_post_commit_gate_blocks_gitleaks_only_category`, `test_post_commit_hook_rewrite_is_refused` pass. Holds for the commit ude makes and for an amending hook. A hook-built merge HEAD escapes inspection: tracked as the gap below, not double-counted here. |
| SC3 | Path parameterization matches only full word / punctuation boundaries | VERIFIED | `test_path_boundary_table` now runs every row over three root spellings; both properties pass. CLI: `d = <fake>2/y` left byte-identical in every probe. |
| SC4 | Templated tokens substitute back cleanly on initialization | VERIFIED (was FAILED) | CLI probe D: `email = {{ git_email }}` in `gitconfig.repo`, local edit, `sync status` says "push skips it: its template has logic", push exit 1 with the skip line, template bytes unchanged, a consumer init with `consumer@example.com` gets its own email. Pre-fix binary: push exit 0, template rewritten with `tester@example.com`, consumer gets the pusher's email. Variants not in the plan's tests: `{{git_email}}` with no spaces, and `{{ user.home }}` mixed with `{% if %}`: both skipped, template unchanged (pre-fix: both overwritten). Control G: placeholder-only template still publishes, placeholder kept, no raw home. |

### Plan must-have groups

| Plan | Truths (summary) | Status | Evidence |
|------|------------------|--------|----------|
| 02-01 | single `scan_bytes`; raw + NUL-stripped views; non-UTF-8/UTF-16 at staging and both gates | VERIFIED | regression: named tests pass in the full run |
| 02-02 | shared `should_render`; init renders `{{` templates, byte-copies the rest | VERIFIED | regression: e2e byte-identical controls and kept-ancestor tests pass |
| 02-03 | R11 tokenizer/entropy, D-02 exemptions, seeded rates, corpus control | VERIFIED | regression: `sanitizer::entropy::tests` pass |
| 02-04 | `ude:allow-entropy` per line, entropy only | VERIFIED | regression: four marker tests pass |
| 02-05 | vendored pinned rules, load failure is an error | VERIFIED | regression: loader and error tests pass; `cargo deny check advisories bans sources` ok |
| 02-06 | per-category CLI refusal, `--force`, parity | VERIFIED | regression: e2e category tests pass |
| 02-07 | boundary predicates, degenerate-home guard, properties, doc placeholders | VERIFIED | regression; table extended in 02-09 |
| 02-08 | render-back gate 5, no engine detail, validated-blob binding | VERIFIED (truths) | gate-5 and binding tests pass; the binding prohibition fails on a hook-built merge HEAD (gap) |
| 02-09 | `normalize_root`; push, `parameterize_paths` and `detect_for_project` normalize both roots; separator-only roots stay rejected; mutation check | VERIFIED | Code: `normalize_root` at src/sanitizer/mod.rs:159 (trims `is_separator`, returns input when empty), shadowed at the top of push_updates (src/sync/mod.rs:1421-1422) before scan, gate 3 and the gate 5 context, inside parameterize_paths (:284-285), in detect_for_project (src/template/mod.rs:53, :59). CLI probe A (HOME=`<fake>/`, XDG=`<fake>/.config/`): exit 0, 0 raw home occurrences, `({{ user.home }})`, `{{ user.home }}/x`, `{{ user.xdg_config }}/app`, status In Sync, template byte-identical to the plain-HOME push (C). Probe B (HOME=`<fake>//`, XDG unset, so `directories` derives `<fake>//.config`; not in the plan's tests): exit 0, 0 raw occurrences. Pre-fix binary: A and B each publish 3 raw home occurrences with exit 0. Mutation re-run: 8 mutants, 8 caught, 0 missed. |
| 02-10 | gate 2 decides with `should_render`; non-UTF-8 unchanged; edit skipped, deletion refused, controls publish; mutation check; phase gates | VERIFIED | Code: `template_has_logic` (src/sync/mod.rs:2459) uses `template::should_render` for UTF-8, `.jinja` for non-UTF-8, removes the two placeholders, then looks for `{{`/`{%`/`{#`; called by gate 2 for LocallyModified and LocalDeleted (:1494-1511, Err -> IoFailed). CLI probe F: local deletion over a `{{ git_email }}` template: push exit 1, template still exists (pre-fix: template deleted, exit 0). Mutation re-run: 7 mutants, 7 caught, 0 missed. Gates re-run: fmt, clippy `-D warnings`, `cargo machete`, `cargo deny` all exit 0; decoy-GIT_DIR `cargo test --bin ude sync::` 138 passed, decoy repo has 0 refs and 0 objects afterwards. |

### Prohibitions (all 19 test-tier)

| Plan | Prohibition | Status | Evidence |
|------|-------------|--------|----------|
| 02-01 | No lossy decode / skipped non-UTF-8 before the scan | VERIFIED | regression tests pass |
| 02-01 | No matched value printed | VERIFIED | regression; probe D: pusher email count in push output = 0 |
| 02-02 | Init never changes bytes of a no-`{{` or non-UTF-8 template | VERIFIED | regression |
| 02-03 | Refusal names rule, file, line | VERIFIED | regression |
| 02-04 | Marker never silences named or gitleaks rules | VERIFIED | regression |
| 02-05 | Unloadable rule set never treated as clean | VERIFIED | regression |
| 02-06 | No inline suppression except `ude:allow-entropy` | VERIFIED | regression |
| 02-06 | `--force` never skips the scan | VERIFIED | regression |
| 02-07 | Never publish a boundary-delimited home/XDG path | VERIFIED (was FAILED) | probes A/B: 0 raw occurrences; pre-fix: 3 |
| 02-07 | Never rewrite a path that is not the pusher's own | VERIFIED | `<fake>2/y` untouched in A, B, C |
| 02-08 | Never publish a template that does not render back | VERIFIED | regression gate-5 tests |
| 02-08 | Never publish a blob differing from the validated bytes, whatever hook produced it | **FAILED** (was UNCERTAIN) | Reproduced through `push_to_remote`; see gap and spot-checks |
| 02-08 | Never print engine detail or content in a reversibility refusal | VERIFIED | regression |
| 02-09 | Never publish a home/XDG path whatever trailing separators the roots carry | VERIFIED | probes A, B; test_push_parameterizes_home_with_trailing_separator, e2e test_cli_push_parameterizes_trailing_separator_home |
| 02-09 | Never rewrite text when a root is only separators | VERIFIED | test_normalize_root rows `/`, `//`, `///`; test_push_does_not_parameterize_root_home; mutants of normalize_root caught |
| 02-09 | Never render placeholders with a different root spelling than push uses | VERIFIED | probe A status In Sync after push; e2e test_trailing_separator_home_renders_and_reads_in_sync |
| 02-10 | Never publish rendered text over a template with non-path logic | VERIFIED | probes D, E, E2 |
| 02-10 | Never delete such a template, nor a non-UTF-8 `.jinja` template with logic | VERIFIED | probe F; test_push_never_deletes_non_utf8_jinja_template_with_logic with its negative control |
| 02-10 | Never skip an edit over a non-rendered or placeholder-only template | VERIFIED | probe G; commit-msg hook and `{% if %}`-without-`{{` rows in test_template_has_logic |

**Score:** 32/33 verified (4 SC + 10 plan groups + 19 prohibitions); 1 FAILED; 0 present-but-behavior-unverified.

### Required Artifacts

| Artifact | Status | Details |
|----------|--------|---------|
| 02-01..02-08 artifacts | VERIFIED | `verify.artifacts` 20/20 (2+3+2+1+3+2+3+4) |
| `src/sanitizer/mod.rs` (02-09) | VERIFIED | `pub fn normalize_root`, used in parameterize_paths |
| `src/sync/mod.rs` (02-09, 02-10) | VERIFIED | `test_push_parameterizes_home_with_trailing_separator`, `test_push_skips_static_template_with_logic` |
| `src/template/mod.rs` (02-09) | VERIFIED | `normalize_root(` in detect_for_project |
| `tests/e2e_lifecycle.rs` (02-09, 02-10) | VERIFIED | `test_cli_push_parameterizes_trailing_separator_home`, `test_push_skips_edit_over_static_template_with_logic` |

`verify.artifacts`: 02-09 4/4, 02-10 2/2 (exit 0).

### Key Link Verification

| From | To | Via | Status |
|------|----|-----|--------|
| src/sync/mod.rs | src/sanitizer/mod.rs | push_updates shadows both roots with `normalize_root(` | WIRED (tool + code read :1421-1422) |
| src/template/mod.rs | src/sanitizer/mod.rs | detect_for_project normalizes both roots | WIRED (:53, :59) |
| src/main.rs | src/sync/mod.rs | Sync command passes BaseDirs roots to `push_updates(` | WIRED (main.rs:724-731, :1151) |
| src/sync/mod.rs | src/template/mod.rs | `template::should_render(` in template_has_logic | WIRED |
| src/main.rs | src/sync/mod.rs | `template_has_logic(` in locally_modified_hint and logic_template | WIRED (main.rs:1404, :1426); CLI status line in probe D confirms |

`verify.key-links`: 02-09 3/3, 02-10 2/2 (exit 0). Pattern presence does not prove call order; order
is shown by the CLI probes (status hint before push, skip at push).

### Behavioral Spot-Checks

Independent CLI probe script, run against `target/debug/ude` (built by the full test run at HEAD) and
against a pre-fix binary built from `git archive 49591e4` in a scratch directory. Synthetic home under
a scratch root; host config isolated (HOME, XDG_*, GIT_CONFIG_GLOBAL, GIT_CONFIG_NOSYSTEM, ceiling).

| Behavior | HEAD | Pre-fix (negative control) | Status |
|----------|------|----------------------------|--------|
| A: HOME and XDG end in `/` | exit 0, 0 raw, placeholders, In Sync | exit 0, 3 raw | PASS |
| B: HOME ends in `//`, XDG unset | exit 0, 0 raw | exit 0, 3 raw | PASS |
| C: plain HOME, byte-compare with A | identical | differ | PASS |
| D: `{{ git_email }}` edit, push, consumer init | skip, exit 1, unchanged, consumer email | exit 0, overwritten, pusher email | PASS |
| E/E2: `{{git_email}}`; `{{ user.home }}` + `{% if %}` | skipped, unchanged | overwritten | PASS |
| F: local deletion over logic template | template kept | template deleted | PASS |
| G: placeholder-only template (control) | publishes, placeholder kept | same | PASS |
| Merge-HEAD post-commit hook via `push_to_remote` (scratch copy of HEAD with an added probe test) | Ok, remote moved, remote blob holds `INJECTED_BY_HOOK` | control (amend only): refused, remote unchanged | FAIL (gap) |

Probe hook for the gap (post-commit, runs once): append a line to the managed file, `git add`,
`git commit --amend --no-edit --no-verify -q`, then
`M=$(git commit-tree "$(git rev-parse 'HEAD^{tree}')" -p "$(git rev-parse HEAD~1)" -p "$(git rev-parse HEAD)" -m merge); git update-ref HEAD "$M"`.
Then `engine.push_to_remote(checkout, "main", false, ...)` on a `seeded_bare_remote`, as in
`test_post_commit_hook_rewrite_is_refused`.

Full suite run once: `cargo test` exit 0, 218 unit passed (1 ignored), 88 e2e passed. Repo working
tree clean and `git config --local --list` unchanged after every run.

### Probe Execution

No `scripts/*/tests/probe-*.sh` declared or present for this phase. Step 7c: not applicable.

### Requirements Coverage

| Requirement | Source Plans | Status | Evidence |
|-------------|--------------|--------|----------|
| SEC-01 entropy / raw Base64 | 02-03, 02-04, 02-06 | SATISFIED | SC1 |
| SEC-02 gitleaks categories before staging | 02-01, 02-05, 02-06 | SATISFIED | SC2 (staging gate scans every published blob; the hook-merge hole is after staging) |
| SEC-03 strict boundaries | 02-07, 02-09 | SATISFIED | SC3, probes A-C |
| SEC-04 placeholder reversibility | 02-02, 02-07, 02-08, 02-09, 02-10 | PARTIAL | SC4 satisfied; 02-08's binding prohibition (filed under SEC-04) fails, see gap |

All four IDs are claimed by plans; none orphaned. REQUIREMENTS.md currently shows all four as "Gaps Found".

### Anti-Patterns Found

No `TBD`/`FIXME`/`XXX`/`TODO`/`HACK`/`todo!`/`unimplemented!` in the five touched source files, and
none in added lines of `git diff 49591e4 HEAD` (negative control: a planted `FIXME` line counts 1).

### Review findings, judged against the goal

| Item | Verdict |
|------|---------|
| CR-01, CR-02 (first review) | Closed, reproduced closed at the CLI with pre-fix controls |
| WR-102 merge HEAD / PR mode binding | Gap (reproduced through push_to_remote in direct mode) |
| WR-101 raw home left after parameterization (`-I<home>`, alias spellings, non-UTF-8) | Not a gap: accepted risk T-02-21 / T-02-33. Note: alias spelling covers `/home/<user>` while HOME is `/var/home/<user>`, the layout of this Fedora Atomic host |
| WR-01 (re-review) `sync resolve` promises a publish gate 2 refuses | Not a goal gap: a wrong message, nothing is published or leaked |
| WR-02 (re-review) placeholder-only unit test passes on a lossy publish | Not a goal gap: the e2e test and probe G show the placeholder survives in the real CLI path, where scan and push roots agree |
| IN-01 (re-review) `xdg_data` and derived `config_dir` inner `//` | Cosmetic; probe B shows no raw home is published in that case |
| IN-02, IN-03, IN-101..103, IN-04, IN-05 | Info, not goal gaps |

### Human Verification Required

None. The remaining item is a decision, not a check: see Gaps Summary.

### Gaps Summary

One gap, one root cause: the post-commit gate binds HEAD to its own first parent instead of to the
pre-commit base, so a hook that wraps the commit in a merge (direct mode) or adds a commit on top
(PR mode, reasoned) publishes unvalidated, unscanned content. The prohibition text says "whatever git
clean filter or commit hook produced it", so the letter is violated. No later phase (3, 4) names it.

If the operator judges a commit-rewriting hook out of scope (it is user-authored code that could push
on its own), accept it instead with:

```yaml
overrides:
  - must_have: "MUST NOT publish a blob whose content differs, beyond CRLF/LF line endings of UTF-8 text, from the bytes push_updates validated, whatever git clean filter or commit hook produced it"
    reason: "Bypass needs a user-configured post-commit hook that builds a merge or extra commit; such a hook already runs with the user's privileges. Tracked as WR-102."
    accepted_by: "<operator>"
    accepted_at: "<ISO timestamp>"
```

### What these checks do not establish

- The merge-HEAD reproduction ran in direct mode only; the PR-mode variant is inferred from code (no continuity check in push_pr_branch).
- Each CLI probe is one run of one spelling; the boundary table and properties carry the general claim.
- Mutation runs use cargo-mutants' function-level mutants (8 and 7); they do not cover every expression inside the functions.
- SC1/SC2 rest on the regression suite this round; the earlier independent CLI and gitleaks-binary parity probes were not repeated (their code is unchanged).
- Windows path and separator behaviour is unverified (backlog Phase 999.1).

---

_Verified: 2026-10-28T00:52:24Z_
_Verifier: Claude (gsd-verifier)_
