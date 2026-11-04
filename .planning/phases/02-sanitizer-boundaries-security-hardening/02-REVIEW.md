---
phase: 02-sanitizer-boundaries-security-hardening
reviewed: 2026-11-04T06:54:17Z
depth: deep
files_reviewed: 4
files_reviewed_list:
  - docs/ARCHITECTURE.md
  - src/main.rs
  - src/sync/mod.rs
  - tests/e2e_lifecycle.rs
findings:
  critical: 1
  warning: 1
  info: 4
  total: 6
status: issues_found
---

# Phase 02: Code Review Report

**Reviewed:** 2026-11-04T06:54:17Z
**Depth:** deep
**Files Reviewed:** 4
**Status:** issues_found

## Narrative Findings (AI reviewer)

## Summary

Scope: `git diff f09e0ca..HEAD` for the four files (plans 02-16 to 02-21), at HEAD 782e58b. Call
chains were traced through `src/process.rs` (`prepare_command`, `GIT_LOCAL_ENV_VARS`) and
`src/main.rs`.

**One critical finding (CR-400).** The fixes for the five earlier findings are in the code. But the
gate trusts the cache checkout's object store for the content of objects the push never sends,
because the remote already holds them. git does not check that a stored object's bytes match its
id when it reads them. So a hook-side change to the local object store can make the gate see the
validated content at a recorded path while the remote holds something else. The push then returns
Ok. This was reproduced on both the "nothing to push" route (DEF-12) and the committed route
(DEF-11), in direct and PR mode. Nothing unvalidated reaches the remote this way. The damage is a
false Ok: direct mode records an ancestor for content the remote does not hold.

**What argues against CR-400:** it needs code in the cache checkout that writes to `.git/objects`.
NOT-10 already lists "a loose object file overwritten during the push, after the gate read it" as
pending the operator. CR-400 is a different case: the object is changed *before* the gate reads it,
and the remote never receives it. Under the doc's rule (1), that makes it a defect against DEF-11
and DEF-12. If the operator reads NOT-10 as covering every forged local object, it becomes a
rule (3) question for the operator instead.

How it was tested: probe tests were added to `mod tests` in a `git archive` copy of HEAD, under
the session scratchpad with its own target directory. They drove the real
`ensure_remote_repo_for_push` and `push_to_remote` against local bare remotes, using the existing
`push_with_empty_status` and `seeded_bare_remote` fixtures. Each run had a control (the same steps
with no object-store change), and every control was refused as DEF-11 and DEF-12 describe. The
scratch copy and its target directory have been removed. `git status` of the repository was clean
afterwards.

What those runs do not show:
- The object-store change was made by the test step, not by an installed hook. This is the same
  model as `test_direct_push_refuses_assume_unchanged_set_after_ensure`, which the doc lists
  under DEF-12. No CLI run (`ude sync push`) was made.
- The data-loss chain (the next `ude sync pull` taking the remote's old content over the user's
  edit) is reasoned from the three-way rule, not run.
- Only local-path remotes on git 2.56.0 were used. A packed object (not loose) was not tried.
  In the fixture the fetched blob was loose.
- The full test suite was not run.

### Prior findings: what the changes since f09e0ca did

Read from the code. These were not re-probed, except where CR-400 says so.
- **CR-300 (a ref named after the commit id): resolved.** Both pushes send `<id>^{commit}` with
  `--no-replace-objects` (`src/sync/mod.rs:2195-2210`, `:2349-2365`), and `resolve_commit` reads
  past replace refs (`:2415`).
- **CR-301 (the fetched head moved by a hook): resolved.** `ensure_remote_repo_for_push`
  (`:1133-1163`) compares the fetched head with `git ls-remote` of the caller's URL, run from `/`.
  `main.rs` push uses it (`src/main.rs:1615-1648`).
- **WR-300 (an index flag gives Ok with nothing published): resolved for index flags, but
  bypassed by CR-400.** `require_fetched_head_holds` (`:2591-2672`) is on both routes that report
  nothing to push.
- **WR-301 (a hook-written pushurl): resolved.** The push names `target.remote_url`, and Ok needs
  `confirm_published` (`:1195-1209`, `:2227`, `:2396`).
- **WR-302 (hook bytes from a `parent` header in a refusal): resolved.** The refusal names only
  the fetched head (`:2552-2561`). The many-parents arm prints only a count.
- **IN-300 (failure paths leave a branch or commit behind): still open.** These paths are
  unchanged: the PR push timeout at `:2212-2214`, the PR push failure at `:2219-2224`, and the
  direct push failure that is not a fast-forward rejection at `:2390-2393`. NOT-14 now records this
  as open, so it is not re-raised.
- **IN-301 (the e2e helper runs `ude init` with the host's HOME): still open** at
  `tests/e2e_lifecycle.rs:913`. The new `CliPushWorld` (`tests/e2e_lifecycle.rs:1739-1932`) runs
  init through the hermetic runner, so the new CLI rows do not have this problem.
- CR-200, CR-201, WR-200 to WR-204, IN-200 to IN-205 and the earlier rows were not re-examined.

### Checked and found sound

- Every test name in the closed list (215 distinct names) exists as a `fn` in `src/` or `tests/`.
  A made-up name used as a negative control was correctly reported missing.
- `remote_head` keeps a line only when its refname is exactly `refs/heads/<branch>`.
  `test_remote_head` puts a branch whose name ends in `refs/heads/main` first and still gets the
  real head.
- `prepare_command` removes `GIT_DIR` and the other repository-local variables, so the reads run
  from `/` cannot be pointed at another repository through UDE's environment.
- `require_fetched_head_holds` requires HEAD to be the fetched head before it reads the tree, so a
  commit on top of the head that holds the content is refused.
- `persist_immediately = remote_cache.is_none()` (`src/main.rs:1150`) has the same meaning as the
  old `remote_checkout_root.is_none()`: the two are `Some` together in both open functions.

## Critical Issues

### CR-400: The gate trusts local copies of objects the remote already holds, so a changed local object store gets Ok while the remote lacks the record

**File:** `src/sync/mod.rs:2879-2891` (`read_blob_by_id`), `src/sync/mod.rs:2568-2582` (`first_path_not_held`), `src/sync/mod.rs:2613-2645` (empty-status route), and the committed gate's check of recorded paths that the commit leaves unchanged
**Reproduced:** yes. Empty-status route: direct and PR. Committed route: direct and PR.
**Issue:** On both routes, "the record is held" is decided by reading blob bytes by id from the
cache checkout's object store. When that id is already on the remote (any object reachable from
the fetched head), the push sends nothing for it. The remote keeps its own copy, and the gate
never compares the two. git returns the bytes stored under an id without checking that they hash
to it (`git fsck` reports such a mismatch; `cat-file` does not). So a local object whose bytes no
longer match its id makes the gate read the validated content while the remote holds the original.

Results (the control in each case is the same steps without the object-store change):
```
empty-status, direct: res=Ok("No changes to push to remote repository.")   remote heads holding the record: 0
empty-status, PR:     res=Ok("No changes to push to remote repository.")   remote heads holding the record: 0
  control:            Err(Failed("Publication gate violation: git status reports nothing to push, but the fetched head does not hold the validated content of templates/projects/generic/gitconfig.repo; ..."))
committed, direct:    res=Ok("Directly pushed changes to remote branch 'main'.")   remote main at the managed path: the seed, not the record
committed, PR:        res=Ok("Pushed changes to branch 'ude-sync-...'...")         same
  control:            Err(Failed("Post-commit publication gate violation: the commit to publish does not hold the validated content of templates/projects/generic/gitconfig.repo"))
```
This breaks the doc's promise 2 ("Ok with nothing to push means the head read that way holds every
recorded path") and the outcomes of DEF-11 and DEF-12. In direct mode `src/main.rs:1277-1289` then
records the validated hash as the ancestor. On the next pull the local file equals that ancestor
and the remote file differs, so the pull would take the remote's old content over the user's edit
without a conflict (reasoned, not run). The same trust applies to tree and commit objects read on
these routes (`ls-tree` of the fetched head reads the commit and its trees from the same store).
That variant is reasoned, not run.

**Fix:** Do not let a local object stand in for the remote's object without checking its id. Either:
1. Re-hash every object whose content these checks rely on, and refuse on a mismatch. For blobs,
   hash `"blob <len>\0" + bytes` in process, using SHA-1 or SHA-256 according to the id length, and
   compare with `oid` in `read_blob_by_id`. Do the same for the raw commit and for each tree on the
   path to a recorded file, on the empty-status route and in the committed gate. Or:
2. Record each validated file's git blob id when the staged gate reads it, and require the
   committed tree, or the fetched head's tree, to list exactly that id at each recorded path. Ids
   then decide "held", and no stored content is trusted.

Add rows in both modes, on both routes, where a local object no longer matches its id: each must be
refused. Keep the existing controls. Then either add the channel to DEF-11 and DEF-12, or, if the
operator rules it out of scope, widen NOT-10 to say so.

## Warnings

### WR-400: The push blames "a hook or config in the cache checkout" for benign mismatches, and a relative remote path can make every push fail permanently

**File:** `src/sync/mod.rs:1032` (cache key), `src/sync/mod.rs:1139-1162`, `src/sync/mod.rs:3075-3087` (`absolute_remote_url`)
**Reproduced:** no, reasoned from the code.
**Issue:** The cache key is the caller's spelling (`{remote_url}#{branch}`). The head read and the
push use that spelling made absolute against the current working directory. The same relative
`--remote ../x` given from two different directories names two repositories but one cache
checkout, whose `origin` is the first one. From the second directory, `head_before` and
`head_again` read repository B and the fetch reads repository A. Every push then fails with "a hook
or config in the cache checkout most likely moved or redirected it", until the user removes the
cache. The cause in that message is wrong, and nothing in it tells the user what to do. The same
message appears when the remote moves twice between the two reads. Status and pull from the second
directory silently read A. That part existed before this phase, but the push now exposes it with a
wrong diagnosis.
**Fix:** Key the cache by `absolute_remote_url(remote_url)`, with a migration or a fallback for
existing caches. At minimum, in `ensure_remote_repo_for_push`, compare the checkout's
`remote.origin.url` with `cache.remote_url` before blaming a hook. If they differ, say that the
cache was made for another repository and how to clear it. Add a test that runs from two
directories.

## Info

### IN-400: `open_push_templates` returns a checkout path nobody uses

**File:** `src/main.rs:1042`, `src/main.rs:1620-1628`
**Issue:** The only caller binds the second tuple element as `_remote_checkout_root`. The push now
gets the checkout from `cache.publish_target`.
**Fix:** Return `(PathBuf, Option<sync::RemoteCache>)` from `open_push_templates`.

### IN-401: DEF-02 says every gate read runs past replace refs, but the staged gate's reads do not

**File:** `docs/ARCHITECTURE.md` (DEF-02 row), `src/sync/mod.rs:1916`, `:1972`, `:2017`, `:2079`
**Issue:** `git status`, `git diff --cached`, `git show :<path>` and `git write-tree` in
`stage_publication` honour replace refs. The committed gate reads raw and is the binding check, so
this is not a known bypass (reasoned). The sentence still states more than the code does.
**Fix:** Say "every committed-gate read", or add `--no-replace-objects` to the staged gate's reads.

### IN-402: The "not at the validated commit" message is also used when the read-back itself failed

**File:** `src/sync/mod.rs:1201-1208`
**Issue:** When `remote_head` returns `Err`, the refusal still says the remote branch "is not at the
validated commit". In fact the state is unknown, and the remote may hold the commit. Direct mode
then rolls the commit back and records nothing. That fails closed, but the text misleads, and the
next pull may show a conflict the user does not expect.
**Fix:** Use separate wording for the `Err` case ("could not confirm the push"), and keep the
pull-then-push advice.

### IN-403: The empty-status deletion check passes silently for a staged path outside the checkout

**File:** `src/sync/mod.rs:2654-2666`
**Issue:** `strip_prefix(repo).unwrap_or(staged)` leaves a path outside the checkout absolute, so it
never matches the listing and the deletion counts as done. Recorded paths fail closed in the same
situation; deletions fail open. This cannot happen today, because staged paths come from
`templates_dir` under the checkout (read, not run).
**Fix:** Refuse when `strip_prefix` fails, as a gate refusal.

---

_Reviewed: 2026-11-04T06:54:17Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: deep_
