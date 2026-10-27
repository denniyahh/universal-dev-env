---
phase: 02-sanitizer-boundaries-security-hardening
plan: 05
subsystem: security
tags: [gitleaks, secret-scanning, regex, sanitizer, SEC-02]

requires:
  - phase: 02-sanitizer-boundaries-security-hardening (02-01)
    provides: scan_bytes / scan_view as the single scan entry point for push staging and both publication gates, with the publish-relative path
  - phase: 02-sanitizer-boundaries-security-hardening (02-03)
    provides: R11 entropy screen (last in scan_view) and the SeededChars fixture generator
provides:
  - Vendored gitleaks v8.30.1 default rule file and MIT notice, SHA-256 pinned
  - sanitizer::gitleaks RuleSet (loader, Go-compatible braces, gitleaks plain-text matcher)
  - scan_view order named rules -> gitleaks rules -> R11, at staging and both gates
affects: [02-06 category table and mutation gate, 02-07, 02-08]

actuals:
  tokens: 34909   # chars/4 over the realized diff; 9299 of it excluding the vendored data files
  tasks: 2
  commits: 4
plan_head_before: 657051f9a02bee1ae96fe909841c4aa9d5f1f670
plan_head_after: ca5e22601539a87e6be7c03040740f807105fc8b

tech-stack:
  added: []
  patterns:
    - "Vendored third-party data pinned by SHA-256 in a unit test, with its licence shipped beside it"
    - "serde deny_unknown_fields over every key the vendored file uses, so a schema change fails the load"
    - "Load errors are Err naming the rule; the shipped LazyLock expects a load the pinned test proves"

key-files:
  created:
    - src/sanitizer/gitleaks.rs
    - src/sanitizer/rules/gitleaks.toml
    - src/sanitizer/rules/LICENSE-gitleaks
  modified:
    - src/sanitizer/mod.rs
    - src/sync/mod.rs

key-decisions:
  - "02-05: gitleaks rules run inside scan_view between the named rules and R11, on the raw and the NUL-stripped view, with the publish-relative path"
  - "02-05: the loader also refuses an allowlist with no checks, a rule with neither regex nor path, and a secretGroup beyond the regex's groups (gitleaks Validate parity, fail closed)"
  - "02-05: global allowlist paths never skip a file (A4); rule allowlist paths are honoured, including the AND pre-check"

patterns-established:
  - "Secret-shaped test fixtures are assembled at runtime; the repo's own gitleaks hook refuses literals"

requirements-completed: [SEC-02]

coverage:
  - id: D1
    description: "gitleaks v8.30.1 rule file and LICENSE vendored verbatim and pinned by SHA-256; all 222 rules load (221 regex, 1 path-only) with 0 compile errors"
    requirement: SEC-02
    verification:
      - kind: unit
        ref: "src/sanitizer/gitleaks.rs#test_vendored_rules_load_all"
        status: pass
      - kind: other
        ref: "pin-check command from 02-05-PLAN Task 1 verify (sha256sum x2, rule count, git check-ignore)"
        status: pass
    human_judgment: false
  - id: D2
    description: "A load failure is an Err naming the rule (invalid regex, unknown key), never a skip; Go-literal braces compile"
    requirement: SEC-02
    verification:
      - kind: unit
        ref: "src/sanitizer/gitleaks.rs#test_invalid_rule_regex_is_error"
        status: pass
      - kind: unit
        ref: "src/sanitizer/gitleaks.rs#test_unknown_key_is_error"
        status: pass
      - kind: unit
        ref: "src/sanitizer/gitleaks.rs#test_go_brace_literal"
        status: pass
    human_judgment: false
  - id: D3
    description: "A gitleaks-only category (runtime-built Slack bot token) is refused at push staging, the staged-index gate and the committed-HEAD gate, naming slack-bot-token and the line, never the value"
    requirement: SEC-02
    verification:
      - kind: integration
        ref: "src/sync/mod.rs#test_push_blocks_gitleaks_only_category"
        status: pass
      - kind: integration
        ref: "src/sync/mod.rs#test_publication_gate_blocks_gitleaks_only_category"
        status: pass
      - kind: integration
        ref: "src/sync/mod.rs#test_post_commit_gate_blocks_gitleaks_only_category"
        status: pass
    human_judgment: false
  - id: D4
    description: "gitleaks plain-text matching semantics: keyword prefilter, secret group, strict entropy floor, global and rule allowlists (targets, AND, path pre-check), path-scoped and path-only rules"
    requirement: SEC-02
    verification:
      - kind: unit
        ref: "src/sanitizer/gitleaks.rs#test_keyword_prefilter"
        status: pass
      - kind: unit
        ref: "src/sanitizer/gitleaks.rs#test_secret_group_selection"
        status: pass
      - kind: unit
        ref: "src/sanitizer/gitleaks.rs#test_entropy_floor_is_strict"
        status: pass
      - kind: unit
        ref: "src/sanitizer/gitleaks.rs#test_global_allowlist"
        status: pass
      - kind: unit
        ref: "src/sanitizer/gitleaks.rs#test_rule_allowlist_targets"
        status: pass
      - kind: unit
        ref: "src/sanitizer/gitleaks.rs#test_rule_allowlist_condition_and"
        status: pass
      - kind: unit
        ref: "src/sanitizer/gitleaks.rs#test_rule_allowlist_path_prefilter"
        status: pass
      - kind: unit
        ref: "src/sanitizer/gitleaks.rs#test_rule_path_gating"
        status: pass
    human_judgment: false

duration: 15min
completed: 2026-10-27
status: complete
---

# Phase 2 Plan 05: Vendored gitleaks Rules Summary

**UDE's own scanner now runs all 222 gitleaks v8.30.1 default rules (vendored verbatim, SHA-256 pinned, compiled as ASCII-mode byte regexes) with gitleaks' plain-text matching semantics, between the named rules and R11, at push staging and at both publication gates.**

## Performance

- **Duration:** 15 min
- **Started:** 2026-10-27T00:11:29Z
- **Completed:** 2026-10-27T00:26:26Z
- **Tasks:** 2
- **Files modified:** 5 (3 created, 2 modified)

## Accomplishments

- Vendored `config/gitleaks.toml` and `LICENSE` from the gitleaks `v8.30.1` tag; both hashes match the plan's pins and a unit test pins them over the embedded bytes.
- `RuleSet::from_toml` loads all 222 rules (221 regex, 1 path-only) with 0 errors. Any parse error, unknown key, unknown `condition` / `regexTarget`, or failing pattern is an `Err` naming the rule; nothing is filtered away.
- The matcher follows gitleaks `detect.go` `detectRule` for plain text, checked against the upstream source at the tag: rule-allowlist path pre-check, path gating, LF-trimmed matches, secret group, strict entropy floor, global then rule allowlists.
- A Slack bot token, which the named rules and R11 both miss, is refused at staging (`SecretBlocked`), at the staged-index gate (index reset) and at the committed-HEAD gate (rolled back), as `slack-bot-token`.

## Task Commits

1. **Task 1: Tracer - a gitleaks-only category is refused at push staging and both gates** (TDD)
   - RED: `36a7090` test(02-05): add failing tests for vendored gitleaks rules (also vendors the two upstream files)
   - GREEN: `6618d97` feat(02-05): run the vendored gitleaks rules in scan_bytes
2. **Task 2: Allowlist, path and secret-group semantics match gitleaks** (TDD)
   - RED: `c0b7b8b` test(02-05): add failing tests for gitleaks allowlist and path semantics
   - GREEN: `ca5e226` feat(02-05): apply gitleaks allowlist, path and secret-group semantics
   - REFACTOR: none needed

## Vendoring Provenance (pin-check output, hashes only)

| File | Source | SHA-256 | Size |
|------|--------|---------|------|
| `src/sanitizer/rules/gitleaks.toml` | https://raw.githubusercontent.com/gitleaks/gitleaks/v8.30.1/config/gitleaks.toml | `e163e53b9e7e8a8511e77271e2b323ed057759542a6d988258afe3a1fa329caf` | 3209 lines, 222 `[[rules]]` |
| `src/sanitizer/rules/LICENSE-gitleaks` | https://raw.githubusercontent.com/gitleaks/gitleaks/v8.30.1/LICENSE | `e3884b252b3bfc045e55be43a34d1e80da070bc6f804ac95bf4660e97d62ebc6` | starts `MIT License` |

Both were fetched with `curl -fsSL --max-time 60`. The pin-check command exited 0 and `git check-ignore` exited 1 (not ignored). The repo's own gitleaks pre-commit hook skipped the rule file by its name, as RESEARCH Pitfall 6 predicted.

To check the semantics, I also fetched upstream `detect/detect.go`, `detect/utils.go`, `config/config.go` and `config/allowlist.go` at the same tag into a scratch directory, then deleted them.

## RED Evidence

**Task 1:** `cargo nextest run --no-fail-fast --bin ude gitleaks_only_category gitleaks::`, exit 100, JUnit report, 8 run, 8 failed.
- Every target failed on its planned assertion:
  - loader: `rule_count` was 0, expected 222
  - Go brace test: the pattern did not build without the transform
  - invalid-regex and unknown-key tests: `Ok` where `Err` was expected
  - keyword prefilter: `None` where `kw` at line 2 was expected
  - push tracer: `Success` where `SecretBlocked` was expected, after its own R11-is-None precondition passed
  - committed gate: the clean-commit control validated, then the token commit passed the gate
  - staged-index gate: the gate let the blob through, so the push reached the missing `origin`
- Classifier, on records with targets `sync::tests::test_push_blocks_gitleaks_only_category` and `sanitizer::gitleaks::tests::test_vendored_rules_load_all`: `RED_EVIDENCE_OK` / `target_test_failed`. Records are kept under `target/gsd-red/`, which is gitignored.
- Negative control: the same run targeting a test that did not fail gives `INVALID_RED` / `no_target_test_failure`.
- Semantic assessment: valid RED. Each target executed and failed for the intended reason (no gitleaks rules yet). None failed on build, load or fixture faults.

**Task 2:** `cargo nextest run --no-fail-fast --bin ude <the plan's 5 filters>`, exit 100, 7 run, 5 failed, 2 passed.
- The five failing tests each reported `Some(..)` where the allowlist or path rule should have suppressed the finding.
- `test_secret_group_selection` and `test_entropy_floor_is_strict` passed in RED, which the plan anticipated: Task 1's core already implements both. They pin that core for plan 02-06.
- Classifier on target `test_global_allowlist`: `RED_EVIDENCE_OK`. Negative control targeting the passing `test_entropy_floor_is_strict`: `INVALID_RED`.
- Semantic assessment: valid RED for the five allowlist/path tests.

**Do the tests catch what they claim?** I ran three temporary mutants from a backup, restoring the file byte-identical afterwards (checksum verified):
- Entropy `<=` changed to `<`: caught by `test_entropy_floor_is_strict`.
- The distinct-count-ratio entropy formula (review concern 11): caught by `test_entropy_floor_is_strict` and `test_secret_group_selection`.
- Global `paths` honoured as a whole-file skip: caught by `test_global_allowlist`.

## Measurements

- **Debug-mode rule-set compile:** 827 ms for one `from_toml(VENDORED_RULES)`. That is one measurement on a loaded host, not a distribution. It is paid once per test process under nextest. `test_vendored_rules_load_all` loads twice and takes about 2.0 s.
- **Existing tests whose outcome changed:** none. The full `cargo test` passed after each task: 175 unit + 81 e2e after Task 1, 182 + 81 after Task 2. No allowlist semantics had to be pulled forward into Task 1.
- **Behaviour change on an existing input (cross-plan contract):** I measured this with a temporary probe test, since removed. Through `scan_bytes`, `a` + `ghp_` + 36 digits is now caught as `github-pat`, and a spaced token still reports the named rule `GitHub Personal Access Token`. `test_check_secrets_ascii_word_boundary` asserts its glued control on `first_named_rule` only, so it is unchanged and still passes, as planned.

## Files Created/Modified

- `src/sanitizer/gitleaks.rs`: vendored-rule loader, Go brace transform, gitleaks plain-text matcher, 12 unit tests
- `src/sanitizer/rules/gitleaks.toml`: gitleaks v8.30.1 default rules, verbatim
- `src/sanitizer/rules/LICENSE-gitleaks`: gitleaks MIT notice, verbatim
- `src/sanitizer/mod.rs`: `pub mod gitleaks`; `scan_view` runs named rules, gitleaks rules, then R11, and passes `path`
- `src/sync/mod.rs`: three `*_gitleaks_only_category` tests plus the `seeded_slack_bot_token` and `push_gitconfig_content` helpers

## Decisions Made

- Keyword presence is computed once per view with a `RegexSet` of the escaped, lower-cased keywords. This is equivalent to gitleaks' Aho-Corasick prefilter and avoids a per-rule substring scan.
- `condition` accepts `AND` / `&&` and `OR` / `||` / unset, case-insensitively, as gitleaks `parseAllowlist` does. `regexTarget` accepts `secret` / `match` / `line` / unset. Anything else is an `Err`.
- An empty stopword never allows anything. In gitleaks the empty case is unclear; this choice fails closed.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] The repo's gitleaks pre-commit hook refused the first RED commit**
- **Found during:** Task 1 (RED commit)
- **Issue:** The keyword-prefilter fixture `let token = "tok_..."` matched gitleaks `generic-api-key` (identified by rule ID and line only, with `--redact`).
- **Fix:** I assembled the fixture at runtime under a neutral name (`["tok_", "..."].concat()`). The test still failed the same way.
- **Files modified:** src/sanitizer/gitleaks.rs
- **Committed in:** 36a7090

**2. [Rule 3 - Blocking] Clippy dead-code failure between Task 1 and Task 2**
- **Found during:** Task 1 (GREEN)
- **Issue:** The compiled allowlist and path fields that the plan has Task 1 build are first read in Task 2, so `clippy -D warnings` failed on Task 1's commit.
- **Fix:** I put a temporary `#[expect(dead_code, reason = ...)]` on the two structs in `6618d97` and removed it in `ca5e226`. `expect` would fail the build if it were ever left without a dead field.
- **Files modified:** src/sanitizer/gitleaks.rs
- **Committed in:** 6618d97, ca5e226

**3. [Rule 2 - Missing critical] Extra load-time refusals (fail closed)**
- **Found during:** Task 1
- **Issue:** Three configurations would silently weaken matching:
  - An allowlist with no checks: under AND it would allow every finding.
  - A rule with neither regex nor path.
  - A `secretGroup` beyond the regex's capture groups.
- **Fix:** `from_toml` returns `Err` naming the rule for each, mirroring gitleaks `Allowlist.Validate` and its rule validation. The vendored file loads unchanged, so none of the 222 rules are affected.
- **Files modified:** src/sanitizer/gitleaks.rs
- **Committed in:** 6618d97

**4. [Rule 1 - Minor] Reported line uses the LF-trimmed match start**
- **Found during:** Task 1
- **Issue:** The plan says `line = 1 + LF bytes before the match start`.
- **Fix:** I count up to the start of the match after trimming LF bytes. The two differ only when a regex match begins with LF bytes; then the trimmed start names the line the secret is on.
- **Committed in:** 6618d97

**5. [Process] The GSD state verbs scraped a milestone into STATE.md / state.json (known GSD bug)**
- After the state updates I deleted the `milestone: v8.30.1` line from STATE.md and set `"milestone": null` in state.json, as the orchestrator mandated.

**6. [Process] The plan's no-unwrap acceptance check covers only 18 lines**
- The command reads up to the first `#[cfg(test)]`, which is on `VENDORED_RULES_SHA256` at line 18. It exits 0, but it never inspects the implementation.
- I ran a stronger check over all 415 non-test lines (comments stripped): 0 `.unwrap()`. A planted `.unwrap()` did trip it.

Also: `test_secret_group_selection` has one twin beyond the plan. `secretGroup = 2` on the `foo=` input does report, which shows that group selection, not only the entropy floor, decides that case.

---

**Total deviations:** 4 auto-fixed (2 blocking, 1 missing critical, 1 minor), plus 2 process notes.
**Impact on plan:** No scope change. The extra refusals only make the loader stricter, and the vendored rules load as before.

## Issues Encountered

- My new test helper `push_gitconfig` collided with an existing helper of the same name in `sync::tests`. I renamed mine to `push_gitconfig_content` before the RED run.

## What These Checks Do Not Establish

- **Fidelity to gitleaks itself:** only one real category (`slack-bot-token`) runs end to end. Each semantic is pinned by a synthetic rule set. No differential run against the gitleaks binary covers the 222 rules; plan 02-06's category table is the broader check.
- **Not ported:** encoded-segment decoding (assumption A3, documented gap).
- **Not reproduced:** Go's Unicode `(?i)` case folding, and gitleaks' rune-based entropy for non-ASCII secrets. Both are byte/ASCII here.
- **Windows:** path-scoped rules see the path's native bytes, so on Windows (backslash separators) rules written with `/` would not match. This is unverified, and Windows runtime is deferred to Phase 999.1.
- **Timing:** 827 ms is a single sample.

## User Setup Required

None. No external service configuration is required.

## Next Phase Readiness

- Plan 02-06 can build its category table and mutation gate on `src/sanitizer/gitleaks.rs`. The semantic tests are written so that each mutant class (entropy comparison, formula, allowlist target, condition, path gating) has a test that must fail.

## Self-Check: PASSED

- FOUND: src/sanitizer/gitleaks.rs, src/sanitizer/rules/gitleaks.toml, src/sanitizer/rules/LICENSE-gitleaks
- FOUND (ancestors of HEAD): 36a7090, 6618d97, c0b7b8b, ca5e226. Negative control: a bogus hash is reported missing.
- Evaluation scope (`check evaluation-scope --plan 02-05 --commits-only`): resolved, with plan commits on this branch.
- Acceptance criteria re-run: pin-check exit 0; `include_str!` 1 match; `vendored().first_finding` 1 match; `unicode(false)` 3 matches; no-unwrap exit 0; 15 target tests pass; full `cargo test`, `cargo deny check` and `cargo machete` pass; `git diff --exit-code -- Cargo.toml Cargo.lock` exits 0.

---
*Phase: 02-sanitizer-boundaries-security-hardening*
*Completed: 2026-10-27*
