---
phase: 02-sanitizer-boundaries-security-hardening
plan: 09
subsystem: sanitizer
tags: [rust, sanitizer, path-parameterization, templates, sync, gap-closure]

requires:
  - phase: 02-sanitizer-boundaries-security-hardening (02-07)
    provides: bounded home-path parameterization (replace_bounded, is_parameterizable_root)
  - phase: 02-sanitizer-boundaries-security-hardening (02-08)
    provides: push gate 5 render-back with the push's own roots
provides:
  - "sanitizer::normalize_root: trims trailing separators, keeps a separator-only root as is"
  - "push_updates, parameterize_paths and TemplateContext::detect_for_project all use normalized home and XDG config roots"
  - "Unit, push-level and CLI e2e tests for HOME / XDG_CONFIG_HOME ending in a separator"
affects: [sync push, init, sync status, sync pull, phase-02 verification gap 1]

actuals:
  tokens: 4486      # chars/4 over the realized src/ + tests/ diff (17944 chars)
  tasks: 3
  commits: 6
plan_head_before: 49591e4c40275ecfe1539d0dc72776fcd81e0272
plan_head_after: a1c53d14afc531e82fc601ab73dd2b735184371e

tech-stack:
  added: []
  patterns:
    - "One root-normalization helper shared by every consumer of the home / XDG config roots, so render and parameterize use one spelling"

key-files:
  created: []
  modified:
    - src/sanitizer/mod.rs
    - src/sync/mod.rs
    - src/template/mod.rs
    - tests/e2e_lifecycle.rs

key-decisions:
  - "02-09: home and XDG config roots are normalized with sanitizer::normalize_root (trailing separators trimmed, a separator-only root kept) in push_updates, parameterize_paths and detect_for_project; src/main.rs and xdg_data are unchanged"

requirements-completed: [SEC-03, SEC-04]

coverage:
  - id: D1
    description: "Push with HOME / XDG_CONFIG_HOME ending in one or two separators publishes {{ user.home }} / {{ user.xdg_config }} exactly as with the plain spelling; a neighbouring path is untouched"
    requirement: SEC-03
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_push_parameterizes_home_with_trailing_separator"
        status: pass
      - kind: e2e
        ref: "tests/e2e_lifecycle.rs#test_cli_push_parameterizes_trailing_separator_home"
        status: pass
    human_judgment: false
  - id: D2
    description: "A root made only of separators is never parameterized (normalize_root keeps it; push publishes byte-identical)"
    requirement: SEC-03
    verification:
      - kind: unit
        ref: "src/sanitizer/mod.rs#test_normalize_root"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_push_does_not_parameterize_root_home"
        status: pass
      - kind: unit
        ref: "src/sanitizer/mod.rs#test_degenerate_home_not_parameterized"
        status: pass
    human_judgment: false
  - id: D3
    description: "parameterize_paths gives the same result for /home/alice, /home/alice/ and /home/alice// on every boundary row and in the round-trip property"
    requirement: SEC-03
    verification:
      - kind: unit
        ref: "src/sanitizer/mod.rs#test_path_boundary_table"
        status: pass
      - kind: unit
        ref: "src/sanitizer/mod.rs#prop_parameterize_render_roundtrip"
        status: pass
      - kind: other
        ref: "cargo mutants --file src/sanitizer/mod.rs --re 'normalize_root|parameterize_paths' -- --bin ude -- sanitizer::tests:: (8 caught, 0 missed)"
        status: pass
    human_judgment: false
  - id: D4
    description: "Init and status render the roots push parameterizes with: single separators, byte-identical to a plain-HOME init, pushed file reads In Sync"
    requirement: SEC-04
    verification:
      - kind: e2e
        ref: "tests/e2e_lifecycle.rs#test_trailing_separator_home_renders_and_reads_in_sync"
        status: pass
    human_judgment: false

duration: 13min
completed: 2026-10-27
status: complete
---

# Phase 2 Plan 09: Trailing-separator home roots Summary

**`sanitizer::normalize_root` trims trailing separators off the home and XDG config roots (keeping `/`, `//`, `///` as they are) in push_updates, parameterize_paths and detect_for_project, so `HOME=/home/alice/` no longer publishes the raw home path and init/status render the same spelling push writes.**

## Performance

- **Duration:** ~13 min
- **Started:** 2026-10-27T17:47:28Z
- **Completed:** 2026-10-27T18:00Z
- **Tasks:** 3 (Task 1 tracer, Tasks 2-3 auto, all TDD)
- **Files modified:** 4

## Accomplishments

- Verification gap 1 is closed at the CLI: before the fix, `ude sync push` with `HOME={fake}/` exited 0 and published `a = ({fake})`, `b = {fake}/x`, `c = {fake}/.config/app` raw (RED run). Now it publishes the placeholders, byte-identical to a plain-HOME push.
- Init, status and push render and parameterize with one spelling. Before Task 2, init wrote `{fake}//.gitconfig.local` and `{fake}/.config//app`. Pushing that published `{{ user.home }}//.gitconfig.local` back, and status reported `.gitconfig: Upstream Updated (ready to pull)` (probe run, see RED evidence).
- parameterize_paths normalizes its own roots, so the boundary table holds for three root spellings for any caller.

## Task Commits

1. **Task 1 (tracer): push with HOME ending in `/` publishes placeholders**: `0ee7d09` (test, RED), `5449c4e` (feat, GREEN)
2. **Task 2: detect_for_project uses the same roots**: `b268972` (test, RED), `8e90eab` (feat, GREEN)
3. **Task 3: parameterize_paths normalizes; three-spelling table and property; mutation check**: `d512681` (test, RED), `a1c53d1` (feat, GREEN)

No REFACTOR commits.

## RED evidence (TDD gate)

Records are in `target/gsd-red/02-09-*.json` (gitignored), built from the nextest JUnit report. All were classified with `gsd-tools check tdd-red-evidence`.

- **Task 1, `sanitizer::tests::test_normalize_root`.** Verdict: `RED_EVIDENCE_OK` / `target_test_failed`. The stub returned its input, so 3 of 10 rows failed (`/home/alice/`, `/home/alice//`, `home/alice/`). The separator-only rows passed. Semantic check: the failure is the planned row assertion.
- **Task 1, `sync::tests::test_push_parameterizes_home_with_trailing_separator`.** Verdict: `RED_EVIDENCE_OK`. For both spellings, push returned Success and published lines a-c raw. This reproduces the gap at push_updates.
- **Task 1, `test_cli_push_parameterizes_trailing_separator_home` (e2e).** Verdict: `RED_EVIDENCE_OK`. `ude sync push` exited 0, then the published-tail assertion failed on the raw `fake_home` lines. R11 did not fire on the raw temp path in this run (TMPDIR `/tmp`).
- **Passed in RED by design:** `test_push_does_not_parameterize_root_home`. Classifier negative control: a record naming this test gives `INVALID_RED` / `no_target_test_failure`.
- **Task 2, `test_trailing_separator_home_renders_and_reads_in_sync`.** Verdict: `RED_EVIDENCE_OK`. It failed on its first render assertion, because init rendered `{fake}//.gitconfig.local`.
  - The post-push status line was RED as well. The test stops at its first assertion, so this was checked with a temporary probe test (removed, never committed). The probe ran init, push and status only. The push exited 0 and published `path = {{ user.home }}//.gitconfig.local` and `cfg = {{ user.xdg_config }}//app`. Status printed `↓ .gitconfig: Upstream Updated (ready to pull)`.
- **Task 3, `test_path_boundary_table`.** Verdict: `RED_EVIDENCE_OK`. 32 of 87 row checks failed: all 16 replaced rows under each trailing spelling. No row failed under the plain spelling, and no untouched row failed under any spelling.
- **Task 3, `prop_parameterize_render_roundtrip`.** Verdict: `RED_EVIDENCE_OK`. The companion assertion failed, minimal input `name = "a"` with one home piece (`"/home/a"` vs `"{{ user.home }}"`).
- **Passed in RED by design:** `test_degenerate_home_not_parameterized` with `///` added.

## Mutation check (Task 3)

- `cargo mutants --list --file src/sanitizer/mod.rs --re 'normalize_root|parameterize_paths'` listed **8** mutants: 2 in normalize_root (return `""`, return `"xyzzy"`) and 6 in parameterize_paths.
- The gate run exited **0**: 8 caught, 0 missed, 0 timeout, 0 unviable. Wall time was 67 s (45 s baseline build). The baseline ran 20 tests, all under `sanitizer::tests::`.
- The index and local git config were unchanged after the run, and `.cargo/mutants.toml` does not exist.
- The scratch paths (`target/mutants-roots`, `-tmp`, `-run.log`, `-run.exit`, `-config.before`) were removed.
- No new mutant-killing rows were needed.
- **What it does not establish:** cargo-mutants generated no mutant for the `"" => path` arm, which keeps a separator-only root.
  - A manual control replaced the body with a plain `trim_end_matches`, which trims to the empty string. test_normalize_root failed on the `/`, `//` and `///` rows.
  - test_degenerate_home_not_parameterized and test_push_does_not_parameterize_root_home still **passed** under that control, because `is_parameterizable_root("")` also rejects the empty string.
  - So that arm is defense in depth: only test_normalize_root pins it, and no published bytes depend on it today.

## Negative controls run

- **Gate 3 normalized without gate 5's render context.** The trailing-separator push test returned `ReversibilityBlocked { path: ".gitconfig", ... line 2 }`. This shows the test requires both uses of the roots to share one spelling. The edit was reverted.
- **normalize_root trimming to the empty string.** The separator-only rows failed, as described in the mutation section.
- **Plain-HOME byte equality in both e2e tests.** Before the fix the slash and plain results differed; after it they are equal.

## Files Created/Modified

- `src/sanitizer/mod.rs`:
  - `pub fn normalize_root`.
  - parameterize_paths shadows both roots with it.
  - test_normalize_root, including the `#[cfg(unix)]` trailing-backslash row.
  - Boundary table over three spellings, `///` degenerate rows, round-trip companion.
- `src/sync/mod.rs`:
  - push_updates shadows `user_home` / `xdg_config` before dummy_ctx, render_ctx and gate 3.
  - Tests test_push_parameterizes_home_with_trailing_separator and test_push_does_not_parameterize_root_home, plus the helper push_gitconfig_with_roots.
- `src/template/mod.rs`: detect_for_project normalizes the home and XDG config roots. xdg_data is unchanged.
- `tests/e2e_lifecycle.rs`:
  - Tests test_cli_push_parameterizes_trailing_separator_home and test_trailing_separator_home_renders_and_reads_in_sync.
  - Helpers trailing_separator_cli and push_gitconfig_lines.

## Decisions Made

All of these followed the plan as written:
- `src/main.rs` is unchanged (`git diff --stat 49591e4..HEAD -- src/main.rs` is empty).
- xdg_data is not normalized.
- Separators are `std::path::is_separator`, so a trailing `\` is kept on Unix.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] clippy `redundant_guards` in the RED test commit**
- **Found during:** Task 1 GREEN full check.
- **Issue:** `PushResult::Success { synced_files, .. } if synced_files == 1` fails `clippy -D warnings`. The RED commit `0ee7d09` was gated on `fmt --check` and `build --all-targets`, not on clippy, because a RED tree cannot pass `cargo test`. So it carries this lint.
- **Fix:** Changed the pattern to `Success { synced_files: 1, .. }`. The assertion is unchanged.
- **Committed in:** `5449c4e`.

**2. [Rule 3 - Blocking] RED commits gated on fmt and build, not the full fmt + clippy + test chain**
- **Issue:** The plan's per-commit gate (`fmt && clippy && cargo test && git commit`) cannot hold for a TDD RED commit, because its tests fail by design.
- **Handling:** Each RED commit was gated on `cargo fmt --check && cargo build --all-targets` plus a classified RED record. Each GREEN commit was gated on the full chain (exit 0).

**3. [Process] proptest regression file from the RED run removed**
- **Issue:** The Task 3 RED run created an untracked `proptest-regressions/sanitizer/mod.txt`. It held the RED-only failing seed (`name = "a"`).
- **Handling:** Removed it. The directory is not tracked in this repository, and the seed only failed on the intermediate RED tree.

**4. [Known GSD defect] milestone line in STATE.md / state.json**
- **Issue:** The gsd-tools STATE writers re-add `milestone: v8.30.1` (scraped from a gitleaks version in the ROADMAP).
- **Handling:** Scrubbed after the state updates, as the orchestrator instructed. The orchestrator logs the incident.

---

**Total deviations:** 1 auto-fixed lint (Rule 1), 1 commit-gate adaptation, 2 process cleanups.
**Impact on plan:** No change to scope or behaviour.

## Issues Encountered

- The Task 3 boundary-table rewrite first used nested iterator closures. They failed to compile (borrow and move of `sanitizer`), so I replaced them with plain nested `for` loops before recording RED.
- The RED record is from the compiling run (start time `2026-10-27T17:55:40Z`). The two failed builds left the previous JUnit report in place, with an mtime older than their run start. They were not used as evidence.

## What this does not establish

- Only trailing separators are normalized. `/home/alice/.`, `/home//alice`, symlinked homes and alias homes are not handled (T-02-33, accepted).
- Windows separator behaviour is unverified (Phase 999.1).
- The e2e tests use one HOME layout under the test temp dir (`/tmp/.tmpXXXX/fake_home`). They show that the CLI path normalizes; they do not cover every HOME a user could set.
- The full suite passed on one run: 213 unit + 86 e2e, 1 pre-existing ignored test.

## User Setup Required

None.

## Next Phase Readiness

- Verification gap 1 is closed. Gap 2 (non-`.jinja` placeholder push) is plan 02-10.
- Phase re-verification is still needed after 02-10.

## Self-Check: PASSED

- `src/sanitizer/mod.rs`, `src/sync/mod.rs`, `src/template/mod.rs` and `tests/e2e_lifecycle.rs` are present, with `pub fn normalize_root`, `test_push_parameterizes_home_with_trailing_separator`, `normalize_root(` and `test_cli_push_parameterizes_trailing_separator_home` respectively.
- Commits `0ee7d09`, `5449c4e`, `b268972`, `8e90eab`, `d512681` and `a1c53d1` are ancestors of HEAD. `git rev-list --count 49591e4..HEAD` is 6.
- Grep gates: push_updates 2, normalize_root definitions 1, detect_for_project 2, parameterize_paths 2.

---
*Phase: 02-sanitizer-boundaries-security-hardening*
*Completed: 2026-10-27*
