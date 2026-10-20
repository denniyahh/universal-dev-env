---
phase: 01-sync-engine-state-hermeticity
verified: 2026-10-20T19:44:58Z
status: gaps_found
score: 10/11 must-haves verified
covered_files: [".config/nextest.toml", ".planning/phases/01-sync-engine-state-hermeticity/01-01-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-01-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-02-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-02-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-03-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-03-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-04-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-04-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-05-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-05-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-06-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-06-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-07-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-07-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-08-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-08-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-09-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-09-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-10-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-10-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-11-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-11-SUMMARY.md", "Cargo.toml", "src/audit/mod.rs", "src/cli/mod.rs", "src/main.rs", "src/process.rs", "src/sanitizer/mod.rs", "src/semantic/mod.rs", "src/sync/merge.rs", "src/sync/mod.rs", "src/sync/staging.rs", "src/sync/state.rs", "src/template/mod.rs", "templates/projects/generic/gitignore.repo", "tests/e2e_lifecycle.rs"]
covered_digest: "v3:sha256:2f49afd22f5f1e9f27ba5e21342d81b8e6f9c2784579a8a4112ad7ec9a4d166a"
behavior_unverified: 0
overrides_applied: 1
overrides:
  - must_have: "01-01 backstop: deadline boundary is inclusive (elapsed >= timeout kills the group; a child that exits before the poll observes the deadline returns its real status; POLL_INTERVAL = 10 ms)"
    reason: "Accepted on the evidence: run_bounded checks child.try_wait() before the `Instant::now() >= deadline` kill and returns the real status on Ok(Some), POLL_INTERVAL is Duration::from_millis(10), and the timeout tests pass. A test pinned to the exact millisecond boundary would itself be flaky."
    accepted_by: "orchestrator ruling (not the operator), on the implementation read plus passing timeout tests"
    accepted_at: "2026-10-20T00:33:36Z"
re_verification:
  previous_status: human_needed
  previous_score: 10/10
  previous_head: cfa26b2
  gaps_closed: []
  gaps_remaining: []
  regressions:
    - "SC1 (no resurrection on later pushes): the fresh-clone keep added in 42803ee lets a fresh clone's first init + plain push republish a template that was deleted upstream"
gaps:
  - truth: "SC1: Deletion of an upstream template propagates locally on `ude sync pull` via state tombstones without resurrecting on subsequent pushes"
    status: partial
    reason: "Holds on a machine that has a state.json. On a fresh clone (state.json is per machine and git-ignored since 1e6615c), init keeps a committed managed file whose template was deleted upstream, records no ancestor for it (kept_ancestor returns None when the template is absent), so sync status reports it 'Locally Modified (ready to push)' and a plain `ude sync push` re-creates the deleted template. Reproduced with the HEAD binary; the pre-42803ee binary (2023d5b) on the same scenario reports 'Deleted upstream' and publishes 0 files, so this is a regression introduced by the fresh-clone keep. The file does not need to be edited: an unedited committed copy of the old template is enough."
    artifacts:
      - path: "src/main.rs"
        issue: "init: a kept_committed file with no template gets `recorded = None` (kept_ancestor -> Ok(None)), so no state record is written; the 'Kept committed ... sync status compares it with the template' line is printed even though no template exists"
      - path: "tests/e2e_lifecycle.rs"
        issue: "test_first_init_on_fresh_clone_keeps_committed_edits covers a present template only; no test covers a fresh clone whose committed managed file has no upstream template"
    missing:
      - "Decide what a fresh clone's kept file with no upstream template should become: e.g. record the kept bytes as ancestor (pre-42803ee behaviour, which classifies it Deleted upstream), or record it as a tombstone/unmanaged so push never republishes it"
      - "e2e test, written first: machine A commits a project, the template is deleted upstream, machine B clones and runs init then a plain push; assert the template stays absent and push publishes 0 files. Negative control: the same clone with the template still present keeps the committed edit as Locally Modified"
deferred:
  - truth: "Windows runtime behaviour of the bounded runner (JobObject kill), the remote cache lock (release on process death) and staged state.json replace"
    addressed_in: "Phase 999.1 (backlog)"
    evidence: "Phase 999.1 goal: 'On a Windows host, run the three checks waived for phase 01 on 2026-10-20 (operator) ...'. 01-UAT.md tests 1-3: skipped, operator waiver 2026-10-20. Compile and clippy for x86_64-pc-windows-msvc re-checked at HEAD f33072b on Linux (exit 0; nothing linked or run)"
  - truth: "Push must not leak the pusher's home path when a {{ user.home }} / {{ user.xdg_config }} placeholder is followed by , ; . ) ] } or = (re-review W2; latent)"
    addressed_in: "Phase 2"
    evidence: "Phase 2 goal: 'Screen and redact all confidential tokens, raw credentials, and user-specific paths before any remote git staging or push.' Pending todo 2026-10-19-home-path-placeholders-leak-..."
  - truth: "A file that ude init kept (custom .gitignore, or a fresh clone's committed edit) must not be publishable over the shared template by a plain push"
    addressed_in: "Phase 4"
    evidence: "Phase 4 goal: '... clean manifests with safe re-initialization protections.' Operator accepted for phase 01 on 2026-10-19; pending todo 2026-10-19-never-publish-files-that-init-kept-over-the-shared-template; deferred-items.md 'Fresh-clone keep (42803ee) uses the current template as the ancestor'. Covers publishing a kept EDIT over an existing template, not re-creating a template deleted upstream (that is the gap above)"
  - truth: "Re-init side effects (01-REVIEW-REINIT WR-02, WR-03, IN-01): explicit --type change not applied to mise.toml; update-index stages an uncommitted hook edit; lost exec bit not restored"
    addressed_in: "Phase 4"
    evidence: "deferred-items.md ('candidate for Phase 4, safe re-initialization'); Phase 4 SC2 covers re-init on an existing workspace"
---

# Phase 1: Sync Engine & State Hermeticity Verification Report

**Phase Goal:** Ensure bidirectional synchronization is hermetic, crash-resilient, and atomic without phantom drift or zombie file resurrections.
**Verified:** 2026-10-20T19:44:58Z, at HEAD f33072b (24 commits over the previous report's cfa26b2; code changes in src/main.rs, src/sync/{mod,state,staging}.rs, src/process.rs, src/cli/mod.rs, tests/e2e_lifecycle.rs)
**Status:** gaps_found
**Re-verification:** Yes. The previous report (cfa26b2, human_needed, no gaps) is stale. Its Windows items are now operator-waived and backlogged (Phase 999.1). The new code (symlink refusal, fresh-clone keep, monorepo detection, recorded-type default, TypeSafe symlink skip, init destination checks) got full checks with negative controls; everything else got regression runs.

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | SC1: Upstream deletion propagates locally on `ude sync pull` via tombstones, without resurrecting on later pushes | ✗ FAILED (partial) | Single machine holds (CLI probe at HEAD: "Deleted upstream" → pull "Removed .gitconfig" → tombstone `['.gitconfig']` in v2 state.json → recreated file "Unmanaged ... never republished" → push 0 files, template stays absent). **Fresh clone resurrects it**: see the probe below. Regression from 42803ee, confirmed against the 2023d5b binary |
| 2 | SC2: Multi-file sync stages before updating, so disk/permission errors cause no partial writes | ✓ VERIFIED | `test_pull_staging_failure_no_partial_writes`, `test_push_staging_failure_no_partial_writes` pass. staging.rs changed only by a `#[cfg(unix)]` on a constant. Per locked D-05 the temp files sit beside each target, and the rename phase is not atomic as a whole (D-01, AR-01-02) |
| 3 | SC3: All git and gh calls have bounded timeouts (30 s local / 60 s network) and kill hung children | ✓ VERIFIED (Linux) | Every `Command::new` in non-test src (audit, main incl. new `committed_paths`, template, sync's runner) feeds `process::run_bounded`; the only other spawn is `curl` in src/semantic. `test_timeout_kills_group`, `test_push_timeout_hanging_hook` pass. run_bounded scrubs `GIT_DIR` etc. (process.rs line 92). Windows runtime: deferred to 999.1 |
| 4 | SC4: Ancestor fingerprints normalize CRLF, no false drift | ✓ VERIFIED | CLI probe at HEAD: CRLF-only conversion of .gitconfig (28 CRs) is "In Sync"; control: a real edit in the CRLF file is "Locally Modified" |
| 5 | SC5: Divergent edits produce `.conflict` files with standard markers while preserving local changes | ✓ VERIFIED | CLI probe at HEAD: pull exit 1, local .gitignore byte-identical, sidecar has 4 marker lines (LOCAL / ANCESTOR / `=======` / UPSTREAM); push while open exit 1 with upstream byte-identical; `ude sync resolve .gitignore` exit 0, sidecar gone |
| 6 | SC6: Outbound push verifies branch continuity and rejects divergent candidates safely | ✓ VERIFIED | Push path changed only by a symlink refusal on the local read. `test_push_continuity_violation_is_hard_failure`, `test_direct_push_aborts_when_upstream_moved`, `test_force_does_not_bypass_upstream_moved` pass |
| 7 | 01-06 backstop: two `ude sync` commands on the same remote and branch serialize on the cache lock | ✓ VERIFIED (Linux) | `test_remote_lock_concurrency_serialization_and_timeout`, `test_remote_lock_released_when_holder_killed`, `test_remote_lock_not_inherited_by_children` pass |
| 8 | 01-01 backstop: deadline boundary inclusive, 10 ms poll | PASSED (override) | Carried forward; process.rs runner unchanged since (only test-only cfg gates added) |
| 9 | Goal ("no zombie resurrections"): re-running `ude init` keeps every prior record and writes no recorded file | ✓ VERIFIED | `test_tombstones_survive_reinit`, `test_open_conflict_survives_reinit`, `test_reinit_keeps_local_edit_of_file_it_would_rewrite`, `test_reinit_keeps_record_of_file_whose_template_is_missing` pass. (This truth is about re-init with a state.json; the first-init-of-a-clone case is truth 1's gap) |
| 10 | 01-REVIEW-REINIT WR-01 closed: per-machine state.json ignored, init warns per missing tracked file | ✓ VERIFIED | `test_project_gitignore_ignores_per_machine_state`, `test_project_gitignore_ignores_state_staging_temp_files`, `test_doctor_reports_tracked_per_machine_state_with_remedy`, `test_reinit_warns_that_push_deletes_template_of_missing_tracked_file`, `test_reinit_missing_file_warning_follows_upstream` pass |
| 11 | 01-REVIEW fixes hold: init refuses symlinked destinations (CR-01); unreadable / non-UTF-8 `.gitignore` is kept or stops init (CR-02); monorepo subdirectory detected (WR-02); re-init without `--type` keeps the recorded type (WR-03); TypeSafe check skips a symlinked AGENTS.md (WR-04) | ✓ VERIFIED | The 7 tests added in bc0cce5 / 6d6b652 / 8e7c6dd / 374a208 pass at HEAD, and **all 7 fail** (assertion panics, e.g. "init overwrote a committed edit: exit Some(0)") when only the src parts of those 4 commits are reverse-applied on HEAD in a scratch worktree. The WR-04 test uses a `curl` stub and asserts the private marker never reaches it, with a regular-file control that does |

**Score:** 10/11 truths verified (1 by override; 0 present-but-behavior-unverified)

#### Fresh-clone resurrection probe (truth 1)

Shared template dir both clones link to as `templates/`; fake HOME/XDG, no inherited git vars.

1. Machine A: `ude init`, `git init`, `git add -A`, commit. state.json is not committed (ignored).
2. Upstream deletes `projects/generic/gitconfig.repo`. A pulls: "Removed .gitconfig". A restores its committed copy: status "Unmanaged (deleted upstream; kept locally, never republished)"; A's push leaves the template absent.
3. Machine B: `git clone` A, `ude init`, `ude sync status`, `ude sync push`.

| Binary | B init | B status for .gitconfig | B push | Template afterwards |
|--------|--------|-------------------------|--------|---------------------|
| HEAD f33072b | "Kept committed .gitconfig (not overwritten); sync status compares it with the template" | Locally Modified (ready to push) | "synchronized 1 file(s)", exit 0 | **re-created** |
| 2023d5b (just before 42803ee) | no keep line | Deleted upstream (pull to remove) | "synchronized 0 file(s)", exit 0 | absent |

The same applies without step 2's pull on A: any clone made after the upstream deletion. The committed file was the unedited old template; no edit is needed to trigger it.

### Deferred Items

| # | Item | Addressed In | Evidence |
|---|------|-------------|----------|
| 1 | Windows runtime: JobObject kill, cache-lock release, state.json replace | Phase 999.1 (backlog) | Operator waiver 2026-10-20 in 01-UAT.md; compile + clippy for the Windows target pass at HEAD (cross-checked on Linux only) |
| 2 | Home-path placeholder followed by `, ; . ) ] } =` not round-tripped (W2, latent) | Phase 2 | Phase 2 goal; pending todo |
| 3 | A kept file (custom `.gitignore`, fresh clone's committed edit) is publishable over an existing template | Phase 4 | Operator-accepted 2026-10-19; pending todo. Does not cover re-creating a deleted template |
| 4 | Re-init side effects (REINIT WR-02, WR-03, IN-01) | Phase 4 | deferred-items.md |

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/process.rs` | Bounded runner, env scrub, redaction | ✓ VERIFIED | only `#[cfg(unix)]` gates on test items since cfa26b2 |
| `src/sync/staging.rs` | Stage-validate-commit transaction | ✓ VERIFIED | one cfg gate added |
| `src/sync/state.rs` | v2 schema, loud failure, staged save | ✓ VERIFIED | load refuses a symlinked state path (`test_dangling_state_symlink_fails_loudly` passes) |
| `src/sync/merge.rs` | diffy diff3 merge and sidecar renderer | ✓ VERIFIED | unchanged |
| `src/sync/mod.rs` | Deletion states, conflicts, resolve, push skips, continuity | ✓ VERIFIED | adds `refuse_symlink_under` (unit test with regular-path control) wired into scan, pull and push reads |
| `src/main.rs` | init carries prior state; destination symlink checks; fresh-clone keep; missing-file warning | ⚠️ PARTIAL | wired and tested, but the fresh-clone keep writes no record when the template is absent (gap above) |
| `templates/projects/generic/gitignore.repo` | `*.conflict`, `.ude/state.json`, `.ude/.tmp*` | ✓ VERIFIED | unchanged |
| `tests/e2e_lifecycle.rs` | Hermetic e2e coverage | ✓ VERIFIED | 50 e2e tests pass; no `std::env::set_var` / `remove_var` anywhere in src or tests |

`verify.artifacts`: 32/32 pass across the 11 plans (existence/pattern level only).

### Key Link Verification

| From | To | Status | Details |
|------|----|--------|---------|
| init `committed_paths` | `process::run_bounded` (`git ls-files -z`) | WIRED | LOCAL_TIMEOUT; ancestor `.git` search makes the monorepo case work (WR-02 test) |
| init write targets | `sync::refuse_symlink_under` | WIRED | checked before anything is written; exits EXIT_HARD_FAILURE (CR-01 tests) |
| `copy_template_file` / `rendered_template` / `kept_ancestor` | `refuse_symlink_under(template root)` | WIRED | `test_init_refuses_symlinked_template` |
| push TypeSafe check | `refuse_symlink_under` on both AGENTS.md paths | WIRED | runs before any read; WR-04 test with curl stub |
| scan / pull / push reads | `read_if_present(root, path)` → `refuse_symlink_under` | WIRED | `test_pull_refuses_symlinked_template`, `test_push_refuses_symlinked_local_file`, `test_push_does_not_treat_dangling_symlink_as_deletion` |
| init kept file (no template) | state record | NOT_WIRED | `recorded = None` → no record → push treats it as a publishable local file (gap) |

Earlier links (run_local/run_network, StagedTransaction, merge_texts, check_continuity, rollback_commit, resolve) are covered by the passing unit and e2e tests above; the source around them did not change.

### Data-Flow Trace (Level 4)

Not applicable: no rendered UI. CLI output comes from state.json and the filesystem, exercised by the probes.

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Full suite (one run) | `env -u TYPESAFE_API_KEY cargo test --workspace` | 136 unit + 50 e2e passed, exit 0 | ✓ PASS |
| Lint / format | `cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --check` | both exit 0 | ✓ PASS |
| Windows target compile | `cargo clippy --target x86_64-pc-windows-msvc --workspace --all-targets -- -D warnings` | exit 0 (check only, nothing linked or run) | ✓ PASS |
| Review-fix tests bite | 7 named tests on HEAD with the 4 fix commits' src reverse-applied | 7/7 FAILED with assertion panics | ✓ PASS (negative control) |
| SC1/SC4/SC5 through the binary | scratch project, fake HOME/XDG | as in the truths table | ✓ PASS |
| Fresh-clone resurrection | two clones, shared templates, HEAD vs 2023d5b binary | HEAD re-creates the deleted template; 2023d5b does not | ✗ FAIL |
| Debt markers in added lines (cfa26b2..HEAD) | rg `TBD\|FIXME\|XXX\|TODO\|HACK\|todo!(\|unimplemented!(` | none (control line matched) | ✓ PASS |

### Probe Execution

No `scripts/*/tests/probe-*.sh` exist and no plan declares one. Step 7c: not applicable.

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| SYNC-01 | 01-07 | Tombstones; upstream deletions propagate without resurrection | ✗ BLOCKED (partial) | Truth 1: holds per machine, fails on a fresh clone's first init + push |
| SYNC-02 | 01-04, 01-06 | Two-phase staging before updating the working tree | ✓ SATISFIED | Truth 2 (per-target temp files under D-05) |
| SYNC-03 | 01-01, 01-02, 01-03 | Bounded git/gh timeouts with cancellation | ✓ SATISFIED (Linux) | Truths 3, 8; Windows runtime deferred to 999.1 |
| SYNC-04 | 01-05 | CRLF-normalized SHA-256 fingerprints | ✓ SATISFIED | Truth 4 |
| SYNC-05 | 01-08, 01-09, 01-10 | `.conflict` files with standard markers, local edits preserved | ✓ SATISFIED | Truth 5 |
| SYNC-06 | 01-10, 01-11 | Safe push candidates and branch-head continuity | ✓ SATISFIED | Truth 6 |

All six IDs are claimed by plan `requirements:` fields; REQUIREMENTS.md maps no other ID to Phase 1, so nothing is orphaned. REQUIREMENTS.md marks SYNC-01 `[x] Complete`; per this report that is premature.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| src/main.rs | fresh-clone keep / `kept_ancestor` | Kept file with absent template gets no record, and init still prints "sync status compares it with the template" | 🛑 Blocker | Zombie resurrection on push (gap) |
| src/sync/mod.rs | `read_if_present` via `scan_project` `?` | One symlinked managed file fails status/pull/push for all files (01-REVIEW WR-01) | ⚠️ Warning | Fail-closed, not unsafe; operator deferred it to a todo (5b2841e), no phase owns it |
| src/main.rs | `committed_paths` | Any ancestor `.git` triggers `git ls-files`; a failure stops init (exit 1) | ℹ️ Info | Deliberate fail-closed; e.g. an unrelated/broken ancestor `.git` blocks init |
| 01-REVIEW-DISPOSITION.md | IN-01..IN-07 | 7 info findings still `open` (untriaged), incl. IN-01: a local deletion of a templated file deletes the shared `.jinja` template | ℹ️ Info | Not decided either way |

### Human Verification Required

None. The three Windows checks are operator-waived and backlogged (Phase 999.1), so they are listed as deferred, not as open human checks.

### Gaps Summary

One gap, a regression introduced after the previous verification. The fresh-clone keep (42803ee, added so a clone's first init does not overwrite committed files) records no ancestor when the managed file's template no longer exists. That file then reads as a publishable local change, and a plain push re-creates a template that was deleted upstream. This is the phase goal's "zombie file resurrection" and SC1's "without resurrecting on subsequent pushes", reached through the cross-machine path that the per-machine, git-ignored state.json makes normal. The binary just before 42803ee handles the same scenario correctly.

It is close to, but not the same as, the operator-accepted Phase 4 item (a kept edit can be published over an existing template). The todo's proposed fix (kept files become project-only overrides that push skips) would likely cover this case too. If the operator prefers to fold it into that item, it can be accepted with an override like:

```yaml
overrides:
  - must_have: "SC1: Deletion of an upstream template propagates locally on `ude sync pull` via state tombstones without resurrecting on subsequent pushes"
    reason: "Fresh-clone resurrection folded into the Phase 4 'never publish files init kept' todo"
    accepted_by: "{operator}"
    accepted_at: "{ISO timestamp}"
```

What this report does not establish:
- **Windows:** nothing ran on Windows. Only compile/clippy for the msvc target was cross-checked on Linux.
- **Crash resilience in the rename phase:** not covered (no fsync, no journal; D-01, accepted as AR-01-02).
- **`--remote` git-backed upstream:** the fresh-clone probe used a shared local template directory, not a git remote. The code path that decides the record (init) does not depend on the remote type, but this was not run.
- **Combined reversal:** the review-fix negative control reverted all four fixes at once, so each test is shown to bite on its own target behaviour (assertion text matches), not proven independent of the other three reverts.
- **Untriaged findings:** IN-01..IN-07 remain `open`.

---

_Verified: 2026-10-20T19:44:58Z_
_Verifier: Claude (gsd-verifier)_
