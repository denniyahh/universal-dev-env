# Phase 01 plans — external adversarial review (2026-10-17)

- **Target:** `.planning/phases/01-sync-engine-state-hermeticity/01-01..01-11-PLAN.md` at commit `dce84d4` (read-only `git archive` snapshot), with CONTEXT/RESEARCH/VALIDATION/PATTERNS, REQUIREMENTS, ROADMAP, REMEDIATION_PLAN as context.
- **Lens:** adversarial. Prompt: `prompt.txt`.
- **Lanes (all completed, none dropped):** agy `gemini-3.8-flash-high` (exit 0, 19 citations) · codex `gpt-5.6-terra` high, read-only sandbox (exit 0) · pi `deepseek-flash` (exit 0, 35 citations). All three verdicts: do not execute as written.
- **Verification:** every finding below was checked by the orchestrator against the snapshot (`src/`, plan line text). Status column reflects that check, not lane agreement.

## Confirmed — deterministic gate failures (executor halts)

| # | Finding | Evidence (checked) | Lanes |
|---|---------|--------------------|-------|
| 1 | 01-02 T2 gate requires exactly 1 `Command::new("git")` in `tests/e2e_lifecycle.rs`; the action converts only two tests, leaving 4 doctor-test sites → count 5, gate fails. Converting the doctor sites instead breaks them: they write `--global` git config into the fake `HOME` the `ude` binary reads, and `GitFixture` redirects `GIT_CONFIG_GLOBAL`. | 13 sites today (`:99 :104 :193 :212` in doctor tests); `01-02-PLAN.md:121` (scope), `:128` (gate `= 1`), `:134`, `:135` | agy, pi |
| 2 | 01-07 acceptance "`rg -n 'push to propagate' src/main.rs` prints one match", but the same task mandates two such strings in `src/main.rs` (status arm and pull arm). | `01-07-PLAN.md:137`, `:169`, `:190` | pi |

## Confirmed — design defects (would ship wrong behaviour)

| # | Finding | Evidence (checked) | Lanes |
|---|---------|--------------------|-------|
| 3 | `resolve` saves state (conflict cleared, ancestor advanced) **then** removes the sidecar. If removal fails, the sidecar is orphaned for good: a second `resolve` says "no open conflict" and pull only regenerates sidecars for ConflictPending. | `01-10-PLAN.md:125` ("Save state (atomic). Then remove the sidecar") | codex, pi |
| 4 | Hook mode regression: `file_mode_for` keeps an existing target's mode, so an existing `scripts/hooks/*` at 0644 stays non-executable. Today's pull forces 0755 on every hook/scratch write. No test covers it. | `src/sync/mod.rs:670-680`; `01-06-PLAN.md:26`, `:144` | pi |
| 5 | State-machine intersections undefined across plans: (a) open conflict record + (ancestor present, local = ancestor, upstream absent) is UpstreamDeleted in 01-07's table but ConflictPending in 01-09; (b) both-absent/no-record convergence records **no** tombstone in 01-09 (c) while 01-10 `resolve` records one "from the local fingerprint when no ancestor exists", and with local absent there is none. | `01-07-PLAN.md:97`; `01-09-PLAN.md:170`; `01-10-PLAN.md:125` | pi |
| 6 | 01-09 never says how pull detects a converged conflict: scan classifies it "as if it had no conflict record", which erases the signal pull needs to clear the record and sidecar. Related: 01-07 adds deletion-only pulls but doesn't mention the existing early return that returns `Success{0}` when nothing is to be written. The TDD RED tests would expose both, but the executor has to invent the mechanism. | `01-09-PLAN.md:170`; `src/sync/mod.rs:641` | agy, pi |
| 7 | State staging creates `.ude/` and writes the temp file with no `validate_safe_write_target`, although CONTEXT requires it "on every staged target". A symlinked `.ude` sends state writes outside the project. The gap already exists today (`save_to_dir` doesn't validate either). | `01-04-PLAN.md:145`; `01-CONTEXT.md:91`; `src/sync/state.rs:50-59` | codex |
| 8 | `resolve` is a new read-modify-write of `state.json` that deliberately takes no lock. A concurrent pull can overwrite its result (lost update). Pre-existing pattern: local-template mode has no lock at all. **Disposition: accepted limitation (operator, 2026-10-18), recorded in 01-CONTEXT.md Deferred Ideas.** The read-modify-write windows are milliseconds (`src/sync/mod.rs:646→693`, `src/main.rs:520→524`) and no file content is lost. | `01-10-PLAN.md:127`; `src/main.rs:369-386` | codex, pi |

## Confirmed — low severity / claim mismatches

- 01-11 threat register T-01-31 claims "the cache never carries an unpublished commit", but the action rolls back only on `[rejected]`; other push failures leave the cache commit (as today). The next command's `ensure_remote_repo` refresh likely discards it (not verified). `01-11-PLAN.md:106`, `:217`. (agy)
- `ude sync status` will print "In Sync" for a managed file that is absent locally, absent upstream and has no ancestor: the state D-12/D-16 deletions leave behind. `scan_project` walks a static list (`src/sync/mod.rs:314-328`), `(None, None)` maps to InSync (`:211-216`), and 01-07's table has no `none + T | absent | absent` row. (agy)
- Timing bounds have about 0.7 s of headroom (`01-01-PLAN.md:128` "<2 s" with a 0.3 s limit plus a 1 s grace), and wave-1 plans may build concurrently. (pi)
- `setsid` missing → the test panics instead of skipping (`01-01-PLAN.md:132`). Fine on this host; red on minimal containers. (pi)
- `StagedTransaction` `Drop` order (pi C2) is accurate Rust: `drop()` runs before the fields drop, and a `Drop` type's fields can't be moved out. It's implementable (drain the temps first, `Option`/`mem::take`), and `test_staging_abort_removes_created_dirs` would catch a wrong order. Not a blocker.

## Disputed / not confirmed

- **agy #5, migration causes false Conflicted:** only when the upstream template bytes contain CRLF. 0 of 18 template files contain `\r`, so for this repo the raw hash equals the fingerprint and the file classifies LocallyModified.
- **codex, migration not persisted by read-only commands:** D-07 says "on first load ... migrate". The plan migrates in memory on load and persists on the next save. That's a reading of D-07, not a defect.
- **codex, "clean pull never leaves .conflict" is vacuous:** weak. The truth is about pull not creating sidecars; whether a user-created orphan `*.conflict` should be deleted is a separate question.
- **agy #6, host `core.hooksPath` defeats the race hook:** unverified. Cheap hardening: set `GIT_CONFIG_NOSYSTEM=1` on the `ude` invocation in that e2e test.
- **codex, PR-mode stale base untested:** consistent with the D-18 scope note (PR mode exempt). Awareness only.

## Residual risk

The internal checker (same model family as the planner) passed these plans three times. The external lanes found two deterministic gate failures and several cross-plan state-machine gaps it missed. The pattern pi named holds: each plan defines its half of a shared state machine (deletion × conflict × resolve × convergence), and no plan owns the intersection.
