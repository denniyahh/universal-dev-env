---
phase: 02-sanitizer-boundaries-security-hardening
reviewed: 2026-10-28T00:41:34Z
depth: deep
files_reviewed: 4
files_reviewed_list:
  - src/sanitizer/mod.rs
  - src/sync/mod.rs
  - src/template/mod.rs
  - tests/e2e_lifecycle.rs
findings:
  critical: 0
  warning: 2
  info: 3
  total: 5
status: issues_found
---

# Phase 02: Code Review Report

**Reviewed:** 2026-10-28T00:41:34Z
**Depth:** deep
**Files Reviewed:** 4
**Status:** issues_found

## Summary

Scope: the diff `a32cd36..HEAD` for the four listed files. These are gap-closure plans 02-09
(normalize trailing separators off the home and XDG roots) and 02-10 (push gate 2 decided by the
shared render rule). Cross-file call chains were traced into `src/main.rs`:
- the `ude sync` root derivation at lines 724-735;
- `locally_modified_hint`, `logic_template` and `run_sync_resolve`;
- `scan_project`, `read_if_present` and `check_render_back`;
- `directories` 6.0.0 and `dirs-sys` 0.5.0 (the locked versions).

Both critical findings of the previous review are closed:
- **Old CR-01 (a trailing-separator HOME parameterized nothing).** `normalize_root` is applied in
  `parameterize_paths`, in `push_updates` and in `TemplateContext::detect_for_project`, the only
  production constructor of a render context. The dependency behaviour cited in the doc comment
  matches the vendored sources: `dirs-sys` returns `$HOME` as given, and `directories` uses an
  absolute `XDG_CONFIG_HOME` as given.
- **Old CR-02 (gate 2 checked only `.jinja` templates).** `template_has_logic` now asks
  `template::should_render`, the rule that scan, init and pull use. The ASCII-only byte mapping
  cannot destroy a marker. Placeholder removal can only create a marker, never hide one, so any
  error falls on the conservative side.

Checks run in this pass:
- `cargo test` filtered to the phase's new tests: 11 unit and 6 e2e tests passed.
- `cargo clippy --all-targets -D warnings` exited 0.
- The working tree was clean afterwards.

What those checks do not show: the passing unit test for the placeholder-only static template
does not show that the placeholder survives push (WR-02). No mutation run was made against gate 2
in this pass.

The previous review's WR-01 (no check for a raw home path after parameterization), WR-02 (the
merge-HEAD and PR-mode binding gate) and its Info items are outside this diff. The code at those
sites is unchanged, so they are carried forward and not re-counted here.

## Warnings

### WR-01: `ude sync resolve` promises a publish or deletion that gate 2 now refuses for static templates

**File:** `src/main.rs:1621-1626` (consequence of `src/sync/mod.rs:2459-2492`)
**Issue:** After `resolve_conflict` returns `KeptLocal`, it records the upstream hash as the
ancestor (`src/sync/mod.rs:913-918`), so the next scan reads the file as `LocallyModified`. For
`AcceptedLocalDeletion` it reads as `LocalDeleted`. Both statuses go through gate 2. Since
`a30c969`, gate 2 also skips non-`.jinja` templates that `should_render` renders, such as
`gitconfig.repo` holding `{{ git_email }}` or a hook template holding `{{ project_name }}`.
`run_sync_resolve` still prints these messages unconditionally:
- "local content will be published on the next push"
- "the next push deletes the upstream template if it still exists"

The next push then lands the file in `skipped_templates` and exits 1. This is the opposite of what
resolve just told the user. The gap already existed for `.jinja` templates, and this phase widened
it to every managed static template. `status`, `init` and the deletion hints all consult
`logic_template`. The resolve message is the one user-facing promise that does not.
**Fix:** Have `run_sync_resolve` check the template the same way the status hints do. For example:
```rust
let has_logic = logic_template(&root_template_dir, file).is_some();
let meaning = match (outcome, has_logic) {
    (sync::ResolveOutcome::KeptLocal, true) => format!(
        "kept locally; push skips it because its template has logic; {}",
        template_logic_remedy(file, &root_template_dir, &template)),
    (sync::ResolveOutcome::AcceptedLocalDeletion, true) =>
        "deletion kept locally; push skips it because its template has logic".to_string(),
    // ... existing arms
};
```
`run_sync_resolve` is offline, so it needs the template root passed in. Add an e2e test:
1. Set up a conflict over a `gitconfig.repo` that holds `{{ git_email }}`.
2. Run `sync resolve`.
3. Assert that the message does not claim a publish, and that `push` reports the skip.

### WR-02: The unit test for a placeholder-only static template passes on a push that erases the placeholder

**File:** `src/sync/mod.rs` (`test_push_publishes_over_placeholder_only_static_template`; `record_rendered_gitconfig`; `push_dirs` at line 4799)
**Issue:** `record_rendered_gitconfig` renders the template with the host context from
`detect_for_project`, so the local `.gitconfig` holds the host's real home path, for example
`/var/home/<user>/.gitconfig.local`. `push_dirs` then pushes with the roots `/home/alice` and
`/home/alice/.config`. Traced through `push_updates`:
1. Gate 3 parameterizes with `/home/alice` and finds nothing to replace.
2. The published text has no `{{` left, so `check_render_back` returns `Ok` without rendering
   (`src/sync/mod.rs:546-548`).
3. The published `gitconfig.repo` therefore carries the host's literal home path where
   `{{ user.home }}` used to be.

The test asserts only that the file is not in `skipped_templates` and checks the suffix, so it
passes on exactly the lossy publish this phase exists to prevent. The comment hands the
placeholder check to the e2e test. That leaves the unit layer, the one gate 2 regressions hit
first, endorsing a placeholder-erasing outcome.

The test reflects a latent API hazard. `push_updates` scans with the host-detected roots
(`dummy_ctx`, line 1423) but parameterizes and renders back (gates 3 and 5) with the roots the
caller passes. When the two disagree, gate 5 cannot catch the loss. In production they agree,
because `main.rs` derives both from `BaseDirs`.
**Fix:** In the test, push with the context's own roots and assert that the placeholder survives:
```rust
let (ctx, rendered) = record_rendered_gitconfig(local.path(), upstream.path(), template_text);
let result = engine.push_updates(local.path(), upstream.path(),
    &ctx.user_home, &ctx.xdg_config, false, true);
// ...
assert!(published.contains("path = {{ user.home }}/.gitconfig.local"));
```
To remove the hazard itself, `push_updates` could build `dummy_ctx` from the roots it is given
instead of detecting them again. Alternatively, it could `debug_assert!` that the detected and
given roots agree after normalization.

## Info

### IN-01: Two roots are still not single-spelled

**File:** `src/template/mod.rs:55-64`
**Issue:** Two cases slip past normalization:
- **`xdg_data`.** It is never passed through `normalize_root`, so `XDG_DATA_HOME=/x/` renders
  `{{ user.xdg_data }}/a` as `/x//a`.
- **A derived `config_dir`.** When `XDG_CONFIG_HOME` is unset, `directories` builds `config_dir`
  as `home_dir.join(".config")`. `Path::join` adds no separator after a trailing one, so
  `HOME=/home/alice//` yields `/home/alice//.config`. `normalize_root` trims only trailing
  separators, so that inner `//` reaches every render of `{{ user.xdg_config }}`.

Both cases are consistent within one machine and reversible. They are cosmetic, but they
contradict the "one spelling of each root" claim in the `detect_for_project` doc. The unit test
pair `("/home/alice//", "/home/alice/.config//")` is not a pair `directories` can produce from
`HOME=/home/alice//`.
**Fix:** Normalize `xdg_data` too. Either derive `xdg_config` from the normalized `user_home` when
`XDG_CONFIG_HOME` is unset, or document the inner-separator case.

### IN-02: `template_has_logic` reads templates differently from `read_if_present`

**File:** `src/sync/mod.rs:2459-2469`, `src/main.rs:1424-1427`
**Issue:** `read_if_present` refuses a symlinked template and reads `NotADirectory` as absent.
`template_has_logic` follows symlinks and turns `NotADirectory` into an error. Inside push, the
scan runs first and fails before gate 2, so push is unaffected. The init and status hints
(`logic_template` via `print_missing_tracked_warning`) can still disagree with
`renders_template`, which refuses symlinks. The two then give opposite answers for the same
template. Also, `logic_template` collapses `Err` to "no logic" with `unwrap_or(false)`.
**Fix:** Read through `read_if_present(upstream_root, template)` (pass the root in), and let
`logic_template` surface `Err` the way `locally_modified_hint` does.

### IN-03: Upgrading re-renders every placeholder file for users whose HOME or XDG root ended in a separator

**File:** `src/template/mod.rs:53,59`
**Issue:** Existing `state.json` ancestors were rendered with the old spelling, for example
`/home/alice//x`. After this change, scan renders upstream as `/home/alice/x`, so each such file
reads as `UpstreamModified`, or as a merge or conflict if it was edited locally. This happens once
and is benign, but it arrives with no note.
**Fix:** Mention it in the changelog or release notes. No code change is needed.

---

_Reviewed: 2026-10-28T00:41:34Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: deep_
