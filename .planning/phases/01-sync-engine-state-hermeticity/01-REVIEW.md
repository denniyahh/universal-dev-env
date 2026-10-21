---
phase: 01-sync-engine-state-hermeticity
reviewed: 2026-10-21T17:55:00Z
depth: deep
diff_base: 97fe52a190f513d65061a172d067c49051fcb17d
files_reviewed: 6
files_reviewed_list:
  - .github/workflows/supply-chain.yml
  - README.md
  - scripts/hooks/pre-push
  - src/cli/mod.rs
  - src/main.rs
  - tests/e2e_lifecycle.rs
findings:
  critical: 3
  warning: 3
  info: 7
  total: 13
status: issues_found
---

# Phase 01: Code Review Report

**Reviewed:** 2026-10-21T17:55:00Z
**Depth:** deep (cross-file into src/sync/, src/template/)
**Files Reviewed:** 6
**Status:** issues_found

## Summary

Scope: `git diff 97fe52a HEAD` for the six listed files, traced into `src/sync/mod.rs`
(`compare_3way_hashes`, `scan_project`, `refuse_symlink_under`, pull deletions),
`src/sync/state.rs` (tombstones, save) and `src/template/mod.rs` (project type detection).
This replaces the earlier review of this phase, which is kept in git at 97fe52a.

Three findings lose data and exit 0. All three were reproduced end to end against a
built `ude` binary, each with a negative control:

1. When init finds no templates directory, it records the user's own files as
   template-managed. The next pull deletes them. The new "leave a file with no template
   unmanaged" protection explicitly skips this case. (CR-01, in the diff)
2. `ude sync pull` with no remote deletes every managed file in two cases: the project has
   its own `templates/` directory, or no templates directory is found at all. This code
   predates the diff, but it is the same template-root lookup that CR-01 depends on.
   (CR-02)
3. In a monorepo subdirectory, any `git ls-files` failure, not just "not a git
   repository", is downgraded to a warning. Init then overwrites committed edits.
   (CR-03, in the diff)

The other new code in scope holds up when traced: the symlink pre-flight, the keep-type
fallback, the non-UTF-8 `.gitignore` append, the TypeSafe symlink skip and the kept
tombstone for committed files with no template. The 18 new and related e2e tests pass
(`cargo test --test e2e_lifecycle -- test_init_ test_reinit_without_type test_push_typesafe test_fresh_clone_does_not_resurrect test_first_init_in_monorepo`:
18 passed). Those tests exercise only the cases they name. None of them covers a missing
templates directory, a project-owned `templates/` directory, or a parent repository that
git cannot read, which is why the three blockers got through.

Items the orchestrator listed as already-logged todos are not re-raised. Two places where
the new code adds to them:
- `main.rs:551-558` adds another refusal point ("Cannot read committed …", exit 2) that
  fires after the other files were already written. This is the same class as the logged
  todo about init refusing after a partial write.
- `main.rs:538-564` leaves an uncommitted kept `.gitignore` with no template unrecorded,
  while the new `unmanaged_local` branch right below it tombstones every other file in the
  same situation. This is the logged ".gitignore with no template is publishable" todo.
  The fix is to route the `.gitignore` through the same branch.

## Critical Issues

### CR-01: Init with no templates directory found records the user's files as managed; the next pull deletes them

**File:** `src/main.rs:568` (also `282`, `311`, `648-663`)
**Issue:** The new "not kept" branch tombstones a pre-existing file that has no template
only under the guard `root_template_dir.is_some() && template().is_none()`. When
`find_templates_dir()` returns `None`, init behaves like this:
- the whole materialization block (`if let Some(ref tdir)`, line 311) is skipped silently,
  so nothing is written;
- every pre-existing managed file falls through to `read => read.ok()` and is recorded
  with its own bytes as the ancestor;
- init still prints "Configured tracked .gitconfig & chained scripts/hooks" and
  "Initialized workflow tasks (mise.toml, scratch/)" and exits 0.

From then on the user's own file has ancestor == local and no upstream (row 1 in
`compare_3way_hashes`, `sync/mod.rs:482`), so status reads "Deleted upstream" and pull
deletes it (`sync/mod.rs:1042`). This is exactly the case the intended design says must
stay Unmanaged.

`find_templates_dir()` does return `None` for a release binary built elsewhere, or once
cargo's gc removes the registry source that `CARGO_MANIFEST_DIR` points to.

**Reproduced:** I built a copy of HEAD, then hid its `templates/`. In a directory holding
only a user-written `.gitignore` and `mise.toml`, `ude init --type rust` exited 0 and
wrote `files: ['.gitignore', 'mise.toml'], tombstones: []` to state.json. `ude sync
status` then showed both as "Deleted upstream (pull to remove)". `ude sync pull` printed
"Removed .gitignore (deleted upstream)" and "Removed mise.toml", exited 0, and both files
were gone.
**Fix:** Init must not run without templates. Refuse up front:
```rust
let Some(root_template_dir) = find_templates_dir() else {
    eprintln!("  {} Initialization failed: no ude templates directory found", "✗".red());
    std::process::exit(EXIT_HARD_FAILURE);
};
```
At a minimum, drop the `root_template_dir.is_some() &&` guard so that a template-less run
tombstones files the same way. Also gate the "Configured …" and "Initialized …" lines on
what was actually written.

### CR-02: `sync pull` without a remote deletes every managed file when the template root is wrong or missing (predates the diff)

**File:** `src/main.rs:1428`, `src/main.rs:1477-1481`; `src/sync/mod.rs:586-600`
**Issue:** With no `--remote` or `central_repo` set, `open_sync_templates` uses
`find_templates_dir()`, which checks cwd `templates/` first. When nothing is found it
silently falls back to `PathBuf::from("templates")`. `scan_project` never checks that the
template root exists or is a ude template tree. Every managed file with a record then
reads as upstream-deleted, and pull deletes all of them with exit 0. Two triggers:
- (a) a project that has its own `templates/` directory, which is common for
  Django/Flask/Jinja/Helm projects;
- (b) a binary that finds no templates (see CR-01).

This code predates the diff. It is reported because the phase goal is state hermeticity,
and because CR-01's fix depends on the same lookup.

**Reproduced:**
- (a) `ude init --path web` run from outside the project, then `templates/index.html`
  added. `ude sync status` showed 11 lines "Deleted upstream". `ude sync pull` exited 0
  and removed all 11 managed files.
- Negative control for (a): the same project without `templates/` showed 11 lines
  "In Sync".
- (b) A project initialized with templates, then pulled by the templates-hidden build:
  all managed files were removed.

**Fix:** In `open_sync_templates`, exit `EXIT_HARD_FAILURE` when no templates directory is
found, instead of falling back to `"templates"`. Make `find_templates_dir` accept a
candidate only if it is a ude template tree, for example
`candidate.join("projects/generic").is_dir()`. Have `scan_project` (or the caller) refuse
an `upstream_root` that fails the same check, before it classifies anything as deleted.

### CR-03: A monorepo subdirectory whose parent repo git cannot read gets its committed edits overwritten (exit 0)

**File:** `src/main.rs:1611-1626`
**Issue:** `committed_paths` was changed to downgrade any non-zero `git ls-files` exit to
a warning when the `.git` is only in a parent directory. The commit's intent was to
ignore an empty leftover `.git`. But the downgrade does not look at why git failed: a
corrupt index, "detected dubious ownership" (`safe.directory`, typical for
container-mounted or sudo-owned checkouts) or any other exit 128 all become
"not a git checkout". Init then treats committed managed files as uncommitted, and
`copy_template_file` / `render_and_write` overwrite them with the template.
**Reproduced:** In a monorepo with `svc/.gitconfig` carrying a committed alias, I cloned
the repo and wrote garbage to `.git/index`. `ude init --type rust` in `svc` printed
"Warning: git ls-files failed: fatal: .git/index: index file smaller than expected; not
treating the project as a git checkout" and exited 0. The committed alias was gone.
Negative control: the identical clone with an intact index printed "Kept committed
.gitconfig" and the alias survived. The dubious-ownership trigger was not reproduced; it
cannot be simulated without a second uid, but it goes down the same branch.
**Fix:** Only "not inside a work tree" may mean "no checkout". Every other failure must
stop init, as it already does for the project's own `.git`:
```rust
let probe = git(["rev-parse", "--is-inside-work-tree"]);   // run_bounded, cwd = project
if !probe.status.success() {
    let err = String::from_utf8_lossy(&probe.stderr);
    if err.contains("not a git repository") { warn(...); return Ok(HashSet::new()); }
    return Err(format!("git cannot read the enclosing repository: {}", err.trim()));
}
// then git ls-files; any failure here is an Err
```

## Warnings

### WR-01: `--type generic` (or `""`) records a type that sync renders differently; mise.toml reads "Upstream Updated" right after init

**File:** `src/main.rs:188-192`, `src/main.rs:518`; `src/template/mod.rs:104-119`
**Issue:** `TemplateContext::detect_for_project` treats `"generic"` and `""` as "detect".
During a first init (no state.json yet) it picks `rust` from `Cargo.toml` or `node` from
`package.json`. Init renders mise.toml with that detected type but records the raw
argument (`Some(project_type)`). Sync passes `"generic"` and reads `"generic"` back from
state.json, so it renders the polyglot branch. Upstream then differs from the recorded
ancestor, and the next pull rewrites mise.toml.

The new keep-recorded-type fallback was added to stop exactly that rewrite. It now also
carries `"generic"` across re-runs.

**Reproduced:**
- `touch Cargo.toml; ude init --type generic; ude sync status` showed "mise.toml:
  Upstream Updated (ready to pull)" and state `"project_type": "generic"`.
- Negative control: `--type rust` showed "mise.toml: In Sync".

**Fix:** Record the type that was rendered: `project_state.project_type =
Some(ctx.project_type.clone());`. Also validate `--type` against the supported set; see
IN-02.

### WR-02: pre-push now blocks every push when the advisory database cannot be fetched

**File:** `scripts/hooks/pre-push:19`, `README.md:81`
**Issue:** Blocking pushes on open advisories was a deliberate project decision (commit
01127f6, documented in the README). It deviates from the operator's default policy, which
makes advisory checks non-blocking. A side effect was not documented or handled:
`cargo deny check` fetches RustSec advisory-db from GitHub on every run, and a failed
fetch exits 1. So any push made while GitHub is unreachable is refused, and the message
looks like an advisory failure. Examples: offline, a captive or filtered network, a LAN
or self-hosted remote, a GitHub outage.
**Reproduced:**
- `unshare -rn cargo-deny check --hide-inclusion-graph advisories` exited 1 with "failed
  to fetch advisory database".
- Control: `bans licenses sources` under the same no-network sandbox exited 0.
- `cargo-deny --offline check advisories` with no network exited 0, using the cached
  database.

**Fix:** Pick a behavior and document it. One option: run the online check, and if it
failed because the fetch failed, re-run with `"$cargo_deny" --offline check` and warn
that the database may be stale. If refusing pushes when offline is intended, say so in
the README line and in the hook comment.

### WR-03: A failed state save is only a warning (exit 0), which silently drops the new kept-tombstone markers

**File:** `src/main.rs:585-595`
**Issue:** The new code exits 2 when a committed file cannot be read, so that init does
not "lose that marker". The e2e test `test_init_refuses_unreadable_committed_file_without_template`
asserts that no state is written in that case. However, if `save_to_dir` itself fails,
init prints a yellow warning and exits 0 with no state at all.

Without state, a user's uncommitted file that has no template has no tombstone. It reads
as `(None, Some, None)` untombstoned, which is LocallyModified, and push treats it as
publishable. That is the outcome the new tombstone logic exists to prevent.

This chain was traced from the code (`compare_3way_hashes` rows 7-8) and not reproduced
end to end.
**Fix:** Treat a failed save as a hard failure, as the unreadable-file path already does:
```rust
if let Err(e) = project_state.save_to_dir(&project_path) {
    eprintln!("  {} Initialization failed: cannot persist .ude/state.json: {e}", "✗".red());
    std::process::exit(EXIT_HARD_FAILURE);
}
```

## Info

### IN-01: Init hard failures exit 1, which this binary defines as "drift"

**File:** `src/main.rs:17-19`, `src/main.rs:545`, `src/main.rs:214`; `tests/e2e_lifecycle.rs` (`test_init_fails_on_unusable_git_dir_in_project`)
**Issue:** `EXIT_DRIFT = 1` and `EXIT_HARD_FAILURE = 2`. Several init failures exit 1:
the new `template()` closure (545), the `committed_paths` error (214), and the
directory and template write failures. The new refusals added in this diff exit 2, and
the new test pins exit 1 for a git failure. A script cannot tell an init failure from
sync drift.
**Fix:** Use `EXIT_HARD_FAILURE` for every init failure and update the test to expect
`Some(2)`.

### IN-02: Fresh-clone default ignores project detection; `--type` is not validated

**File:** `src/main.rs:192`, `src/cli/mod.rs:66-69`
**Issue:**
- With no `--type` and no state (a fresh clone), the fallback is a hard-coded `"rust"`,
  even though `TemplateContext` already detects `node` from `package.json` when given
  `"generic"`.
- A typo such as `--type rsut` is accepted, renders the polyglot branch, and is now kept
  on every later init run.

**Fix:** Fall back to detection rather than `"rust"`, and restrict the argument with
`value_parser = ["rust", "node", "polyglot"]`.

### IN-03: `--mempalace` and `--graphify` cannot be turned off, so their pre-flight guards never skip

**File:** `src/cli/mod.rs:71-77`, `src/main.rs:245-250`
**Issue:** `bool` with `default_value_t = true` is SetTrue. I confirmed that
`ude init --mempalace=false` fails with "unexpected value 'false'". As a result,
`if init_args.mempalace` / `graphify` is always true. A symlinked `.planning` or
`graphify-out` therefore always fails the new pre-flight, and the user has no flag to
opt out. This predates the diff, but the pre-flight makes it matter.
**Fix:** `#[arg(long, default_value_t = true, action = clap::ArgAction::Set)]`, or add
`--no-mempalace` / `--no-graphify` flags.

### IN-04: "No shared template" is judged against the local templates, not the remote that sync uses (unverified)

**File:** `src/main.rs:282`, `src/main.rs:560-575`
**Issue:** Init decides whether a file is unmanaged by looking in `find_templates_dir()`.
When `.ude.toml` sets `central_repo`, sync compares against the remote cache instead. A
file that has a template on the remote but not locally gets tombstoned at init and then
reads Conflicted on the first pull. This was traced from the code only; not reproduced.
**Fix:** Resolve the same template root that sync would use (`open_sync_templates`)
before classifying anything, or document the limitation.

### IN-05: The supply-chain workflow has never run; push and pull_request both trigger it

**File:** `.github/workflows/supply-chain.yml:5-12`
**Issue:** The branch is not pushed, and `gh run list --branch …` returns no runs, so the
workflow is unverified in CI. What was checked:
- both pinned SHAs match their tags (`actions/checkout` v7.0.1 → `3d3c42e…`,
  `jdx/mise-action` v5.0.1 → `7a4e45a…`);
- `install_args` and `version` are valid inputs at that SHA;
- mise treats config as trusted when `CI` is set, so `mise run` works without
  `mise trust`;
- a local full-history `gitleaks git --redact` found no leaks across 188 commits;
- a local `cargo deny check` passes.

None of this proves the jobs pass on the runner. Separately, an unfiltered `push:` plus
`pull_request:` runs every same-repo PR twice.
**Fix:** Push and assert on `gh pr checks <PR>` for the head SHA. Optionally limit `push`
to `branches: [main, develop]`.

### IN-06: e2e tests that assume "no git" now depend on the tempdir not being inside a repository

**File:** `tests/e2e_lifecycle.rs:1197-1225` (`sync_cli_with_env`); `test_init_ignores_unusable_git_dir_above_project`
**Issue:** `committed_paths` now walks every ancestor looking for `.git`, and git does
its own discovery. If `TMPDIR` sits inside a checkout, the "no git" tests run against
that repository. `test_init_ignores_unusable_git_dir_above_project` would then find the
real repo and fail its assertion.
**Fix:** Set `GIT_CEILING_DIRECTORIES` to the tempdir's parent in `sync_cli_with_env`.

### IN-07: The pre-push hook checks the working tree, not the commits being pushed

**File:** `scripts/hooks/pre-push:15-19`
**Issue:** `cargo deny check` reads the working tree's `Cargo.lock`. A push of commits
whose lockfile differs from the checkout (uncommitted lock edits, or pushing another
ref) is judged on the wrong graph. This predates the diff; it now applies to advisories
too.
**Fix:** Document it, or check the pushed ref's tree, for example with
`git worktree add --detach` on the local sha read from stdin.

---

_Reviewed: 2026-10-21T17:55:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: deep_
