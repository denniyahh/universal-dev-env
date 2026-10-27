---
phase: 02-sanitizer-boundaries-security-hardening
plan: 06
subsystem: security
tags: [gitleaks, secret-scanning, mutation-testing, parity, SEC-02, SEC-01]

requires:
  - phase: 02-sanitizer-boundaries-security-hardening (02-05)
    provides: vendored gitleaks v8.30.1 rule set and the plain-text matcher in src/sanitizer/gitleaks.rs
  - phase: 02-sanitizer-boundaries-security-hardening (02-02)
    provides: e2e push helpers (sync_cli, project_with_local_templates, reinit_rust)
  - phase: 02-sanitizer-boundaries-security-hardening (02-03/02-04)
    provides: SeededChars, template_corpus_files, ALLOW_MARKER, class_cycling_token
provides:
  - CLI-level proof that ude sync push refuses 10 gitleaks-only categories without printing the value, with or without --force
  - 15-row category table (CATEGORY_RULE_IDS) with benign twins and an uncovered-rule report
  - D-03 tests for gitleaks rules; full-scanner template corpus control
  - mutation gate over gitleaks.rs with 0 missed; parity with the real gitleaks 8.30.1 binary
affects: [02-07, 02-08, phase verification]

actuals:
  tokens: 6609   # chars/4 over the realized diff (git diff plan_head_before..HEAD)
  tasks: 3
  commits: 4
plan_head_before: f969eb9b231c30ec9fbc8d3298163b5971e64d46
plan_head_after: 6020f7378c911acdf2ef6b0685c83ebb7e584f11

tech-stack:
  added: []
  patterns:
    - "Each assurance test carries a twin that must give the opposite result"
    - "Port vs upstream parity: an ignored test dumps a corpus after checking the port's verdicts, the real binary scans it, and a scripted fresh-report run is the negative control"
    - "Iterator-driven byte walks so loop-advance mutants cannot hang the gate"

key-files:
  created: []
  modified:
    - tests/e2e_lifecycle.rs
    - src/sanitizer/gitleaks.rs

key-decisions:
  - "02-06: trim_lf rewritten with take_while counts instead of while-loop index advances (behaviour unchanged, checked by running the new LF-trim test against the old code); this removes the += 1 to *= 1 hang class from the mutation gate"
  - "02-06: the 15 category rows stay; 207 of 222 vendored rules have no behavioural fixture and are recorded as residual risk"

patterns-established:
  - "Secret-shaped fixtures whose text sits near a provider keyword in source (curl ... -u user:pass) are bound in a separate variable so the repo's own gitleaks hook does not match the source"

requirements-completed: [SEC-02, SEC-01]

coverage:
  - id: D1
    description: "ude sync push refuses each of 10 gitleaks-only categories with exit 1, naming .gitconfig, the rule id and the line, never printing the value, and leaves the shared template unchanged; --force still refuses; a benign Slack twin is published"
    requirement: SEC-02
    verification:
      - kind: e2e
        ref: "tests/e2e_lifecycle.rs#test_push_refuses_gitleaks_categories_without_printing_value"
        status: pass
    human_judgment: false
  - id: D2
    description: "One runtime-built positive per category maps to its rule id through the vendored set (15 rows, 12 entropy-gated); every benign twin gives no finding; uncovered rules are counted (207 of 222)"
    requirement: SEC-02
    verification:
      - kind: unit
        ref: "src/sanitizer/gitleaks.rs#test_category_positives"
        status: pass
      - kind: unit
        ref: "src/sanitizer/gitleaks.rs#test_report_rules_without_positive_fixture"
        status: pass
    human_judgment: false
  - id: D3
    description: "D-03 for gitleaks rules: ude:allow-entropy does not silence a gitleaks hit, and gitleaks' inline allow comment is not honoured"
    requirement: SEC-01
    verification:
      - kind: unit
        ref: "src/sanitizer/gitleaks.rs#test_marker_does_not_silence_gitleaks_rules"
        status: pass
      - kind: unit
        ref: "src/sanitizer/gitleaks.rs#test_inline_gitleaks_allow_not_honoured"
        status: pass
    human_judgment: false
  - id: D4
    description: "Full scanner (named rules, gitleaks rules, R11) reports 0 findings on every file under templates/ and template/ (at least 33 files); a Slack token appended to a copy is caught"
    requirement: SEC-02
    verification:
      - kind: unit
        ref: "src/sanitizer/gitleaks.rs#test_full_scanner_templates_corpus_zero_hits"
        status: pass
    human_judgment: false
  - id: D5
    description: "cargo mutants over src/sanitizer/gitleaks.rs, scoped to sanitizer::gitleaks:: tests: exit 0, 105 mutants, 98 caught, 7 unviable, 0 missed, 0 timeouts"
    requirement: SEC-02
    verification:
      - kind: other
        ref: "02-06-PLAN Task 3 run command + check command (exit 0)"
        status: pass
    human_judgment: false
  - id: D6
    description: "Parity with gitleaks 8.30.1: on the 15-file category corpus gitleaks reports exactly each file's own rule id and nothing for benign.txt; the scripted negative control (slack token removed, fresh report) no longer reports slack-bot-token"
    requirement: SEC-02
    verification:
      - kind: other
        ref: "02-06-PLAN Task 3 parity command (exit 0), run before and after the trim_lf refactor"
        status: pass
    human_judgment: false

duration: 23min
completed: 2026-10-27
status: complete
---

# Phase 2 Plan 06: gitleaks Port Assurance Summary

**The gitleaks port now has CLI-level refusal tests for 10 gitleaks-only categories, a 15-category table with benign twins, D-03 tests for gitleaks rules, a clean full-scanner template corpus, a mutation gate with 0 missed mutants (98 caught, 7 unviable), and exact rule-for-rule agreement with the real gitleaks 8.30.1 binary on the category corpus.**

## Performance

- **Duration:** 23 min
- **Started:** 2026-10-27T00:31:12Z
- **Completed:** 2026-10-27T00:54:25Z
- **Tasks:** 3
- **Files modified:** 2

## Accomplishments

- `ude sync push` refuses slack-bot-token, stripe-access-token, gcp-api-key, npm-access-token, pypi-upload-token, anthropic-api-key, vault-batch-token, datadog-access-token, curl-auth-user and jwt:
  - each exits 1 with `.gitconfig`, `<rule-id> at line <n>` (the exact appended line) on stderr;
  - neither stream contains the value, and the shared template is byte-identical afterwards.
  - `--force` with the Slack token still exits 1 and publishes nothing. A twin `xoxb-` line with 3 digits pushes with exit 0 and lands in the template.
- `test_category_positives`: all 15 rows give their own rule id as the first finding of the vendored set, and all 15 twins give None.
  - The aws-access-token and github-pat twins match the regex and are rejected only by the entropy floor, so the table also checks the floor.
- `test_report_rules_without_positive_fixture` prints `vendored rules without a positive fixture: 207 of 222`. I counted it a second way from the TOML (222 `id =` lines, 0 duplicates, each of the 15 ids present once), which also gives 207.
- D-03: a Slack token on a line carrying `ude:allow-entropy` is still refused. The control: the same marker does silence the R11 tracer token, and the unmarked tracer is caught. gitleaks' inline allow comment does not suppress the Slack hit; the comment alone scans clean.
- Template corpus: 0 findings from the full scanner. The negative control (Slack token appended to a copy of `gitconfig.repo`) reports `slack-bot-token` at the appended line.

## Task Commits

1. **Task 1: Tracer - `ude sync push` refuses every gitleaks-only category** - `5f234b8` (test)
2. **Task 2: Category table, D-03 for gitleaks rules, full-scanner corpus control** - `1d2d7f9` (test)
3. **Task 3: Mutation gate and parity**
   - `90f429f` (refactor): `trim_lf` with iterators
   - `6020f73` (test): mutant-killing tests and `test_dump_parity_corpus`

No TDD RED step: the plan is `type: execute` with `tdd="false"` tasks and adds no production behaviour. The failing direction comes from twins, negative controls, the mutation gate and the parity negative control.

## Mutation Gate

- Listed before the refactor: 115 mutants. The `trim_lf` refactor changed the set, and the gate run found **105 mutants**.
- Run command (plan verbatim, wrapped in `timeout 5400`): **exit 0**.
  - 105 tested in 9 min; baseline 74 s build + 1 s test.
  - **98 caught, 7 unviable, 0 missed, 0 timeouts.**
- Check command: exit 0. The baseline ran 23 tests, all inside `sanitizer::gitleaks::`. caught.txt and outcomes.json both say 98.
  - Negative control: with the exit file set to 2, the check's exit-code branch failed (exit 1). With 0 restored, it passed.
- Reproduction command: no-op ("no timeouts to reproduce (gate run exit 0)"). There are no accepted hangs.
- Exclusions: none (`--exclude-re` was not used). `.cargo/mutants.toml` was not created.
- The 7 unviable mutants do not compile:
  - six `Default::default()` replacements (`from_toml`, `first_finding` Some, `compile_rule`, `compile_allowlist`, `vendored`, `compile`); the types have no `Default`;
  - `&&` to `||` inside the `if let ... && group >= ...` let-chain in `compile_rule`.

**New tests and the mutants only they catch.** I took this from the per-mutant logs: these are mutants whose failing tests are all among the new five.

| Test | Mutant(s) it alone catches |
|------|----------------------------|
| `test_rule_allowlist_or_path_skips_file` | `skips_file`: `&&` to `\|\|` (272:13) |
| `test_rule_allowlist_and_needs_path_and_stopword` | `skips_file`: delete `!` (272:45) |
| `test_line_target_is_the_whole_line` | `line_span`: `+` to `*` and `+` to `-` (323:26), `==` to `!=` (326:26), `+` to `-` (327:38) |
| `test_match_is_trimmed_of_lf` | `trim_lf`: `+` to `-` (340:12) |
| `test_secret_group_zero_is_unset` | `compile_rule`: `>` to `>=` in the `secretGroup` filter (380:55) |

I wrote these five before the run, from reading the mutant list against the existing tests, so the run had no missed mutants to start from. The table shows each is load-bearing: without it, the listed mutants would have been missed.

## Parity with gitleaks 8.30.1

- The plan's parity command, verbatim: **exit 0**, run once before and once after the `trim_lf` refactor.
- Positive check: a separate diagnostic run (scratch copy, deleted) shows gitleaks reported **exactly 15 findings, one per category file, each with that file's own rule id, and nothing for benign.txt**. The plan permits extra generic findings via `any(...)`, but none occurred.
- Scripted negative control: `slack-bot-token.txt` was overwritten with the benign text and gitleaks re-ran into a fresh `report-nc.json`.
  - It exited 1 with 14 findings, so the second run happened.
  - The slack-bot-token comparison returned jq exit 1 (not reported), as required.

## Residual Risk

- `vendored rules without a positive fixture: 207 of 222`. These 207 rules load and compile (the 02-05 loader test), and the shared matcher is pinned by the semantics tests and the mutation gate. No test shows that any one of their regexes matches a real token. A vendored rule whose regex never fires would go unnoticed (T-02-29, accepted).

## Files Created/Modified

- `tests/e2e_lifecycle.rs`: `FixtureChars` (a seeded per-character draw, a local copy of `SeededChars`), `gitleaks_only_fixtures`, and `test_push_refuses_gitleaks_categories_without_printing_value`.
- `src/sanitizer/gitleaks.rs`:
  - `#[cfg(test)] CATEGORY_RULE_IDS` and `RuleSet::rule_ids`;
  - `trim_lf` refactor;
  - 11 new tests: 5 Task 2 tests, 5 mutant-killing tests, and the ignored `test_dump_parity_corpus`.

## Decisions Made

- `trim_lf` uses `take_while` counts. The plan prefers iterator-driven walks so that a loop-advance mutant cannot hang. Before committing, the new LF-trim test passed against the old while-loop code as well, so behaviour is unchanged.
- Expected rule ids: none changed. Every row's first finding was its planned id on the first run.
- The e2e asserts the exact line (`<rule> at line <n>`), not only `at line`.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] The repo's gitleaks pre-commit hook refused the first Task 1 commit**
- **Found during:** Task 1 commit
- **Issue:** The source line `format!("# {} -u deploy:{curl_password} ...", "curl")`, near the `"curl-auth-user"` string, matched gitleaks `curl-auth-user` across lines. `{curl_password}` satisfies `[\w${}@.-]+`. I identified it by rule id and line only, with `--redact`.
- **Fix:** I bound the credential in a separate variable (`curl_credential`), so the curl line in the source has no `user:pass` shape. The staged rescan reports 0 (it reported 1 before the fix). The test still passes.
- **Files modified:** tests/e2e_lifecycle.rs
- **Committed in:** 5f234b8

**2. [Plan-preferred refactor] `trim_lf` rewritten with iterators**
- **Found during:** Task 3, while reading the mutant list
- **Issue:** The `while` loops advanced indices by hand (`start += 1`, `end -= 1`). With a test that exercises them, a `*= 1` mutant would hang and force the solo-reproduction procedure.
- **Fix:** `take_while(...).count()` from both ends, committed separately as `90f429f` after the full gated test run.
- **Verification:** The full suite passes; the new LF test passes on both the old and new code; the gate reports 0 timeouts.

**3. [Process] The mutation run command was wrapped in `timeout 5400`**
- The operator rules require bounded long commands. The inner command is verbatim. It finished in about 9 min, well inside the bound.

**4. [Process] GSD state verbs scrape `milestone` (known GSD bug)**
- After the state updates I removed the scraped milestone line from STATE.md and set `"milestone": null` in state.json, as the orchestrator mandated. The orchestrator logs the incident.

---

**Total deviations:** 1 auto-fixed (blocking), 1 plan-preferred refactor, 2 process notes.
**Impact on plan:** No scope change. The only production-code change is behaviour-preserving.

## Issues Encountered

None beyond the hook refusal above.

## What These Checks Do Not Establish

- **Parity is shown only on 15 runtime-built positives and one benign file.** It is not shown on the other 207 rules, on the 14 other twins (only the Slack twin went to the binary), or on real-world files.
- **The e2e covers 10 categories at the CLI.** The other 5 (private-key, aws, github, gitlab, generic) are covered only at unit level, where the UDE named rules may report first under their own names.
- **The mutation gate covers `gitleaks.rs` only, against its own module's tests.** It does not cover `scan_view` ordering in `mod.rs` or the push wiring; the e2e and the 02-05 integration tests cover those.
- **The fixtures are deterministic seeded draws.** Each row is one sample, not a distribution. A positive that passes says that one token is caught.
- **Encoded secrets are not ported** (T-02-17, accepted).

## Cleanup

- Removed: target/mutants-gitleaks, -gitleaks-tmp, -gitleaks-run.log, -gitleaks-run.exit, -gitleaks-solo, -gitleaks-solo.log, -gitleaks-hangs.txt, -gitleaks-repro.exit and target/gitleaks-parity. I checked that each is absent.
- Kept: target/gsd-red/ from earlier plans.
- After the mutation run, `git status --porcelain` showed only the intended change to gitleaks.rs, and `git config --local --list` matched the pre-run snapshot.

## User Setup Required

None. No external service configuration is required.

## Next Phase Readiness

- Plans 02-07 and 02-08 can proceed. This plan touched neither `parameterize_paths` nor `mod.rs`.

## Self-Check: PASSED

- FOUND: tests/e2e_lifecycle.rs, src/sanitizer/gitleaks.rs (modified; both contain the planned test names).
- FOUND (ancestors of HEAD): 5f234b8, 1d2d7f9, 90f429f, 6020f73. `git rev-list --count` from plan_head_before gives 4.
- Acceptance criteria re-run:
  - the e2e test-name count is 1;
  - each of the 15 ids appears 2 or more times in gitleaks.rs;
  - the report prints 207 of 222;
  - full `cargo test` passes (192 unit + 1 ignored, 82 e2e), along with fmt and clippy;
  - `cargo deny check` and `cargo machete` pass;
  - no `.cargo/mutants.toml`, and `git diff --exit-code -- Cargo.toml Cargo.lock` exits 0;
  - all scratch paths are gone.

---
*Phase: 02-sanitizer-boundaries-security-hardening*
*Completed: 2026-10-27*
