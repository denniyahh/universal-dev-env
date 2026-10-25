---
phase: 02-sanitizer-boundaries-security-hardening
plan: 01
subsystem: security
tags: [sanitizer, secret-scan, regex-bytes, utf-16, publication-gate, rust]

requires:
  - phase: 01-sync-engine-state-hermeticity
    provides: push_updates pass 1, stage_publication / validate_committed_tree gates with reset_index and rollback
provides:
  - "Finding { rule, line } + Finding::reason (rule and 1-based line, never the value)"
  - "Sanitizer::scan_bytes(bytes, path): raw view, then NUL-stripped view when the bytes hold a NUL"
  - "private scan_view (per-view detector pass) and first_named_rule (named-rule pass)"
  - "NAMED_RULES: LazyLock<Vec<(&'static str, regex::bytes::Regex)>>, ASCII mode, compiled once"
  - "check_secrets as a thin wrapper over scan_bytes"
  - "push staging, staged-index gate and committed-HEAD gate all call scan_bytes"
affects: [02-03 entropy R11, 02-04, 02-05 gitleaks rules, 02-07 parameterize_paths rewrite]

actuals:
  tokens: 7645       # chars/4 over the realized diff of src/ (30579 chars)
  tasks: 2
  commits: 4         # git rev-list --count plan_head_before..plan_head_after
plan_head_before: 125c246e4a65724e8729b82af37fef4a0ee14097
plan_head_after: d1b15aa0b6d36afd3d6b7e4089cab378f59493e3

tech-stack:
  added: []
  patterns:
    - "One byte-level scan entry point (scan_bytes) used by every outbound check"
    - "Detectors go inside scan_view; scan_bytes runs it on the raw and NUL-stripped views"
    - "Compile-once rule tables via LazyLock + RegexBuilder::unicode(false)"

key-files:
  created: []
  modified:
    - src/sanitizer/mod.rs
    - src/sync/mod.rs

key-decisions:
  - "Named secret rules use ASCII byte semantics (Go/gitleaks parity): a token next to a non-ASCII letter is now caught; ASCII adjacency unchanged"
  - "UTF-16/UTF-32 content is scanned through a NUL-stripped second view instead of being refused; the second view can only add findings"
  - "Push staging scans the published bytes (after parameterization and EOL styling), so staging and both gates scan the same bytes"
  - "Gate-level secret hits keep exiting 2 via PushRemoteError::Failed (RESEARCH A6, resolved); staging refusal stays exit 1"

patterns-established:
  - "Refusal text: `Forbidden secret pattern matched: <rule> at line <n>`"
  - "Tests assert the token body is absent from every refusal and gate error"

requirements-completed: [SEC-02]

coverage:
  - id: D1
    description: "Push staging refuses a token in a non-UTF-8 managed file before anything is staged; benign non-UTF-8 is published byte-for-byte"
    requirement: SEC-02
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_push_blocks_secret_in_non_utf8_file"
        status: pass
      - kind: unit
        ref: "src/sanitizer/mod.rs#sanitizer::tests::test_scan_bytes_non_utf8"
        status: pass
    human_judgment: false
  - id: D2
    description: "UTF-16LE/BE (with and without BOM) tokens are refused at push staging and at both publication gates; benign UTF-16 passes"
    requirement: SEC-02
    verification:
      - kind: unit
        ref: "src/sanitizer/mod.rs#sanitizer::tests::test_scan_bytes_utf16"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_push_blocks_secret_in_utf16_file"
        status: pass
      - kind: integration
        ref: "src/sync/mod.rs#sync::tests::test_publication_gate_scans_utf16_blob"
        status: pass
      - kind: integration
        ref: "src/sync/mod.rs#sync::tests::test_post_commit_gate_scans_utf16_blob"
        status: pass
    human_judgment: false
  - id: D3
    description: "Both publication gates scan raw git-show bytes (no lossy decode), keep their error prefixes, reset the index / roll back as before"
    requirement: SEC-02
    verification:
      - kind: integration
        ref: "src/sync/mod.rs#sync::tests::test_publication_gate_scans_raw_blob_bytes"
        status: pass
      - kind: integration
        ref: "src/sync/mod.rs#sync::tests::test_post_commit_gate_scans_raw_blob_bytes"
        status: pass
      - kind: integration
        ref: "src/sync/mod.rs#sync::tests::test_publication_gate_scans_staged_blob_rejecting_clean_filter_secrets"
        status: pass
      - kind: integration
        ref: "src/sync/mod.rs#sync::tests::test_post_commit_verification_catches_hook_injected_secrets_and_rolls_back"
        status: pass
    human_judgment: false
  - id: D4
    description: "Refusals name rule and 1-based line, never the matched value; check_secrets (pre-API path) gets ASCII word-boundary semantics"
    requirement: SEC-02
    verification:
      - kind: unit
        ref: "src/sanitizer/mod.rs#sanitizer::tests::test_scan_bytes_reports_rule_and_line_without_value"
        status: pass
      - kind: unit
        ref: "src/sanitizer/mod.rs#sanitizer::tests::test_check_secrets_ascii_word_boundary"
        status: pass
    human_judgment: false

duration: 8min
completed: 2026-10-25
status: complete
---

# Phase 2 Plan 01: Byte-Level Secret Scanner Summary

**One byte-level scanner, `Sanitizer::scan_bytes`, now checks push staging and both publication gates. It scans the raw bytes, plus a NUL-stripped copy when the bytes contain a NUL. Non-UTF-8 and UTF-16 managed files can no longer be published unscanned, and a refusal names the rule and line but never the value.**

## Performance

- **Duration:** 8 min
- **Started:** 2026-10-25T18:18:09Z
- **Completed:** 2026-10-25T18:26:27Z
- **Tasks:** 2 (each a RED + GREEN pair)
- **Files modified:** 2

## Accomplishments
- A token in a non-UTF-8 or UTF-16 managed file now stops `ude sync push` at staging, before anything is written upstream (RESEARCH Pitfall 4 closed). Before this plan such a file was published unscanned.
- The staged-index and committed-HEAD gates scan raw `git show` bytes. No lossy UTF-8 decode is left on any outbound path, and the error prefixes, `reset_index` and rollback paths are unchanged.
- The 6 named rules are now ASCII-mode `regex::bytes` patterns, compiled once per process. A token directly after a non-ASCII letter (`x=é` + token) is detected now; before, the Unicode-mode `\b` missed it.
- `check_secrets` is a wrapper over `scan_bytes`, so the pre-API AGENTS.md scan in `src/main.rs` gets the same verdict with no code change there.
- Plans 02-03 and 02-05 add their detectors inside `scan_view` only. That puts each new detector on all three call sites and both views without touching the call sites again.

## Task Commits

1. **Task 1 RED: failing tests for byte-level push staging scan.** `06229d1` (test)
2. **Task 1 GREEN: scan the exact published bytes at push staging.** `47242fa` (feat)
3. **Task 2 RED: failing tests for raw-byte publication gates.** `4f58de4` (test)
4. **Task 2 GREEN: scan raw blob bytes at both publication gates.** `d1b15aa` (feat)

No REFACTOR commits were needed.

## TDD Evidence

RED evidence came from `cargo nextest run --no-fail-fast --bin ude <filters>` (JUnit report at target/nextest/default/junit.xml). Each target test got its own record, and each record went through `gsd-tools check tdd-red-evidence`. All 10 returned `RED_EVIDENCE_OK` / `target_test_failed`. As a negative control on the classifier, a record naming a test that passed (`test_local_only_boundary`) returned `INVALID_RED` / `no_target_test_failure`.

**Task 1 RED** (stubs: `Finding`, `scan_bytes` returning None, `first_named_rule` returning None). 5 run, 0 passed, 5 failed, exit 100:
- `test_scan_bytes_non_utf8`: `assert_eq` failed, left `None`, right `Some(Finding { GitHub PAT, line 2 })`.
- `test_scan_bytes_utf16`: `assert_eq` failed on "UTF-16LE + BOM", left `None`.
- `test_scan_bytes_reports_rule_and_line_without_value`: the `expect("GitLab token on line 3 must be found")` panicked on `None`.
- `test_push_blocks_secret_in_non_utf8_file` and `test_push_blocks_secret_in_utf16_file`: "Expected SecretBlocked, got Success { synced_files: 1, ... }". The file was published unscanned, which is the hole being closed.
- Semantic assessment: every target test compiled, ran, and failed on its planned behavioural assertion. None failed on a build, fixture or setup fault.

**Task 2 RED** (Task 1 code in place; gates still on the lossy decode plus Unicode `check_secrets`). 5 run, 0 passed, 5 failed, exit 100:
- `test_publication_gate_scans_raw_blob_bytes` and `test_publication_gate_scans_utf16_blob`: the gate let the blob through. The push then failed later with `git push origin main failed: ... 'origin' does not appear to be a git repository`, so the `Publication gate violation: secret detected in staged index blob` assertion failed.
- `test_post_commit_gate_scans_raw_blob_bytes` and `test_post_commit_gate_scans_utf16_blob`: `expect_err` panicked because `validate_committed_tree` returned `Ok(())`.
- `test_check_secrets_ascii_word_boundary`: "Expected detection after a non-ASCII letter" (`check_secrets` returned Clean).
- Semantic assessment: every failure is the planned miss: lossy decode with Unicode `\b`, and NUL-interleaved UTF-16. None failed on a fixture fault. Passing in GREEN shows the clean-filter injection really ran, because the same harness is refused once the scanner is fixed.

**GREEN**
- After Task 1, the 8 tests in the Task 1 verify command all passed (8 PASS lines).
- After Task 2, the Task 2 filter set ran 20 tests and all 20 passed: 14 `sanitizer::` tests and 6 named sync tests.
- Extra check (a mutation test) on the NUL-stripped view: with that view forced off, `test_scan_bytes_utf16` and `test_push_blocks_secret_in_utf16_file` both FAIL, while `test_scan_bytes_non_utf8` still passes. The mutation was reverted and the revert verified.

**TDD gate compliance:** each `feat(02-01)` commit has a `test(02-01)` commit before it (06229d1 before 47242fa, 4f58de4 before d1b15aa).

## Verification Results

| Check | Result |
|-------|--------|
| `cargo fmt --check` | exit 0 (before every commit) |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 (before every commit) |
| `cargo test` (final) | unit 150 passed / 0 failed; e2e 73 passed / 0 failed |
| Task 1 acceptance: `pub fn scan_bytes`, `fn scan_view`, `pub struct Finding` each 1 match; `unicode(false)` ≥1; `scan_bytes(&published` count 1 | PASS |
| Task 2 verify 2 (exactly 2 `Regex::new`, no `secret_patterns`, NAMED_RULES typed `regex::bytes::Regex`) | exit 0. At 4f58de4 the same file had 8 `Regex::new`, so the check would fail there |
| Task 2 verify 3 (no lossy blob decode, no check_secrets/SanitizationResult in sync, scan_bytes ≥3 (3), main.rs call present, no single-literal JWT header) | exit 0. At 4f58de4 there were 2 lossy decodes and 1 JWT header literal |
| `git diff --exit-code -- Cargo.toml Cargo.lock` | exit 0 (no dependency change) |
| `cargo deny check` | exit 0: advisories ok, bans ok, licenses ok, sources ok |
| `cargo machete` | exit 0, `Analyzing dependencies` present |
| gitleaks pre-commit hook | "no leaks found" on all 4 commits |

**What these checks do not establish:**
- The UTF-16 coverage is proven for ASCII-range text. The stripped view does not decode non-ASCII UTF-16 characters, so a token would only be found if it is ASCII, which is the only text the detectors can match anyway.
- The named rules are the only detectors so far. Entropy (02-03) and the gitleaks rules (02-05) are not in place yet.
- The e2e suite exercises the CLI, but no e2e test pushes a UTF-16 file through the real `ude` binary. The coverage is unit/integration tests against the engine API.

## Files Created/Modified
- `src/sanitizer/mod.rs`: adds `Finding`, `Finding::reason`, the `NAMED_RULES` static, `line_at`, `first_named_rule`, `scan_bytes`, `scan_view`, and the `check_secrets` wrapper. `Sanitizer` is now a fieldless struct. 4 new tests, and the JWT fixture is now assembled at runtime.
- `src/sync/mod.rs`: push_updates gates 3/4 (scan the published bytes in both branches); stage_publication and validate_committed_tree now scan raw bytes; the `SanitizationResult` import is removed. 6 new tests and 5 test helpers.

## Decisions Made
- Followed the plan's recorded decisions (ASCII byte semantics, NUL-stripped second view, exit codes unchanged).
- `publish_rel` for push staging is `upstream_target` with `upstream_root` stripped off, falling back to `state.relative_path` as specified. Both gates pass the staged repo-relative path `p`. `scan_view` does not read the path yet (`_path`); plan 02-05 starts using it.
- The new JWT fixture has a third (signature) segment, as the plan described (header, payload, signature). The original literal had only header and payload. `test_secret_detection_bearer_and_unquoted_key` still asserts `Bearer Token`.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] RED stubs carried `#[allow(dead_code)]`**
- **Found during:** Task 1 RED
- **Issue:** `ude` is a binary crate. Stubs used only by tests (`Finding`, `scan_bytes`, `first_named_rule`) trigger dead_code, which `clippy -D warnings` turns into an error. The plan requires fmt and clippy to pass on RED commits.
- **Fix:** Added a temporary `#[allow(dead_code)]` with a comment to each stub in the RED commit. The GREEN commit removed all of them, once the code was wired into push staging.
- **Files modified:** src/sanitizer/mod.rs
- **Committed in:** 06229d1 (added), 47242fa (removed)

**2. [Rule 3 - Blocking] Dropped a fixture-sanity assertion that rustc rejects**
- **Found during:** Task 1 RED
- **Issue:** `assert!(std::str::from_utf8(&[0xFF, ...]).is_err())` on a literal slice trips rustc's `invalid_from_utf8` lint, which `-D warnings` makes a hard error.
- **Fix:** In `test_scan_bytes_non_utf8`, replaced the assertion with a comment: 0xFF never occurs in UTF-8. The sync test builds its bytes at runtime and keeps its `from_utf8(..).is_err()` assertion.
- **Files modified:** src/sanitizer/mod.rs
- **Committed in:** 06229d1

**3. [Structure] Test harnesses factored into helpers instead of copied inline**
- **Found during:** Tasks 1 and 2
- **Issue:** The plan describes the new gate tests as "a copy of the clean-filter harness" and the post-commit tests as "like" the existing test. Inline copies would have repeated about 40 lines four times.
- **Fix:** Added test-only helpers: `push_local_hook`, `upstream_hooks_empty`, `assert_hook_secret_blocked`, `assert_hook_published`, `utf16le_bom`, `push_through_clean_filter` and `validate_committed_base_bytes`. Each one reproduces the existing harness steps. The existing tests are unchanged.
- **Files modified:** src/sync/mod.rs
- **Committed in:** 06229d1, 4f58de4

---

**Total deviations:** 3 (2 blocking fixes, 1 test-structure choice). **Impact:** no change to production behaviour beyond the plan, and no change in scope.

## Issues Encountered
- No existing test changed outcome. The full suite went from 145+73 (after Task 1) to 150+73 (after Task 2), all passing.
- One user-visible text change, as the plan intended: every secret refusal now ends in ` at line <n>`. That includes the pre-API AGENTS.md message in `src/main.rs`, which gets the new text through `check_secrets`. No test asserted the old reason text.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- Plans 02-03 (entropy R11) and 02-05 (gitleaks rules) can append their detectors after `first_named_rule` inside `scan_view`. The `scan_bytes`/`scan_view` signatures and the `path` argument are already in place.
- Plan 02-07 still has to remove `use regex::Regex;` and the two `Regex::new` builds in `parameterize_paths`.

## Self-Check: PASSED
- FOUND: src/sanitizer/mod.rs, src/sync/mod.rs
- FOUND commits: 06229d1, 47242fa, 4f58de4, d1b15aa (all ancestors of HEAD)
- `check evaluation-scope --plan 02-01 --commits-only` resolved the 4 plan commits on this branch

---
*Phase: 02-sanitizer-boundaries-security-hardening*
*Completed: 2026-10-25*
