---
phase: 01
phase_name: "Sync Engine & State Hermeticity"
project: "Universal Development Environment (UDE)"
generated: "2026-10-22"
counts:
  decisions: 7
  lessons: 7
  patterns: 6
  surprises: 6
missing_artifacts: []
---

# Phase 01 Learnings: Sync Engine & State Hermeticity

## Decisions

### Every git/gh spawn goes through one bounded, env-scrubbed runner
All git and gh subprocesses run through `crate::process::run_bounded` (process-wrap 10.x, whole process group killed at the deadline; LOCAL_TIMEOUT 30 s, NETWORK_TIMEOUT 60 s). `prepare_command` scrubs the 15 repo-local git env vars, sets `GIT_TERMINAL_PROMPT=0`, `GH_PROMPT_DISABLED=1` and a null stdin. Labels are static and never contain argv, so token URLs cannot leak into timeout errors.

**Rationale:** Killing only the leader leaves grandchildren (git-remote-https, ssh) alive; unbounded spawns hang the CLI. The scrub list comes from `git rev-parse --local-env-vars`, and `GIT_CONFIG_GLOBAL`/`HOME`/`XDG_*` are deliberately not scrubbed because tests rely on them.
**Source:** 01-01-SUMMARY.md, 01-CONTEXT.md (D-03), 01-RESEARCH.md

### Fingerprints hash LF-normalized UTF-8; state.json moves to v2 with in-memory migration
Managed files are classified by SHA-256 of LF-normalized UTF-8 (raw bytes for non-UTF-8). Version 1 state files are migrated on load; hash-only records adopt the new fingerprint only on a raw-hash match, and keys failing `validate_safe_write_target` are never read. Writes use the destination's dominant line-ending style.

**Rationale:** CRLF-only differences produced phantom drift. The migration is one-way, so it re-uses the existing path-safety validator rather than trusting old keys.
**Source:** 01-05-SUMMARY.md, 01-CONTEXT.md (D-07), .planning/STATE.md

### Multi-file writes are staged and validated before any rename; state.json is committed last
Every pull/push write goes through `StagedTransaction` (stage and validate all, then rename). Deletions are staged after writes, and `state.json` is staged first and committed last. Existing files keep their mode; new hooks/scripts get 0o755, others 0o644.

**Rationale:** A failure mid-pull must not leave a half-applied workspace or a state file that disagrees with disk.
**Source:** 01-06-SUMMARY.md, 01-CONTEXT.md (D-05), .planning/STATE.md

### Unreadable is not absent
`scan_project` treats only NotFound and NotADirectory as absent; any other read error fails the scan. Deletions of local-only paths (e.g. `scratch/*`) are BoundaryBlocked just like modifications.

**Rationale:** Once absence drives deletions, `fs::read(..).ok()` would turn a permission error into a destructive delete on the other side.
**Source:** 01-07-SUMMARY.md

### With a remote, pull deletes only what the remote's history shows was deleted (D-12 amended)
Each upstream deletion is confirmed with `git log --diff-filter=D` in the cache checkout (unshallowing the depth-1 clone first; merges diffed against their first parent). A template the remote never had reads "Missing upstream" and the local file is kept. Push got the same check (539cbb7).

**Rationale:** A thin or foreign remote made pull delete every unedited managed file it lacked. Operator decision 2026-10-21.
**Source:** 01-CONTEXT.md (D-12), 01-SECURITY.md, git log 20cf077 / d875afe / 539cbb7

### Push never deletes a template that has template logic (D-14 amended)
A local deletion of a templated file (e.g. mise.toml rendered from a `.jinja` with logic) is skipped with exit 1 and a message to restore the file or delete the template upstream. Templates without logic are still deleted.

**Rationale:** Deleting the shared `.jinja` would break every project that renders from it. Operator decision 2026-10-21.
**Source:** 01-CONTEXT.md (D-14), .planning/STATE.md, git log 65b9931

### Push is never forced; an upstream race rolls back and exits 1
Direct push checks continuity (cache commit parent == fetched head) and uses plain `git push --porcelain`. A `!` line with a `[rejected]` summary rolls the commit back and exits 1 with "upstream changed, run ude sync pull"; other `!` lines are hard failures (exit 2). Classification reads only the flag field and summary prefix, not the parenthetical wording.

**Rationale:** Only the `(fetch first)` wording was produced by a real race; `(non-fast-forward)` is assumed, so keying on it would be fragile (Assumption A3).
**Source:** 01-11-SUMMARY.md, 01-CONTEXT.md (D-18), 01-RESEARCH.md

---

## Lessons

### Plan acceptance greps fight rustfmt
Three plans had single-line `rg` acceptance checks (a method signature, `"push", "--porcelain"`) that rustfmt wrapped past 100 columns. Fixes were renaming parameters, binding the refspec to a local first, or verifying with `rg -U` multiline patterns.

**Context:** Plans 01-03, 01-10 and 01-11 each hit this as a blocking deviation. Acceptance checks on source text should be multiline-tolerant or assert behaviour instead.
**Source:** 01-03-SUMMARY.md, 01-10-SUMMARY.md, 01-11-SUMMARY.md

### The repository baseline fails `cargo fmt --check`
The baseline had 167 pre-existing rustfmt hunks in 9 files, so the planned `cargo fmt --check` gate could never pass. Plans gated instead on `rustfmt --check` of new files plus "workspace hunk count does not increase".

**Context:** The substitute gate proves no new drift was added, not that the repo is formatted. A repo-wide fmt is still undecided.
**Source:** 01-01-SUMMARY.md, 01-02-SUMMARY.md, .planning/STATE.md

### Restoring a mutated file with `cp -p` makes cargo reuse the mutant binary
After hand-mutation checks, `state.rs` was restored with `cp -p`, which reset its mtime to before the last mutant build. Cargo skipped the rebuild and a test failed against the mutant code even though the file's sha256 matched GREEN.

**Context:** Restore mutated files without `-p` or `touch` them, then rebuild before trusting a gate.
**Source:** 01-04-SUMMARY.md

### Planned tests often could not discriminate; positive controls and hand mutants found it
Several planned assertions passed whatever the code did: ancestor writers were never checked (InSync passes for any ancestor), a v1-migration test passed without migration, and mode tests left `|` -> `^` mutants alive. Each was fixed by adding a case that must produce the opposite result.

**Context:** Rule 2 "test strength" deviations recurred in plans 01-05, 01-06, 01-09 and 01-11; cargo-mutants generated only 2 mutants for the porcelain classifier, so hand mutants were needed.
**Source:** 01-05-SUMMARY.md, 01-06-SUMMARY.md, 01-11-SUMMARY.md

### Gate a commit on the check's exit code, not a pipeline tail
`cargo clippy ... | tail -2 && git commit` gated on `tail`'s exit code. It caused no harm in 01-04 because clippy was clean, but it is the 2026-10-09 incident pattern.

**Context:** All later gates ran clippy unpiped.
**Source:** 01-04-SUMMARY.md

### A remote's absence of a template does not mean it was deleted
Treating "missing from the remote" as "deleted upstream" made pull delete files when the remote was thin or foreign. A presence check (8349e70, then 577c520 requiring a managed template) was not enough; only a history check distinguishes deletion from never-existed.

**Context:** Found by three successive re-verifications on 2026-10-21; each partial fix left a gap the next pass found.
**Source:** 01-SECURITY.md, 01-REVIEW-DISPOSITION.md, git log 8349e70 / 577c520 / 20cf077

### Secrets reach tests and child processes through the host environment
The first 01-10 e2e run printed "TypeSafe System One (Jev) enabled", showing the test binary saw the host's `TYPESAFE_API_KEY`; new spawns now `env_remove` it. Separately, the key was passed to curl as a `-H` argument, visible in the process list, until 03fcc78 moved it to a config on stdin. STATE.md records that the key leaked and must be rotated by the operator.

**Context:** Test spawns need an explicit env scrub for secrets, not just git vars; pre-existing spawns are logged in deferred-items.md.
**Source:** 01-10-SUMMARY.md, 01-SECURITY.md, .planning/STATE.md

---

## Patterns

### Negative control on every verification
Each mutation check, hermeticity probe and review fix carries a control that must produce the opposite result (a missing template gives UpstreamDeleted; an empty parent `.git` is still ignored; with the fence removed, 3 tests fail).

**When to use:** Any test or probe whose passing result would otherwise be indistinguishable from a broken check.
**Source:** 01-07-SUMMARY.md, 01-REVIEW-DISPOSITION.md, 01-VERIFICATION.md

### Hand-mutant script that checks all patterns first and restores in `finally`
After a mutant script aborted on a pattern rustfmt had reflowed (leaving mutant M11 applied), it was rewritten to verify every pattern up front and restore original bytes in a `finally`, then confirm with `cmp`.

**When to use:** Any hand-rolled mutation testing that edits tracked sources.
**Source:** 01-09-SUMMARY.md, 01-06-SUMMARY.md

### Fence git discovery with GIT_CEILING_DIRECTORIES in every fixture
`ude_command`, `scrubbed_git`, `GitFixture::cmd` and `gitconfig_env` set `GIT_CEILING_DIRECTORIES` to the temp dir, and cargo-mutants runs set it to `$PWD/target`.

**When to use:** Any test or tool run whose temp dir might sit inside a real checkout (TMPDIR inside the repo, cargo-mutants copies without `.git`).
**Source:** 01-11-PLAN.md, git log 9bd2777 / 3d1a544 / 2a14361

### Hermetic git fixtures via a fixture gitconfig, not per-command flags
Fixture git commands come only from `GitFixture::cmd` (sync tests) or `scrubbed_git` (e2e); identity and `gpgsign=false` come from a pinned fixture `GIT_CONFIG_GLOBAL`, and engine spawns get `gitconfig_env()` injected.

**When to use:** Tests that drive real git and must not depend on the host's `~/.gitconfig` (which here sets `core.hooksPath`).
**Source:** 01-02-SUMMARY.md, 01-03-SUMMARY.md

### "What the verification does NOT establish" section in every summary
Each plan summary states the limits of its evidence (e.g. 4-6 green runs is a weak flakiness bound; decoy byte-identity does not cover mtimes; the `(non-fast-forward)` wording is only a fixed string in a test).

**When to use:** Every SUMMARY and VERIFICATION report, so a reader can tell "passed" from "proven".
**Source:** 01-02-SUMMARY.md, 01-03-SUMMARY.md, 01-11-SUMMARY.md

### Split TDD RED commits so RED actually fails on the target path
When RED did not fail on the planned path (git clone sets its own GIT_DIR), RED was extended to a second status run over the existing cache, which did fail on `git fetch` against the decoy. RED stubs got a temporary `#![allow(dead_code)]` removed in GREEN.

**When to use:** TDD plans where the first behaviour exercised happens to be immune to the defect.
**Source:** 01-03-SUMMARY.md, 01-01-SUMMARY.md

---

## Surprises

### git ignores `--depth` for a plain local-path clone
The production cache is a depth-1 clone, but a test remote given as a plain path produced a full clone, so the unshallow path in the history check was never exercised. Tests use `file://` URLs to get a really shallow cache.

**Impact:** Without `file://`, the mutant "skip the unshallow" would have survived; with it, never-confirm, always-confirm and skip-unshallow mutants all fail the e2e.
**Source:** tests/e2e_lifecycle.rs (comment at the shallow-remote fixture), 01-VALIDATION.md, git log 20cf077

### Running e2e with TMPDIR inside a checkout let `ude init` stage into the real index
With TMPDIR inside a repository, the "no git" tests ran against that repository, two failed, and init's `git update-index` staged fixture hooks into the enclosing repo's index. The first fence (9bd2777) covered only `sync_cli`, leaving 29 direct calls unfenced.

**Impact:** Host-repo pollution from tests; took three commits (9bd2777, 3d1a544, 2a14361) to fence every fixture path. Verified fenced 66/66 vs 3 failures and 3 staged files unfenced.
**Source:** 01-REVIEW-DISPOSITION.md (IN-06), git log 9bd2777 / 3d1a544 / 2a14361

### Init and pull could delete user files when no ude templates directory was found
Init with no templates directory recorded the user's files as managed, and the next pull deleted them; pull without a remote did the same for a wrong or missing template root (predating the phase diff).

**Impact:** Two of the three critical code-review findings; fixed by requiring `projects/generic` and exiting 2 when none is found.
**Source:** 01-REVIEW-DISPOSITION.md (CR-01, CR-02), git log 464bfc4

### Re-verifications kept finding real defects after 15/15 and 18/18 passes
Successive re-verifications on 2026-10-21 found a foreign-remote deletion, a thin-remote deletion, merge-resolution deletions, a push/resolve mismatch, and the TypeSafe key in curl argv, each after an earlier pass reported the phase goal met.

**Impact:** The phase closed only after 47 post-close fix and doc commits (f3eb59b~40..HEAD); two push test gaps remain backlogged as a todo.
**Source:** 01-SECURITY.md, 01-VERIFICATION.md, .planning/STATE.md, git log f3eb59b~40..HEAD

### Token estimates were far above actual diff size
Plan 01-01 came in at about 9.4k tokens of diff against a 70k estimate; the estimate seems to have counted context reading.

**Impact:** Estimate calibration is unreliable until estimates and actuals measure the same thing.
**Source:** 01-01-SUMMARY.md

### A gate failure under CPU load with no recorded cause
One pre-commit gate run failed while a background mutation run loaded the CPU; its `cargo test` output went to /dev/null, so the cause is unknown. A load-sensitive timing test is suspected but unverified.

**Impact:** Timing-based tests (lock timeout, process kill) may be flaky under load; gate output should be kept, not discarded.
**Source:** 01-06-SUMMARY.md
