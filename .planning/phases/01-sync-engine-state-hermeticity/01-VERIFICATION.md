---
phase: 01-sync-engine-state-hermeticity
verified: 2026-10-20T20:49:39Z
status: passed
score: 12/12 must-haves verified
covered_files: [".config/nextest.toml", ".planning/phases/01-sync-engine-state-hermeticity/01-01-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-01-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-02-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-02-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-03-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-03-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-04-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-04-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-05-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-05-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-06-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-06-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-07-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-07-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-08-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-08-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-09-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-09-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-10-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-10-SUMMARY.md", ".planning/phases/01-sync-engine-state-hermeticity/01-11-PLAN.md", ".planning/phases/01-sync-engine-state-hermeticity/01-11-SUMMARY.md", "Cargo.toml", "src/audit/mod.rs", "src/cli/mod.rs", "src/main.rs", "src/process.rs", "src/sanitizer/mod.rs", "src/semantic/mod.rs", "src/sync/merge.rs", "src/sync/mod.rs", "src/sync/staging.rs", "src/sync/state.rs", "src/template/mod.rs", "templates/projects/generic/gitignore.repo", "tests/e2e_lifecycle.rs"]
covered_digest: "v3:sha256:59c4da8f14d79d2f6fef1a96124d9e3673a5caffcb6ccefbedce58da611d457b"
behavior_unverified: 0
overrides_applied: 1
overrides:
  - must_have: "01-01 backstop: deadline boundary is inclusive (elapsed >= timeout kills the group; a child that exits before the poll observes the deadline returns its real status; POLL_INTERVAL = 10 ms)"
    reason: "Accepted on the evidence: run_bounded checks child.try_wait() before the `Instant::now() >= deadline` kill and returns the real status on Ok(Some), POLL_INTERVAL is Duration::from_millis(10), and the timeout tests pass. A test pinned to the exact millisecond boundary would itself be flaky."
    accepted_by: "orchestrator ruling (not the operator), on the implementation read plus passing timeout tests"
    accepted_at: "2026-10-20T00:33:36Z"
re_verification:
  previous_status: gaps_found
  previous_score: 11/12
  previous_head: 99ffc22
  gaps_closed:
    - "A file init kept is never deleted by pull on the strength of an ancestor sync never saw: a committed own .gitignore with no template and an uncommitted pre-init edit on a fresh clone both survive pull (539a3d7)"
  gaps_remaining: []
  regressions: []
deferred:
  - truth: "Windows runtime behaviour of the bounded runner (JobObject kill), the remote cache lock (release on process death) and staged state.json replace"
    addressed_in: "Phase 999.1 (backlog)"
    evidence: "Phase 999.1 goal: 'On a Windows host, run the three checks waived for phase 01 on 2026-10-20 (operator) ...'. 01-UAT.md tests 1-3: skipped, operator waiver 2026-10-20. Compile and clippy -D warnings for x86_64-pc-windows-msvc re-checked at HEAD 539a3d7 on Linux (exit 0; nothing linked or run)"
  - truth: "Push must not leak the pusher's home path when a {{ user.home }} / {{ user.xdg_config }} placeholder is followed by , ; . ) ] } or = (re-review W2; latent)"
    addressed_in: "Phase 2"
    evidence: "Phase 2 goal: 'Screen and redact all confidential tokens, raw credentials, and user-specific paths before any remote git staging or push.' Pending todo 2026-10-19-home-path-placeholders-leak-..."
  - truth: "A file that ude init kept must not be publishable by a plain push. This includes an uncommitted own .gitignore with no gitignore template, which push still publishes as a new gitignore.repo (probe case 4, unchanged by 539a3d7)"
    addressed_in: "Phase 4"
    evidence: "Phase 4 goal: '... safe re-initialization protections.' Operator accepted for phase 01 on 2026-10-19 ('A custom .gitignore that ude init kept must not be publishable ... by a plain push'); pending todo 2026-10-19-never-publish-files-that-init-kept-over-the-shared-template"
  - truth: "Re-init side effects (01-REVIEW-REINIT WR-02, WR-03, IN-01): explicit --type change not applied to mise.toml; update-index stages an uncommitted hook edit; lost exec bit not restored"
    addressed_in: "Phase 4"
    evidence: "deferred-items.md ('candidate for Phase 4, safe re-initialization'); Phase 4 SC2 covers re-init on an existing workspace"
---

# Phase 1: Sync Engine & State Hermeticity Verification Report

**Phase Goal:** Ensure bidirectional synchronization is hermetic, crash-resilient, and atomic without phantom drift or zombie file resurrections.
**Verified:** 2026-10-20T20:49:39Z, at HEAD 539a3d7. The only code change since the previous report (99ffc22) is 539a3d7: src/main.rs +27/-9 and tests/e2e_lifecycle.rs +74/-8.
**Status:** passed (on Linux; Windows runtime deferred to backlog 999.1 by operator waiver)
**Re-verification:** Yes. The previous gap is closed:
- A kept file with no template was being deleted by pull. A committed file that init keeps with no template now gets a kept tombstone and reads Unmanaged.
- I re-ran my own probes against the HEAD binary, including what happens when the template later appears or reappears.
- I found no new path that deletes, publishes or drifts.

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | SC1: Upstream deletion propagates locally via tombstones, without resurrecting on later pushes | ✓ VERIFIED | Single machine: carried forward (sync code unchanged). Fresh clone: probe case 1 shows status Unmanaged, a kept tombstone in state.json, push publishing 0 files, template still absent, and the file kept by pull. Mutants: without the tombstone, status reads Locally Modified; with 99ffc22's own-bytes record, it reads Deleted upstream. Both fail the test's Unmanaged assertion (line 3193) |
| 2 | SC2: Staged writes, no partial writes on errors | ✓ VERIFIED | staging tests pass; staging.rs unchanged |
| 3 | SC3: Bounded git/gh timeouts with hung-child kill | ✓ VERIFIED (Linux) | no new spawns; timeout tests pass; Windows runtime deferred to 999.1 |
| 4 | SC4: CRLF-normalized fingerprints, no false drift | ✓ VERIFIED | CRLF tests pass. Probe case 9: a template that reappears equal to the Unmanaged file except for CRLF reads In Sync |
| 5 | SC5: Divergent edits produce `.conflict` files and keep local changes | ✓ VERIFIED | Conflict tests pass. Probe cases 7 and 5c: a template that (re)appears with different content gives Conflicted. Pull exits 1, writes a sidecar with LOCAL / ANCESTOR (empty) / `=======` / UPSTREAM markers, and leaves the local file byte-identical. Push exits 1 and leaves upstream unchanged |
| 6 | SC6: Push verifies branch continuity | ✓ VERIFIED | continuity tests pass; push code unchanged |
| 7 | 01-06 backstop: cache lock serializes | ✓ VERIFIED (Linux) | lock tests pass |
| 8 | 01-01 backstop: deadline boundary, 10 ms poll | PASSED (override) | carried forward; process.rs unchanged |
| 9 | Re-init keeps every prior record | ✓ VERIFIED | Re-init tests pass. Probe 5b: re-init without `--type` keeps the kept tombstone, still Unmanaged |
| 10 | REINIT WR-01: state.json ignored; per-file missing warning | ✓ VERIFIED | its tests pass |
| 11 | REVIEW fixes CR-01, CR-02, WR-02, WR-03, WR-04 hold | ✓ VERIFIED | tests pass; 539a3d7 does not touch those paths |
| 12 | A file init kept is never deleted by pull on an ancestor sync never saw | ✓ VERIFIED | Probe case 5: committed own `.gitignore`, no template. It reads Unmanaged; pull exits 0 and keeps `secrets/`; push publishes 0 files and creates no template. Probe case 3: an uncommitted edit before a fresh clone's first init survives pull. Mutants fail `test_init_keeps_committed_gitignore_when_template_is_absent` at its Unmanaged assertion (line 3237) |

**Score:** 12/12 truths verified (1 by override; 0 present-but-behavior-unverified)

#### Probes against the HEAD 539a3d7 binary

Scratch setup: a shared template dir linked as `templates/` into each clone, fake HOME/XDG, no inherited git variables.

| # | Scenario | Result | Verdict |
|---|----------|--------|---------|
| 1 | Fresh clone; unedited committed `.gitconfig`; template deleted upstream | Init: "no shared template, so sync leaves it unmanaged". Status Unmanaged; tombstone `kept_unmanaged: true`, no files record. Push 0 files, template absent. Pull exit 0, file present | ✓ |
| 2 | Committed edit of `.gitconfig` (made on A); template deleted upstream | A: Conflicted; pull exit 1, writes `.gitconfig.conflict`, file kept. B: Unmanaged; push 0 files, template absent; pull exit 0, file and edit kept | ✓ no deletion and no publication on either machine (A conflicts, B ignores) |
| 3 | Fresh clone; uncommitted edit before the first init; template deleted upstream | Unmanaged; pull exit 0, edit kept; push 0 files, template absent | ✓ (lost on 99ffc22) |
| 4 | No git; uncommitted own `.gitignore`; no template | Locally Modified; push publishes 1 file, which creates `gitignore.repo` | deferred to Phase 4 (operator-accepted; deliberately out of scope for 539a3d7) |
| 5 | Existing repo; committed own `.gitignore`; no template; first init | Unmanaged; pull keeps it; push 0 files, no template created | ✓ (deleted on 99ffc22) |
| 5b | ...then re-init without `--type` | still Unmanaged; tombstone kept | ✓ |
| 5c | ...then a template is created upstream with different content | Conflicted; push exit 1, upstream stays `target/`; pull exit 1, local `.gitignore` kept, sidecar written | ✓ |
| 6 | Case 1, then the template reappears identical to B's file | In Sync; pull records a files entry and clears the tombstone; push 0 files, template unchanged. A later local edit reads Locally Modified (normal managed flow) | ✓ no drift; the file becomes managed again |
| 7 | Case 1, then the template reappears with different content | Conflicted; push exit 1 (upstream unchanged); pull exit 1 (local unchanged, sidecar with an empty ANCESTOR section). After `ude sync resolve .gitconfig` ("local content will be published on the next push"), the tombstone is cleared, status is Locally Modified, and push publishes 1 file over the reappeared template | ✓ by design: publishing happens only after an explicit resolve that says it will publish (D-09) |
| 9 | Case 1, then the template reappears equal except for CRLF | In Sync | ✓ no phantom drift |

**The `(None, Some, Some)` row (tombstone kept, template present):**
- **Equal by fingerprint** (CRLF-insensitive): In Sync. Pull then records an ancestor; `record_file` clears the tombstone (invariant I1).
- **Different:** Conflicted. Pull and push both stop with exit 1, and nothing is deleted or published without a resolve.
- **`deleted_hash`:** written by init but never read for a decision (only `record_tombstone` call sites use it outside tests).

#### Negative controls for the fix's tests (orchestrator claims checked)

| Mutant (scratch worktree at HEAD) | `test_fresh_clone_does_not_resurrect...` | `test_init_keeps_committed_gitignore...` | `test_init_keeps_uncommitted_gitignore...` |
|-----------------------------------|----|----|----|
| M1: `record_tombstone` call removed | FAILED at line 3193 (status "Locally Modified") | FAILED at line 3237 | passed |
| M2: 99ffc22 behaviour (own bytes as ancestor) | FAILED at line 3193 (status "Deleted upstream") | FAILED at line 3237 | passed |

Both claims hold. Lines 3193 and 3237 are the Unmanaged-status assertions, so neither mutant exercises the tests' pull/push assertions. Probe cases 1, 3 and 5 cover those behaviours.

### Deferred Items

| # | Item | Addressed In | Evidence |
|---|------|-------------|----------|
| 1 | Windows runtime: JobObject kill, cache-lock release, state.json replace | Phase 999.1 (backlog) | Operator waiver 2026-10-20; Windows-target clippy passes at HEAD (Linux cross-check only) |
| 2 | Home-path placeholder followed by `, ; . ) ] } =` (W2, latent) | Phase 2 | Phase 2 goal; pending todo |
| 3 | Kept files publishable by a plain push (including case 4, which creates `gitignore.repo`) | Phase 4 | Operator-accepted 2026-10-19; pending todo |
| 4 | Re-init side effects (REINIT WR-02, WR-03, IN-01) | Phase 4 | deferred-items.md |

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/process.rs` | Bounded runner, env scrub | ✓ VERIFIED | unchanged |
| `src/sync/staging.rs` | Stage-validate-commit | ✓ VERIFIED | unchanged |
| `src/sync/state.rs` | v2 schema, tombstones, staged save | ✓ VERIFIED | unchanged; `record_tombstone(.., kept_unmanaged)` reused by init |
| `src/sync/merge.rs` | diff3 merge, sidecar renderer | ✓ VERIFIED | unchanged |
| `src/sync/mod.rs` | Deletion states, conflicts, resolve, push skips, continuity | ✓ VERIFIED | unchanged |
| `src/main.rs` | init: prior state, symlink checks, fresh-clone keep, ancestors / kept tombstones | ✓ VERIFIED | no-template branch records a kept tombstone and says so |
| `templates/projects/generic/gitignore.repo` | `*.conflict`, `.ude/state.json`, `.ude/.tmp*` | ✓ VERIFIED | unchanged |
| `tests/e2e_lifecycle.rs` | Hermetic e2e coverage | ✓ VERIFIED | 53 e2e tests pass; no `set_var` / `remove_var` in src or tests |

`verify.artifacts`: 32/32 at an earlier HEAD; artifact paths unchanged since.

### Key Link Verification

| From | To | Status | Details |
|------|----|--------|---------|
| init: kept committed file, template present | template bytes as ancestor | WIRED | `test_first_init_on_fresh_clone_keeps_committed_edits`; `pre-commit` In Sync control |
| init: kept committed file, template absent | `record_tombstone(rel, fp, true)` → status Unmanaged | WIRED | cases 1, 2, 3, 5; mutants M1/M2 |
| Unmanaged status | push never publishes / pull never deletes | WIRED | cases 1, 3, 5 (push 0 files; files kept) |
| Template (re)appears | `(None, Some, Some)` → In Sync (records an ancestor, clears the tombstone) or Conflicted | WIRED | cases 6, 7, 5c, 9 |
| init "Kept committed" line | `unmanaged_kept` | WIRED | "no shared template, so sync leaves it unmanaged" printed in cases 1, 2, 3, 5 |

Earlier links (runner, staging, merge, continuity, symlink refusal) are unchanged and their tests pass.

### Data-Flow Trace (Level 4)

Not applicable: no rendered UI.

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Full suite (one run) | `env -u TYPESAFE_API_KEY cargo test --workspace` | 136 unit + 53 e2e passed, exit 0 | ✓ PASS |
| Lint / format | `cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --check` | exit 0, exit 0 | ✓ PASS |
| Windows target | `cargo clippy --target x86_64-pc-windows-msvc --workspace --all-targets -- -D warnings` | exit 0 (check only) | ✓ PASS |
| Fix tests bite | mutants M1 and M2 | as in the table above | ✓ PASS |
| Kept-file and reappearance probes | scratch clones, HEAD binary | as in the probe table | ✓ PASS (case 4 deferred) |
| Debt markers in added lines (f33072b..HEAD) | rg `TBD\|FIXME\|XXX\|TODO\|HACK\|todo!(\|unimplemented!(` | none | ✓ PASS |

### Probe Execution

No `scripts/*/tests/probe-*.sh` exist and no plan declares one. Step 7c: not applicable.

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| SYNC-01 | 01-07 | Tombstones; deletions propagate without resurrection | ✓ SATISFIED | Truth 1, including the fresh clone |
| SYNC-02 | 01-04, 01-06 | Two-phase staging | ✓ SATISFIED | Truth 2 |
| SYNC-03 | 01-01, 01-02, 01-03 | Bounded git/gh timeouts | ✓ SATISFIED (Linux) | Truths 3, 8; Windows deferred to 999.1 |
| SYNC-04 | 01-05 | CRLF-normalized fingerprints | ✓ SATISFIED | Truth 4, case 9 |
| SYNC-05 | 01-08, 01-09, 01-10 | `.conflict` files, local edits preserved | ✓ SATISFIED | Truths 5, 12 |
| SYNC-06 | 01-10, 01-11 | Safe push candidates, continuity | ✓ SATISFIED | Truth 6 |

All six IDs are claimed by plan `requirements:` fields; none are orphaned.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| src/main.rs (status arm) | Unmanaged label | Status says "deleted upstream; kept locally, never republished" for a file whose template never existed (case 5) | ℹ️ Info | Wording only; behaviour is correct |
| src/sync/mod.rs | `scan_project` `?` on symlink refusal | One symlinked managed file fails every sync command (01-REVIEW WR-01) | ⚠️ Warning | Fail-closed; operator-deferred to a todo, no phase owns it |
| 01-REVIEW-DISPOSITION.md | IN-01..IN-07 | Still `open` (untriaged) | ℹ️ Info | - |

### Human Verification Required

None. The Windows checks are operator-waived and backlogged (Phase 999.1).

### Gaps Summary

No gaps. All six roadmap criteria and the plan/review must-haves hold at HEAD 539a3d7 on Linux. The fresh-clone path no longer resurrects a deleted template, and it no longer deletes a kept file (committed or edited) when no template exists. A template that later (re)appears either converges to In Sync or stops at a conflict that only an explicit resolve can publish over.

Machines A and B can still end up in different states, by design:
- **Unedited file, template deleted upstream:** A's pull removes the file, while a fresh clone B keeps it as Unmanaged. B cannot tell "deleted upstream" from "never existed".
- **Edited file, template deleted upstream:** A conflicts, while B ignores it.

Neither machine deletes data or publishes anything.

What this does not establish:
- **Windows:** not covered; compile/clippy only.
- **Crash during the rename phase:** not covered (no fsync, no journal; AR-01-02).
- **`--remote` git-backed upstream:** the probes used a shared local template dir. The kept-tombstone decision is made in init, independently of the remote type, but this was not run.
- **Mutant coverage:** M1 and M2 trip only the tests' status assertions. The tests' pull/push assertions are covered by my probes, not by a mutant.
- **Case 4** (an uncommitted own `.gitignore` with no template is published as a new `gitignore.repo`) remains, under the operator's Phase 4 deferral.

---

_Verified: 2026-10-20T20:49:39Z_
_Verifier: Claude (gsd-verifier)_
