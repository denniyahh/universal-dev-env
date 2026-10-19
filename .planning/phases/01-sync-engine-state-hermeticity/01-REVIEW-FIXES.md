---
phase: 01-sync-engine-state-hermeticity
reviewed: 2026-10-19T19:05:00Z
depth: standard
scope: fix diff 564641a..HEAD (commits 98261e6..ee296c4) for CR-01, CR-02, CR-03, WR-01
files_reviewed: 3
files_reviewed_list:
  - src/sync/mod.rs
  - src/main.rs
  - tests/e2e_lifecycle.rs
findings:
  critical: 1
  warning: 3
  info: 1
  total: 5
status: issues_found
---

# Phase 01: Code Review Report (fix re-review)

**Reviewed:** 2026-10-19T19:05:00Z (HEAD cd88aed, diff base 564641a)
**Depth:** standard, with binary reproductions in a scratch project
**Files Reviewed:** 3
**Status:** issues_found

## Summary

Each of the four fixes closes the case its finding described:

- **CR-01, closed.** A boundary deletion or edit is now skipped, the rest of the push is published, the skip is listed, and push exits 1. Pull and status no longer tell the user to push a path that push will refuse. Conflict and merge skips are handled before the boundary gate, so the two kinds of skip cannot hide each other. They share exit 1, and the exit-2 paths are unchanged. A push where every file is skipped reaches `push_to_remote` with nothing staged and returns "No changes to push". I confirmed that by reading the code. The new e2e test exercises this path in remote mode.
- **CR-02, closed for the shipped `mise.toml.jinja`.** I reproduced it with the binary: the template stays byte-identical and the `.gitconfig` edit in the same push is published.
- **CR-03, closed.** A kept custom `.gitignore` now reads as Locally Modified. A later upstream change produces a `.gitignore.conflict` sidecar instead of an overwrite (reproduced). A fresh project's `.gitignore` is still In Sync (e2e negative control, plus my own run).
- **WR-01, closed.** The test now fails 0 of 300 runs alone and 0 of 480 runs at 8-way parallel. The original reviewer measured 3/200 and 24/480 before the fix. These runs show the failure rate dropped well below the old ~4%. They are not a guarantee that the test can never flake. The fix does not weaken what the test proves. The negative control, a child that is handed the lock file, still has to be seen holding it. The new `filter_map` only drops descriptors that close while the scan runs, and a leaked lock descriptor stays open.

The fixes introduced or left open the following:

- **CR-01 (new, regression from the CR-03 fix):** re-running `ude init` on an already-initialized project turns a stale but unmodified `.gitignore` into "Locally Modified (ready to push)". A plain push then silently reverts other people's upstream `gitignore.repo` changes, exiting 0. The pre-fix binary reported "Upstream Updated" for the same steps.
- **WR-01:** status says "ready to push" for every `mise.toml` edit, but push now always skips it. The skip message points users at the wrong remedy.
- **WR-02 and WR-03 (latent):** two gaps let the CR-02 guard pass rendered text over template constructs. Both are confirmed with modified templates. No template shipped today triggers either one.

Baseline: `cargo test` passes (135 unit tests and 20 e2e tests) and `cargo clippy --all-targets -- -D warnings` exits 0. Passing tests do not cover the scenarios below. No existing test exercises re-init, status-versus-push agreement, or templates containing placeholders.

The known consequence the operator accepted (a custom `.gitignore` kept by init can be pushed over `gitignore.repo`) is not re-reported. CR-01 below is a different case, and how it overlaps with that consequence is stated there.

## Critical Issues

### CR-01: Re-running `ude init` makes a stale, unmodified `.gitignore` look like a local edit, and push reverts upstream with exit 0 (regression from the CR-03 fix)

**File:** `src/main.rs:221`, `src/main.rs:236-237`, `src/main.rs:414-417`
**Issue:** `kept_gitignore_template` is set whenever `.gitignore` exists, not only when the file is custom. That includes the `.gitignore` a previous `ude init` materialized. Init then rewrites `.ude/state.json` from scratch with the current template as the ancestor. If upstream `gitignore.repo` changed since the first init, the untouched local copy (the old template) now differs from that ancestor while upstream matches it. Scan therefore classifies the file `LocallyModified` instead of `UpstreamModified`. Before this fix, the ancestor was the local bytes, which gave the correct `UpstreamModified`.
**Failure scenario (confirmed with both binaries, scratch project, local templates in cwd):**
1. `ude init --type rust`. Status shows `.gitignore: In Sync`.
2. Append `new_upstream_rule/` to `templates/projects/generic/gitignore.repo`, simulating another developer's upstream change. Status shows `.gitignore: Upstream Updated (ready to pull)`.
3. Re-run `ude init --type rust`.
   - Fixed binary: status shows `.gitignore: Locally Modified (ready to push)`. `ude sync push` exits 0 with "Successfully synchronized 1 file(s)", and `gitignore.repo` no longer contains `new_upstream_rule/`.
   - Pre-fix binary (564641a), same steps: `.gitignore: Upstream Updated (ready to pull)`.

**Overlap with the accepted consequence:** the operator accepted that a *custom* kept `.gitignore` can be pushed. This case involves a file the user never edited, and the push reverts someone else's upstream change. The fix planned in the pending todo (push skips files init kept) would block the revert. It would also leave this unmodified file as a permanent project-only override that never receives template updates, which is still wrong.
**Fix:** when the project already has a state record for `.gitignore`, carry that ancestor forward instead of substituting the template. Use the template only when no prior record exists:
```rust
let prior = sync::ProjectState::load_from_dir(&project_path).ok();
// in the managed_rels loop, for ".gitignore" when kept:
if let Some(prev) = prior.as_ref().filter(|s| s.get_ancestor_hash(rel).is_some()) {
    project_state.record_file(rel, prev.get_ancestor_hash(rel).unwrap().to_string(),
        prev.get_ancestor_content(rel).map(String::from));
    continue;
}
```
Add an e2e test that runs init, changes upstream, re-runs init, and asserts status is "Upstream Updated", with the fresh-project In Sync case as the negative control.

## Warnings

### WR-01: Status says "ready to push" for a `mise.toml` edit that push always skips, and the skip message points at the wrong remedy

**File:** `src/main.rs:531-535` (status hint), `src/main.rs:873-881` (push skip message), `src/sync/mod.rs:1252-1267` (Gate 2)
**Issue:** The shipped `mise.toml.jinja` contains `{% if %}` logic, so after the CR-02 fix every local `mise.toml` edit in every project is skipped by push with exit 1. Status was not updated to match. It computes the hint only from `push_boundary`, so it still says "ready to push". The skip message says "edit the template upstream instead". For a project-specific edit, such as adding a project's `[env]` or tasks, following that advice publishes one project's settings to every project. The remedy that actually clears the skip is a `mise.local.toml` override, and the message never mentions it. Until the user finds that out, every `ude sync push` exits 1.
**Failure scenario (confirmed via binary):** after init, append `[env]\nFOO = "1"` to `mise.toml`. `ude sync status` prints `mise.toml: Locally Modified (ready to push)`. `ude sync push` prints `mise.toml: upstream template ... contains template logic ...; skipped (edit the template upstream instead)` and exits 1, and so does every push after it. Creating `mise.local.toml` turns status into "Layered Override Active" and push exits 0.
**Fix:** make `template_has_logic` `pub` and use it in the status `LocallyModified` arm, for example "Locally Modified (template has logic; push skips it)". Change the push message to name both remedies: "for this project only, move the change into `<stem>.local.<ext>`; for every project, edit `<template>` upstream".

### WR-02: The `{{ user.home }}` / `{{ user.xdg_config }}` exemption lets push erase a placeholder and publish the pusher's absolute home path (latent)

**File:** `src/sync/mod.rs:2075`, `src/sync/mod.rs:2094-2097`; the reason lies in `src/sanitizer/mod.rs:81` and `src/sanitizer/mod.rs:98`
**Issue:** The exemption assumes `parameterize_paths` always turns the rendered value back into its placeholder. It does not. The regex only matches a path followed by `/ \ " ' whitespace :` or end of text, so a placeholder followed by `,` `;` `.` `)` `]` `}` or `=` is not round-tripped. Gate 2 then treats the template as logic-free, and push writes the literal home path over the placeholder. That breaks the template for everyone and leaks the pusher's home path into the shared repository. A related problem: `{{ user.home }}/.config` comes back as `{{ user.xdg_config }}` because XDG is replaced first, which silently changes the template's meaning.
**Failure scenario (confirmed via binary with a modified local template):** set `mise.toml.jinja` to `[env]\nA = "{{ user.home }}/bin"\nB = "{{ user.home }},/opt"\n`, init, append `C = "1"` to `mise.toml`, then push. Push exits 0, and the template now reads `B = "/tmp/.../rereview/home,/opt"`. Line `A` round-tripped in the same run, which serves as the in-run control. This is latent because no shipped template contains these placeholders. It becomes critical as soon as one does, including any template in a remote template repository.
**Fix:** replace the string exemption with a round-trip check. Allow the push only if parameterizing the rendered template reproduces the raw template exactly. Otherwise treat it as logic:
```rust
let rendered = renderer.render_str(&raw, ctx)?;
let round_trip = sanitizer.parameterize_paths(&rendered, user_home, xdg_config);
Ok(normalize_eol(&round_trip) != normalize_eol(&raw))
```
Alternatively, drop the exemption and treat any `{{` in a rendered template as logic.

### WR-03: Gate 2 only checks `.jinja`, but scan and pull render any template containing `{{`, so a `.repo` or hook template still gets rendered text pushed over it (latent)

**File:** `src/sync/mod.rs:2081` versus `src/sync/mod.rs:611-613` (scan) and `src/sync/mod.rs:931-933` (pull)
**Issue:** The renderer's rule is "extension is `.jinja` OR the text contains `{{`". The CR-02 guard's rule is "extension is `.jinja`". Any non-`.jinja` template with a `{{ ... }}` construct is rendered into the project on pull, and the rendered text is then published over the template by push. That is exactly the destruction CR-02 describes.
**Failure scenario (confirmed via binary with a modified local template):** append `[user]\n\temail = {{ git_email }}` to `gitconfig.repo`, init, pull (the `.gitconfig` becomes `email = developer@example.com`), add a line to `.gitconfig`, then push. Push exits 0, and `gitconfig.repo` now contains `email = developer@example.com`, so the placeholder is gone. This is latent because no shipped `.repo` or hook template contains `{{`.
**Fix:** extract one `fn renders_as_template(path: &Path, text: &str) -> bool` and use it in scan, pull and `template_has_logic`, so that the guard covers exactly the files the renderer rewrites.

## Info

### IN-01: The lock test's settle timeout outlives the child it waits for

**File:** `src/sync/mod.rs:5649`, `src/sync/mod.rs:5731`
**Issue:** `CHILD_SETTLE_TIMEOUT` is 10 s, but the child runs `sleep 5`. On a heavily loaded host where exec and loading take more than 5 s, the child exits and becomes a zombie, and the test fails after 10 s with "never settled into sleeping". That message hides the real cause. The test fails loudly rather than passing falsely, so the assertion is not weakened. Confirmed by reading. Measured: 0 failures in 300 runs alone and 480 runs at 8-way parallel.
**Fix:** make the child outlive the deadline (for example `sleep 30`; the guard kills it on drop), or derive the sleep length from `CHILD_SETTLE_TIMEOUT`.

---

_Reviewed: 2026-10-19T19:05:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
