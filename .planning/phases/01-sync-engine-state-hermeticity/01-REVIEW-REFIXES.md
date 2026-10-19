---
phase: 01-sync-engine-state-hermeticity
reviewed: 2026-10-19T20:25:00Z
depth: standard
scope: fix diff 5c26898..HEAD (fa1be9f, 555933c, 1b08229, a6cc3c3) for 01-REVIEW-FIXES.md CR-01 and WR-01
files_reviewed: 3
files_reviewed_list:
  - src/main.rs
  - src/sync/mod.rs
  - tests/e2e_lifecycle.rs
findings:
  critical: 1
  warning: 2
  info: 2
  total: 5
status: issues_found
---

# Phase 01: Code Review Report (re-fix review)

**Reviewed:** 2026-10-19T20:25:00Z (HEAD a6cc3c3, diff base 5c26898)
**Depth:** standard, with reproductions using the HEAD binary in scratch projects (TYPESAFE_API_KEY unset, HOME redirected)
**Files Reviewed:** 3
**Status:** issues_found

## Summary

**Both findings are closed for the cases they described. One variant of the critical finding is still open: this diff did not cause it, and the pre-fix code behaves the same way.**

- **Old CR-01 (re-init turns a stale `.gitignore` into a local edit): closed for the reported case.** The new e2e test passes. I also confirmed it with a version-1 state file: after migration on re-init, status still reads `Upstream Updated`. **Not closed for one variant (CR-01 below).** Init itself appends the graphify block to a `.gitignore` that has a prior record. That still produces "Locally Modified (ready to push)" on a file the user never edited, and push reverts upstream with exit 0.
- **New exit-2 refusal: correct, and no regression.** I tested these with the binary:
  - These cases exit 0: first init into a new nested path, a project with no `.ude/`, and a project with an empty `.ude/` and no `state.json`.
  - These cases exit 2: a corrupt `state.json`, a 0-byte `state.json`, version 0, and version 3. Each message names the remedy. For version 3, nothing except `templates/` existed afterwards, so init touched no files.
- **Old WR-01 (status vs push on template logic): closed for shipped templates.** Status and push agree for `mise.toml`, with and without `--remote`.
  - Following the hint works. Moving the change into `mise.local.toml` makes push exit 0.
  - It does not freeze `mise.toml`. After the move, an upstream template change still pulls into `mise.toml`. I confirmed this by checking that the file contains the change.
  - **Still open:** a disagreement for `.agents/AGENTS.md` when its template has logic (WR-01 below). It is latent, because the shipped `agents.md.jinja` has no constructs.

Baseline at HEAD: `cargo test` passes (135 unit tests and 23 e2e tests), and `cargo clippy --all-targets -- -D warnings` exits 0. What this does not establish: no test covers init's own append on re-init, `--force`, or a ForceOnly path under template logic. I did not run the new tests against the pre-fix source, so I have not verified that they fail without the fix. The test-first commit order suggests they would.

Not re-reported, because they are known and deferred: a custom kept `.gitignore` with no prior record (including one under a `kept_unmanaged` tombstone) can be pushed; W2 (home-path placeholders); W3 (the guard only checks `.jinja`).

## Critical Issues

### CR-01: Re-init still turns an unedited `.gitignore` into a local edit when init's own graphify append fires, and push reverts upstream with exit 0 (old CR-01 not fully closed; not a regression)

**File:** `src/main.rs:239-252` (unconditional append), `src/main.rs:424-433` (carry-forward)
**Issue:** The carry-forward keeps the prior ancestor, but init still rewrites the kept file. It appends the graphify block whenever `.gitignore` lacks `graphify-out/*`, even when the file is already tracked in prior state. If upstream's `gitignore.repo` has no graphify block, the local `.gitignore` won't have one after a pull either. This happens when another developer removes the block, or when a remote template repository never had it. Every later re-init then appends the block, the file no longer matches the carried ancestor, and status reads "Locally Modified (ready to push)". A plain push publishes init's append into the shared template and exits 0. This has the same consequence as the original CR-01: a file the user never edited silently reverts someone else's upstream change. The pre-fix ancestor (the current template) gives the same result, so this diff did not introduce it. The fix's stated invariant ("an unedited but stale file" must not become a local modification) does not hold for this path.
**Failure scenario (confirmed with the HEAD binary):**
1. Run `ude init --type rust` with local `templates/`.
2. Delete the three `graphify-out` lines from `templates/projects/generic/gitignore.repo`. Status shows `Upstream Updated`. Run `ude sync pull`. Status shows `In Sync`.
3. Re-run `ude init --type rust` with no user edit. Status shows `.gitignore: Locally Modified (ready to push)`.
4. Run `ude sync push`. It prints "Successfully synchronized 1 file(s)" and exits 0, and `gitignore.repo` regains the graphify block (diff against a pre-push copy). This is a re-add, not a merge.
**Fix:** let sync own a `.gitignore` that already has a prior record. Append only when there is no prior record, and compute `prior_state` before step 2 (it already is):
```rust
let gitignore_tracked = prior_state.get_ancestor_hash(Path::new(".gitignore")).is_some();
if target_gitignore.exists() {
    kept_gitignore_template = Some(src_gitignore.clone());
    if !gitignore_tracked {
        // existing append block, unchanged
    }
}
```
Add the repro above as an e2e test. For the negative control, a first init over a custom `.gitignore` without `graphify-out/*` must still get the block appended.

## Warnings

### WR-01: Status and push disagree for `.agents/AGENTS.md` when its template has logic: push tells the user to re-run with `--force`, which only leads to a second skip (latent)

**File:** `src/main.rs:1037-1059` (`locally_modified_hint`), compared with `src/sync/mod.rs:1228-1267` (push order: Gate 1 using `allow_agent_rules`, then Gate 2), and the push message at `src/main.rs:880-882`
**Issue:** Status checks in this order: `Never`, then template logic, then `ForceOnly`. Push checks `Never` and `ForceOnly` in Gate 1, then template logic in Gate 2. The doc comment claims status matches push ("publish boundary first, then the template-logic gate"), but the code does not follow that order. For a ForceOnly path whose template has logic, status describes the final outcome. Plain push instead says "re-run with --force", and push `--force` then skips the file for template logic. Both pushes exit 1, and the first one's advice is a dead end.
**Failure scenario (confirmed with the HEAD binary and a modified template):** append `{% if project_type %}…{% endif %}` to `agents.md.jinja`, init, and append a line to `.agents/AGENTS.md`.
- Status: `Locally Modified (push skips it: its template has logic; … creating .agents/AGENTS.local.md …)`.
- `ude sync push`: `edited locally, agent rules are published only with --force; skipped (re-run with --force)`, exit 1.
- `ude sync push --force`: `contains template logic …; skipped (…)`, exit 1.
This is latent because the shipped `agents.md.jinja` contains no `{{`, `{%` or `{#`. A remote template repository can contain them.
**Fix:** in push's Gate 1 skip message, check `template_has_logic` for ForceOnly paths and give the template-logic remedy instead of "re-run with --force". Alternatively, evaluate Gate 2 before the ForceOnly half of Gate 1. Then correct the `locally_modified_hint` doc comment to describe the order it really uses. Add a `--force` push and a ForceOnly-under-logic case to `test_status_and_push_agree_on_edit_under_template_logic`.

### WR-02: Re-init still discards the prior record of any managed file it does not rewrite, adopting local edits as ancestors, so a later pull silently overwrites them (pre-existing; the carry-forward this diff added covers only `.gitignore`)

**File:** `src/main.rs:414-445`
**Issue:** The carry-forward is limited to `kept_gitignore_template`. Any other managed file that init does not write gets its current local bytes recorded as its ancestor, and its prior record is dropped:
- `copy_template_file` and `render_and_write` silently do nothing when the template source is missing.
- When `root_template_dir` is `None`, nothing is written at all.
This is the stale-record problem in reverse: a local edit becomes the ancestor, so it reads as In Sync and the next pull replaces it. The pre-fix code behaves the same way. The `prior_state` this diff added makes the fix a one-line extension.
**Failure scenario (confirmed with the HEAD binary):**
1. Init, then append `# local hook edit` to `scripts/hooks/pre-push`.
2. Temporarily move `templates/projects/generic/hooks/pre-push` away. Status shows `Conflicted`.
3. Re-run init. Status shows `Deleted upstream (pull to remove)`.
4. Restore the template. Status shows `Upstream Updated`, `ude sync pull` exits 0, and the local hook edit is gone.
**Fix:** for every managed path, if init did not write the file in this run and `prior_state` has a record, carry that record forward instead of hashing the local bytes. Equivalently, record local bytes only for files init just wrote.

## Info

### IN-01: Re-init drops open conflict records, so `ude sync resolve` fails until a pull re-records them (pre-existing; the `.gitignore` case is now safe)

**File:** `src/main.rs:409-412` (state built from `Default`, so `conflicts` and `tombstones` are discarded)
**Issue:** These steps were confirmed with the binary. With an open `.gitignore` conflict and its sidecar, re-init clears `conflicts`. Status still shows `Conflicted`, because the carried ancestor re-detects it, and push still skips the file with exit 1. Before this fix, the template ancestor would have reported Locally Modified and published the local side. Afterwards, `ude sync resolve .gitignore` exits 2 with "No open conflict for .gitignore; nothing to resolve". Recovery works: a `ude sync pull` re-records the conflict and rewrites `.gitignore.conflict`. Not verified: whether that rewrite discards edits the user already made in the sidecar.
**Fix:** copy `prior_state.conflicts` entries for files init kept (and drop entries for files init rewrote) when building the new state.

### IN-02: The new tests do not cover the remaining gaps

**File:** `tests/e2e_lifecycle.rs` (`test_reinit_keeps_ancestor_of_unedited_gitignore`, `test_status_and_push_agree_on_edit_under_template_logic`)
**Issue:** The re-init test always keeps `graphify-out/*` in the local file, so init's append never fires (CR-01 above). The agreement test never runs `push --force` and never covers a ForceOnly path (WR-01 above). Both tests pass while those defects exist.
**Fix:** add the repros from CR-01 and WR-01 as test cases.

---

_Reviewed: 2026-10-19T20:25:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
