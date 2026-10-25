---
phase: 02-sanitizer-boundaries-security-hardening
plan: 03
subsystem: security
tags: [sanitizer, entropy, r11, secret-scanning, proptest, regex]

requires:
  - phase: 02-01
    provides: byte-level Sanitizer::scan_bytes / scan_view, Finding { rule, line }, staging and both gates routed through scan_bytes
provides:
  - R11 entropy screen (src/sanitizer/entropy.rs) run inside scan_view after the named rules, reported as rule `high-entropy` with the line only
  - OD-1 tokenizer (`=` is a delimiter except 1-2 trailing padding `=`)
  - Exact D-02 shape exemptions (single-case hex 40/64/128, single-case UUID, SRI at exact digest length, ssh key body after a key type)
  - Test helpers SeededChars (uniform per-character draws), template_corpus_files, class_cycling_token
affects: [02-04 marker and mutation gate, 02-05 gitleaks rules between named rules and R11, 02-06 full-scanner corpus control]

actuals:
  tokens: 11600   # chars/4 over the realized src diff (46,399 bytes)
  tasks: 2
  commits: 4
plan_head_before: 7bcfe3db8299db14a4641af2a06bd62a7ed9d6d9
plan_head_after: ab0ddabf2b16fa446580ec061c699384716a4df7

tech-stack:
  added: []
  patterns:
    - "Seeded fixtures: proptest TestRunner::deterministic() + sample::select, one uniform index draw per character; never regex-string strategies"
    - "Seeded values a test relies on are chosen by predicate (first of 100 draws that passes_r11 accepts), not a fixed first draw"
    - "Exact-integer form for ratio thresholds so ties are decided exactly"

key-files:
  created:
    - src/sanitizer/entropy.rs
  modified:
    - src/sanitizer/mod.rs
    - src/sync/mod.rs

key-decisions:
  - "The scan_view hook and RED stubs were committed together in the RED commit, because a bin crate's unused private items fail clippy -D warnings; the stubs returned None, so the tracer still failed RED"
  - "class_cycling_token() is a shared cfg(test) helper in entropy.rs, used by the push tracer and the corpus negative control"
  - "test_r11_single_class_never_blocks also asserts that at least 300 of 1000 single-class draws clear the entropy bar (measured 622), so the test shows the randomness test, not the entropy test, stops them"

patterns-established:
  - "R11 lives in a pure submodule with documented constants; scan_view calls it last; plan 02-05 inserts gitleaks between"

requirements-completed: [SEC-01]

coverage:
  - id: D1
    description: "A prefix-less high-entropy token in a managed file is refused by push as high-entropy at file:line without the value; an ordinary long word pushes"
    requirement: SEC-01
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_push_blocks_prefixless_high_entropy_token"
        status: pass
    human_judgment: false
  - id: D2
    description: "Rust R11 meets every calibrated seeded floor and ceiling, with exact length, entropy-tie and randomness-tie boundaries and single-class never blocking"
    requirement: SEC-01
    verification:
      - kind: unit
        ref: "src/sanitizer/entropy.rs#test_r11_block_rates_seeded"
        status: pass
      - kind: unit
        ref: "src/sanitizer/entropy.rs#test_r11_fixture_generator_uniform"
        status: pass
      - kind: unit
        ref: "src/sanitizer/entropy.rs#test_r11_length_boundary"
        status: pass
      - kind: unit
        ref: "src/sanitizer/entropy.rs#test_r11_entropy_tie_blocks"
        status: pass
      - kind: unit
        ref: "src/sanitizer/entropy.rs#test_r11_switch_tie_blocks"
        status: pass
      - kind: unit
        ref: "src/sanitizer/entropy.rs#test_r11_single_class_never_blocks"
        status: pass
    human_judgment: false
  - id: D3
    description: "OD-1 tokenizer: NAME=value lines block at exactly the bare-value rate; padding rules pinned"
    requirement: SEC-01
    verification:
      - kind: unit
        ref: "src/sanitizer/entropy.rs#test_r11_env_assignment_rates_seeded"
        status: pass
      - kind: unit
        ref: "src/sanitizer/entropy.rs#test_r11_tokenizer_padding_and_delimiters"
        status: pass
    human_judgment: false
  - id: D4
    description: "Exact D-02 shape exemptions; CONTEXT negative fixtures scan clean with appended-token positives; templates corpus 0 hits"
    requirement: SEC-01
    verification:
      - kind: unit
        ref: "src/sanitizer/entropy.rs#test_r11_negative_fixtures"
        status: pass
      - kind: unit
        ref: "src/sanitizer/entropy.rs#test_r11_shape_exemption_is_whole_token"
        status: pass
      - kind: unit
        ref: "src/sanitizer/entropy.rs#test_exempt_shape_predicate"
        status: pass
      - kind: unit
        ref: "src/sanitizer/entropy.rs#test_r11_templates_corpus_zero_hits"
        status: pass
    human_judgment: false
  - id: D5
    description: "Real-world false-positive rate of R11 on other users' managed files"
    verification: []
    human_judgment: true
    rationale: "CONTEXT D-05: the corpus is this repo's templates only; the rate on other machines is unmeasured and no test can establish it"

duration: 15min
completed: 2026-10-25
status: complete
---

# Phase 2 Plan 03: R11 Entropy Screen Summary

**Calibrated R11 entropy rule (OD-1 tokenizer, f64 Shannon entropy, exact-integer randomness test, exact D-02 shapes) blocks prefix-less credentials in `ude sync push` as `high-entropy` at file:line, reproducing the plan-revision probe rates to the third decimal.**

## Performance

- **Duration:** about 15 min
- **Started:** 2026-10-25T18:42:39Z
- **Completed:** 2026-10-25T18:57:05Z
- **Tasks:** 2 (both TDD: RED then GREEN)
- **Files modified:** 3

## Accomplishments

- `src/sanitizer/entropy.rs` (new): `RULE_NAME`, `first_high_entropy_line`, OD-1 `candidate_tokens`, `shannon_entropy`, `passes_randomness_test` (integer form, no float), `passes_r11`, `is_exempt_shape`, `ssh_key_body_spans`, and the documented constants the plan names.
- `scan_view` runs R11 after the named rules, so a named-rule token keeps its rule name; R11 reaches staging and both gates through `scan_bytes` with no call-site change, the NUL-stripped (UTF-16) view included.
- Push tracer: a `.gitconfig` comment holding a 32-char class-cycling token is refused (`SecretBlocked`, reason has `high-entropy` and `at line 2`, not the token); `ThisIsAPerfectlyNormalSentenceWithWords` in the same place pushes.

## Measured results (rates only, never samples)

Seeded block rates, `test_r11_block_rates_seeded` (1000 samples per row, fresh `SeededChars` per row), next to the plan-revision probe:

| Row | Bound | Measured | Plan-revision probe |
|-----|-------|----------|---------------------|
| B64 20 / 32 / 64 | >= 0.88 / 0.90 / 0.95 | 0.931 / 0.951 / 0.997 | 0.931 / 0.951 / 0.997 |
| URL 20 / 32 / 64 | >= 0.88 / 0.90 / 0.95 | 0.931 / 0.951 / 0.997 | 0.931 / 0.951 / 0.997 |
| AWS-secret-shaped 40 | >= 0.95 | 0.981 | 0.981 |
| JWT-shaped | >= 0.99 | 0.999 | 0.999 |
| leading `+` 24 | >= 0.85 | 0.932 | 0.932 |
| contains `/` 24 | >= 0.85 | 0.938 | 0.938 |
| lower+digit 20 / 32 / 40 | >= 0.70 | 0.795 / 0.811 / 0.825 | 0.795 / 0.811 / 0.825 |
| upper+digit 20 / 32 / 40 | >= 0.70 | 0.795 / 0.811 / 0.825 | identical to lower+digit |
| hex-32 (control) | <= 0.01 | 0.000 | 0.000 |
| lowercase-32 (control) | == 0 | 0.000 | 0.000 |

URL rows equal B64 rows because both use the same index stream and swap only the two symbols; upper+digit equals lower+digit for the same reason. Floors were not lowered.

`test_r11_env_assignment_rates_seeded` (1000 values, 32-char B64): bare 0.951; OD-1 rate 0.951 for `API_TOKEN=`, `CONFIG_PARAMETER_KEY=` and `MY_SERVICE_CLIENT_SECRET_VALUE=` (equal to bare by count, asserted exactly); glued negative control 0.711 / 0.166 / 0.003 (plan revision, Python 2000 values: 0.706 / 0.159 / 0.006; host 0.702 / 0.130 / 0.003).

`test_r11_fixture_generator_uniform`: 64,000 uniform draws, symbol counts min 940, max 1077 (band 850-1150); the skewed alphabet (`+` listed 14 times) fails the same band check.

`test_r11_single_class_never_blocks`: 0 of 1000 lowercase and 0 of 1000 uppercase 32-char draws block, while 622 of 1000 of each clear the entropy bar (floor 300).

Templates corpus under the OD-1 tokenizer: 33 files, 256 candidate tokens, 0 hits (plan revision: 33 / 256 / 0). The injected-token control in a TempDir copy hits.

What these do not establish: the rates are one deterministic seed (the plan's 20-seed check is the evidence the floors are not seed-dependent; not re-run here). The corpus is this repo's templates only.

**Known limit, repeated as required (risk T-02-07, accepted by the operator 2026-10-23):** hex-only and single-case secrets are never caught by R11 (named and gitleaks rules only); about 3-7% of random mixed-case and 18-23% of base36 keys at 20-40 chars pass by chance; base36 tokens of exactly 64 chars are blocked only about 16-20%. D-05 constants unchanged.

## Task Commits

1. **Task 1: R11 core, tracer, seeded rates, exact boundaries**
   - RED `7a3b18b` test(02-03): add failing tests for the R11 entropy screen
   - GREEN `8bf5522` feat(02-03): implement the R11 entropy screen in scan_view
2. **Task 2: D-02 shapes, negative fixtures, corpus control**
   - RED `2ae4448` test(02-03): add failing tests for the D-02 shape exemptions
   - GREEN `ab0ddab` feat(02-03): exempt the exact D-02 shapes from R11

No REFACTOR commits.

## TDD RED evidence

Records built from the nextest JUnit report (`target/nextest/default/junit.xml`, report mtime after run start) and classified with `gsd-tools check tdd-red-evidence`, one record per target test.

**Task 1** (`cargo nextest run --no-fail-fast --bin ude test_push_blocks_prefixless_high_entropy_token test_r11_`, exit 100; 8 failed, 1 passed):
- `test_push_blocks_prefixless_high_entropy_token`: RED_EVIDENCE_OK / target_test_failed. Semantic: failed on "Expected SecretBlocked, got Success" — push published the token, the defect.
- `test_r11_block_rates_seeded`: OK. Every positive row measured 0.000 against its floor.
- `test_r11_length_boundary`: OK. No blocking 20-char draw within 100 draws (nothing blocked yet).
- `test_r11_entropy_tie_blocks`, `test_r11_switch_tie_blocks`: OK. Failed on "must block" (None vs Some(1)); the switch-tie fixture self-checks (65 / 64 changes, equal symbol counts) passed first.
- `test_r11_single_class_never_blocks`: OK. Failed on the entropy-bar floor (0 draws cleared the stubbed bar).
- `test_r11_env_assignment_rates_seeded`: OK. Bare rate 0.000 below 0.90.
- `test_r11_tokenizer_padding_and_delimiters`: OK. The two-padding case returned no candidates.
- `test_r11_fixture_generator_uniform`: passed by design (it checks the helper); its record returned INVALID_RED / no_target_test_failure, which doubles as a classifier negative control.

**Task 2** (`cargo nextest run --no-fail-fast --bin ude test_r11_negative_fixtures test_r11_shape_exemption_is_whole_token test_exempt_shape_predicate test_r11_templates_corpus_zero_hits`, exit 100; 3 failed, 1 passed):
- `test_r11_negative_fixtures`: OK. Failed on "SRI sha384 must scan clean" (Some(1)); the test stops at its first failing case, so the ssh lines' RED was not observed individually.
- `test_r11_shape_exemption_is_whole_token`: OK. Failed on "exact SRI sha256" (Some(1)).
- `test_exempt_shape_predicate`: OK. Failed on "lower hex 40 must be exempt".
- `test_r11_templates_corpus_zero_hits`: passed in RED (INVALID_RED / no_target_test_failure). The corpus holds no token R11 flags even without the exemptions, so this control cannot be RED here; its injected-token control is what shows it bites.

Extra checks run after GREEN (reverted, not committed): `>=` changed to `>` in the randomness comparison fails `test_r11_switch_tie_blocks`; the same change in the entropy comparison fails `test_r11_entropy_tie_blocks`; removing `:` from the ssh key-type context fails `test_r11_negative_fixtures` on the gitconfig literal-key line.

## Files Created/Modified

- `src/sanitizer/entropy.rs` - R11 scorer, D-02 shapes, test helpers and the 12 entropy tests
- `src/sanitizer/mod.rs` - `pub mod entropy;`; scan_view runs R11 after the named rules
- `src/sync/mod.rs` - push tracer `test_push_blocks_prefixless_high_entropy_token` and its `push_gitconfig_comment` helper

## Decisions Made

- The ssh key-body regex, SRI table and hex/UUID predicates follow the plan text exactly. `fookey::ssh-ed25519 <body>` is exempt as the plan accepted.
- `first_high_entropy_line` computes the ssh body spans once per line, then skips exempt and ssh-body tokens before R11.
- Fixtures changed because of R11: none. The full suite (164 bin + 81 e2e tests) passed with R11 active, so no shape had to be pulled forward into Task 1.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] RED commit carried the scan_view hook and a temporary lint allow**
- **Found during:** Task 1 RED
- **Issue:** In a bin crate, unused private stubs and constants fail `cargo clippy -D warnings`, so the planned RED state (stubs only) could not pass the commit gate.
- **Fix:** The RED commit wired the stubs into `first_high_entropy_line` and the scan_view hook (still returning None), deferred the six constants only GREEN uses, and put a `cfg_attr(not(test), allow(dead_code))` on `MIN_TOKEN_LEN`. GREEN removed the allow and added the constants. Task 2 RED did the same for the shape stubs.
- **Verification:** The tracer still failed RED (Success); `rg 'allow\(dead_code\)|RED stub' src/sanitizer/entropy.rs` finds nothing after GREEN.
- **Committed in:** 7a3b18b, 2ae4448

**2. [Rule 2 - Test strength] Single-class test asserts an entropy-pass floor**
- **Found during:** Task 1 RED
- **Issue:** As written, the test would pass against the stub, because "never blocks" holds trivially when nothing blocks.
- **Fix:** It also asserts that at least 300 of 1000 draws clear the entropy bar (measured 622). That makes it fail in RED and shows the randomness test is what stops these draws.
- **Committed in:** 7a3b18b

**3. [Scope - helper] Shared `class_cycling_token()` test helper**
- **Found during:** Task 1
- **Issue:** The tracer token is needed by the sync tracer and by the corpus control.
- **Fix:** It is one `#[cfg(test)] pub(crate)` builder in entropy.rs instead of two copies. The construction is the plan's (`i % 3` picks the class, `i / 3` the symbol).
- **Committed in:** 7a3b18b

---

**Total deviations:** 3 (1 blocking, 1 test-strength, 1 helper placement). **Impact:** no behaviour beyond the plan; the R11 constants and floors are unchanged.

## Issues Encountered

- A mutation spot-check in Task 2 was applied to an uncommitted working tree and then reverted with `git checkout -- src/sanitizer/entropy.rs`, which also discarded the uncommitted Task 2 GREEN edit. The identical edit was re-applied from the same script, and the full gate (fmt, clippy, full `cargo test`, nextest verify, acceptance checks) was re-run before the GREEN commit. Committed history is unaffected. Later spot-checks were done only on committed state.
- TDD gate note: Rust tests live in `src/*.rs`, so the path-based RED-commit gate cannot recognise these `test(02-03)` commits (the known Rust gap in tdd.md). The RED commits do precede their GREEN commits in history.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Plan 02-04 can add the `ude:allow-entropy` marker on top of `line_has_high_entropy_token` and run the mutation gate on entropy.rs; `test_exempt_shape_predicate` pins every shape branch for it.
- Plan 02-05 inserts gitleaks rules at the marked comment in `scan_view`; plans 02-04 to 02-06 can reuse `SeededChars` and `template_corpus_files`.
- Open for the verifier: D5, the false-positive rate on other users' files, is unmeasured by design (CONTEXT D-05).

---
*Phase: 02-sanitizer-boundaries-security-hardening*
*Completed: 2026-10-25*

## Self-Check: PASSED

- Files: entropy.rs, mod.rs (sanitizer), sync/mod.rs and this SUMMARY exist.
- Commits 7a3b18b, 8bf5522, 2ae4448, ab0ddab are ancestors of HEAD; `check evaluation-scope --plan 02-03 --commits-only` resolves them on this branch.
- Acceptance criteria re-run after the last commit: Task 1 (1/1 fn matches, 6 constants, no float in the randomness test with a negative control, SeededChars and `sample::select` present, no regex strategy `.new_tree`) and Task 2 (4 helper/fn lines, 3 SRI constants, `signingKey` and `key::ssh-ed25519` in non-comment code, Cargo.toml/Cargo.lock unchanged) all pass. `cargo deny check` exit 0, `cargo machete` exit 0.
