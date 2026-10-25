---
phase: 02-sanitizer-boundaries-security-hardening
plan: 02
subsystem: templates
tags: [minijinja, init, render-rule, placeholder-roundtrip, ancestor-state, rust]

requires:
  - phase: 01-sync-engine-state-hermeticity
    provides: init materialization, kept_committed / kept_ancestor ancestor recording, missing-tracked-file notice, sync scan/pull render rule (.jinja or `{{`)
provides:
  - "template::should_render(path, text) -> bool: the single render rule (.jinja extension or text contains `{{`)"
  - "materialize_template in src/main.rs (replaces the byte-copy helper): UTF-8 text matching the rule is rendered with the machine's TemplateContext, everything else is copied byte for byte; a render failure stops init with exit 2 naming the template"
  - "kept_ancestor records the rendered template whenever should_render holds; a non-UTF-8 template is recorded raw"
  - "renders_template: the missing-tracked-file recovery hint picks its wording by should_render, no extension-only check left in src/main.rs"
affects: [02-08 push render-back check and sync scan/pull switch to template::should_render]

actuals:
  tokens: 4614       # chars/4 over the realized diff of src/ and tests/ (18457 chars)
  tasks: 2
  commits: 4         # git rev-list --count plan_head_before..plan_head_after
plan_head_before: c75ee46d4b21bc27305522de559bf584cb8c6f1d
plan_head_after: 4ed9bff186e75e09ed4bc815b49dfcfdd1b122e7

tech-stack:
  added: []
  patterns:
    - "One render rule (template::should_render) decides render vs byte copy at every init decision site: write, ancestor, recovery hint"
    - "Render only text that is UTF-8; non-UTF-8 template bytes are never rendered (matches scan and pull)"

key-files:
  created: []
  modified:
    - src/template/mod.rs
    - src/main.rs
    - tests/e2e_lifecycle.rs

key-decisions:
  - "Init renders a `{{`-bearing non-.jinja template with the initializing machine's context (real minijinja rendering, not string replacement), so a pushed `{{ user.home }}` comes back as the new machine's home"
  - "A template that fails to render stops init with exit 2 and `Failed to render template <src>` (pull's wording) instead of being copied raw"
  - "A non-UTF-8 .jinja template is now recorded raw as a kept ancestor instead of failing on a string read (consistent with scan, which never renders non-UTF-8)"
  - "The recovery hint reads the template only through refuse_symlink_under; an unreadable or symlinked template counts as empty text, so a .jinja template still gets the re-render wording"

patterns-established:
  - "Each init decision site calls template::should_render directly; plan 02-08 points sync scan, pull and push at the same function"

requirements-completed: [SEC-04]

coverage:
  - id: D1
    description: "A home path that push parameterized to `{{ user.home }}` in a shared static template is substituted back with the initializing machine's home when init materializes it on another project; a second machine gets its own home, not the pusher's"
    requirement: SEC-04
    verification:
      - kind: e2e
        ref: "tests/e2e_lifecycle.rs#test_push_then_init_substitutes_home_on_new_project"
        status: pass
      - kind: e2e
        ref: "tests/e2e_lifecycle.rs#test_init_renders_placeholders_in_static_templates_in_sync"
        status: pass
    human_judgment: false
  - id: D2
    description: "Templates without `{{` (hook with `${#FIRST_LINE}`) and non-UTF-8 templates holding `{{` bytes are copied byte for byte by init"
    requirement: SEC-04
    verification:
      - kind: e2e
        ref: "tests/e2e_lifecycle.rs#test_init_copies_templates_without_placeholders_byte_identical"
        status: pass
    human_judgment: false
  - id: D3
    description: "A `{{`-bearing template that cannot render stops init with exit 2 naming the template and writes no .gitconfig"
    requirement: SEC-04
    verification:
      - kind: e2e
        ref: "tests/e2e_lifecycle.rs#test_init_refuses_unrenderable_template"
        status: pass
    human_judgment: false
  - id: D4
    description: "A committed file init keeps records the rendered template as ancestor: an edit next to a placeholder reads Locally Modified, an unedited one reads In Sync"
    requirement: SEC-04
    verification:
      - kind: e2e
        ref: "tests/e2e_lifecycle.rs#test_init_kept_committed_file_records_rendered_ancestor"
        status: pass
      - kind: e2e
        ref: "tests/e2e_lifecycle.rs#test_init_kept_committed_file_equal_to_rendered_template_is_in_sync"
        status: pass
    human_judgment: false
  - id: D5
    description: "The missing-tracked-file recovery hint follows the render rule: a placeholder template gets the re-render wording, a plain one offers a copy"
    requirement: SEC-04
    verification:
      - kind: e2e
        ref: "tests/e2e_lifecycle.rs#test_missing_tracked_file_hint_for_rendered_static_template"
        status: pass
      - kind: e2e
        ref: "tests/e2e_lifecycle.rs#test_missing_tracked_file_hint_offers_copy_for_plain_template"
        status: pass
    human_judgment: false
  - id: D6
    description: "template::should_render pins the shared rule: .jinja true, `{{` true, `${#` without `{{` false, `{%` alone false"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/template/mod.rs#template::tests::test_should_render"
        status: pass
    human_judgment: false

duration: 7min
completed: 2026-10-25
status: complete
---

# Phase 02 Plan 02: Render Placeholders at Init Summary

**`ude init` now renders `{{ user.home }}` and other placeholders in shared non-`.jinja` templates through one rule, `template::should_render`, used for the written file, the kept-file ancestor and the recovery hint; templates without `{{` or not UTF-8 stay byte-identical.**

## Performance

- **Duration:** 7 min
- **Started:** 2026-10-25T18:30:38Z
- **Completed:** 2026-10-25T18:37:16Z
- **Tasks:** 2
- **Files modified:** 3

## Accomplishments

- ROADMAP success criterion 4 holds on initialization: a home path pushed as `{{ user.home }}` into `gitconfig.repo` comes back as the initializing machine's home on a new project. A second machine with a different HOME gets its own home, which shows the value comes from the machine running init, not from the pusher.
- `materialize_template` replaces the byte-copy helper at all 5 call sites (.gitconfig, .gitignore, hooks, check-in-container.sh, scratch scripts). It keeps the symlink refusal, the skip of a missing template and parent-directory creation.
- A committed `.gitconfig` that init keeps on a fresh clone now gets the rendered template as its ancestor. Before this change a committed edit next to a placeholder read `Conflicted`.
- No extension-only `.jinja` render decision is left in src/main.rs (2 at plan start). There are 3 `should_render(` call sites.

## Task Commits

1. **Task 1 (tracer): a pushed placeholder is substituted back by init**
   - RED `298f269` test(02-02): add failing tests for placeholder rendering at init
   - GREEN `0393e6b` feat(02-02): render placeholders when init materializes static templates
2. **Task 2: kept ancestor and recovery hint on the same rule**
   - RED `0993784` test(02-02): add failing tests for rendered kept ancestor and hint
   - GREEN `4ed9bff` feat(02-02): record rendered ancestor and hint by the shared render rule

No REFACTOR commits.

## TDD Evidence

RED evidence came from `cargo nextest run --no-fail-fast --test e2e_lifecycle <filters>` (JUnit report at target/nextest/default/junit.xml, fresh per run: report mtime after run start). Each target test got its own record, and each record went through `gsd-tools check tdd-red-evidence`.

**Task 1 RED (run exit 100):**
- test_push_then_init_substitutes_home_on_new_project: `RED_EVIDENCE_OK` / `target_test_failed`. Semantic check: it failed on the assertion that project_b/.gitconfig contains `<fake_home>/.gitconfig.local`. The file held the literal `{{ user.home }}` placeholder, which is the defect.
- test_init_renders_placeholders_in_static_templates_in_sync: `RED_EVIDENCE_OK` / `target_test_failed`. Semantic check: it failed on the same "contains rendered home" assertion.
- test_init_refuses_unrenderable_template: `RED_EVIDENCE_OK` / `target_test_failed`. Semantic check: it failed on the exit-code assertion (got 0, expected 2). Init copied the unrenderable text.
- test_init_copies_templates_without_placeholders_byte_identical **passed in RED by design**. It is the byte-identical control. Its record returned `INVALID_RED` / `no_target_test_failure`, which also serves as a negative control on the classifier. A mutation check showed this control catches blind rendering: with `should_render` forced to `true`, the test failed because init exited on `Failed to render template .../commit-msg: ... unexpected end of comment`. The mutation was reverted before commit.

**Task 2 RED (run exit 100):**
- test_init_kept_committed_file_records_rendered_ancestor: `RED_EVIDENCE_OK` / `target_test_failed`. Semantic check: it failed on `contains("Locally Modified")`. The status line read `.gitconfig: Conflicted (requires manual resolution)`.
- test_missing_tracked_file_hint_for_rendered_static_template: `RED_EVIDENCE_OK` / `target_test_failed`. Semantic check: it failed on `contains("init does not re-render its template")`. The hint read `... if it is committed, or copy templates/projects/generic/gitconfig.repo ...`.
- The two controls test_init_kept_committed_file_equal_to_rendered_template_is_in_sync and test_missing_tracked_file_hint_offers_copy_for_plain_template passed in RED, as expected. Both records returned `INVALID_RED` / `no_target_test_failure`.
- template::tests::test_should_render passed in RED, because the function already existed from Task 1. The plan anticipated this: the test pins the rule for plan 02-08.

**GREEN:** all 5 Task 1 verify tests and all 4 Task 2 verify tests pass, plus the 2 extra controls. `cargo nextest run --bin ude template::` ran 2 tests, both passing, including `test_should_render`.

## Verification Results (final, at 4ed9bff)

- `cargo fmt --check`: exit 0
- `cargo clippy --workspace --all-targets -- -D warnings`: exit 0
- `cargo test`: 151 unit tests passed (150 before this plan plus 1) and 81 e2e tests passed (73 pre-existing plus 8 new); 0 failed
- Render-rule check (`ext == "jinja"` absent, at least 3 `should_render(` in src/main.rs): exit 0. Negative control: `git show c75ee46:src/main.rs` has 2 `ext == "jinja"` matches, so the check would fail on the plan-start code.
- `rg -c 'materialize_template\(' src/main.rs` printed 6 (after Task 1), and `copy_template_file` has no matches. `rg -c 'fs::copy' src/main.rs` printed 1.
- `cargo deny check`: exit 0 (advisories, bans, licenses and sources ok). `cargo machete`: exit 0, and its output includes `Analyzing dependencies`.
- `git diff --exit-code -- Cargo.toml Cargo.lock`: exit 0. No dependency changed.
- No pre-existing e2e test changed outcome. All 73 passed before and after.

What these checks do not establish: the round trip is proven for `{{ user.home }}` in `gitconfig.repo` only. Other context variables and the other four static template paths are covered by the shared code path, not by a dedicated e2e test each. Sync scan and pull still use their own inline copies of the rule until plan 02-08.

## Files Created/Modified

- `src/template/mod.rs`: adds `pub fn should_render` and `test_should_render`.
- `src/main.rs`: adds `materialize_template` (replaces the byte-copy helper) and `renders_template` (the hint's rule check), and rewrites `kept_ancestor`.
- `tests/e2e_lifecycle.rs`: adds 8 e2e tests and the helpers `init_from`, `fake_home`, `kept_gitconfig_status_after_init` and `missing_gitconfig_recovery_hint`.

## Decisions Made

- I kept the render decision inline at each call site (materialize_template, kept_ancestor, renders_template) and did not factor it into a shared helper. The plan's acceptance check counts one `should_render(` call per decision site (at least 3), and I briefly tried a helper that merged two sites into one call. The duplicated part is a 6-line match with the same error wording.
- The hint's template read goes through `sync::refuse_symlink_under` and counts a refused or unreadable file as empty text. The plan specified only "content when it reads as UTF-8, empty otherwise". I added the symlink guard so the hint never follows a symlink under the template root, in line with every other template read in init.

## Deviations from Plan

**1. [Rule 2 - Test granularity] I split the negative controls into separate tests**
- **Found during:** Task 2
- **Issue:** The plan placed the `In Sync` control and the `or copy` control inside the kept-ancestor and hint tests. The operator's test rules require atomic tests, so a failure in step 1 cannot hide step 2.
- **Fix:** I added test_init_kept_committed_file_equal_to_rendered_template_is_in_sync and test_missing_tracked_file_hint_offers_copy_for_plain_template. They share setup helpers with the target tests.
- **Files modified:** tests/e2e_lifecycle.rs
- **Verification:** Both passed in RED, as controls should, and both pass in GREEN.
- **Committed in:** 0993784

**Total deviations:** 1 (test structure only). **Impact:** there are 8 new e2e tests instead of 6. No scope change.

## Issues Encountered

None.

## Known Stubs

None.

## User Setup Required

None. No external service configuration is required.

## Next Phase Readiness

- Plan 02-08 can switch the two inline render-rule copies in src/sync/mod.rs (scan and pull) to `template::should_render`, and use the same function for push's render-back check.
- src/sync/mod.rs was not touched (plan 02-01 owned it in wave 1).

## Self-Check: PASSED

- FOUND: src/template/mod.rs, src/main.rs, tests/e2e_lifecycle.rs
- FOUND commits in HEAD history: 298f269, 0393e6b, 0993784, 4ed9bff
- TDD gates: RED `test(02-02)` commits precede both GREEN `feat(02-02)` commits

---
*Phase: 02-sanitizer-boundaries-security-hardening*
*Completed: 2026-10-25*
