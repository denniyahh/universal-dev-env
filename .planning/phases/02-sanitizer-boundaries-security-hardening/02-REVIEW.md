---
phase: 02-sanitizer-boundaries-security-hardening
reviewed: 2026-11-04T15:26:37Z
depth: deep
files_reviewed: 3
files_reviewed_list:
  - docs/ARCHITECTURE.md
  - src/process.rs
  - src/sync/mod.rs
findings:
  critical: 0
  warning: 3
  info: 2
  total: 5
status: issues_found
---

# Phase 02: Code Review Report

**Reviewed:** 2026-11-04T15:26:37Z
**Depth:** deep
**Files Reviewed:** 3
**Status:** issues_found

## Narrative Findings (AI reviewer)

## Summary

Scope: `git diff 88bb986..05dc379` for the three files (plans 02-22 and 02-23). Call chains were
traced into `process::spawn_prepared`, `prepare_command`, `stage_publication`,
`validate_committed_tree`, `require_fetched_head_holds` and the push functions. `src/main.rs` and
`tests/` did not change in this range.

**No critical finding.** CR-400 is closed for the case it described: an object file changed
before the gate reads it. Blobs and the trees above recorded or deleted paths are now hashed
against their ids on both routes, in direct and PR mode. All 20 new tests pass at HEAD.

**Three warnings:**
- **WR-500.** The tree check hashes a second read of each tree. The listing that decides "held"
  comes from the first read. A process left behind by a hook can swap the object file between the
  two reads and get a false Ok. Reproduced at function level, with the timing simulated.
- **WR-501.** The root-tree rows cited under DEF-12 cannot fail. They are refused at `git status`
  before the gate runs. The same steps without the forgery are refused too.
- **WR-502.** The new process test does not check the property it is named for. A mutant that
  writes stdin without a thread passes it, and that mutant overruns its deadline.

**What argues against WR-500:** it needs a concurrent process, not just a hook. NOT-10 already
leaves to the operator "a loose object file overwritten during the push, after the gate read it".
If the operator reads that row as covering an overwrite between two gate reads, WR-500 becomes a
NOT-10 wording question, not a defect.

How it was tested: probe tests and mutants were run in a `git archive` copy of HEAD under the
session scratchpad, with its own `TMPDIR` and target directory. That directory has been removed.
`git status --porcelain` shows only the existing ` M .planning/state.json`, and
`git config --local --list | wc -l` is 22. The full test suite was not run. Only git 2.56.0 and
local-path remotes were used.

### Prior findings: status at 05dc379

- **CR-400 (local objects stood in for the remote's objects): fixed** for objects forged before
  the gate reads them. `read_verified_object` (`src/sync/mod.rs:3001-3044`) hashes blobs and trees.
  It covers blobs changed in the commit (`:2651`), recorded blobs and the trees above them on both
  routes (`:2809-2835`, `:2842-2864`), and the trees above deleted paths on the empty-status route
  (`:2961`). All 20 new tests were run and pass. A residual channel remains: an object swapped
  between two gate reads (WR-500).
- **WR-400 (relative remote path, cache key, wrong "hook" diagnosis): still open.** The code is
  unchanged: the cache key at `src/sync/mod.rs:1042` and the message at `:1169`.
- **IN-400 (`open_push_templates` returns an unused checkout path): still open.** `src/main.rs`
  is unchanged (`:764`, `:919`, `:1042`, `:1615`).
- **IN-401 (DEF-02 overstates the replace-ref coverage of the staged gate's reads): still open.**
  The DEF-02 row and `stage_publication` (`:1926`, `:1982`, `:2027`, `:2089`) are unchanged.
- **IN-402 ("not at the validated commit" is also used when the read-back failed): still open**
  at `src/sync/mod.rs:1211-1219`.
- **IN-403 (the empty-status deletion check passes silently for a staged path outside the
  checkout): still open, now at `:2957`.** The new `verify_ancestor_trees` call at `:2961` gets the
  same absolute path. It finds no named ancestors, so it checks nothing and stays silent. This
  still cannot happen today (reasoned).
- **IN-300 (failure paths leave a branch or commit behind): still open.** The PR push and direct
  push failure paths are unchanged, and NOT-14 records them.
- **IN-301 (the e2e helper runs `ude init` with the host's HOME): still open.**
  `tests/e2e_lifecycle.rs` is unchanged.

### Every git read on the gate paths, classified

On the **empty-status route** (`require_fetched_head_holds`):

| Read | Location | What decides the result |
|------|----------|-------------------------|
| `rev-parse --verify HEAD^{commit}` | `:2890` via `resolve_commit` | A ref read compared with the fetched head that `ls-remote` confirmed. It is not object content. |
| `ls-tree -r -t -z --full-tree <head>` | `:2899-2917` | git checks the commit and the root tree in this read. Probed: a forged root gives `error: hash mismatch` and exit 128. git does not check subtrees here. Probed: a forged subtree is walked with exit 0. The subtrees are checked separately, see the next row. |
| subtree and blob reads (`cat-file -t`, `cat-file <kind>`, `hash-object`) | `read_verified_object` | Checked against the id. The bytes returned for a blob are the bytes that were hashed. For a tree, the listing came from a different read (WR-500). |

On the **committed route** (`validate_committed_tree`):

| Read | Location | What decides the result |
|------|----------|-------------------------|
| `cat-file commit <commit>` | `:2553` | Not hashed by UDE. The commit is new and is sent, so what the remote gets is this same local object. The later `diff-tree` and `ls-tree` reads make git check the commit. |
| `diff-tree base commit`, two-argument form | `:2607-2618` | Reads the real tree, not the commit-graph's. Probed with a commit-graph whose tree id was patched: `git log -1 --format=%T` and the one-argument form `diff-tree <commit>` followed the forged graph and dropped an added file. The gate's two-argument form and `ls-tree` did not. The two-argument form is load-bearing; it should stay. The base-side subtrees it reads are not checked: that is the deferred DEF-09 class, below. |
| `ls-tree -r -t -z --full-tree <commit>` | `:2696-2718` | Same as on the empty-status route. |
| changed blobs and recorded blobs and trees | `read_verified_object` | Checked against the id. |

In the **staged gate** (`stage_publication`):

| Read | Location | What decides the result |
|------|----------|-------------------------|
| `git status`, `git diff --cached`, `git show :<path>`, `git write-tree` | `:1926`, `:1982`, `:2027`, `:2089` | Not checked against ids. They are not the binding check: the committed gate re-reads every changed blob by id, and the tree id is bound. |

### The deferred DEF-09-class channel (not counted as a finding)

This is a forged base-side tree that hides an added file from `diff --cached` and `diff-tree`.
**The new code does not make it worse.** It only adds checks.

It is **not reachable through UDE alone.** UDE writes objects only through git commands, and git
hashes what it writes. With a hook writing object files, the channel is reasoned to be narrow, not
run. To hide file F, the forged base tree must list F under the same blob id. The push's object
walk reads that same forged tree and treats F's blob as already on the remote, so it does not send
it. receive-pack's connectivity check then rejects the push unless the remote already holds that
blob. So F can be published only with content the remote already has, at a path nothing validated.

### Checked and found sound

- **Hash format.** Ids are compared in full against `hash-object` output. The test in a SHA-256
  repository passes. An abbreviated id never matches; it returns `IdMismatch` and fails closed.
  `ls-tree` prints full ids, so this case is not reachable today.
- **`hash-object` flags.** `--literally` keeps `-t tree` to pure hashing. Without it, git would
  run fsck checks. `--no-filters` is implied for `--stdin` without `--path`, so it is redundant
  but harmless. No `-w` is passed, so the check writes nothing; a test confirms this. Exactly one
  trailing LF is stripped.
- **No secret values in the new refusals.** The new refusal texts are `OBJECT_ID_MISMATCH` plus a
  path from the record, from the staged list, or one that passed the allowlist.
- **No deadlock in the stdin helper.** stdout and stderr are drained on their own threads. stdin
  is written on a detached thread. A broken pipe is ignored, because Rust ignores SIGPIPE.
  Probed: with a 500 ms timeout and a child that neither reads nor exits, the call returns `Err`
  after 0.51 s.
- **Test hermeticity.** `GitFixture::cmd` (`:3650-3664`) removes the repository-local git
  variables and sets `GIT_CONFIG_GLOBAL`, `GIT_CONFIG_NOSYSTEM` and `GIT_CEILING_DIRECTORIES`.
  Every repository is a `TempDir`. No new test uses `std::env::set_var`. `forge_loose_object`
  removes the target file before writing, so a hardlinked object in a bare remote is not changed.
- **Assertions and controls.** The id-mismatch rows assert `ID_MISMATCH` and the absence of the
  "not held" text, with controls that assert the opposite. There are no regex alternations. Every
  test name added to DEF-11 and DEF-12 exists as exactly one `fn`. A made-up name used as a
  negative control was reported missing.
- **Refusal path with two recorded paths.** HashMap order does not decide which path the
  `beside_added` rows name: the added path's trees are new and honest, so only `GATE_REL` can
  fail.
- **Cost.** There are three short git processes per verified object, and ancestor trees are
  deduplicated within a call. For template-sized records this is not a real cost.

## Warnings

### WR-500: The tree check hashes a second read; the listing that decides "held" comes from the first

**File:** `src/sync/mod.rs:2809-2835` (`first_path_not_held`), `src/sync/mod.rs:2842-2864` (`verify_ancestor_trees`), `src/sync/mod.rs:2899-2952` (listing and the deletion check)
**Reproduced:** at function level, with the timing simulated. It was not run with a real background process.

**Issue:** `verify_ancestor_trees` calls `read_verified_object(repo, oid, "tree")` and throws
away the bytes it hashed. The blob ids, the modes and the deleted-path check all come from the
earlier `ls-tree` process, which read each subtree separately and does not check it (probed).
Suppose a process outlives its hook and swaps a subtree's object file:
1. It holds forged bytes while `ls-tree` runs.
2. It puts the honest bytes back before the `cat-file` reads.

The listing then names the validated blob at the recorded path, while the check hashes the honest
tree and passes. A file watch on the object file makes this timing easy. `finish_after_exit`
deliberately leaves background processes running.

Probe: `templates_repo` with `templates/f` = "f base". The `templates` tree object was forged to
list the validated blob, the listing was taken, and then the honest bytes were put back. Result:
```
require_validated_content(...) = Ok(())
HEAD:templates/f = fa000d9...   (the seed blob)
validated blob   = 304e837...
```
The impact depends on the route:
- **Empty-status route:** a false Ok, against promise 2, DEF-11 and DEF-12. The doc's text says
  "each tree read for it must hash to its id", but the tree read that produced the listing is not
  the one hashed.
- **Deleted paths on the same route:** a deleted path still on the remote can read as gone.
- **Committed route:** recorded content held in an unchanged subtree can read as present.

**Fix:** Decide only from bytes that were hashed. Starting at the root, which git checks inside the
`ls-tree` read, walk each recorded or deleted path through the raw tree bytes that
`read_verified_object(..., "tree")` returns. Parse the entries (`<mode> SP <name> NUL
<raw id>`), and take the child id and mode for the next component from those bytes. Compare the
final blob id and mode with that, not with `listed_blobs`. Add a row that changes the object file
after `ls-tree` and before the check, using a test seam or a pre-built listing as in the probe.
Then either name this channel in DEF-11 and DEF-12, or have the operator widen NOT-10 to cover
"overwritten between two gate reads".

### WR-501: The root-tree rows cited under DEF-12 are refused before the gate runs, and would be refused without the forgery

**File:** `src/sync/mod.rs:15444-15480` (`assert_fetched_root_tree_not_matching_its_id_refused`), `docs/ARCHITECTURE.md:110` (DEF-12: "git itself refuses a root tree that does not")
**Reproduced:** yes.

**Issue:** The doc comment says the row "pins git's own check of the root tree, which the gate
relies on". It does not:
- With the forgery, the push fails at `git status`, inside `stage_publication`, before
  `require_fetched_head_holds` runs: `Err(Failed("git status failed in remote checkout ... error:
  hash mismatch ... | error: bad tree object HEAD"))`.
- The same step without the forgery is also refused: `Err(Failed("Publication gate violation: git
  status reports nothing to push, but the fetched head does not hold the validated content of
  templates/projects/generic/gitconfig.repo ..."))`.
- The forged root keeps the honest `templates` entry and adds `extra`. Even a git that did not
  check the root would show the seed at `GATE_REL` and refuse it as not held.

So the row passes whether git checks the root or not, and whether the gate is reached or not. The
DEF-12 claim itself is true; a direct probe showed `ls-tree -r` on a forged root fails with
`hash mismatch`. But the cited tests do not establish it, and the PR row is the same.

**Fix:** Pin the claim where the gate relies on it. Call `require_fetched_head_holds` directly on a
checkout whose HEAD root object is forged so that its `templates` entry points to an honest tree
holding `HOOKED_TEMPLATE` at `GATE_REL`. That forgery would read as held if git did not check the
root. Assert the refusal `"the fetched head's tree cannot be listed"`, and add a control: the same
forged listing with git's check out of the way, for example by taking the listing from the forged
tree id, must read as held. Either reword the doc comment of the existing rows to "refused before
the gate (git status fails)", or remove them from DEF-12.

### WR-502: The "child ignores stdin" test does not check the bound it is named for

**File:** `src/process.rs:493-509` (`test_run_bounded_with_input_returns_when_the_child_ignores_stdin`), `src/process.rs:185-196`
**Reproduced:** yes, by mutation.

**Issue:** The test uses `git --version`, which exits at once. A mutant that writes stdin on the
calling thread (`{ let _ = stdin.write_all(&input); drop(stdin); }`) passes both new process
tests. That mutant breaks the deadline: with `sh -c 'sleep 3'`, 4 MiB of input and a 500 ms
timeout, it returned after 3.006 s, and as `Ok`, not as a timeout. The real code returned `Err`
after 0.515 s. So the property the doc comment states ("a child that never reads cannot hold the
call past its deadline") has no test that would catch a regression.

**Fix:** Use a child that neither reads nor exits, such as `sh("sleep 30", &[])`, with a short
timeout and more input than a pipe buffer holds. Assert `Err` containing `"timed out"` and
`elapsed < timeout + KILL_GRACE + margin`. Keep the `git --version` case as the "exits without
reading" row.

## Info

### IN-500: A stdin writer thread can be leaked for the life of the process

**File:** `src/process.rs:193-195`
**Issue:** The writer is detached. A descendant that left the process group and kept the inherited
stdin pipe open without reading blocks `write_all` forever. That leaks one thread and the input
buffer per call until UDE exits. `git hash-object` starts no such descendant, so this cannot
happen today (reasoned, not run).
**Fix:** Document this in the `run_bounded_with_input` doc comment. Or, on timeout or exit, keep the
writer's `JoinHandle` and drop the stdin handle once the deadline passes. Closing the write end
from another thread needs an owned fd, for example passing `ChildStdin` through an `Arc<Mutex<Option<_>>>`.

### IN-501: A failed object read is reported as "content not held, a hook most likely hid the change"

**File:** `src/sync/mod.rs:2823`, `:2834`, `:2933`, `:2969-2970`, `:2659-2663`
**Issue:** `ObjectDefect::Unreadable` covers several failures: a spawn failure, a timeout of the
new `hash-object` or tree read, or a non-zero exit. In `first_path_not_held` it becomes
`NotHeld::Content`, and the refusal then says the head "does not hold the validated content ...
a hook in the cache checkout most likely hid the change". In the deletion loop it becomes "the
fetched head's tree cannot be listed". These refusals fail closed, but they name the wrong cause.
This is the same class as WR-400 and IN-402 (reasoned).
**Fix:** Add a `NotHeld::Unreadable` variant whose message says the object could not be read or
hashed (`"could not read or hash an object for {path}"`).

---

_Reviewed: 2026-11-04T15:26:37Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: deep_
