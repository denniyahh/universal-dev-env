---
phase: 02-sanitizer-boundaries-security-hardening
plan: 07
subsystem: security
tags: [sanitizer, path-parameterization, boundaries, proptest, cargo-mutants, SEC-03, SEC-04]

requires:
  - phase: 02-sanitizer-boundaries-security-hardening (02-05)
    provides: scan_bytes facade in src/sanitizer/mod.rs (unchanged here); push gate 4 scans the parameterized bytes
provides:
  - parameterize_paths on a single match_indices pass with hand-written leading and trailing boundary predicates
  - is_parameterizable_root guard (no parameterization for "", ".", relative, "/", "//")
  - Boundary table, degenerate-home, encoding, non-UTF-8 push and two property tests
  - Mutation gate over the five boundary functions (34 mutants, 34 caught)
  - docs/ARCHITECTURE.md placeholder list matching what push writes
affects: [02-08, phase-02 verification, home-path placeholder todo]

actuals:
  tokens: 5950   # chars/4 over the realized diff e4ac964..e26ebdd (4959 over added lines only)
  tasks: 3
  commits: 6
plan_head_before: e4ac964912ad9a3c2a7a4c500baaafc3dcf89cfa
plan_head_after: e26ebdde1a7fb23d00bb2486c05991a116e58965

tech-stack:
  added: []
  patterns:
    - "Boundary predicates read neighbours from the input, never from the output, so a replacement cannot create a boundary for the next match"
    - "Table tests collect every failing row and report them all in one assertion"
    - "Property companions: each no-change property is paired with a case that must change"

key-files:
  created: []
  modified:
    - src/sanitizer/mod.rs
    - src/sync/mod.rs
    - docs/ARCHITECTURE.md

key-decisions:
  - "02-07: a home or XDG path is replaced only when both boundaries hold; a leading '-' counts only after ':' (shell default), a trailing '.' only when not followed by a word char, '_' or '-'"
  - "02-07: parameterize_paths skips a home or XDG path that is not absolute with a normal component (the '.' no-home fallback, HOME=/)"
  - "02-07: no xdg.config alias; ARCHITECTURE.md lists only the two placeholders push writes"

patterns-established:
  - "Mutation gate per hand-written predicate, scoped by --re to the predicate functions and by test filter to the module's tests"

requirements-completed: [SEC-03, SEC-04]

coverage:
  - id: D1
    description: "Push publishes (, ), comma, :-, } and sentence-final-. delimited home paths as {{ user.home }}, leaves neighbouring paths (alice2, .bak, -old, /backup/..., /backup-/...) byte-identical, and the file scans In Sync"
    requirement: SEC-03
    verification:
      - kind: integration
        ref: "src/sync/mod.rs#test_push_parameterizes_punctuation_delimited_home_paths"
        status: pass
      - kind: integration
        ref: "src/sync/mod.rs#test_defect3_path_parameterization_roundtrip_static_template"
        status: pass
    human_judgment: false
  - id: D2
    description: "Boundary table (23 rows) for home /home/alice: 12 replaced, 11 untouched, including /backup-/home/alice/x and x-/home/alice"
    requirement: SEC-03
    verification:
      - kind: unit
        ref: "src/sanitizer/mod.rs#test_path_boundary_table"
        status: pass
      - kind: unit
        ref: "src/sanitizer/mod.rs#test_path_parameterization_boundary_safety"
        status: pass
      - kind: unit
        ref: "src/sanitizer/mod.rs#test_path_parameterization_repeated_adjacent_paths"
        status: pass
    human_judgment: false
  - id: D3
    description: "A degenerate home or XDG path (empty, '.', relative, '/', '//') rewrites nothing; encoding rows (Unicode neighbours, no NFC/NFD normalization); a non-UTF-8 file is published byte for byte"
    requirement: SEC-03
    verification:
      - kind: unit
        ref: "src/sanitizer/mod.rs#test_degenerate_home_not_parameterized"
        status: pass
      - kind: unit
        ref: "src/sanitizer/mod.rs#test_path_encoding_rows"
        status: pass
      - kind: integration
        ref: "src/sync/mod.rs#test_push_never_parameterizes_non_utf8_file"
        status: pass
    human_judgment: false
  - id: D4
    description: "No partial-substring corruption on either side (property with companions) and parameterize-then-render returns the input, with a cross-machine control (property)"
    requirement: SEC-04
    verification:
      - kind: unit
        ref: "src/sanitizer/mod.rs#prop_no_partial_substring_corruption"
        status: pass
      - kind: unit
        ref: "src/sanitizer/mod.rs#prop_parameterize_render_roundtrip"
        status: pass
    human_judgment: false
  - id: D5
    description: "Mutation gate over parameterize_paths, replace_bounded, is_leading_boundary, is_trailing_boundary, is_parameterizable_root: 34 mutants, 34 caught, 0 missed, 0 timeouts, 0 unviable"
    requirement: SEC-03
    verification:
      - kind: other
        ref: "02-07-PLAN Task 3 run, reproduction and check commands (exit 0, 0, 0)"
        status: pass
    human_judgment: false
  - id: D6
    description: "docs/ARCHITECTURE.md lists only {{ user.home }} and {{ user.xdg_config }}"
    requirement: SEC-03
    verification:
      - kind: other
        ref: "02-07-PLAN Task 2 doc check command (exit 0; exit 1 on the old doc)"
        status: pass
    human_judgment: false

duration: 17min
completed: 2026-10-27
status: complete
---

# Phase 2 Plan 07: Bounded Home-Path Parameterization Summary

**`ude sync push` now replaces the pusher's home and XDG config paths only at word and punctuation boundaries on both sides. It uses a single `match_indices` pass with hand-written predicates, with no regex and no fixpoint loop. `)`, `,`, `;`, `}`, `:-` and a sentence-final `.` delimit the path; neighbouring paths stay byte-identical; a degenerate home rewrites nothing. A parameterize-then-render property proves the round trip.**

## Performance

- **Duration:** 17 min
- **Started:** 2026-10-27T00:57:45Z
- **Completed:** 2026-10-27T01:14:30Z
- **Tasks:** 3
- **Files modified:** 3

## Accomplishments

- The pending todo's delimiter half is fixed. Before this plan, `(/home/alice)`, `${H:-/home/alice}`, `${W:-/home/alice/Github/x}`, `see /home/alice.`, `/home/alice,`, `a;/home/alice;b` and `/home/alice}` were published with the raw path, and so with the username. All of them now publish `{{ user.home }}`.
- Neighbouring paths are never rewritten: `/home/alice2`, `/home/alice.bak`, `/home/alice-old`, `/home/alice_x`, `/backup/home/alice`, `./home/alice`, `/backup-/home/alice/x` and `x-/home/alice`.
- The RED run found a defect the plan did not list: the old fixpoint loop turned `/home/alice/home/alice` into `{{ user.home }}{{ user.home }}`. After the first replacement, the `}` it left became a boundary for the second occurrence. The new pass reads boundaries from the input only, giving `{{ user.home }}/home/alice`.
- A home or XDG path of `""`, `.`, a relative path, `/` or `//` is never parameterized. `.` is the no-home fallback in `src/main.rs` and in `TemplateContext::detect_for_project`.
- `use regex::Regex;` is gone from `src/sanitizer/mod.rs`.

## Task Commits

1. **Task 1: Tracer - push publishes punctuation-delimited home paths as placeholders** (TDD)
   - RED: `95cfe96` test(02-07): add failing tests for bounded home-path parameterization
   - GREEN: `da5d005` feat(02-07): parameterize home paths only at word and punctuation boundaries
   - Tracer feedback gate (interactive, `end-of-phase`, automated-only verify): verify re-run on the committed tree, 6/6 pass, expanded.
2. **Task 2: Degenerate-home guard, encoding rows, properties, doc fix** (TDD)
   - RED: `3e71f66` test(02-07): add degenerate-home, encoding and round-trip property tests
   - GREEN: `cb588c1` feat(02-07): never parameterize a degenerate home and fix placeholder doc
3. **Task 3: Mutation gate over the boundary functions**
   - `191469d` test(02-07): kill surviving trailing-dot boundary mutants
   - No production change, so no GREEN commit.
- Extra (Rule 2): `e26ebdd` test(02-07): pin that push never parameterizes a non-UTF-8 file
- REFACTOR: none needed.

## RED Evidence

**Task 1:** `cargo nextest run --no-fail-fast --bin ude test_push_parameterizes_punctuation_delimited_home_paths test_path_boundary_table` exited 100 (JUnit, 2 run, 2 failed).
- `test_path_boundary_table`: 8 of 20 rows failed, each named in the message. Seven were delimiter rows that were not replaced; the eighth was the `/home/alice/home/alice` double replacement described above.
- Push tracer: push returned `Success`. The published text kept the raw path on lines b-e. Line a was already replaced today, as the RESEARCH probe predicted.
- Classifier: `RED_EVIDENCE_OK` / `target_test_failed` for both targets (records `target/gsd-red/02-07-t1-table.json` and `02-07-t1-push.json`, gitignored).
- Negative control: the same report with the non-failing target `test_path_parameterization_boundary_safety` gives `INVALID_RED` / `no_target_test_failure`.
- Semantic assessment: valid RED. Both targets executed and failed on their planned assertions, not on setup.

**Task 2:** the plan's 4-test command exited 100 (4 run, 1 failed, 3 passed).
- `test_degenerate_home_not_parameterized` failed on home `.` (`./z` became `{{ user.home }}/z`). Classifier `RED_EVIDENCE_OK` (`target/gsd-red/02-07-t2-degenerate.json`).
- The `/` and `//` rows already passed under Task 1's trailing rule: a `/` is always followed by a word char in that content. The plan's "RED today" note on `/` described the old regex.
- `test_path_encoding_rows`, `prop_no_partial_substring_corruption` and `prop_parameterize_render_roundtrip` passed in RED by design: they pin Task 1's predicate.
- Because they passed in RED, I proved they bite with four planted faults, each restored byte-identical (checksum) afterwards:

| Planted fault | Caught by |
|---|---|
| every leading `-` is a boundary | `prop_no_partial_substring_corruption` and `test_path_boundary_table` |
| a trailing `.` is always a boundary | `prop_no_partial_substring_corruption` |
| the XDG pass emits the home placeholder | `prop_parameterize_render_roundtrip` |
| the cross-machine control renders with the same home (test-side fault) | `prop_parameterize_render_roundtrip`; the control is reachable and can fail |

## Mutation Gate (Task 3)

- **Listed:** 34 mutants (`--list` with the plan's `--re`), over all five functions.
- **Run 1:** exit 2, 31 caught, 3 missed, 0 timeouts, 0 unviable, about 2 min.
  - The missed mutants were all on the `.` arm of `is_trailing_boundary` (line 122):
    - `replace || with && in is_trailing_boundary` (122:63)
    - `replace == with != in is_trailing_boundary` (122:56)
    - `replace == with != in is_trailing_boundary` (122:68)
  - No row had a `.` followed by `_`, `-` or a non-word char other than end of text.
- **New tests:** three boundary-table rows in `test_path_boundary_table`, each shown RED against its mutants and GREEN on the code:

| New row | Kills |
|---|---|
| `"see /home/alice. Then"` -> replaced | 122:56, 122:68 |
| `"/home/alice._x"` -> untouched | 122:63, 122:56 |
| `"/home/alice.-x"` -> untouched | 122:63, 122:68 |

- **Run 2:** exit 0, `Found 34 mutants to test`, 34 caught, 0 missed, 0 timeouts, 0 unviable, about 2 min wall time (01:08:55Z to 01:10:54Z).
- **Reproduction command:** run verbatim. It exited 0 with "no timeouts to reproduce (gate run exit 0)". There are no hang mutants to name.
- **Check command:** exit 0. The baseline ran 19 tests, all in `sanitizer::tests::`; caught.txt and outcomes.json both report 34. Negative control: the same check on run 1's outputs (recorded exit 2) exits 1.
- **Exclusions:** none. No `.cargo/mutants.toml` exists. `git diff --exit-code -- Cargo.toml Cargo.lock` exits 0.
- **Hazard checks after the runs:** `git status --porcelain` showed only the intended test edit, and `git config --local --list` was identical to the copy captured before the plan.
- **Scratch paths removed:** `target/mutants-boundaries`, `-tmp`, `-run.log`, `-run.exit`, `-repro.exit` (the solo, solo.log and hangs paths were never created); `ls target/mutants-boundaries*` finds nothing.

## Files Created/Modified

- `src/sanitizer/mod.rs`:
  - `is_leading_boundary`, `is_trailing_boundary`, `is_parameterizable_root`, `replace_bounded`
  - `parameterize_paths` rewritten on top of them, without the regex import
  - 4 new tests and 2 properties; 3 mutant-killing rows
- `src/sync/mod.rs`:
  - `test_push_parameterizes_punctuation_delimited_home_paths`
  - `test_push_never_parameterizes_non_utf8_file` with its helper `push_gitconfig_as_alice`
- `docs/ARCHITECTURE.md`: section 3 item 2 lists only `{{ user.home }}` and `{{ user.xdg_config }}`, plus one sentence saying push writes no others.

## Decisions Made

- Predicates as planned, read from the input text; `match_indices` drives the walk, so no loop-advance (hang) mutant exists.
- The predicates are written inline (no shared `is_word_char` helper), so every mutable expression sits inside the five functions the gate mutates.
- The degenerate-home guard keeps the plan's existing `!xdg_config.is_empty()` condition alongside `is_parameterizable_root`. It is redundant for correctness, but it is not an equivalent mutant (run 2 caught every mutant on that line).

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing critical] Test for "non-UTF-8 files are never parameterized"**
- **Found during:** final verification of the must-have truths
- **Issue:** The truth is held by push gate 3's `from_utf8` branch, but no test pinned it. The existing non-UTF-8 push tests cover secret blocking only.
- **Fix:** Added `test_push_never_parameterizes_non_utf8_file`: a Latin-1 `.gitconfig` holding `/home/alice` is published byte for byte. The control is the same text in UTF-8, which is parameterized. This is a characterization test (it passes on the code by design). A planted fault, lossy parameterization of non-UTF-8 text, makes it fail; the file was restored byte-identical.
- **Files modified:** src/sync/mod.rs
- **Committed in:** e26ebdd

**2. [Rule 2 - Privacy] Synthetic example path in ARCHITECTURE.md**
- **Found during:** Task 2
- **Issue:** The item-2 line being rewritten named the operator's real home path as its example.
- **Fix:** It now uses `/home/alice`, `C:\Users\alice` and `/Users/alice`. The doc no longer contains the operator's username.
- **Committed in:** cb588c1

**3. [Process] GSD state verbs scrape a milestone into STATE.md / state.json (known GSD bug)**
- After the state updates, I deleted any `milestone: v8.30.1` line from STATE.md and set `"milestone": null` in state.json, as the orchestrator mandated. The orchestrator logs the incident.

**4. [Process] Planted-fault runs left `proptest-regressions/`**
- Proptest wrote failure seeds for the planted faults to `proptest-regressions/sanitizer/mod.txt`. I created that file (timestamp checked; no tracked or ignored copy existed) and removed it.

---

**Total deviations:** 2 auto-fixed (Rule 2), plus 2 process notes.
**Impact on plan:** No scope change to production code beyond the plan. One extra test and a doc example edit.

## Issues Encountered

- **A trailing `.` followed by `.` or `/` is a boundary, as the plan's truth states.** As a result `/home/alice..bak` and `/home/alice./x` become `{{ user.home }}..bak` and `{{ user.home }}./x` (probe output, temporary test removed).
  - Those are paths in a different directory (`alice..bak`, `alice.`), so this is a narrow conflict with the "never rewrite a foreign path" prohibition.
  - I kept the plan's stated rule and surfaced the choice to the orchestrator instead of changing a contract the plan spells out.
  - The fix is one predicate: treat `.` as a boundary only when the next char is not `.` or `/` either. Without the change, such a path renders as another user's path on another machine; with it, the shape is published raw and keeps the username.

## What These Checks Do Not Establish

- **Alias home spellings are still published as-is**, as are `-I/home/<user>/include` and the colon-less `${VAR-/home/<user>}` (T-02-21, accepted). Example alias: `/home/<user>` when `$HOME` is `/var/home/<user>`. The pending todo "Home-path placeholders leak the pusher's home path" is re-checked: the delimiter half is fixed, the alias half is open.
- **Overlapping occurrences:** `match_indices` returns non-overlapping matches. A rejected match can hide an overlapping occurrence that starts inside it. That needs a home path whose own text repeats with a non-word char before the repeat, which no realistic home path has. Untested.
- **`prop_no_partial_substring_corruption` is false on a vanishingly small slice of its own suffix space:** a suffix such as `x//home/<name>` contains a second, `/`-delimited home occurrence, which is correctly replaced. At roughly (1/66)^8 per case it is not hit in practice, so a clean run does not exercise it.
- **Properties run proptest's default 256 cases per run.** They bound the generated shapes, not all text. The planted faults show the properties detect the fault classes listed above, not every fault.
- **The mutation gate covers only the five boundary functions,** tested by `sanitizer::tests::`. It says nothing about push gate 3's call site, which the push tracer and the non-UTF-8 test cover instead.
- **Windows:** `is_parameterizable_root` uses `Path::is_absolute`, which on Windows needs a drive prefix. That is unverified there (Phase 999.1).

## User Setup Required

None. No external service configuration is required.

## Next Phase Readiness

- Plan 02-08 (the push gate refusing `{{` in local text) builds on the same `PARAMETERIZED_PLACEHOLDERS` pair; parameterize_paths emits exactly those two strings.
- One choice is open for the orchestrator/operator: the trailing `.` before `.` or `/` (Issues Encountered).

## Self-Check: PASSED

- FOUND: src/sanitizer/mod.rs, src/sync/mod.rs, docs/ARCHITECTURE.md.
- FOUND (ancestors of HEAD): 95cfe96, da5d005, 3e71f66, cb588c1, 191469d, e26ebdd. Negative control: the bogus hash `deadbee` is reported MISSING.
- Evaluation scope (`check evaluation-scope --plan 02-07 --commits-only`): resolved; plan commits on this branch.
- Acceptance re-run:
  - Task 1: 6/6 named tests pass. The structural check exits 0 (1 on HEAD's old code); the regex-import check exits 0 (1 on the old code).
  - Task 2: 5/5 pass. `fn is_parameterizable_root` has 1 match. The doc check exits 0 (1 on the old doc).
  - Task 3: run, reproduction and check commands exit 0.
  - Full gate after the last commit: `cargo fmt --check`, `clippy -D warnings` and `cargo test` (199 unit + 82 e2e) pass. `cargo deny check` and `cargo machete` (output has `Analyzing dependencies`) pass.
- TDD gates: a `test(02-07)` RED commit precedes each `feat(02-07)` GREEN commit (95cfe96 -> da5d005, 3e71f66 -> cb588c1).
- No `.unwrap()` in non-test code of src/sanitizer/mod.rs. Comments were stripped, the first `#[cfg(test)]` is at line 311 of 862, and a planted `.unwrap()` trips the check.

## Post-plan amendment (2026-10-27): trailing-dot rule

**Defect.** `is_trailing_boundary` treated a `.` right after the home path as a boundary whenever the next char was not alphanumeric, `_` or `-`. So `/home/alice..bak` became `{{ user.home }}..bak` and `/home/alice./x` became `{{ user.home }}./x`. Those name other directories (`alice..bak`, `alice.`), which broke the rule that only the pusher's own home is rewritten. This was the open choice under Issues Encountered above.

**Operator decision.** Tighten the rule. A `.` is a trailing boundary only at the end of the text, or when the next char is not alphanumeric, `_`, `-`, `.` or `/`.
- Accepted cost: these rare shapes are published raw, with the username in them. One example is a sentence ending in an ellipsis right after the path (`/home/alice...`).

**Commits.**
- `2a2a78b` test(02-07): pin that a dot before a dot or slash is not a boundary (RED).
  - Adds 7 boundary-table rows (29 rows total).
  - Must stay byte-identical: `/home/alice..bak`, `/home/alice./x`, `/home/alice.../y`.
  - Must still be rewritten (sentence-final): `/home/alice. next`, `/home/alice.)`, `/home/alice.\n`. The `see /home/alice.` and `see /home/alice. Then` rows already existed.
- `3c99371` fix(02-07): treat a dot before a dot or slash as part of the path (GREEN).
  - Adds `.` and `/` to the `.` arm of `is_trailing_boundary`.
  - Updates its doc comment and the `parameterize_paths` doc.
  - No file under docs/ describes the dot rule, so no doc file changed. `rg` over docs/, README.md and AGENTS.md found no such text.

**RED evidence.** `cargo nextest run --no-fail-fast --bin ude test_path_boundary_table` exited 100 (JUnit: 1 run, 1 failed).
- The message reads "3 of 29 boundary rows failed" and names exactly the three new dot-dot / dot-slash rows. The four sentence-final rows passed, as intended.
- Classifier: `RED_EVIDENCE_OK` / `target_test_failed`. The record is `target/gsd-red-amend/02-07-amend-table.json` (gitignored).
- Negative control: the same report with the target `sanitizer::tests::test_path_encoding_rows`, which did not run, gives `INVALID_RED` / `no_target_test_failure`.
- Semantic assessment: valid RED. The target ran and failed on its planned assertion, not on setup or compilation.

**Mutation gate (Task 3 re-run, same `-boundaries` paths and commands).**
- Listed: 38 mutants (34 before). The 4 new ones are the two new `||` and two new `==` operators in the `.` arm.
- Run: exit 0. `Found 38 mutants to test`: 38 caught, 0 missed, 0 timeouts, 0 unviable. Wall time about 4 min (08:20:23Z to 08:24:33Z). All four new mutants (123:75, 123:87, 123:80, 123:92) are in caught.txt.
- No survivors, so no new killing tests were needed.
- Reproduction command: a Claude Code safety check blocked the verbatim command, because it contains `rm`. Its run-exit-not-3 branch was run by hand: it printed "no timeouts to reproduce (gate run exit 0)" and recorded 0. That branch removes nothing, since neither the hangs file nor the solo directory existed.
- Check command: run verbatim, exit 0. The baseline ran 19 tests, all in `sanitizer::tests::`. caught.txt and outcomes.json both say 38.
- Exclusions: none. There is no `.cargo/mutants.toml`.
- Scratch paths removed: `target/mutants-boundaries`, `-tmp`, `-run.log`, `-run.exit`, `-repro.exit`. `ls target/mutants-boundaries*` finds nothing.
- After the run, `git status --porcelain` was clean and `git config --local --list` matched the copy taken before.

**Full gate after `3c99371`.** All of these exit 0:
- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test` (199 unit with 1 ignored, and 82 e2e)
- `cargo deny check`: advisories, bans, licenses and sources all ok
- `cargo machete`

`git diff --exit-code -- Cargo.toml Cargo.lock` also exits 0.

**What this does not establish.**
- The rows pin three dot-dot / dot-slash shapes and four sentence-final shapes. No property asserts that a `.` followed by `.` or `/` stays raw.
  - `prop_no_partial_substring_corruption`'s delimiter suffix has no `.`.
  - `prop_parameterize_render_roundtrip` can generate such text, but it passes under either rule.
  - Only the table rows and the mutation gate cover the new arm.
- A clean mutation run shows each operator in the five functions is pinned by some test. It does not show the rule is the right one for every text shape.

---
*Phase: 02-sanitizer-boundaries-security-hardening*
*Completed: 2026-10-27*
