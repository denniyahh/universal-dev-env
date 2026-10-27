---
phase: 02-sanitizer-boundaries-security-hardening
plan: 10
subsystem: sync
tags: [rust, sync, push, templates, template-logic, gap-closure, mutation-testing]

requires:
  - phase: 02-sanitizer-boundaries-security-hardening (02-02)
    provides: "template::should_render, the single render rule init, scan and pull use"
  - phase: 02-sanitizer-boundaries-security-hardening (02-09)
    provides: "normalized home / XDG roots in push_updates (unchanged here)"
provides:
  - "template_has_logic (push gate 2) decides with template::should_render for UTF-8 templates; non-UTF-8 templates keep the previous answer (Phase 01 D-14)"
  - "Unit, push-level and CLI e2e tests: edit over a static template with {{ git_email }} skipped, deletion refused, consumer init gets its own email; placeholder-only and shell-syntax controls still publish"
affects: [sync push, sync status, phase-02 verification gap 2, SEC-04, ROADMAP SC4]

actuals:
  tokens: 5010      # chars/4 over the realized src/ + tests/ diff (20042 chars)
  tasks: 3
  commits: 4
plan_head_before: 03eb4774e3874b7345b3a5b15f51128e30ce4337
plan_head_after: a08e4394195b80b5a8fdf8838b5c857052ed1e11

tech-stack:
  added: []
  patterns:
    - "Push gate 2 and init share one render rule, so the set of templates push protects is the set init renders"

key-files:
  created: []
  modified:
    - src/sync/mod.rs
    - tests/e2e_lifecycle.rs

key-decisions:
  - "02-10: push gate 2 (template_has_logic) uses template::should_render for UTF-8 templates (operator option A); a non-UTF-8 template keeps the previous answer (logic only when .jinja and a marker remains after placeholder removal, D-14); the marker check runs on raw bytes with non-ASCII bytes read as NUL instead of a lossy decode"

patterns-established:
  - "Pin tests that pass in RED by design are each shown to fail under the wrong rule they guard against before being committed"

requirements-completed: [SEC-04]

coverage:
  - id: D1
    description: "A local edit over a non-.jinja shared template holding {{ git_email }} is skipped; the template bytes are unchanged and the rendered email is never published"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_push_skips_static_template_with_logic"
        status: pass
      - kind: e2e
        ref: "tests/e2e_lifecycle.rs#test_push_skips_edit_over_static_template_with_logic"
        status: pass
    human_judgment: false
  - id: D2
    description: "At the CLI, status reports the skip, push exits 1 naming .gitconfig, and a consumer's init gets consumer@example.com, not the pusher's email (ROADMAP SC4)"
    requirement: SEC-04
    verification:
      - kind: e2e
        ref: "tests/e2e_lifecycle.rs#test_push_skips_edit_over_static_template_with_logic"
        status: pass
    human_judgment: false
  - id: D3
    description: "A local deletion never deletes a static template with logic, nor a non-UTF-8 .jinja template with logic (D-14); the same non-UTF-8 template without a marker is still deleted, and a plain template is still deleted"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_push_never_deletes_static_template_with_logic"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_push_never_deletes_non_utf8_jinja_template_with_logic"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_push_deletes_non_utf8_jinja_template_without_logic"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_push_local_deletion"
        status: pass
    human_judgment: false
  - id: D4
    description: "Placeholder-only static templates, the commit-msg ${#...} shape, non-UTF-8 non-.jinja templates and unreadable templates get the right gate-2 answer (logic table); placeholder-only edits still publish with the placeholder intact"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#test_template_has_logic"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#test_push_publishes_over_placeholder_only_static_template"
        status: pass
      - kind: e2e
        ref: "tests/e2e_lifecycle.rs#test_push_publishes_edit_over_placeholder_only_static_template"
        status: pass
      - kind: e2e
        ref: "tests/e2e_lifecycle.rs#test_push_refuses_unrenderable_parameterized_hook"
        status: pass
    human_judgment: false
  - id: D5
    description: "Every cargo-mutants mutant of template_has_logic is caught by the seven scoped tests; phase final gates pass"
    verification:
      - kind: other
        ref: "cargo mutants --file src/sync/mod.rs --re template_has_logic (7 caught, 0 missed)"
        status: pass
      - kind: other
        ref: "hermeticity gate: GIT_DIR=<decoy> GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 cargo test --bin ude sync::"
        status: pass
    human_judgment: false

duration: 14min
completed: 2026-10-27
status: complete
---

# Phase 02 Plan 10: Push Gate 2 on the Shared Render Rule Summary

**Push gate 2 (`template_has_logic`) now protects every template init renders: an edit or deletion over a non-`.jinja` template holding `{{ git_email }}` is skipped, so the pusher's email never replaces the placeholder and each consumer's init substitutes its own; non-UTF-8 templates keep exactly the previous answer (Phase 01 D-14).**

## Performance

- **Duration:** 14 min
- **Started:** 2026-10-27T18:03:40Z
- **Completed:** 2026-10-27T18:18Z
- **Tasks:** 3
- **Files modified:** 2 (src/sync/mod.rs, tests/e2e_lifecycle.rs)

## Accomplishments

- Verification gap 2 (review CR-02, ROADMAP SC4) closed at the code level: `template_has_logic` reads every mapped template; a UTF-8 template is checked when `template::should_render` selects it (operator option A), so init and push protect the same set.
- Phase 01 D-14 unchanged for non-UTF-8 templates: a `.jinja` template whose raw bytes hold a marker after placeholder removal is still never overwritten or deleted; a non-UTF-8 non-`.jinja` template still has no logic.
- No caller changed: `src/main.rs` (status hint, deletion hint, CLI messages) and `src/template/mod.rs` are untouched; status now reports the skip because it already routes through `template_has_logic`.
- Pending todo "Template-logic push guard only checks .jinja files" resolved with `gsd-tools todo complete` (`{"completed": true, "date": "2026-10-27"}`); moved to `.planning/todos/completed/`.

## Task Commits

1. **Task 1 (tracer): status and push skip an edit over a static template with `{{ git_email }}`**
   - RED `126d5ea` test(02-10): add failing tests for push over a static template with logic
   - GREEN `a30c969` feat(02-10): decide push gate 2 with the shared render rule
2. **Task 2: pin the rule in both directions** - `5a60084` test(02-10) (passed in RED by design; no GREEN change needed)
3. **Task 3: mutation check and final gates** - `a08e439` test(02-10): require an unreadable template to be an error in gate 2 (kills the one missed mutant)

**Plan metadata:** recorded in the docs(02-10) commit that adds this file.

## RED Evidence

Records are in `target/gsd-red/02-10-*.json` (gitignored), built from the nextest JUnit report, each classified with `gsd-tools check tdd-red-evidence`.

- **Task 1, `sync::tests::test_push_skips_static_template_with_logic`.** `RED_EVIDENCE_OK` / `target_test_failed`. Failed on its first planned assertion: `skipped_templates` was `[]`, expected `[".gitconfig"]` (gate 2 let the edit through).
- **Task 1, `test_push_skips_edit_over_static_template_with_logic` (e2e).** `RED_EVIDENCE_OK`. Failed on the status-hint assertion: `! .gitconfig: Locally Modified (ready to push)`. Because the first failure masks parts (b) and (c), a throwaway probe test (removed before the commit, never committed) ran the same setup without the status/push assertions on the pre-GREEN code: push exited 0, the template then held `tester@example.com`, and the consumer's init got `email = tester@example.com`. So all three parts reproduce the gap.
- **Task 2: all six tests passed in RED, by design** (`test_template_has_logic` with its new rows, `test_push_never_deletes_static_template_with_logic`, `test_push_never_deletes_non_utf8_jinja_template_with_logic`, `test_push_deletes_non_utf8_jinja_template_without_logic`, `test_push_publishes_over_placeholder_only_static_template`, e2e `test_push_publishes_edit_over_placeholder_only_static_template`). Each was then shown to fail under the wrong rule it guards against (temporary edits of `template_has_logic`, restored byte-identical with `cmp`):
  - **Old rule (`.jinja` only, lossy decode):** fails `test_template_has_logic` (at the `git_email` row), `test_push_never_deletes_static_template_with_logic`, `test_push_skips_static_template_with_logic`; the non-UTF-8 D-14 pair and the placeholder control pass. This matches the plan's claim that only the UTF-8 static true rows differ from the old rule.
  - **"Non-UTF-8 has no logic":** fails `test_template_has_logic` (at the non-UTF-8 `.jinja` row) and `test_push_never_deletes_non_utf8_jinja_template_with_logic` (`skipped_templates` `[]`); its negative control still deletes.
  - **No placeholder removal (over-broad):** fails `test_template_has_logic` and `test_push_publishes_over_placeholder_only_static_template`; the e2e placeholder control also fails (status says `push skips it: its template has logic`).
- **Task 3, mutant-killing row in `test_template_has_logic`.** `RED_EVIDENCE_OK` with the missed mutant applied by hand (row "template path that is a directory" failed); passes on the real code.

## Mutation Check (Task 3)

- `cargo mutants --list --file src/sync/mod.rs --re template_has_logic`: **7 mutants**.
- First run: 6 caught, **1 missed** (`replace match guard err.kind() == std::io::ErrorKind::NotFound with true`): no test produced a read error other than NotFound, so an unreadable template reading as "no logic" went unnoticed. Exit 2, about 71 s.
- Fix: a new row in `test_template_has_logic` (no new test function, so the seven-test filter and the baseline count stay as planned): a template path that is a directory must give `Err`. This kills that mutant (shown RED under it, above).
- Re-run: **7 caught, 0 missed, 0 unviable, 0 timeouts, exit 0**, 78 s for the mutants (wall time under 81 s, including a 53 s baseline build). Result check passes: baseline ran exactly the 7 named tests. Its negative control (same check expecting 6) exits 1.
- `git diff --cached --quiet` exits 0 and `git config --local --list` equals the copy saved before the run.

## Final Phase Gates

| Gate | Result |
|------|--------|
| `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test` | exit 0; 218 unit passed (1 ignored), 88 e2e passed |
| Hermeticity (decoy `GIT_DIR`, `GIT_CONFIG_GLOBAL=/dev/null`, `cargo test --bin ude sync::`) | exit 0; 138 passed; decoy repo has 0 commits afterwards |
| `cargo deny check advisories bans sources` | exit 0; advisories ok, bans ok, sources ok |
| `cargo machete` | exit 0; no unused dependencies |
| `git diff --exit-code -- Cargo.toml Cargo.lock` | exit 0 |
| `test -e .cargo/mutants.toml` | exit 1 (absent, as required) |

Scratch paths removed: target/mutants-logic, target/mutants-logic-tmp, target/mutants-logic-run.log, target/mutants-logic-run.exit, target/mutants-logic-config.before, target/hermetic-gate (checked absent afterwards).

## What These Checks Do Not Establish

- **The unit tests render with the host's git email**, or `developer@example.com` under the hermeticity gate's empty global config, because `detect_for_project` cannot be injected. The hermetic proof with a fixed email is the e2e test (fixture `tester@example.com`, consumer `consumer@example.com`).
- **Only `.gitconfig` is exercised end to end.** Hooks and `scripts/check-in-container.sh` reach gate 2 through the same `map_to_upstream` and `template_has_logic` path; that is covered only by the `pre-push` logic-table row, not a CLI test.
- **Non-UTF-8 equivalence with the old rule** rests on the argument in the doc comment (every placeholder and marker is ASCII), three non-UTF-8 logic-table rows and the two non-UTF-8 push tests. I did not repeat the planner's fuzz comparison.
- **The mutation check covers only the 7 mutants cargo-mutants generates in `template_has_logic`**, against seven scoped tests. It says nothing about the callers in `src/main.rs` or about gate 2's position in `push_updates`.
- **Requirement status:** SEC-04 is listed in `requirements-completed` because the plan declares it. REQUIREMENTS.md was deliberately not marked: the verifier decides after re-verification, following the operator's revert in aafd0a1.

## Files Created/Modified

- `src/sync/mod.rs`: `template_has_logic` rewritten (`///` doc rewritten; reads first, then uses `should_render` for UTF-8 and the `.jinja` suffix for non-UTF-8, then removes placeholders and checks markers on raw bytes); test helpers `record_rendered_gitconfig` and `record_deleted_non_utf8_mise`; five new push tests; logic-table rows.
- `tests/e2e_lifecycle.rs`: `EMAIL_PLACEHOLDER`, `test_push_skips_edit_over_static_template_with_logic`, `test_push_publishes_edit_over_placeholder_only_static_template`.

## Decisions Made

- The local in `template_has_logic` is named `guarded`, not `rendered`, because for a non-UTF-8 `.jinja` template it means "D-14 protects it", not "it is rendered".
- The unreadable-template row uses a directory at a `.repo` path, so it also shows that non-`.jinja` templates are now read (T-02-36: fails closed with `Err`).
- The two-part `push` / consumer probe was a throwaway, never committed, rather than a permanent test: the committed e2e test already asserts both parts after GREEN.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Test gap] Missed mutant in the first mutation run**
- **Found during:** Task 3
- **Issue:** The `NotFound` match-guard-to-`true` mutant survived: no scoped test produced a non-NotFound read error.
- **Fix:** Added the planned kind of fix: a row in `test_template_has_logic` asserting `Err` for a template path that is a directory. Committed as `a08e439`.
- **Verification:** RED under the hand-applied mutant, GREEN on the code, re-run 7/7 caught.

**2. [Process - known GSD defect] Milestone line re-added by GSD state writers**
- **Found during:** state updates after this SUMMARY
- **Issue:** The gsd-tools STATE writers re-add `milestone: v8.30.1` to STATE.md frontmatter and set `"milestone": "v8.30.1"` in state.json.
- **Fix:** Deleted the STATE.md line and set state.json `milestone` to null before the metadata commit; confirmed `rg -c v8.30.1 .planning/STATE.md .planning/state.json` exits 1. The orchestrator logs the incident.

### Minor setup differences (no behavior change)

- `test_push_never_deletes_static_template_with_logic` never writes the local `.gitconfig`, instead of writing and then removing it. The scan precondition asserts `LocalDeleted` either way.

---

**Total deviations:** 1 test-gap auto-fix (planned procedure), 1 known GSD-defect correction.
**Impact on plan:** None on scope. No production file other than src/sync/mod.rs changed.

## Issues Encountered

- **The RED commit's gate was not exit-code clean.** The RED commit (`126d5ea`) was gated on `cargo fmt --check && cargo build --all-targets 2>&1 | tail -2`, so the chain saw tail's exit code, not the build's. The build did finish (`Finished dev profile` printed), and every later commit was gated on exit codes without a pipe.
- The first fmt check after GREEN failed because the `.map` closure needed reformatting. I ran `cargo fmt` and re-ran the full chain before committing.

## TDD Gate Compliance

RED `126d5ea` (test) precedes GREEN `a30c969` (feat), and GREEN descends from RED (`git merge-base --is-ancestor` exit 0). Task 2 and Task 3 add test-only commits (pins and a mutant-killing row); no REFACTOR commit.

## User Setup Required

None.

## Next Phase Readiness

- This is the last plan of phase 02. Both verification gaps now have code and tests behind them. The phase needs re-verification (`/gsd-verify-work 02`) before SEC-03/SEC-04 are marked.
- Review WR-02 (merge HEAD or an extra commit at the post-commit gate) is still not planned and has no operator decision (out of scope here).

## Self-Check: PASSED

- src/sync/mod.rs and tests/e2e_lifecycle.rs exist and hold the named tests (`test_push_skips_static_template_with_logic`, `test_push_skips_edit_over_static_template_with_logic`).
- Commits 126d5ea, a30c969, 5a60084, a08e439 are ancestors of HEAD; `git rev-list --count 03eb477..a08e439` = 4.
- `git diff --stat 03eb477..a08e439 -- src/main.rs src/template/mod.rs` is empty.

---
*Phase: 02-sanitizer-boundaries-security-hardening*
*Completed: 2026-10-27*
