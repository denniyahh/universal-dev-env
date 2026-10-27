---
phase: 02-sanitizer-boundaries-security-hardening
reviewed: 2026-10-27T08:54:26Z
depth: deep
files_reviewed: 9
files_reviewed_list:
  - src/main.rs
  - src/sanitizer/entropy.rs
  - src/sanitizer/gitleaks.rs
  - src/sanitizer/mod.rs
  - src/sanitizer/rules/gitleaks.toml
  - src/sync/mod.rs
  - src/template/mod.rs
  - tests/e2e_lifecycle.rs
  - docs/ARCHITECTURE.md
findings:
  critical: 2
  warning: 2
  info: 5
  total: 9
status: issues_found
---

# Phase 02: Code Review Report

**Reviewed:** 2026-10-27T08:54:26Z
**Depth:** deep
**Files Reviewed:** 9
**Status:** issues_found

## Summary

Scope: the diff `2070f2d..HEAD` for the nine listed files, plus the surrounding code it calls:
`scan_project`, `template_has_logic`, `map_to_upstream`, `open_remote_cache`, and `dirs-sys` 0.5.0's
`home_dir`. The secret-scan layers are sound for what they claim. Named rules, then the vendored
gitleaks rules, then R11, each run on the raw view and on a NUL-stripped view. The gates run in the
right order: the scan and the render-back check run in pass 1, before anything is staged. The staged
and committed blob gates scan the real blob bytes (`git show` does not apply textconv; checked on
git 2.56.0). They also tie each blob to the digest recorded in pass 1, which closes the clean-filter
and working-tree TOCTOU.

Two ways remain for the pusher's identity to reach a published commit, and both are verified:

1. If `HOME` ends in `/`, home-path parameterization does nothing (CR-01).
2. Push gate 2 still detects template logic by the `.jinja` extension, while the new shared render
   rule also renders non-`.jinja` templates that contain `{{`. A rendered value such as the git email
   can then be published over the placeholder (CR-02).

Nothing checks the published text for the raw home path after parameterization. So every occurrence
the boundary rules reject is published silently (WR-01). The post-commit gate passes vacuously when
HEAD is a merge commit, and PR mode never checks ancestry (WR-02).

Vendored rule pin: `test_vendored_rules_load_all` hashes `VENDORED_RULES`, the same
`include_str!` bytes compiled into the binary, so the file on disk and the compiled rules cannot
diverge. It has a negative control (one extra byte changes the digest). The checked-out file's
SHA-256 matches the pinned value. The pin only runs under `cargo test` (IN-03).

## Critical Issues

### CR-01: If HOME ends in `/`, parameterization does nothing and the raw home path is published

**File:** `src/main.rs:724-727`, `src/sanitizer/mod.rs:135-141, 149-165, 256-266`
**Issue:** `user_home` is `BaseDirs::home_dir().to_string_lossy()`. `dirs-sys` 0.5.0 (the locked
version) returns `PathBuf::from($HOME)` as is, so `HOME=/home/alice/` yields the needle
`/home/alice/`. `is_parameterizable_root` accepts it, but `replace_bounded` can then never match a
real occurrence:
- `cd /home/alice` does not contain the trailing `/`.
- In `/home/alice/x`, the needle is followed by `x`, an alphanumeric, which fails
  `is_trailing_boundary`.

The text therefore passes through unchanged. Gate 5 then compares unchanged text to itself, so it
passes. No other gate looks for the home path. Verified with verbatim copies of
`is_leading_boundary`, `is_trailing_boundary`, `is_parameterizable_root` and `replace_bounded`:
`home="/home/alice/"` leaves `cd /home/alice/x and cd /home/alice` unchanged. The control
`home="/home/alice"` yields `cd {{ user.home }}/x`. The XDG pass still works in this case,
because `config_dir` joins `.config` onto the slash-terminated home. Only the home pass fails, and
it fails silently.
**Fix:** Normalize both roots before use. Strip trailing separators, keeping a lone `/` so the
degenerate-root check still rejects it, and refuse or warn when a root is not parameterizable.
`TemplateContext::detect_for_project` should apply the same normalization so render and parameterize
agree:
```rust
fn normalize_root(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() { path } else { trimmed }
}
// in parameterize_paths (and wherever user_home / xdg_config are derived):
let user_home = normalize_root(user_home);
let xdg_config = normalize_root(xdg_config);
```
Add a test row with `home = "/home/alice/"`. Add a negative control showing that `"/"` is still
rejected.

### CR-02: Gate 2 only treats `.jinja` templates as having logic, so push can overwrite logic in other rendered templates and publish the pusher's values

**File:** `src/sync/mod.rs:1491-1508`, `src/sync/mod.rs:2445-2466`, `src/sync/mod.rs:546-548`, `src/template/mod.rs:146-148`, `src/main.rs:1689`
**Issue:** This phase makes `template::should_render` the single render rule: `.jinja`, or text
containing `{{`. Init (`materialize_template`), scan and pull now render every template that
matches it, including non-`.jinja` templates such as `gitconfig.repo` and the hook templates.
`template_has_logic` (push gate 2) still returns `Ok(false)` for any non-`.jinja` path
(line 2446). Consider an upstream `projects/generic/gitconfig.repo` containing
`email = {{ git_email }}` or `name = {{ project_name }}`:
1. Init writes the rendered value, the pusher's real git email, into `.gitconfig`.
2. The user edits any line, so scan reports `LocallyModified`.
3. Gate 2 does not skip the file, because the template is not `.jinja`.
4. Gate 3 only replaces home and XDG paths.
5. Gate 4 does not treat an email as a secret.
6. Gate 5 is skipped, because `should_render("gitconfig.repo", published)` is false when the
   published text has no `{{`.

The published template then has the placeholder replaced by the pusher's email. This erases the
template logic for every project (the invariant CR-02 was meant to protect) and publishes personal
data. If the template also has `{{ user.home }}`, Gate 5 runs, but rendering plain text returns the
same text, so it passes anyway. No template in the current `templates/` tree triggers this, but the
template repository is remote content the pusher does not control.
**Fix:** Make Gate 2 use the same rule as rendering. Run the logic check on any template the render
rule would render, not only on `.jinja` files:
```rust
pub fn template_has_logic(template: &Path) -> Result<bool, String> {
    let bytes = match fs::read(template) { /* as today */ };
    let Ok(raw) = std::str::from_utf8(&bytes) else { return Ok(false) };
    if !crate::template::should_render(template, raw) {
        return Ok(false);
    }
    let mut text = raw.to_string();
    for placeholder in PARAMETERIZED_PLACEHOLDERS {
        text = text.replace(placeholder, "");
    }
    Ok(["{{", "{%", "{#"].iter().any(|m| text.contains(m)))
}
```
Add a regression test: a non-`.jinja` template with `{{ git_email }}`, a local edit, and push. The
expected result is that the file lands in `skipped_templates` and the email never appears in the
staged bytes.

## Warnings

### WR-01: Nothing checks for the raw home path after parameterization, so every rejected occurrence is published silently

**File:** `src/sync/mod.rs:1543-1595`, `src/sanitizer/mod.rs:102-126`
**Issue:** Parameterization is the only defense for the home path. Every occurrence it declines to
rewrite is published raw, Gate 5 passes it (the text renders to itself), and no gate refuses it.
Beyond the accepted cases (alias spellings, `.`/`./` suffixes), these inputs also publish the
pusher's home path verbatim (verified with the copied functions):
- Compiler and linker flags such as `CFLAGS=-I/home/alice/include -L/home/alice/lib` (for example in
  `mise.toml` env). The leading `I` or `L` is alphanumeric, so `is_leading_boundary` rejects the
  match.
- JSON with escaped slashes, such as `"\/home\/alice\/x"`.
- Any managed file that is not UTF-8 (Latin-1, UTF-16). Gate 3 is skipped for it (lines 1559-1561),
  although Gate 4 scans the same file through the NUL-stripped view.

Afterwards, main prints "Sanitization gate passed: clean of secrets & parameters applied" with no
warning.
**Fix:** After Gate 3, scan the published bytes for the raw `user_home` byte string (and the
UTF-16LE/BE encodings, reusing the NUL-stripped view). Refuse on any hit with a fixed-text reason and
a line number, for example `PushResult::HomePathBlocked { path, line }`. Do this whether or not the
occurrence was at a boundary. The decision to leave `/home/alice.bak` unrewritten then stays a
rewriting rule, and no longer lets the path through silently.

### WR-02: The post-commit binding gate passes vacuously on a merge HEAD, and PR mode never checks ancestry

**File:** `src/sync/mod.rs:2133-2221`, `src/sync/mod.rs:1949-1955`, `src/sync/mod.rs:2026-2034`
**Issue:** `validate_committed_tree` lists only `git diff-tree -r HEAD`, which prints nothing for a
merge commit, and accepts an empty listing. The comment at lines 2201-2202 names a post-commit hook
that amends the commit as the threat this gate exists for. Verified on git 2.56.0: a merge HEAD whose
first parent is the base and whose tree adds `secret.txt` produced empty `diff-tree` output with
exit 0. The control, the same commands on a non-merge commit, listed `A secret.txt`. `HEAD~1` still
equalled the base, so `check_continuity` passes too, and direct mode would push it. In PR mode
(`push_pr_branch`) there is no continuity check at all. A hook that amends the commit and then adds
an empty commit on top leaves a HEAD diff that validates, and `git push -u origin <sync_branch>`
publishes both commits.
**Fix:** Check the commit's shape before checking its contents:
1. Require exactly one parent: `git rev-list --parents -n 1 HEAD` has two fields.
2. In both modes, require that parent to equal `fetched_sha`. PR mode needs `fetched_sha` passed in.
3. Require the committed path set to equal `expected_set`, which rules out an empty listing.

Roll back on any failure.

## Info

### IN-01: A home path that cannot be parameterized is skipped with no diagnostic

**File:** `src/sanitizer/mod.rs:256-266`, `src/main.rs:727`
**Issue:** When `user_home` is `.` (the fallback), relative, `/` or `//`, `parameterize_paths`
silently returns the input. Push then still reports "parameters applied". This conflicts with the
project's fail-closed observability rule.
**Fix:** Have `parameterize_paths`, or its caller in `push_updates`, report that the root was
rejected. Emit a stderr warning naming which root was skipped and why, without printing the value.

### IN-02: Init has two template-render paths with different rules

**File:** `src/main.rs:1714-1752` vs `src/main.rs:1675-1712`
**Issue:** `render_and_write` / `rendered_template` always render (`.jinja` call sites) through
`fs::read_to_string`. `materialize_template` renders by `should_render` and copies non-UTF-8 files.
The phase's stated goal was a single render rule (SEC-04). Today the hardcoded `.jinja` arguments
keep the two paths consistent. A future call site that picks the wrong helper would diverge without
any error.
**Fix:** Route the `.jinja` call sites through `materialize_template` and delete
`render_and_write` / `rendered_template`. `kept_ancestor` already uses the shared rule.

### IN-03: The vendored rule pin is enforced only under `cargo test`, and line-ending conversion would break it

**File:** `src/sanitizer/gitleaks.rs:17-20, 557-576`
**Issue:** The pin is a real guard for CI: it hashes the compiled-in bytes and has a negative
control. It does not run in `cargo build`, so a release built without tests ships whatever
`gitleaks.toml` holds. The repository also has no `.gitattributes`. A checkout with
`core.autocrlf=true` would rewrite the file to CRLF, fail the hash, and change the compiled bytes.
**Fix:** Add `src/sanitizer/rules/* -text` (or `binary`) to `.gitattributes`. Optionally, make CI
run the pin test as a separate required job.

### IN-04: ARCHITECTURE.md overstates what the gate guarantees

**File:** `docs/ARCHITECTURE.md:75, 77-80`
**Issue:**
- "Rejecting any files containing high-entropy strings" contradicts the measured limits stated in
  `entropy.rs:5-8`: hex-only and single-case secrets are never caught, and 3-23% of random keys
  pass.
- Section 2 lists `C:\Users\alice` and claims the paths are parameterized. It does not mention that
  boundary-rejected occurrences and non-UTF-8 files are published raw (WR-01), or the
  exact-spelling-only matching (forward-slash `C:/Users/alice` and escaped `C:\\Users\\alice` are
  not matched).

**Fix:** State the limits. Point to `entropy.rs` for the R11 detection rates and to the
`test_path_boundary_table` rows for which spellings are rewritten.

### IN-05: The TypeSafe drift check sends unparameterized AGENTS.md text to an external API (pre-existing, outside the phase diff)

**File:** `src/main.rs:1079-1100`
**Issue:** Before `evaluate_drift`, the local `.agents/AGENTS.md` is only secret-scanned
(`check_secrets`). Its raw text, including any home path or username, is sent to the TypeSafe API.
This is outside the phase goal ("before remote git staging or push") and these lines did not change
in this phase. It is recorded here because it is a second outbound channel the sanitizer only partly
covers.
**Fix:** Send `parameterize_paths(local_txt, user_home, xdg_config)` instead of the raw text, or
document the egress.

---

Verification notes:
- CR-01 and WR-01 were reproduced by compiling verbatim copies of the four boundary functions from
  `src/sanitizer/mod.rs:102-165`. The trailing-slash behaviour of `HOME` comes from reading
  `dirs-sys` 0.5.0 `home_dir`. It was not run end to end through `ude sync push`.
- WR-02's `git diff-tree` behaviour was reproduced in a scratch repo, with a control. The full hook
  scenario was not run against `push_to_remote`.
- CR-02 was confirmed by reading the code: `template_has_logic` returns early for non-`.jinja`
  files, and `check_render_back` is skipped when the published text has no `{{`. It was not run end
  to end.

_Reviewed: 2026-10-27T08:54:26Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: deep_
