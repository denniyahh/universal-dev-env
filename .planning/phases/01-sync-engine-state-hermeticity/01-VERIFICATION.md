---
phase: 01-sync-engine-state-hermeticity
verified: 2026-10-20T01:17:35Z
status: human_needed
score: 10/10 must-haves verified
covered_files: [".config/nextest.toml", ".planning/phases/01-sync-engine-state-hermeticity/01-01-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-01-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-02-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-02-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-03-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-03-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-04-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-04-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-05-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-05-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-06-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-06-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-07-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-07-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-08-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-08-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-09-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-09-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-10-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-10-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-11-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-11-SUMMARY.md", "Cargo.toml", "src/audit/mod.rs", "src/cli/mod.rs", "src/main.rs", "src/process.rs", "src/sanitizer/mod.rs", "src/semantic/mod.rs", "src/sync/merge.rs", "src/sync/mod.rs", "src/sync/staging.rs", "src/sync/state.rs", "src/template/mod.rs", "templates/projects/generic/gitignore.repo", "tests/e2e_lifecycle.rs"]
covered_digest: "v3:sha256:b3f7467c638fe721a1e825d5bb4fda5a9dee0c6c7465e0cf1de7f06c82df5abe"
behavior_unverified: 0
overrides_applied: 1
overrides:
  - must_have: "01-01 backstop: deadline boundary is inclusive (elapsed >= timeout kills the group; a child that exits before the poll observes the deadline returns its real status; POLL_INTERVAL = 10 ms)"
    reason: "Accepted on the evidence: run_bounded checks child.try_wait() before the `Instant::now() >= deadline` kill and returns the real status on Ok(Some), POLL_INTERVAL is Duration::from_millis(10), and the timeout tests pass. A test pinned to the exact millisecond boundary would itself be flaky."
    accepted_by: "orchestrator ruling (not the operator), on the implementation read plus passing timeout tests"
    accepted_at: "2026-10-20T00:33:36Z"
re_verification:
  previous_status: human_needed
  previous_score: 9/9
  previous_head: 956c5e7
  gaps_closed:
    - "01-REVIEW-REINIT WR-01 (decision finding): a second machine's init no longer leads to a silent template deletion; state.json is ignored by the project template, init warns per missing tracked file, status says push deletes the shared template, doctor flags a tracked-but-ignored state.json"
  gaps_remaining: []
  regressions: []
deferred:
  - truth: "Push must not leak the pusher's home path when a {{ user.home }} / {{ user.xdg_config }} placeholder is followed by , ; . ) ] } or = (re-review W2; latent, no synced template uses these placeholders)"
    addressed_in: "Phase 2"
    evidence: "Phase 2 goal: 'Screen and redact all confidential tokens, raw credentials, and user-specific paths before any remote git staging or push.' Tracked in .planning/todos/pending/2026-10-19-home-path-placeholders-leak-the-pusher-s-home-path-into-shar.md"
  - truth: "A custom .gitignore that ude init kept must not be publishable over the shared gitignore.repo by a plain push"
    addressed_in: "Phase 4"
    evidence: "Phase 4 goal: '... clean manifests with safe re-initialization protections.' Operator accepted for phase 01 on 2026-10-19 (deferred-items.md); tracked in .planning/todos/pending/2026-10-19-never-publish-files-that-init-kept-over-the-shared-template.md"
  - truth: "Re-init side effects from 01-REVIEW-REINIT WR-02/WR-03/IN-01: a changed --type is not applied to mise.toml; git update-index stages an uncommitted edit of a tracked hook; a tracked hook's lost exec bit is not restored"
    addressed_in: "Phase 4"
    evidence: "Phase 4 goal: 'safe re-initialization protections'; SC2: 'Re-running `ude init` on an existing workspace detects modified files ...'. Recorded open in deferred-items.md ('candidate for Phase 4, safe re-initialization') by the operator"
human_verification:
  - test: "On a Windows host or CI runner: `cargo check --target x86_64-pc-windows-msvc`, then `ude sync status --remote <unreachable https URL>`"
    expected: "Compiles; the command returns within about 60 s with the 'timed out after 60.0s and was terminated' message, and no git / git-remote-https process is left running (JobObject kill)"
    why_human: "Only x86_64-unknown-linux-gnu is installed on this host; the #[cfg(windows)] JobObject branch of run_bounded has never been compiled or run (WINDOWS.md #1, T-01-04 / AR-01-01)"
  - test: "On Windows: run the remote-lock tests' scenario (two `ude sync` commands on the same remote and branch; kill the first while it holds the lock)"
    expected: "The second command waits on the cache lock, and proceeds once the holder is killed (std File::try_lock / LockFileEx released on process death)"
    why_human: "RemoteLockGuard's std File::try_lock path was only compiled and tested on Linux; the lock tests are cfg(target_os = linux) (WINDOWS.md #2)"
  - test: "On Windows: `ude sync pull` in an initialized project that already has .ude/state.json"
    expected: "state.json is replaced (NamedTempFile persist over an existing file succeeds on Windows) and reloads as version 2"
    why_human: "Staged state.json persist over an existing file was never compiled or run on Windows; test_state_save_mode and test_state_save_refuses_symlinked_ude_dir are Unix-only (WINDOWS.md #3)"
---

# Phase 1: Sync Engine & State Hermeticity Verification Report

**Phase Goal:** Ensure bidirectional synchronization is hermetic, crash-resilient, and atomic without phantom drift or zombie file resurrections.
**Verified:** 2026-10-20T01:17:35Z, at HEAD cfa26b2 (adds a4450d9, 1e6615c, fad8cdb, 1df10a3, 1ed695c, cfa26b2 over the previous report's 956c5e7)
**Status:** human_needed (only the three Windows items; nothing on Linux is open)
**Re-verification:** Yes. The previous report (956c5e7) had no gaps. This one is regenerated at the new HEAD. The changed code (init output, status hint, doctor check, project `.gitignore` template) got full checks with negative controls. Everything else got regression runs.

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | SC1: Upstream deletion propagates locally on `ude sync pull` via tombstones, without resurrecting on later pushes | ✓ VERIFIED | CLI probe at HEAD: status "Deleted upstream", pull "Removed .gitconfig", tombstone in state.json, recreated file "Unmanaged ... never republished", push published 0 files, template stayed absent. `src/sync/` unchanged since 956c5e7; unit tests pass in the full run |
| 2 | SC2: Multi-file sync stages before updating, so disk/permission errors cause no partial writes | ✓ VERIFIED | `StagedTransaction` (src/sync/staging.rs) unchanged; `test_pull_staging_failure_no_partial_writes` passes. Per locked D-05 the temp files sit beside each target rather than in one shadow folder, and the rename phase is not atomic as a whole (D-01, AR-01-02) |
| 3 | SC3: All git and gh calls have bounded timeouts (30 s local / 60 s network) and kill hung children | ✓ VERIFIED (Linux) | `process::run_bounded` unchanged. The only new spawns (doctor's `git ls-files` and `git check-ignore`) go through `run_bounded` with `LOCAL_TIMEOUT`. Decoy-`GIT_DIR` run of the new check: still reads the right repo (Warning, where a leak would give Pass). Every `.output()` / `.status()` / `.spawn()` left in non-test src (outside process.rs) is either past `#[cfg(test)]` at sync/mod.rs line 2168 or the `curl` call in src/semantic, which is not git or gh. Windows JobObject branch: Human Verification 1 |
| 4 | SC4: Ancestor fingerprints normalize CRLF, no false drift | ✓ VERIFIED | CLI probe at HEAD: CRLF-only conversion of .gitconfig (28 CRs) is "In Sync"; a real edit in the CRLF file is "Locally Modified" (negative control) |
| 5 | SC5: Divergent edits produce `.conflict` files with standard markers while preserving local changes | ✓ VERIFIED | CLI probe at HEAD: pull exit 1, local .gitignore byte-identical, sidecar has LOCAL / ANCESTOR / `=======` / UPSTREAM markers; push while open skipped it (exit 1, upstream untouched); `ude sync resolve` cleared it |
| 6 | SC6: Outbound push verifies branch continuity and rejects divergent candidates safely | ✓ VERIFIED | `push_direct` / `check_continuity` / `rollback_commit` unchanged since a6cc3c3; continuity and upstream-moved tests pass in the full run |
| 7 | 01-06 backstop: two `ude sync` commands on the same remote and branch serialize on the cache lock | ✓ VERIFIED | `src/sync/` unchanged since the lock probe passed at 956c5e7 (3.32 s waiting vs 0.10 s alone). Not re-run at this HEAD |
| 8 | 01-01 backstop: deadline boundary inclusive, 10 ms poll | PASSED (override) | Orchestrator ruling, not the operator's: accepted on the implementation read plus passing timeout tests. An exact-millisecond boundary test would itself be flaky |
| 9 | Goal ("no zombie resurrections"): re-running `ude init` keeps every prior record (ancestor, tombstone, open conflict) and writes no recorded file | ✓ VERIFIED | Re-init probe at HEAD, cases A–E: graphify block not re-appended, local hook edit kept, sidecar edits and conflict record kept with resolve exit 0, deleted `.gitconfig` not resurrected, tombstone `['.gitconfig']` kept in a v2 state.json. Controls: first init still appends the block and still creates `.gitconfig` |
| 10 | 01-REVIEW-REINIT decision finding (WR-01) is closed: on a second machine, init followed by a plain push does not silently delete shared templates, and init warns about every tracked file that is missing | ✓ VERIFIED | Second-machine probe at HEAD, below. Machine A commits a project but leaves `.gitconfig` and `pre-push` uncommitted; machine B clones it. Default case: state.json is not committed, B's init recreates both files, push publishes 0 files, shared templates byte-identical. Control (legacy project that committed state.json): init prints one warning per file, status and doctor warn, and the push that follows does delete both templates. So the control trips, and the deletion is warned about, not silent. All 6 new e2e tests **fail on the pre-fix source** at their target assertion |

**Score:** 10/10 truths verified (1 by override; 0 present-but-behavior-unverified). Truth 10 is new in this report.

#### Second-machine probe at HEAD (truth 10)

Upstream templates are one shared directory both clones see. Machine A runs `ude init`, then `git add -A`, then un-stages `.gitconfig` and `scripts/hooks/pre-push`, then commits. Machine B runs `git clone`, `ude init`, `ude doctor`, `ude sync status`, and a plain `ude sync push`.

| Case | state.json in B's clone | B init | doctor state check | push | Shared templates |
|------|------------------------|--------|-------------------|------|------------------|
| 1. Default (template ignores state.json) | absent | recreates both files, "initialization complete", no warning | Pass | 0 files | byte-identical |
| 2. Control: legacy commit of state.json (`add -f`) | present | `! .gitconfig is tracked but missing; ... a plain ude sync push will delete its shared template` (and the same for `pre-push`), with a restore line each; "complete; 2 tracked file(s) missing" | Warning, with `git rm --cached .ude/state.json` | deletes `gitconfig.repo` and `hooks/pre-push`, exit 0 | changed (expected; warned in advance) |
| 3. Project with its own `.gitignore` before the first init | present (`add -A` commits it, because the kept `.gitignore` has no state.json rule) | warns for `.gitconfig` | **Pass** (the file is tracked but not ignored, which this check does not cover) | (not run) | - |

Doctor exits 2 in every case. The cause is the host signing checks failing under the fake HOME (allowed_signers, gpg.format, commit.gpgsign), not the new check.

#### Negative controls for the new tests

| Run | Result |
|-----|--------|
| HEAD's `tests/e2e_lifecycle.rs` on 956c5e7 source, the 6 new tests | 6 failed, all assertion panics |
| The same on fad8cdb source, which already ignores state.json but has no doctor check and no `.tmp*` rule | `test_doctor_reports_tracked_per_machine_state_with_remedy` fails at its own assertion (no "Per-machine sync state" check); `test_project_gitignore_ignores_state_staging_temp_files` fails at `check_ignored(.ude/.tmpQ1w2E3)`; `test_project_gitignore_ignores_per_machine_state` passes (its fix is in 1e6615c) |

### Deferred Items

| # | Item | Addressed In | Evidence |
|---|------|-------------|----------|
| 1 | Home-path placeholder followed by `, ; . ) ] } =` is not round-tripped, so push can publish the pusher's home path (W2, latent) | Phase 2 | Phase 2 goal covers redacting user-specific paths before push; pending todo exists |
| 2 | A custom `.gitignore` kept by init can be pushed over the shared template (operator-accepted 2026-10-19) | Phase 4 | Phase 4 goal covers safe re-initialization; pending todo exists |
| 3 | Three re-init side effects (REINIT WR-02, WR-03, IN-01): a changed `--type` is not applied to `mise.toml`; `update-index` stages an uncommitted hook edit; a lost exec bit is not restored | Phase 4 | deferred-items.md (operator-recorded, "candidate for Phase 4"); Phase 4 SC2 covers re-init on an existing workspace |

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/process.rs` | Bounded runner, env scrub, redaction | ✓ VERIFIED | unchanged since a6cc3c3 |
| `src/sync/staging.rs` | Stage-validate-commit transaction | ✓ VERIFIED | unchanged |
| `src/sync/state.rs` | v2 schema, loud failure, forward guard, migration, staged save, `has_record` | ✓ VERIFIED | unchanged since 956c5e7 |
| `src/sync/merge.rs` | diffy diff3 merge and sidecar renderer | ✓ VERIFIED | unchanged |
| `src/sync/mod.rs` | Deletion states, conflicts, resolve, push skips, continuity | ✓ VERIFIED | unchanged since 956c5e7 |
| `src/main.rs` | init carries prior state; per-file missing-tracked warning; step lines; status hint | ✓ VERIFIED | `missing_tracked_files` + `print_missing_tracked_warning` wired after the state save; all 11 managed paths are covered by one of the three step lines |
| `src/audit/mod.rs` | doctor's per-machine state check | ✓ VERIFIED | `sync_state_tracking_status` pushed into the repo reports; bounded git calls |
| `templates/projects/generic/gitignore.repo` | `*.conflict`, `.ude/state.json`, `.ude/.tmp*` | ✓ VERIFIED | all three rules present |
| `tests/e2e_lifecycle.rs` | Hermetic e2e coverage | ✓ VERIFIED | 36 e2e tests pass (30 before, plus 6 new) |

`verify.artifacts`: 32/32 pass across the 11 plans.

### Key Link Verification

`verify.key-links` reports 6/26 verified. All 20 unverified rows fail with "Source file not found", which means the verb could not parse the link, not that the link is missing. The plans put symbol names in `from:` (for example `src/sync/mod.rs SyncEngine`), and the verb accepts only a file path there. I checked them by grep at HEAD:
- `run_local`/`run_network`: 23 uses in sync/mod.rs
- `StagedTransaction` and `stage_delete`: 8 each
- `merge_texts`, `render_conflict_sidecar` and `fn resolve_conflict`: present
- `check_continuity` and `rollback_commit`: present
- `diff-tree` and `--name-status`: present
- main.rs: `resolve_conflict`, `EXIT_DRIFT` and `EXIT_HARD_FAILURE` dispatch present

The behavioural tests cover the same links. Links added by the new commits:

| From | To | Status | Details |
|------|----|--------|---------|
| init (main.rs) | `missing_tracked_files` → `print_missing_tracked_warning` | WIRED | called after `save_to_dir`, with the prior state and `managed_rels`. Excludes open conflicts and tombstones (`test_reinit_does_not_warn_of_deletion_for_open_conflict` has a negative control) |
| warning text | `push_boundary` | WIRED | Publishable / ForceOnly / Never each give their own wording (`test_reinit_missing_file_warning_follows_push_boundary`) |
| `Auditor` repo reports | `sync_state_tracking_status` → `run_bounded` | WIRED | probe cases 1–2 plus the decoy-`GIT_DIR` run |

### Data-Flow Trace (Level 4)

Not applicable. There is no rendered UI. The data the CLI prints comes from state.json and the filesystem, and the probes above exercise it.

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Full suite (one run) | `env -u TYPESAFE_API_KEY cargo test` | 135 unit + 36 e2e passed, exit 0 | ✓ PASS |
| Lint / format | `cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --check` | both exit 0 | ✓ PASS |
| New tests fail without the fix | HEAD tests on 956c5e7 and on fad8cdb source, scratch worktree | as in the table above | ✓ PASS (negative control) |
| Second-machine scenario through the binary | scratch clones, shared templates, fake HOME/XDG | as in the probe table above | ✓ PASS |
| SC1/SC4/SC5 through the binary | scratch project, fake HOME/XDG | as in the truths table | ✓ PASS |
| Re-init cases A–E | scratch project, fake HOME/XDG | as in truth 9 | ✓ PASS |
| Doctor check ignores an inherited `GIT_DIR` | decoy pointing at a repo where state.json is untracked | Warning both with and without the decoy | ✓ PASS |
| Cache-lock serialization | not re-run; `src/sync/` unchanged since it passed at 956c5e7 | - | carried forward |
| Windows JobObject / lock / state save | n/a | no Windows target on host | ? SKIP → human |

### Probe Execution

No `scripts/*/tests/probe-*.sh` exist and no plan declares one. Step 7c: not applicable.

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| SYNC-01 | 01-07 | Tombstones; upstream deletions propagate without resurrection | ✓ SATISFIED | Truths 1, 9, 10 |
| SYNC-02 | 01-04, 01-06 | Two-phase staging before updating the working tree | ✓ SATISFIED | Truth 2 (per-target temp files under D-05, not a single staging directory); stray `.ude/.tmp*` files are now ignored by git |
| SYNC-03 | 01-01, 01-02, 01-03 | Bounded git/gh timeouts with cancellation | ✓ SATISFIED (Linux) | Truths 3 and 8; Windows routed to human |
| SYNC-04 | 01-05 | CRLF-normalized SHA-256 fingerprints | ✓ SATISFIED | Truth 4 |
| SYNC-05 | 01-08, 01-09, 01-10 | `.conflict` files with standard markers, local edits preserved | ✓ SATISFIED | Truths 5 and 9 |
| SYNC-06 | 01-10, 01-11 | Safe push candidates and branch-head continuity | ✓ SATISFIED | Truth 6 |

All six IDs are claimed by at least one plan's `requirements:`. REQUIREMENTS.md maps no other ID to Phase 1, so no requirement is orphaned.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| added lines in src/main.rs, src/audit/mod.rs, gitignore.repo, tests/e2e_lifecycle.rs (956c5e7..HEAD) | - | TBD / FIXME / XXX / TODO / HACK / todo!() / unimplemented!() / placeholder | none found | - |
| src/audit/mod.rs | `sync_state_tracking_status` | The check covers only "tracked **and** ignored". A tracked state.json in a project whose `.gitignore` lacks the rule reports Pass (probe case 3) | ⚠️ Warning | Init's per-file warning still fires in that case, so the template deletion is still not silent. But the root-cause fix (the ignore rule) and the doctor check both miss projects whose `.gitignore` init kept |
| src/main.rs | status arm | After the sidecar is hand-deleted, status still says "see .gitignore.conflict" | ℹ️ Info | Wording only; the conflict correctly stays open |

### Human Verification Required

1. **Windows timeout kill (JobObject).** On Windows, run `cargo check --target x86_64-pc-windows-msvc`, then `ude sync status --remote <unreachable https URL>`. Expected: it returns within about 60 s with the timeout message, and no git processes are left behind. Why human: this host has no Windows target.
2. **Windows cache lock.** Run two sync commands on the same remote and branch on Windows, then kill the first. Expected: the second waits, then proceeds. Why human: the lock tests are Linux-only.
3. **Windows state.json replace.** Run `ude sync pull` over an existing state.json on Windows. Expected: the file is replaced and reloads as version 2. Why human: this path has never been compiled or run on Windows.

These three items stay human verification by instruction (WINDOWS.md); none is waived. The 01-01 deadline-boundary sign-off is not here: an orchestrator ruling resolved it (truth 8).

### Gaps Summary

No gaps block the phase goal. All six roadmap criteria hold at HEAD cfa26b2 on Linux. The 01-REVIEW-REINIT decision finding is closed by its options (a) and (c): init warns per missing tracked file, and the project template ignores state.json. On top of that, status wording changed and doctor gained a check. Option (b), a restore path in init, was not implemented, and none was required.

What this does not establish:
- **Windows:** nothing here covers Windows. The three WINDOWS.md entries are still open.
- **Crash resilience:** this covers failures and process death before the rename phase. It does not cover a crash or power loss during the rename phase (no fsync and no journal; D-01, accepted as AR-01-02).
- **Option (c) reaches only template-generated `.gitignore` files:**
  - A project that had its own `.gitignore` before its first init never gets the state.json rule. Init keeps that file. Pull's three-way merge sees no upstream change against the template ancestor, so it keeps the local file.
  - In such a project, a plain `git add -A` commits state.json, and doctor reports Pass (probe case 3).
  - Projects initialized before 1e6615c receive the rule only through a pull of an unmodified `.gitignore`.
  - In all of these cases the per-file init warning and the status hint are the only protection. After a warning, a plain push still deletes the templates.
- **`--remote` not probed:** the second-machine probe used a shared local template directory, not a git-backed `--remote` upstream. For a git-backed upstream, the warning's "copy templates/..." path refers to the local template root.
- **No code review of the new diff:** 01-REVIEW-REINIT.md reviewed up to 956c5e7. Commits 1e6615c and cfa26b2 have not been reviewed. My probes and negative controls cover the behaviour they claim; they are not a diff review.
- **Untriaged first-review findings:** IN-01..IN-07 are still `open` in 01-REVIEW-DISPOSITION.md. Two of them affect exit codes:
  - IN-01: a successful push is reported as exit 2 when an escaped descendant holds the pipes.
  - IN-02: a remote direct push exits 0 when saving state fails after publishing.
- **Template-logic push guard (re-review W3):** the guard still checks only `.jinja` files. This is latent: no synced, non-`.jinja` template contains `{{`. It is tracked as a pending todo, and no later phase clearly owns it.

---

_Verified: 2026-10-20T01:17:35Z_
_Verifier: Claude (gsd-verifier)_
