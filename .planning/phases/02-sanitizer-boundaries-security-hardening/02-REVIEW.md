---
phase: 02-sanitizer-boundaries-security-hardening
reviewed: 2026-11-01T16:05:22Z
depth: deep
files_reviewed: 1
files_reviewed_list:
  - src/sync/mod.rs
findings:
  critical: 2
  warning: 5
  info: 6
  total: 13
status: issues_found
---

# Phase 02: Code Review Report

**Reviewed:** 2026-11-01T16:05:22Z
**Depth:** deep
**Files Reviewed:** 1
**Status:** issues_found

## Narrative Findings (AI reviewer)

## Summary

Scope: `git diff ed8dc0d6 HEAD -- src/sync/mod.rs` (plans 02-11 and 02-12), at HEAD 392b82b. The
call chains were traced through `src/process.rs` and the caller in `src/main.rs`.

**The gate's main claim is still false.** A post-commit hook can publish unvalidated bytes through
`push_to_remote`, and the call returns Ok in both modes. The gate looks a blob up by path, so a tree
listing the same path twice shows it the first entry. Git's checkout writes the last one. A fresh
`ensure_remote_repo` on the remote then held `INJECTED_BY_HOOK` in the managed file (CR-200). A hook
can also delete or revert a validated file and still get Ok (CR-201).

How the claims were tested: each `reproduced` below comes from a run in a scratch copy of HEAD, with
probe tests added to `mod tests` and driving the real `push_to_remote` against local bare remotes on
git 2.56.0.

What those runs do not show:
- No hosted remote was tried. A bare remote with `receive.fsckObjects=true` rejected the duplicate
  tree in a plain-git probe, so CR-200 and WR-200 depend on the host. Whether GitHub or another host
  refuses these trees is unverified.
- WR-203 is a race, seen in 1 of 10 runs on a local-path remote. That shows it can happen, not how
  often it would happen over a network.
- WR-202 was reproduced with a shim printing the output expected from git older than 2.31. No real
  old git was run.
- 96 existing gate and hook tests pass on the unmodified gate (filtered run, not the full suite).
  They do not cover any channel reported here, so passing says nothing about these findings.

### Standing findings

- **WR-102 (merge HEAD gate, and no ancestry check in PR mode) is fixed.** The gate now requires
  exactly one raw parent equal to the fetched head in both modes and lists changes from that head.
  `test_direct_push_refuses_hook_merge_over_amended_commit`, its PR twin and
  `test_pr_push_refuses_hook_added_commit` passed in this run. The separate defects below are not
  part of WR-102.
- WR-01, WR-02, WR-101, IN-01..IN-05 and IN-101..IN-103 are outside this diff, stayed out of scope
  by operator decision, and were not re-examined.
- T-02-52 and T-02-53 are accepted and not re-reported. File mode and type changes are also out of
  scope (orchestrator decision in plan 02-11).

### Checked and found sound

- Header scan false positives: 9 ordinary idents, including domains holding `key`, `token`, `secret`,
  `auth` and `api`, were not flagged. A PAT-shaped author, the control, was flagged.
- An empty directory already in the fetched head does not refuse a push: UDE's `git commit` rebuilds
  the tree from the index and drops it. The push returned Ok.
- Hook-written `remote.origin.mirror` makes the push fail, and hook-written `remote.origin.push` is
  ignored in favour of the explicit refspec. Both were plain-git probes.
- `raw_commit_parents` counts every `parent ` header line, a superset of git's parser, so a
  disagreement refuses. This comes from reading the code and git's parser as recalled; it was not
  run against a crafted object.

## Critical Issues

### CR-200: A tree listing the managed path twice publishes unvalidated bytes, and consumers check them out

**File:** `src/sync/mod.rs:2349-2408` (the per-entry loop), `src/sync/mod.rs:2369` (`show` by path)
**Reproduced:** yes, direct and PR mode.
**Issue:** A post-commit hook rebuilds UDE's commit, keeping the parent and message bytes, so that the
leaf tree holds the managed file twice: the validated blob first, the injected blob second
(`git mktree` accepts this). `diff-tree` lists `M <path>` and `A <path>`. Both pass the allowlist, and
`git show <commit>:<path>` returns the first entry both times, so both compare equal to the record.
`count_empty_directories` sees nothing wrong.

Observed results:
- `push_to_remote` returned Ok in both modes.
- The remote's `ls-tree -r` listed both blobs under the same path, and `cat-file blob` on the injected
  id returned `INJECTED_BY_HOOK`.
- `git show` on the remote returned the validated bytes.
- A fresh `ensure_remote_repo` on that remote left `INJECTED_BY_HOOK` in the managed file with
  `status` reporting `M`. The pusher's own cache held the same bytes after its next ensure. So the
  bytes reach what `ude sync pull` reads as the upstream template.

This breaks plan 02-08's prohibition, which plan 02-12 records as `status: resolved` with one
exception (T-02-52). It also breaks T-02-38. No secret scan runs on the second blob, so an injected
secret publishes the same way.

**Fix:** Refuse a tree whose raw listing repeats a path. UDE's own commit comes from the index, which
cannot hold duplicates, so a clean push should never trip this (inferred from how the index works; a
clean push was not run with this fix). Add hook tests in both modes that assert the refusal, that the
remote does not hold the injected blob, and a control that a push without the check delivers it.

```rust
// In the ls-tree -r -t -z pass (count_empty_directories generalised):
let mut seen = HashSet::new();
for record in listing.split_inclusive(|byte| *byte == 0) {
    // ...parse meta and path as today...
    if !seen.insert(path) {
        return Err(TreeDefect::DuplicateEntry); // never print the name
    }
}
```

Reading each listed blob by its own object id (`diff-tree -r -z --raw`, then `cat-file blob <oid>`)
would also stop the path lookup from hiding an entry. It is worth doing as well as the check above.

### CR-201: A hook can delete or revert a validated file and the push still returns Ok

**File:** `src/sync/mod.rs:2364-2367` (the `D` skip), `src/sync/mod.rs:2349-2408` (no check that validated content is present)
**Reproduced:** deletion in direct and PR mode; revert in direct mode.
**Issue:** The gate checks only what the diff lists, and it skips every `D` entry on the allowlist,
including a path that `validated` holds a digest for. Its doc comment says every changed file is
"identical to what `validated` recorded".
- Hook `git rm --cached "$F"` plus `git commit --amend --no-edit`: Ok, and the remote's tree no longer
  holds the template.
- Hook `git checkout HEAD~1 -- "$F"` plus `git commit --amend --allow-empty`: Ok and the success
  message, with the remote blob still the old bytes. Nothing the user validated was published.

Consequences, read from the code and not run end to end:
- After Ok in direct mode, `src/main.rs:1266-1285` records the validated hash as the new ancestor, so
  local state claims a publish that did not happen.
- `SyncStatus::UpstreamDeleted` (`src/sync/mod.rs:27`) makes pull delete an unchanged local copy, so a
  hook's deletion spreads to other machines.

**Fix:** Bind the commit to the record in both directions. A real deletion has no record
(`deleted_templates` are added to `staged_files` at line 1707 without a `record` call), so the
distinction already exists.

```rust
if status.starts_with('D') {
    if validated_rel.contains_key(&p) {
        return Err(format!(
            "Post-commit publication gate violation: the commit deletes validated file {}",
            p.display()
        ));
    }
    continue;
}
// After the loop: every validated path must hold its recorded content in `commit`.
for (rel, digest) in validated_rel.iter().filter(|(rel, _)| paths.contains(*rel)) {
    // `git --no-replace-objects show {commit}:{rel}`; refuse when missing or
    // when Self::fingerprint(&bytes) != *digest.
}
```

## Warnings

### WR-200: A tree entry named `.` passes the allowlist and makes the remote unclonable

**File:** `src/sync/mod.rs:2358` (`paths.contains(&p)`), `src/sync/mod.rs:2456` (`PathBuf::from`)
**Reproduced:** yes, direct mode.
**Issue:** `PathBuf` equality ignores a `.` component in the middle of a path, so
`templates/projects/generic/./gitconfig.repo` equals the allowlisted path. A hook adds a subtree named
`.` holding the validated blob. The listed path is allowed, `git show` reads that entry, and the bytes
match. `push_to_remote` returned Ok. `git clone` of the remote then failed with exit 128,
`invalid path 'templates/projects/generic/./gitconfig.repo'`, so one hook breaks every later clone of
the shared repository. No unvalidated bytes are involved, which is why this is a warning.
**Fix:** Compare listed paths as raw bytes, not through `PathBuf`, or refuse any path component that
is empty, `.` or `..` in the same listing pass as CR-200. Add a hook test.

### WR-201: `rollback_commit` leaves the cache checkout wrong after a refusal, and no test checks it

**File:** `src/sync/mod.rs:2191-2206`, callers at `src/sync/mod.rs:1967`, `1980`, `2082`, `2094`, `2131`
**Reproduced:** yes, each state below.
**Issue:** Rollback moves relative to `HEAD~1`, and it ignores the `fetched_sha` the gate just bound to.
After a refusal in direct mode:
- Hook-added commit: HEAD and `main` stay on the hook's amended commit, and the managed file holds
  `INJECTED_BY_HOOK` with a clean `status`.
- Parentless commit, or a hook that reset to a root base: `refs/heads/main` is deleted and the managed
  file is gone from the working tree.
- Hook detached HEAD and amended: HEAD stays detached, and `main` stays on UDE's commit.

The `.unwrap_or(false)` at line 2197 also takes the destructive branch (delete the ref, then
`clean -fd`) when the `rev-parse` itself fails to run. That was read from the code and not run.

Limit on the impact: in all six cases tried, the next `ensure_remote_repo` restored HEAD to the fetched
head, and nothing was published. So this leaves unvalidated bytes on disk until the next run; it is not
a publication path. The refusal tests call `assert_remote_unchanged` but never check the checkout. Only
the stray-commit test checks HEAD, and that asserts the relative behaviour.
**Fix:** Pass `fetched_sha` and `branch` to rollback and run `checkout -f -B <branch> <fetched_sha>`,
then `clean -fd`. Add a helper asserting HEAD, branch and managed file bytes after each refusal, and
call it from the hook tests.

### WR-202: `refuse_grafts_file` passes when the printed path is not one absolute path

**File:** `src/sync/mod.rs:2227-2236`
**Reproduced:** with a shim, on a repository that really holds `.git/info/grafts`. Real git older than
2.31 was not run, so what it prints is unverified.
**Issue:** `rev-parse --path-format=absolute` was added in git 2.31. An older `rev-parse` is expected to
echo the unknown flag and print a relative path. The code uses that whole output as one path, and the
lookup is against UDE's working directory, not the repository. The result is NotFound, then `Ok(())`.
Control: the same repository with one absolute line was refused. `test_refuse_grafts_file_outcomes` has
no row for this, and the project states no minimum git version that I could find.
**Fix:** Fail closed unless the output is a single line and the path is absolute.

```rust
let path = Path::new(located.trim_end_matches('\n'));
if located.trim_end_matches('\n').contains('\n') || !path.is_absolute() {
    return Err(UNLOCATED.to_string());
}
```

### WR-203: A post-commit hook's background process can write grafts after the gate has passed

**File:** `src/sync/mod.rs:2266`, push sites at `src/sync/mod.rs:2009` and `2124`
**Reproduced:** 1 of 10 runs on a local-path remote. All 10 returned Ok; one delivered the injected blob
to the remote. A one-in-ten race on a local path is a weak bound, and a network handshake before the
object walk may widen the window (not measured).
**Issue:** The check runs once, before the push. A post-commit hook can leave a process running that
waits for the push and then writes `.git/info/grafts`. This is not T-02-52: that accepted exception is
worded as a pre-push hook, and the prohibition names only that case. The operator's reason for accepting
T-02-52 (a hostile hook can already push with the user's credentials) may well apply here too, but the
acceptance does not currently cover it.

The comment at lines 2263-2265 treats the variable as unusable only because `prepare_command` removes
it. In a plain-git probe, `GIT_GRAFT_FILE=/dev/null git push` sent no injected blob with a grafts file
present, while the control without the variable did send it.
**Fix:** Either let the two pushes set `GIT_GRAFT_FILE` to the null device, which needs a carve-out in
`process::prepare_command` (the probe covers a file present at push time, not this race), or extend the
T-02-52 wording to any hook-started process and record that acceptance.

### WR-204: Tests named for inherited `GIT_TEST_*` variables never exercise inheritance

**File:** `src/sync/mod.rs:2007`, `src/sync/mod.rs:2122`, tests at `src/sync/mod.rs:10150-10180`
**Reproduced:** yes, by mutation.
**Issue:** `test_direct_push_ignores_inherited_git_test_commit_graph` and its PR twin set the variable
through the engine's extra environment, which `Command::get_envs()` also finds. Replacing
`std::env::vars_os().map(|(key, _)| key)` with an empty iterator at both pushes left those two tests and
`test_remove_git_test_env` passing. With the real process variable set, the forged-graph tests failed
under that mutation (`the remote holds the injected blob`) and passed without it. So the code works
today, but the argument that makes it work is unpinned. `02-SECURITY.md` T-02-51 notes the extra-env
setup; it does not say the call-site argument can be removed unnoticed.
**Fix:** Add one test that spawns the built binary or a child test process with the variable in its real
environment. Alternatively, build the push command in one function with the key source injected, and
test that function.

## Info

### IN-200: A hook-written `push.pushOption` reaches the remote

**File:** `src/sync/mod.rs:1997-2006`, `src/sync/mod.rs:2111-2121`
**Reproduced:** plain git. A remote's pre-receive hook saw `INJECTED_OPTION_TEXT` in `GIT_PUSH_OPTION_0`.
On a remote that does not advertise push options, the push fails instead. The text is not stored in the
repository, but it is hook-written text sent beyond the commit, and some hosts act on push options
(not tested). It belongs on the T-02-42 list.
**Fix:** Add `-c push.pushOption=` to both pushes. Probed: the pre-receive hook then saw no option, and
the control without it still did.

### IN-201: The push command and its comment block are written out twice

**File:** `src/sync/mod.rs:1985-2007`, `src/sync/mod.rs:2098-2122`
**Issue:** Plan 02-12's per-option matrix exists because each option must be pinned at each of two sites.
A security option added to one site can be missed at the other. IN-200's fix would be a third such option.
**Fix:** Add one `fn publication_push(&self, repo, refspec, extra_args) -> Command`.

### IN-202: `validate_committed_tree` is about 200 lines

**File:** `src/sync/mod.rs:2248-2445`
**Issue:** It runs five separate checks, and CR-200, CR-201 and WR-200 add more. The project rule is about
40 lines per function.
**Fix:** Split it into parent and message binding, the changed-blob loop, and the tree listing check.

### IN-203: The `GIT_TEST_` prefix match is case-sensitive, and Windows environment names are not

**File:** `src/sync/mod.rs:2492`
**Reproduced:** no, unverified; no Windows host was available. `src/process.rs:121` has a Windows path, and
`test_remove_git_test_env` pins `git_test_lower` as kept. On Windows that key would probably be the same
variable git reads.
**Fix:** Compare with `eq_ignore_ascii_case` under `cfg(windows)`.

### IN-204: The `commit_command` doc comment says "no hooks"

**File:** `src/sync/mod.rs:2165`
**Issue:** `--no-verify` skips only pre-commit and commit-msg. Both plans rest on post-commit and
prepare-commit-msg still running. This comment predates the diff.
**Fix:** Reword it to say which hooks still run.

### IN-205: Refusals print hook-chosen path names

**File:** `src/sync/mod.rs:2355`, `src/sync/mod.rs:2360`
**Issue:** The gate's comment at line 2411 says a hook-chosen directory name "is never printed", yet the
unvalidated-file and unparseable-entry refusals print the name unredacted, and
`test_push_refuses_hook_added_file` asserts it. A secret-shaped filename would reach the terminal or CI log.
It is not published. Read from the code; not run with a secret-shaped name.
**Fix:** Pass the name through the sanitizer before printing, or print a count.

---

_Reviewed: 2026-11-01T16:05:22Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: deep_
