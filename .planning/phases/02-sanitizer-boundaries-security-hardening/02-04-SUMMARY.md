---
phase: 02-sanitizer-boundaries-security-hardening
plan: 04
subsystem: security
tags: [sanitizer, entropy, r11, allow-marker, cargo-mutants, mutation-testing]

requires:
  - phase: 02-sanitizer-boundaries-security-hardening (plan 02-03)
    provides: R11 entropy screen in src/sanitizer/entropy.rs (first_high_entropy_line, D-02 shape exemptions, SeededChars, class_cycling_token)
  - phase: 02-sanitizer-boundaries-security-hardening (plan 02-01)
    provides: Sanitizer::scan_bytes (named rules before R11, NUL-stripped view) and the check_secrets wrapper
provides:
  - "pub const ALLOW_MARKER = \"ude:allow-entropy\": per-line override that silences only the R11 entropy check on its own line (D-02 item 2)"
  - "first_high_entropy_line skips a line containing the exact, case-sensitive marker before tokenizing; signature unchanged"
  - "D-03 proof: named rules (ghp_, glpat-) still refuse a marked line; near-miss spellings and adjacent-line markers do not silence"
  - "Mutation gate over src/sanitizer/entropy.rs: 149 mutants, 129 caught, 20 unviable, 0 missed, 0 timeouts"
affects: [02-05, 02-06, 02-07, sanitizer, secret-gate]

actuals:
  tokens: 1750
  tasks: 2
  commits: 4
plan_head_before: 4ea418e09fac01881efde32c16f6765a2fa9fab4
plan_head_after: 0629653dfc68b08316919069340bed34b6a557ae

tech-stack:
  added: []
  patterns:
    - "Per-line allow marker checked by exact byte-window match before tokenizing; named rules stay authoritative because scan_view runs them first"
    - "Mutation gate per module with per-gate scratch paths (target/mutants-entropy*) and per-run --exclude-re, no shared .cargo/mutants.toml"

key-files:
  created: []
  modified:
    - src/sanitizer/entropy.rs

key-decisions:
  - "ude:allow-entropy is matched as an exact, case-sensitive substring anywhere on the line (no comment syntax required, no normalization); it silences only R11 on that line"
  - "No mutant exclusions were needed: the entropy.rs mutation gate exited 0 with 0 missed and 0 timeouts, so no --exclude-re and no new mutant-killing tests"

patterns-established:
  - "Mutation-gate negative control: score one known-killed mutant against a test filter that excludes its killing tests and require cargo-mutants exit 2 (missed), proving the scoped tests actually decide outcomes"

requirements-completed: [SEC-01]

coverage:
  - id: D1
    description: "A line containing ude:allow-entropy has its entropy-only hit suppressed through scan_bytes and check_secrets; other lines of the same file are still scored"
    requirement: SEC-01
    verification:
      - kind: unit
        ref: "src/sanitizer/entropy.rs#test_marker_silences_entropy_only_on_its_line"
        status: pass
    human_judgment: false
  - id: D2
    description: "The marker never silences a named rule: a runtime-built ghp_ or glpat- token on a marked line is still refused (D-03)"
    requirement: SEC-01
    verification:
      - kind: unit
        ref: "src/sanitizer/entropy.rs#test_marker_does_not_silence_named_rules"
        status: pass
    human_judgment: false
  - id: D3
    description: "Near-miss spellings (ude:allow_entropy, UDE:ALLOW-ENTROPY, ude: allow-entropy) and the marker on the line before or after the token do not silence R11"
    requirement: SEC-01
    verification:
      - kind: unit
        ref: "src/sanitizer/entropy.rs#test_marker_near_misses_do_not_silence"
        status: pass
    human_judgment: false
  - id: D4
    description: "Every viable mutant of src/sanitizer/entropy.rs is caught by the sanitizer::entropy:: tests (no missed, no timeouts, no exclusions)"
    requirement: SEC-01
    verification:
      - kind: other
        ref: "cargo mutants --file src/sanitizer/entropy.rs -o target/mutants-entropy -- --bin ude -- sanitizer::entropy:: (exit 0; 149 found, 129 caught, 20 unviable, 0 missed) + plan check command (exit 0)"
        status: pass
    human_judgment: false

duration: 14min
completed: 2026-10-27
status: complete
---

# Phase 2 Plan 04: ude:allow-entropy Marker and R11 Mutation Gate Summary

**Per-line `ude:allow-entropy` override that silences only the R11 entropy check on its own line (named rules still refuse the line), plus a clean cargo-mutants gate over the whole entropy module: 149 mutants, 129 caught, 20 unviable, 0 missed.**

## Performance

- **Duration:** 14 min for this resumed run (Task 2 and close-out, 2026-10-26T23:53Z to 2026-10-27T00:08Z); Task 1 ran on 2026-10-25 between 15:03 and 15:05 -0400 (RED evidence to GREEN commit)
- **Started:** 2026-10-25T19:03:24Z (Task 1 RED run)
- **Completed:** 2026-10-27T00:08Z
- **Tasks:** 2 of 2
- **Files modified:** 1 (src/sanitizer/entropy.rs)

## Accomplishments

- `pub const ALLOW_MARKER: &str = "ude:allow-entropy"` and marker handling in `first_high_entropy_line`: a line holding the exact byte sequence is skipped before tokenizing. The public signature and `scan_bytes` are unchanged, so plan 02-05 (same wave) is unaffected.
- D-03 holds by construction and by test: `scan_view` runs the named rules before R11, so a `ghp_` or `glpat-` token on a marked line is still reported under its named rule.
- Mutation gate over src/sanitizer/entropy.rs passed on the first run with no exclusions and no new tests needed. All 11 mutants in the new marker code (`has_allow_marker`, the `!has_allow_marker(line) &&` guard in `first_high_entropy_line`) were caught.

## Task Commits

1. **Task 1: Tracer - ude:allow-entropy silences only the entropy hit on its own line** (TDD)
   - RED: `137c4a7` test(02-04): add failing tests for the ude:allow-entropy marker
   - GREEN: `9a3781e` feat(02-04): honour the ude:allow-entropy marker in R11 only
   - REFACTOR: none (no cleanup needed)
2. **Task 2: Mutation gate over src/sanitizer/entropy.rs** - no commit. The gate found 0 missed mutants, so the plan's "add a test per missed mutant" step produced no code change. The result is recorded here.

The `commits: 4` measured from the ledger (`4ea418e..0629653`) includes two orchestrator state commits made between Task 1 and this resume (`35daaf7`, `0629653`, both `docs(02): record phase 2 ...`). This plan's own code commits are the two Task 1 commits.

## RED Evidence (Task 1)

- Command: `cargo nextest run --no-fail-fast --bin ude test_marker_` (exit 100), JUnit report, 3 tests, 1 failure.
- Target `sanitizer::entropy::tests::test_marker_silences_entropy_only_on_its_line` failed on its planned assertion: `the marked line must scan clean`, left `Some(Finding { rule: "high-entropy", line: 1 })`, right `None`. That is the intended reason (the marker was not implemented yet), not a build, load or fixture fault.
- Classifier (re-run in this session on the saved record): `RED_EVIDENCE_OK`, reason `target_test_failed`.
- The D-03 and near-miss tests passed in RED by design: they are the negative controls that must stay green. Negative control on the classifier: the same run with `test_marker_does_not_silence_named_rules` as the target gives `INVALID_RED` / `no_target_test_failure`, so the classifier does not accept any failing run as RED.
- Semantic assessment: valid RED; the target executed and failed on the behaviour under test.

## Mutation Gate Result (Task 2)

| Measure | Value |
|---------|-------|
| `cargo mutants --list --file src/sanitizer/entropy.rs` | 149 mutants |
| Gate run exit | 0 |
| Found / tested | 149 / 149 |
| Caught | 129 (caught.txt and outcomes.json `caught` agree) |
| Unviable | 20 |
| Missed | 0 |
| Timeouts | 0 (so no solo re-run and no reproduction run; the reproduction command exited 0 with "no timeouts to reproduce") |
| Exclusions (`--exclude-re`) | none |
| New mutant-killing tests | none needed |
| Wall time | 11 min (baseline 69 s build + 1 s test; auto test timeout 20 s; host load average was about 8 during the run) |
| Baseline scope | 15 tests, all under `sanitizer::entropy::` (check command: total = in-scope) |
| Plan check command | exit 0 |

**Unviable mutants (20):** all are return-value replacements that do not compile, so they never reach the tests: 7 for `ssh_key_body_spans` (`vec![Range::new(..)]`, `Range::from(..)`, `Range::from_iter(..)`), 6 each for `pieces` and `candidate_tokens` (`::std::iter::once((0|1, Vec::leak(..)))`, which cannot satisfy the borrowed-slice lifetime), and 1 for `CharClass::of` (`Some(Default::default())`, `CharClass` has no `Default`).

**Negative control (added; not in the plan):** the same cargo-mutants invocation, restricted with `--re` to `replace has_allow_marker -> bool with false` and with the test filter narrowed to `sanitizer::entropy::tests::test_r11_length_boundary` (a test that does not exercise the marker), exited 2 and listed that mutant in missed.txt. So the gate reports a mutant as missed when the scoped tests cannot see it, and the 0-missed result above comes from the real tests, not from a filter that runs nothing.

**What this does not establish:** the gate shows each generated mutation changes some `sanitizer::entropy::` test outcome. It does not show the tests cover mutations cargo-mutants does not generate (for example changed constants such as `MIN_TOKEN_LEN = 20` or `ENTROPY_FACTOR = 0.80`; those are pinned by the plan 02-03 boundary and tie tests, not by this gate), and it says nothing about block rates on real credentials, which plan 02-03's seeded rate tests measure.

## Files Created/Modified

- `src/sanitizer/entropy.rs` - `ALLOW_MARKER`, `has_allow_marker`, the marker skip in `first_high_entropy_line`, and the three marker tests (Task 1). Unchanged by Task 2.

## Decisions Made

- The marker is an exact, case-sensitive substring anywhere on the line (planner reading of D-02; managed files use several comment styles).
- No mutant exclusions and no shared `.cargo/mutants.toml` (acceptance: `test -e .cargo/mutants.toml` fails).

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Stale mutation scratch from an interrupted executor**
- **Found during:** Task 2 (before the run)
- **Issue:** target/mutants-entropy, -tmp, -run.log and -run.exit already existed from the earlier executor of this plan (a complete run dated 2026-10-25 15:12 -0400, exit 0, 149 mutants, 129 caught, 20 unviable), plus target/red-evidence-02-04 from Task 1. src/ was unchanged since then (`git diff 9a3781e HEAD -- src` empty), but that run was not verified in this session, and cargo-mutants would have rotated the old output beside the new one.
- **Fix:** removed the stale scratch paths and re-ran the gate fresh. The fresh result matches the stale one exactly.
- **Files modified:** none tracked
- **Verification:** fresh run exit 0 with the counts above
- **Committed in:** n/a (scratch only)

**2. [Rule 2 - Verification] Added a negative control for the mutation gate**
- **Found during:** Task 2
- **Issue:** a 0-missed result without a control cannot distinguish "the tests catch everything" from "the scoped filter does not run the tests that matter".
- **Fix:** one extra single-mutant run (target/mutants-entropy-nc) with a non-killing test filter; it reported the mutant missed (exit 2) as required.
- **Files modified:** none tracked
- **Verification:** exit 2, mutant listed in missed.txt
- **Committed in:** n/a (scratch only, removed)

---

**Total deviations:** 2 auto-handled (1 blocking, 1 verification). **Impact on plan:** none on code; both strengthen the evidence for the gate result.

## Issues Encountered

None beyond the deviations above.

## Verification Run (this session)

- Plan check command for the gate: exit 0.
- `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test`: exit 0; test results 167 passed and 81 passed, 0 failed; no "running 0 tests" line.
- `cargo deny check`: exit 0 (advisories, bans, licenses, sources ok).
- `cargo machete`: exit 0, output contains `Analyzing dependencies`, no unused dependencies.
- `git diff --exit-code -- Cargo.toml Cargo.lock`: exit 0. `test -e .cargo/mutants.toml`: fails (no file).
- `rg -n 'pub const ALLOW_MARKER: &str = "ude:allow-entropy";' src/sanitizer/entropy.rs`: 1 match (line 21). `rg -c 'gitleaks:allow' src/sanitizer/entropy.rs src/sanitizer/mod.rs`: no matches.
- Host hermeticity after the TMPDIR-in-repo run: `git status --porcelain` empty, and `git config --local --list` identical to the copy captured before the run (22 lines).

## Scratch Cleanup

Removed after recording the result, and checked with `ls` (no such file or directory): target/mutants-entropy, target/mutants-entropy-tmp, target/mutants-entropy-run.log, target/mutants-entropy-run.exit, target/mutants-entropy-repro.exit, target/mutants-entropy-nc, target/mutants-entropy-nc.log, target/red-evidence-02-04. target/mutants-entropy-solo, -solo.log and -hangs.txt were never created (no timeouts); the `ls` check confirms they are absent too. target/tmp predates this plan and was left alone.

## TDD Gate Compliance

RED `137c4a7` test(02-04) precedes GREEN `9a3781e` feat(02-04), and GREEN descends from RED. No REFACTOR commit (none needed). Task 2 added no behaviour, so it has no RED/GREEN pair.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Plan 02-05 (same wave) can rely on `first_high_entropy_line`'s unchanged signature; the marker only affects R11.
- Plan 02-06 asserts the gitleaks-rule half of D-03 and that gitleaks' own inline allow comment is ignored.
- Residual risk T-02-09 stands as accepted in D-02: a prefix-less credential on a marked line is not blocked.

---
*Phase: 02-sanitizer-boundaries-security-hardening*
*Completed: 2026-10-27*
