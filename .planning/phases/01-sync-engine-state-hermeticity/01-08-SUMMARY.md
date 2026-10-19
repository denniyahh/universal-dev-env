---
phase: 01-sync-engine-state-hermeticity
plan: 08
subsystem: sync
tags: [rust, diffy, merge, diff3, conflict-markers, proptest, cargo-mutants]
status: complete

requires:
  - phase: 01-05
    provides: fingerprint classifier (classify_file) that merges only when all three versions are text, LF-normalized inputs, destination EOL re-styling on write
  - phase: 01-07
    provides: deletion statuses, so absent sides never reach the merge as empty strings
provides:
  - "merge_texts: diffy =0.5.2 Diff3 merge with Git incomplete-hunk style; MergeOutcome::Clean / ::Conflict(marked text)"
  - "relabel_conflict_markers: exact-line rewrite of diffy's ours/original/theirs labels to LOCAL/ANCESTOR/UPSTREAM"
  - "ConflictSide + render_conflict_sidecar: pure `.conflict` body renderer (text diff3, explicit block for deletions, fingerprint-only for binary)"
  - "Adjacent-line edits now conflict on scan/pull (D-02)"
affects: [01-09, 01-10, 01-11]

actuals:
  tokens: 8611        # chars/4 over the realized diff a16b103..df500ae (34,446 bytes)
  tasks: 2
  commits: 4          # measured: git rev-list --count a16b103..HEAD before the SUMMARY commit
plan_head_before: a16b103b3943b0dd6a0e9b25d8153ea4246913e8
plan_head_after: df500aebc94ff4c15e8fc66f7d529fe907d79017

tech-stack:
  added: ["diffy =0.5.2 (transitive: foldhash)"]
  removed: ["similar 2.7"]
  patterns:
    - "Conflict labels are rewritten on exact full marker lines only; diffy has no label API"
    - "Sidecar rendering is a pure function of (path, ancestor, local, upstream); the caller writes the file"

key-files:
  created: []
  modified:
    - src/sync/merge.rs
    - src/sync/mod.rs
    - Cargo.toml
    - Cargo.lock

key-decisions:
  - "01-08: merge_texts uses diffy =0.5.2 with ConflictStyle::Diff3 and IncompleteHunkStyle::Git; adjacent-line edits conflict (D-02); similar removed"
  - "01-08: render_conflict_sidecar header order is file, resolve command, legend, then one note per absent/binary side in LOCAL, ANCESTOR, UPSTREAM order; any binary side suppresses the text body"
  - "01-08: three text sides that diffy merges cleanly still render as an explicit LOCAL/ANCESTOR/UPSTREAM block, so a sidecar always shows all versions"

patterns-established:
  - "Exact-output tests for every renderer variant, so the mutation gate's 10 viable mutants are all caught"

requirements-completed: [SYNC-05]

coverage:
  - id: D1
    description: "Scan and pull merge with diffy; adjacent-line edits are Conflicted, edits one unchanged line apart are Merged and pulled as a=2/x/b=2; repeated pulls stay idempotent"
    requirement: SYNC-05
    verification:
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_pull_adjacent_edits_conflict"
        status: pass
      - kind: unit
        ref: "src/sync/mod.rs#sync::tests::test_multi_pull_merge_idempotence"
        status: pass
      - kind: unit
        ref: "src/sync/merge.rs#sync::merge::tests::test_adjacent_edits_conflict"
        status: pass
    human_judgment: false
  - id: D2
    description: "Conflict text carries LOCAL / ANCESTOR / UPSTREAM markers that always start a line; only exact diffy marker lines are relabeled"
    requirement: SYNC-05
    verification:
      - kind: unit
        ref: "src/sync/merge.rs#sync::merge::tests::test_relabel_conflict_markers"
        status: pass
      - kind: unit
        ref: "src/sync/merge.rs#sync::merge::tests::test_merge_conflict_labels"
        status: pass
      - kind: unit
        ref: "src/sync/merge.rs#sync::merge::tests::test_merge_no_trailing_newline_markers_start_line"
        status: pass
      - kind: unit
        ref: "src/sync/merge.rs#sync::merge::tests::test_relabel_prop"
        status: pass
    human_judgment: false
  - id: D3
    description: "render_conflict_sidecar produces text, deletion (both directions plus never-synced) and binary sidecar bodies"
    requirement: SYNC-05
    verification:
      - kind: unit
        ref: "src/sync/merge.rs#sync::merge::tests::test_render_conflict_sidecar_text"
        status: pass
      - kind: unit
        ref: "src/sync/merge.rs#sync::merge::tests::test_render_conflict_sidecar_deleted_upstream"
        status: pass
      - kind: unit
        ref: "src/sync/merge.rs#sync::merge::tests::test_render_conflict_sidecar_binary"
        status: pass
    human_judgment: false
  - id: D4
    description: "Merge of CRLF orthogonal edits keeps CRLF; the same edits in LF contain no CR (SYNC-04 merge-level validation row)"
    verification:
      - kind: unit
        ref: "src/sync/merge.rs#sync::merge::tests::test_merge_crlf"
        status: pass
    human_judgment: false
  - id: D5
    description: "diffy pinned =0.5.2, similar removed; cargo deny advisories/bans/sources and cargo machete clean"
    verification:
      - kind: other
        ref: "cargo deny check advisories bans sources"
        status: pass
      - kind: other
        ref: "cargo machete (no unused dependencies reported)"
        status: pass
    human_judgment: false
  - id: D6
    description: "Mutation gate over src/sync/merge.rs scoped to sync::merge:: tests: 12 mutants, 10 caught, 0 missed, 2 unviable"
    verification:
      - kind: other
        ref: "cargo mutants --file src/sync/merge.rs -o target/mutants -- --bin ude -- sync::merge:: (exit 0) + plan check command (exit 0)"
        status: pass
    human_judgment: false

duration: 9min
completed: 2026-10-19
---

# Phase 01 Plan 08: diffy merge and conflict sidecar renderer Summary

**The hand-written merge and `similar` are replaced by diffy 0.5.2 (Diff3, Git incomplete hunks): adjacent-line edits now conflict, conflicts carry LOCAL / ANCESTOR / UPSTREAM markers that always start a line, and a pure `render_conflict_sidecar` produces the text, deletion and binary `.conflict` bodies that plan 01-09 will write.**

## Performance

- **Duration:** about 9 min
- **Started:** 2026-10-19T11:17:34Z
- **Completed:** 2026-10-19T11:26Z
- **Tasks:** 2 (Task 1 tracer, Task 2 auto; both TDD)
- **Files modified:** 4

## Accomplishments

- `merge_texts` (src/sync/merge.rs) wraps diffy `MergeOptions` with Diff3 and the Git incomplete-hunk style; the classifier in src/sync/mod.rs maps Clean to Merged and Conflict to Conflicted. The old merge enum, entry function, edit-overlap helpers and the `similar` dependency are gone.
- Adjacent-line edits conflict at both levels (`test_pull_adjacent_edits_conflict` through scan and pull; `test_adjacent_edits_conflict` at the merge level), each with a one-unchanged-line negative control that merges.
- `relabel_conflict_markers` rewrites only exact `<<<<<<< ours\n`, `||||||| original\n`, `>>>>>>> theirs\n` lines; a proptest (0-12 random lines) proves other content is unchanged and an inserted exact marker line is the only line relabeled.
- `render_conflict_sidecar` + `ConflictSide`: header (file, `ude sync resolve <file>`, legend, notes for absent or binary sides), blank line, then the relabeled diff3 body, an explicit block for deletions or never-synced ancestors (absent side = empty section, every section LF-terminated), or a single comment line when any side is binary.
- **SYNC-04 merge-level row:** `sync::merge::tests::test_merge_crlf` exists and passes. CRLF ancestor `a=1\r\nb=1\r\nc=1\r\nd=1\r\n`, local edits line 1, upstream edits line 4: result is exactly `Clean("a=2\r\nb=1\r\nc=1\r\nd=2\r\n")`. LF negative control: same edits merge to `a=2\nb=1\nc=1\nd=2\n` with no `\r`.

## Task Commits

1. **Task 1 RED:** `d71683b` test(01-08): add failing test for adjacent-edit conflicts on pull
2. **Task 1 GREEN:** `8fcdc62` feat(01-08): merge managed files with diffy diff3 instead of similar
3. **Task 2 RED:** `ec7f29a` test(01-08): add failing tests for conflict labels and sidecar rendering
4. **Task 2 GREEN:** `df500ae` feat(01-08): relabel conflict markers and render .conflict sidecars

No REFACTOR commits (nothing to clean up).

## Files Created/Modified

- `src/sync/merge.rs` - rewritten: MergeOutcome, merge_texts, relabel_conflict_markers, ConflictSide, render_conflict_sidecar and 13 tests
- `src/sync/mod.rs` - classifier calls merge::merge_texts; new test_pull_adjacent_edits_conflict (+ gitconfig_three_way helper); idempotence fixture rewritten with an unchanged middle line
- `Cargo.toml` / `Cargo.lock` - `diffy = "=0.5.2"` replaces `similar = "2.7"` (lockfile adds diffy and foldhash, removes similar)

## Verification Results

| Gate | Result | What it does not establish |
|------|--------|----------------------------|
| Task 1 target tests (`cargo test --bin ude -- sync::tests::test_pull_adjacent_edits_conflict sync::tests::test_multi_pull_merge_idempotence sync::merge::tests`) | 6 passed, exit 0; re-run after commit (tracer gate) also 6/6 | Only the fixtures exercised; adjacency semantics beyond these shapes rest on diffy |
| Dependency check (no `similar` in Cargo.toml or `use similar`; diffy pinned) | exit 0; `name = "similar"` count in Cargo.lock: 0 | — |
| `cargo deny check advisories bans sources` | exit 0 (advisories ok, bans ok, sources ok) | `licenses` not run: repo has no deny.toml (pre-existing, per plan) |
| cargo machete gate | exit 0; "didn't find any unused dependencies" | No negative control run this time; the plan recorded at plan time that the list pattern matches a listed crate |
| Task 2 merge tests (`cargo test --bin ude sync::merge::tests`) | 13 passed, exit 0 | proptest default 256 cases, a sampled bound, not exhaustive |
| Mutation gate (background, 63 s wall) | exit 0: 12 mutants found, 10 caught, 0 missed, 0 timeout, 2 unviable; check command exit 0; baseline ran 13 tests, all `sync::merge::` | cargo-mutants 27.1.0 generated no mutants for relabel's match arms, the incomplete-hunk setter, or the header format strings; those are pinned by exact-output tests, not by the mutation gate |
| `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test` | exit 0 each task; final: 115 bin + 11 e2e tests passed | — |
| Hermeticity gate (decoy `GIT_DIR`, `/dev/null` global config, `cargo test --bin ude sync::`) | exit 0, 88 passed | — |

Mutant list (12): 1 whole-return replacement of merge_texts (unviable: MergeOutcome has no Default), 2 each for relabel_conflict_markers, render_conflict_sidecar and sidecar_body, the let-chain `&&` to `||` in sidecar_body (unviable: does not compile), and 4 in push_section (replace with `()`, `&&` to `||`, two `!` deletions). The plan's pre-rewrite figure was 65; the rewritten file has 12 because diffy now holds the algorithm. No `.cargo/mutants.toml` exclusion was needed.

## TDD Gate Compliance

- Task 1: RED `d71683b` precedes GREEN `8fcdc62`. RED evidence: nextest JUnit report, `gsd_run check tdd-red-evidence` returned `RED_EVIDENCE_OK` / `target_test_failed` for `sync::tests::test_pull_adjacent_edits_conflict`. Semantic assessment: the target ran and failed on the planned assertion (`left: Merged, right: Conflicted`): the old merge auto-merged adjacent edits.
- Task 2: RED `ec7f29a` precedes GREEN `df500ae`. `RED_EVIDENCE_OK` / `target_test_failed` for all 7 new-behavior targets (test_relabel_conflict_markers, test_merge_conflict_labels, test_merge_no_trailing_newline_markers_start_line, the three render tests, test_relabel_prop). Semantic assessment: each failed on its planned assertion against the stubs (labels still `ours`/`original`/`theirs`, renderer returned `""`, proptest negative control saw `<<<<<<< ours` where `<<<<<<< LOCAL` was required).
- **Not RED at Task 2's RED step:** `test_adjacent_edits_conflict` and `test_merge_crlf` passed immediately, because they test diffy behavior Task 1 had already wired in (adjacent edits conflict; CRLF preserved). Their negative controls are inside the tests, but neither was ever observed failing against code that lacked the behavior. The sync-level adjacent test (Task 1) was observed failing against the old merge.

## Decisions Made

- No `#[expect(dead_code)]` on ConflictSide / render_conflict_sidecar: main.rs declares `pub mod sync`, so unused pub items are not linted (an `expect` attribute was tried and failed as an unfulfilled expectation).
- Render tests assert exact full output for every variant (beyond the plan's contains-checks) and add three cases the plan did not list: three text sides that diffy merges cleanly (explicit-block fallthrough), a never-synced ancestor with an empty local text side (no blank section), and a single binary side. These cover branches the mutation gate would otherwise leave unexercised.

## Deviations from Plan

None in scope or design. Test-case additions are listed under Decisions Made.

## Issues Encountered

- The failing proptest at RED wrote `proptest-regressions/sync/merge.txt` at the repo root; it was removed before the RED commit and none exists now.

## Known Stubs

None. `render_conflict_sidecar` has no production caller yet by design; plan 01-09 writes the sidecars.

## Threat Flags

None. T-01-23 (exact-line relabel) is covered by test_relabel_conflict_markers' negative control and test_relabel_prop. Residual, inherent to exact-line relabeling: a file whose conflicting section itself contains the exact line `<<<<<<< ours` gets that line relabeled too.

## User Setup Required

None.

## Next Phase Readiness

- 01-09 can call `merge::render_conflict_sidecar(rel, ancestor, local, upstream)` with LF-normalized texts and write the result; the sidecar mode rule (copy the local file's mode, Pitfall 4) is 01-09's responsibility.
- SYNC-05 is complete only once 01-09 writes the sidecars; this plan delivers the merge and rendering half.

---
*Phase: 01-sync-engine-state-hermeticity*
*Completed: 2026-10-19*

## Self-Check: PASSED

- Files: src/sync/merge.rs, src/sync/mod.rs, Cargo.toml, Cargo.lock: all FOUND
- Commits: d71683b, 8fcdc62, ec7f29a, df500ae: all FOUND as ancestors of HEAD
- Acceptance greps: `pub fn merge_texts` (1), `merge::merge_texts` in mod.rs (1), old names (0), `pub fn relabel_conflict_markers` / `pub enum ConflictSide` / `pub fn render_conflict_sidecar` / `set_incomplete_hunk_style(IncompleteHunkStyle::Git)` (1 each)
