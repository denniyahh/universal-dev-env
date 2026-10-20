---
phase: 01-sync-engine-state-hermeticity
verified: 2026-10-20T19:57:56Z
status: gaps_found
score: 11/12 must-haves verified
covered_files: [".config/nextest.toml", ".planning/phases/01-sync-engine-state-hermeticity/01-01-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-01-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-02-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-02-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-03-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-03-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-04-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-04-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-05-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-05-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-06-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-06-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-07-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-07-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-08-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-08-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-09-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-09-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-10-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-10-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-11-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-11-SUMMARY.md", "Cargo.toml", "src/audit/mod.rs", "src/cli/mod.rs", "src/main.rs", "src/process.rs", "src/sanitizer/mod.rs", "src/semantic/mod.rs", "src/sync/merge.rs", "src/sync/mod.rs", "src/sync/staging.rs", "src/sync/state.rs", "src/template/mod.rs", "templates/projects/generic/gitignore.repo", "tests/e2e_lifecycle.rs"]
covered_digest: "v3:sha256:7db6f433a3c0847b0f07687d10040b3e14dba6047845f2919451ea6a3ef2ba29"
behavior_unverified: 0
overrides_applied: 1
overrides:
  - must_have: "01-01 backstop: deadline boundary is inclusive (elapsed >= timeout kills the group; a child that exits before the poll observes the deadline returns its real status; POLL_INTERVAL = 10 ms)"
    reason: "Accepted on the evidence: run_bounded checks child.try_wait() before the `Instant::now() >= deadline` kill and returns the real status on Ok(Some), POLL_INTERVAL is Duration::from_millis(10), and the timeout tests pass. A test pinned to the exact millisecond boundary would itself be flaky."
    accepted_by: "orchestrator ruling (not the operator), on the implementation read plus passing timeout tests"
    accepted_at: "2026-10-20T00:33:36Z"
re_verification:
  previous_status: gaps_found
  previous_score: 10/11
  previous_head: f33072b
  gaps_closed:
    - "SC1: a fresh clone's first init + plain push no longer re-creates a template deleted upstream (99ffc22)"
  gaps_remaining: []
  regressions:
    - "CR-03 guarantee: on the first init of a repo that already committed its own .gitignore, with no gitignore template in the template root, the first pull now deletes that .gitignore (introduced by 99ffc22; kept by 2023d5b)"
gaps:
  - truth: "A file init kept is never deleted by pull on the strength of an ancestor sync never saw (CR-03 guarantee: the first pull keeps a custom .gitignore init preserved; SC5: local changes are preserved on divergence)"
    status: partial
    reason: "When a kept committed file has no template, 99ffc22 records the file's current working-tree bytes as its ancestor. It then reads 'Deleted upstream (pull to remove)', and a pull deletes it with exit 0. (A) Regression: a repo that committed its own .gitignore before adopting ude, with no gitignore template in the template root, loses that .gitignore on the first pull. 2023d5b kept it (Locally Modified, pull exit 0, file present). (B) Pre-existing on the same line, identical on 2023d5b: an uncommitted edit made on a fresh clone before its first init is baked into the ancestor. When the template was deleted upstream, the pull deletes the file and the edit cannot be recovered from git. Machine A, which has a state.json, gets a conflict sidecar for the same edit-vs-delete instead."
    artifacts:
      - path: "src/main.rs"
        issue: "init record loop: `Ok(None) if committed_keep => fs::read(project_path.join(rel)).ok()` uses working-tree bytes as the ancestor. Kept content therefore equals the ancestor, and a missing template reads as an upstream deletion, even when the template never existed and even when the bytes include edits never committed"
      - path: "tests/e2e_lifecycle.rs"
        issue: "test_init_keeps_uncommitted_gitignore_when_template_is_absent covers the uncommitted .gitignore only. No test covers a committed own .gitignore with no template, or an uncommitted edit before the first init"
    missing:
      - "Decide what a kept committed file with no template records. One option fixes both cases and keeps the 99ffc22 guarantee: record a tombstone with no ancestor. The file then reads Unmanaged, which pull never deletes and push never publishes; this is what machine A ends up with after its pull. Another option is to use the committed bytes (git HEAD) as the ancestor instead of the working tree. That turns (B) into a conflict, but (A) still deletes the committed .gitignore"
      - "e2e tests, written first: (A) git repo with a committed own .gitignore, no gitignore template, first init, then pull; assert .gitignore is unchanged. (B) fresh clone, edit .gitconfig, template deleted upstream, first init, then pull; assert the edit survives. Keep test_fresh_clone_does_not_resurrect_template_deleted_upstream green (push must still publish 0 files)"
deferred:
  - truth: "Windows runtime behaviour of the bounded runner (JobObject kill), the remote cache lock (release on process death) and staged state.json replace"
    addressed_in: "Phase 999.1 (backlog)"
    evidence: "Phase 999.1 goal: 'On a Windows host, run the three checks waived for phase 01 on 2026-10-20 (operator) ...'. 01-UAT.md tests 1-3: skipped, operator waiver 2026-10-20. Compile and clippy -D warnings for x86_64-pc-windows-msvc re-checked at HEAD 99ffc22 on Linux (exit 0; nothing linked or run)"
  - truth: "Push must not leak the pusher's home path when a {{ user.home }} / {{ user.xdg_config }} placeholder is followed by , ; . ) ] } or = (re-review W2; latent)"
    addressed_in: "Phase 2"
    evidence: "Phase 2 goal: 'Screen and redact all confidential tokens, raw credentials, and user-specific paths before any remote git staging or push.' Pending todo 2026-10-19-home-path-placeholders-leak-..."
  - truth: "A file that ude init kept must not be publishable by a plain push. This includes an uncommitted own .gitignore with no gitignore template, which push publishes as a new gitignore.repo (probe case 4, identical on 2023d5b)"
    addressed_in: "Phase 4"
    evidence: "Phase 4 goal: '... safe re-initialization protections.' Operator accepted for phase 01 on 2026-10-19 ('A custom .gitignore that ude init kept must not be publishable ... by a plain push'); pending todo 2026-10-19-never-publish-files-that-init-kept-over-the-shared-template"
  - truth: "Re-init side effects (01-REVIEW-REINIT WR-02, WR-03, IN-01): explicit --type change not applied to mise.toml; update-index stages an uncommitted hook edit; lost exec bit not restored"
    addressed_in: "Phase 4"
    evidence: "deferred-items.md ('candidate for Phase 4, safe re-initialization'); Phase 4 SC2 covers re-init on an existing workspace"
---

# Phase 1: Sync Engine & State Hermeticity Verification Report

**Phase Goal:** Ensure bidirectional synchronization is hermetic, crash-resilient, and atomic without phantom drift or zombie file resurrections.
**Verified:** 2026-10-20T19:57:56Z, at HEAD 99ffc22 (the only code change since the previous report at f33072b is 99ffc22: src/main.rs +9/-4, tests/e2e_lifecycle.rs +61)
**Status:** gaps_found
**Re-verification:** Yes. The previous gap (fresh-clone template resurrection) is closed. The fix introduces one regression on a neighbouring path, and the probe also found a pre-existing data-loss case on the same line. Both have the same root cause and form one gap.

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | SC1: Upstream deletion propagates locally on `ude sync pull` via tombstones, without resurrecting on later pushes | ✓ VERIFIED | Single machine: carried forward (unchanged code). Fresh clone: my own probe (case 1 below) against the HEAD binary shows status "Deleted upstream (pull to remove)", push "synchronized 0 file(s)", template stays absent. Before the fix (99ffc22 src reverse-applied), `test_fresh_clone_does_not_resurrect_template_deleted_upstream` fails at its "Deleted upstream" assertion (line 3185) |
| 2 | SC2: Multi-file sync stages before updating; no partial writes on errors | ✓ VERIFIED | `test_pull_staging_failure_no_partial_writes`, `test_push_staging_failure_no_partial_writes` pass; staging.rs unchanged since previous report |
| 3 | SC3: git/gh calls have bounded timeouts and kill hung children | ✓ VERIFIED (Linux) | No new spawns in 99ffc22; timeout tests pass. Windows runtime deferred to 999.1 |
| 4 | SC4: CRLF-normalized fingerprints, no false drift | ✓ VERIFIED | sync code unchanged; `test_sync_status_crlf_only_change_in_sync`, `test_fingerprint_crlf_lf_equivalence_prop` pass (CLI probe with control in the previous report, same sync code) |
| 5 | SC5: Divergent edits produce `.conflict` files with standard markers while preserving local changes | ✓ VERIFIED (sync engine) | `test_sync_pull_conflict_exits_1_and_writes_sidecar`, `test_sync_resolve_end_to_end` pass; probe case 2 shows machine A writing `.gitconfig.conflict` for edit-vs-delete. The fresh-clone path that loses an edit is truth 12 |
| 6 | SC6: Push verifies branch continuity and rejects divergent candidates | ✓ VERIFIED | continuity / upstream-moved tests pass; push code unchanged |
| 7 | 01-06 backstop: cache lock serializes same remote+branch | ✓ VERIFIED (Linux) | lock tests pass |
| 8 | 01-01 backstop: deadline boundary inclusive, 10 ms poll | PASSED (override) | carried forward; process.rs unchanged |
| 9 | Re-running `ude init` keeps every prior record and writes no recorded file | ✓ VERIFIED | re-init tests pass (`test_tombstones_survive_reinit`, `test_open_conflict_survives_reinit`, ...) |
| 10 | 01-REVIEW-REINIT WR-01: per-machine state.json ignored; init warns per missing tracked file | ✓ VERIFIED | its 5 tests pass |
| 11 | 01-REVIEW fixes CR-01, CR-02, WR-02, WR-03, WR-04 hold | ✓ VERIFIED | 7 tests pass; previously shown to fail on reverted source; 99ffc22 does not touch those paths |
| 12 | A file init kept is never deleted by pull on the strength of an ancestor sync never saw (CR-03 guarantee; SC5 on the fresh-clone path) | ✗ FAILED (partial) | Probe cases 3 and 5: pull deletes the file, exit 0. Case 5 is new in 99ffc22 (2023d5b keeps the file); case 3 is identical on 2023d5b |

**Score:** 11/12 truths verified (1 by override; 0 present-but-behavior-unverified). Truth 12 is new: it comes from the coordinator's request to look for other kept-file paths.

#### Kept file with no template: probe results (HEAD 99ffc22 vs 2023d5b, the commit before the fresh-clone keep)

Scratch setup: shared template dir linked as `templates/` into every clone, fake HOME/XDG, no inherited git vars.

| # | Scenario | HEAD 99ffc22 | 2023d5b | Verdict |
|---|----------|--------------|---------|---------|
| 1 | Fresh clone, unedited committed `.gitconfig`, template deleted upstream; B: init, status, push | Deleted upstream; push 0 files; template absent | same | ✓ fixed (the 42803ee..f33072b binaries re-created the template) |
| 2 | Committed *edit* of `.gitconfig` (on A), template deleted upstream | A: Conflicted, pull exit 1, writes `.gitconfig.conflict`, file kept. B (fresh clone): Deleted upstream; pull exit 0 "Removed .gitconfig"; git shows ` D .gitconfig` | same | ⚠️ pre-existing; the two machines resolve differently. Recoverable with `git restore` |
| 3 | Fresh clone, *uncommitted* edit to `.gitconfig` before the first init, template deleted upstream | Deleted upstream; pull exit 0 removes the file; the edit is not in git | same | ✗ pre-existing data loss (part of the gap) |
| 4 | No git, own *uncommitted* `.gitignore`, no gitignore template; init, push | Locally Modified; push publishes 1 file; `gitignore.repo` created with `secrets/` | same | deferred: the Phase 4 item "kept .gitignore publishable by a plain push" (operator-accepted) |
| 5 | Existing repo with *committed* own `.gitignore`, no gitignore template; first init, pull | init "Kept committed .gitignore ... compares it with the template"; Deleted upstream; pull exit 0 "Removed .gitignore (deleted upstream)" | Locally Modified; pull exit 0; file kept | ✗ **regression from 99ffc22** (part of the gap) |

Misread output on the same path: init prints "sync status compares it with the template" when no template exists. In case 5, pull also says "deleted upstream" for a template that never existed.

#### Negative controls for the fix's own tests (orchestrator claims checked)

| Run | Result |
|-----|--------|
| HEAD tests, 99ffc22 src reverse-applied | `test_fresh_clone_does_not_resurrect...` FAILED (panic at line 3185); `test_init_keeps_uncommitted_gitignore...` passed (expected: the old behaviour kept it too) |
| HEAD tests, `Ok(None) if committed_keep =>` changed to `Ok(None) =>` | `test_init_keeps_uncommitted_gitignore...` FAILED (panic at line 3209); the other passed |

Both claims hold.

### Deferred Items

| # | Item | Addressed In | Evidence |
|---|------|-------------|----------|
| 1 | Windows runtime: JobObject kill, cache-lock release, state.json replace | Phase 999.1 (backlog) | Operator waiver 2026-10-20; Windows-target clippy passes at HEAD (Linux cross-check only) |
| 2 | Home-path placeholder followed by `, ; . ) ] } =` (W2, latent) | Phase 2 | Phase 2 goal; pending todo |
| 3 | Kept files publishable by a plain push (incl. probe case 4, which creates `gitignore.repo`) | Phase 4 | Operator-accepted 2026-10-19; pending todo |
| 4 | Re-init side effects (REINIT WR-02, WR-03, IN-01) | Phase 4 | deferred-items.md |

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/process.rs` | Bounded runner, env scrub | ✓ VERIFIED | unchanged |
| `src/sync/staging.rs` | Stage-validate-commit | ✓ VERIFIED | unchanged |
| `src/sync/state.rs` | v2 schema, loud failure, staged save | ✓ VERIFIED | unchanged |
| `src/sync/merge.rs` | diff3 merge, sidecar renderer | ✓ VERIFIED | unchanged |
| `src/sync/mod.rs` | Deletion states, conflicts, resolve, push skips, continuity | ✓ VERIFIED | unchanged |
| `src/main.rs` | init: prior state, symlink checks, fresh-clone keep, ancestors | ⚠️ PARTIAL | the no-template ancestor for a committed kept file comes from the working tree (gap) |
| `templates/projects/generic/gitignore.repo` | `*.conflict`, `.ude/state.json`, `.ude/.tmp*` | ✓ VERIFIED | unchanged |
| `tests/e2e_lifecycle.rs` | Hermetic e2e coverage | ✓ VERIFIED | 52 e2e tests pass; no `set_var` / `remove_var` in src or tests |

`verify.artifacts`: 32/32 at the previous HEAD; artifact paths unchanged since.

### Key Link Verification

| From | To | Status | Details |
|------|----|--------|---------|
| init kept committed file, template present | template bytes as ancestor | WIRED | `test_first_init_on_fresh_clone_keeps_committed_edits`, case 1 control (`pre-push` In Sync) |
| init kept committed file, template absent | working-tree bytes as ancestor | WIRED, wrong source | no resurrection (case 1), but pull deletes kept content (cases 3, 5) |
| init kept uncommitted `.gitignore`, template absent | no record | WIRED | pull keeps it (`test_init_keeps_uncommitted_gitignore...`); push publishes it (case 4, deferred) |

The earlier links (runner, staging, merge, continuity, symlink refusal) are unchanged and their tests pass.

### Data-Flow Trace (Level 4)

Not applicable: no rendered UI.

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Full suite (one run) | `env -u TYPESAFE_API_KEY cargo test --workspace` | 136 unit + 52 e2e passed, exit 0 | ✓ PASS |
| Lint / format | `cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --check` | exit 0, exit 0 | ✓ PASS |
| Windows target | `cargo clippy --target x86_64-pc-windows-msvc --workspace --all-targets -- -D warnings` | exit 0 (check only) | ✓ PASS |
| Fix tests bite | revert fix / drop guard, scratch worktree | as in the table above | ✓ PASS |
| Kept-file probes 1-5, HEAD vs 2023d5b binary | scratch clones | as in the probe table | 1 ✓, 3 and 5 ✗ |
| Debt markers in added lines (f33072b..HEAD) | rg `TBD\|FIXME\|XXX\|TODO\|HACK\|todo!(\|unimplemented!(` | none | ✓ PASS |

### Probe Execution

No `scripts/*/tests/probe-*.sh` exist and no plan declares one. Step 7c: not applicable.

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| SYNC-01 | 01-07 | Tombstones; deletions propagate without resurrection | ✓ SATISFIED | Truth 1, incl. fresh clone |
| SYNC-02 | 01-04, 01-06 | Two-phase staging | ✓ SATISFIED | Truth 2 |
| SYNC-03 | 01-01, 01-02, 01-03 | Bounded git/gh timeouts | ✓ SATISFIED (Linux) | Truths 3, 8; Windows deferred to 999.1 |
| SYNC-04 | 01-05 | CRLF-normalized fingerprints | ✓ SATISFIED | Truth 4 |
| SYNC-05 | 01-08, 01-09, 01-10 | `.conflict` files, local edits preserved | ✗ BLOCKED (partial) | Truth 5 holds in the sync engine; truth 12: a fresh clone's first init can make pull delete a local edit (case 3) |
| SYNC-06 | 01-10, 01-11 | Safe push candidates, continuity | ✓ SATISFIED | Truth 6 |

All six IDs are claimed by plan `requirements:` fields; none are orphaned.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| src/main.rs | init record loop, `Ok(None) if committed_keep` | Working-tree bytes recorded as the ancestor | 🛑 Blocker | Pull deletes kept content (cases 3, 5) |
| src/main.rs | "Kept committed ..." line | Says status compares with the template when none exists | ℹ️ Info | Misleading output |
| src/sync/mod.rs | `scan_project` `?` on symlink refusal | One symlinked managed file fails every sync command (01-REVIEW WR-01) | ⚠️ Warning | Fail-closed; operator-deferred to a todo, no phase owns it |
| 01-REVIEW-DISPOSITION.md | IN-01..IN-07 | Still `open` (untriaged) | ℹ️ Info | - |

### Human Verification Required

None. The Windows checks are operator-waived and backlogged (Phase 999.1).

### Gaps Summary

The resurrection gap is closed, and both of the orchestrator's negative-control claims hold. Its fix chose the working-tree bytes as the ancestor of a kept committed file that has no template. Any such file therefore reads as "Deleted upstream", and the next pull deletes it. That is right only when the file is an unedited copy of a template that really was deleted. It is wrong in two cases:
- **A committed own `.gitignore` with no gitignore template** (case 5). This regresses from 2023d5b, and it is the CR-03 class of bug: init keeps a custom `.gitignore` and the first pull removes it. Git can restore it.
- **An uncommitted edit made before a fresh clone's first init** (case 3). This is pre-existing, and the edit is lost.

A tombstone-without-ancestor record (reads Unmanaged) would satisfy cases 1, 2, 3 and 5 at once. It matches what machine A ends up with after its own pull.

What this does not establish:
- **Windows:** not covered; compile/clippy only.
- **Crash during the rename phase:** not covered (AR-01-02).
- **`--remote` git-backed upstream:** the probes used a shared local template dir. The ancestor is decided in init, independent of the remote type, but this was not run.
- **Case 5's reach:** it needs a template root without `gitignore.repo` (the bundled templates ship one; a custom template repo may not). The same no-template rule applies to any committed managed file, e.g. `mise.toml` in a template repo without `mise.toml.jinja`. By code reading only (not probed), 2023d5b deletes such non-`.gitignore` files too.

---

_Verified: 2026-10-20T19:57:56Z_
_Verifier: Claude (gsd-verifier)_
