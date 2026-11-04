---
phase: 02-sanitizer-boundaries-security-hardening
reviewed: 2026-11-04T19:59:24Z
depth: deep
files_reviewed: 3
files_reviewed_list:
  - docs/ARCHITECTURE.md
  - src/process.rs
  - src/sync/mod.rs
findings:
  critical: 0
  warning: 1
  info: 1
  total: 2
status: issues_found
---

# Phase 02: Code Review Report

**Reviewed:** 2026-11-04T19:59:24Z
**Depth:** deep
**Files Reviewed:** 3
**Status:** issues_found

## Narrative Findings (AI reviewer)

## Summary

Scope: `git diff 805bf52..HEAD -- src docs`, which covers plans 02-24 to 02-26. `src/main.rs` and
`tests/` did not change in this range. Call chains were traced through `validate_committed_tree`,
`require_fetched_head_holds`, `stage_publication`, `read_verified_object`, `verified_tree_listing`,
`push_tree_entries`, `parse_tree_object`, `listing_diff`, `raw_diff_entries`, `tree_listing_blobs`,
`count_empty_directories`, `first_path_not_held` and `process::spawn_prepared`.

**No critical finding. I found no decision on the publication path that still comes from
unhashed bytes.**
- Both routes now read the commit, every tree and every blob they decide from through
  `read_verified_object`. Each decision uses the same bytes that were hashed.
- Production code no longer runs `git ls-tree` or `git diff-tree`.
- The only git reads left that are not hashed are these:
  - `rev-parse` of HEAD, a ref compared with the fetched head;
  - `cat-file -t`, which the hash makes redundant, because the hash input carries the type;
  - the staged gate's `git status`, `diff --cached`, `show` and `write-tree`. These are not
    binding. The committed gate re-derives every one of their decisions from hashed bytes, and
    the tree id is bound.

**One warning (WR-600).** A legitimate tree whose subtrees are shared builds a listing that grows
exponentially from a few dozen objects. The new in-process walk has no size limit and no overall
deadline. The `git ls-tree` and `git diff-tree` runs it replaced were each bounded by the 30 s
local timeout (SYNC-03). This was reproduced at function level.

**What argues against WR-600:**
- It is an availability problem. A push is never published wrongly: the gate fails closed when
  it finishes.
- The only practical trigger is a hook in the cache checkout, which can already exhaust the host
  by itself.
- The plan's threat T-02-102 places a growing tree in NOT-14's availability class.

The case T-02-102 did not consider is memory: a few dozen objects can drive gigabytes of memory
in UDE itself, and its claim that "each read is bounded" does not bound the walk.

**How it was tested:**
- Probes ran in a `git archive` copy of HEAD under the session scratch directory, with its own
  `TMPDIR` and target directory, as `#[ignore]` tests added to the copy only.
- 8 hash-valid but odd trees, each listed by the gate and by `git ls-tree`:
  - modes `0100644`, `1100644`, `0` and `000000100644`;
  - a name containing `/`;
  - unsorted entries, a repeated name, and the name `..`.

  The two listings were byte-identical in all 8 cases. Git 2.56.0 only.
- A plain-git probe: `rev-parse --verify HEAD^{commit}` on a forged commit file fails with
  `hash mismatch`, exit 128, and the honest control exits 0.
- The full suite was not run. The 13 control rows and the mutation run of 02-26 were not re-run.

### Prior findings: status at HEAD (ca5e4c9)

- **WR-500 (the listing came from an unchecked first read): fixed.**
  - `verified_tree_listing` (`src/sync/mod.rs:3099-3127`) builds both routes' listings from
    hashed tree bytes, the root included.
  - The commit is read with `read_verified_object(..., "commit")` (`:2581`, `:2941`).
  - The fetched head's trees on the committed route come through the same map (`:2709`).
  - The six swap rows place the swap after the listing, through the `cfg(test)` seam. Moving the
    seam call before the listing would turn them into "not held" refusals, so they would fail
    (reasoned).
- **WR-501 (root rows refused before the gate): fixed.**
  `test_fetched_head_holds_refuses_root_tree_not_matching_its_id` calls the gate directly. Its
  control (the same bytes under their own id) reads as held. The push rows now say what they
  pin: git status, before the gate. DEF-12 says the same.
- **WR-502 (stdin deadline untested): fixed** by
  `test_run_bounded_with_input_times_out_when_the_child_neither_reads_nor_exits`
  (`src/process.rs:510-536`). 02-26 row 12 records the mutant kill; I did not re-run it.
- **IN-500 (a detached stdin writer can leak): still open.** `src/process.rs:191-196` is unchanged.
- **IN-501 (a failed object read is reported as "not held"): still open, not taken.** It is now
  at `src/sync/mod.rs:2898`.
- **WR-400, IN-400, IN-401, IN-402, IN-300, IN-301: still open.** Their code and doc rows are
  outside the diff.
- **IN-403: still open.** The deletion check at `src/sync/mod.rs:3012-3022` still falls back to
  the absolute staged path through `strip_prefix(repo).unwrap_or(staged)`.

### Checked and found sound

- **No gap between the hash and its use.** Each of these is the exact `Vec` that was passed to
  `hash-object`, or a slice parsed from it:
  - `read_verified_object` returns the bytes it hashed (`:3078`, `:3087`);
  - `push_tree_entries` parses the bytes it stored in `verified` (`:3149-3151`);
  - the change loop and `first_path_not_held` scan and fingerprint the bytes that
    `read_blob_by_id` returned.

  A swap between `cat-file -t` and `cat-file <kind>` cannot pass, because the `-t tree`/`commit`
  hash covers the type header. A tree cannot contain its own id, so the walk cannot loop.
- **Tree decoder.** `parse_tree_object` matches git's `decode_tree_entry` and `canon_mode` on
  every form probed: legacy and odd modes, mode 0 (treated as a gitlink), names containing `/`,
  `..`, tab, newline or byte 0xFF, unsorted or repeated entries, and SHA-1 and SHA-256. The one
  difference is documented and fails closed: a mode that overflows u32.
- **`listing_diff` against `diff-tree -r --no-renames`.**
  - Every commit record not matched exactly (mode and id) by an unmatched base record at its
    path is listed, and every unconsumed base record becomes `D`.
  - Type changes become `T`. Gitlinks are compared like blobs. Tree records are ignored, as with
    `-r`.
  - The result is never narrower than git's. On duplicate or unsorted base trees it is the more
    conservative of the two.
  - A `T` to a gitlink at an allowlisted path fails closed: `read_blob_by_id` reports Unreadable.
- **Order of refusals.** The commit's listing is built before the fetched head's, and on the same
  map, so a tree both commits hold is read once.
- **Refusal text.** `first_path_under` names only recorded, staged or allowlisted paths, and the
  change loop prints only paths that passed the allowlist (DEF-14).
- **Empty-status route, commit id check (`:2944-2948`).** git's own check in `rev-parse ^{commit}`
  shadows it for a commit file forged before the gate (probed above). It still covers a swap
  between `rev-parse` and `cat-file`, so it is not dead code.
- **process.rs.** The new test uses a child that neither reads nor exits, 4 MiB of input and a
  0.5 s limit. It asserts `Err` with the label, the limit and an elapsed bound. On timeout the
  writer gets EPIPE after the group kill, so the test leaks nothing.
- **Docs.** The DEF-09, DEF-11 and DEF-12 rows match the code. No NOT row changed. NOT-10 still
  covers an overwrite after the gate's last read, during the push.

## Warnings

### WR-600: The verified tree walk has no size or time limit, and shared subtrees make its output grow exponentially

**File:** `src/sync/mod.rs:3099-3161` (`verified_tree_listing`, `push_tree_entries`), called at `:2659`, `:2709` and `:2954`. Also `src/sync/mod.rs:3318-3362` (`listing_diff` copies the listing again).
**Reproduced:** yes, at function level: `verified_tree_listing` and `validate_committed_tree` were called directly in a scratch copy, in a debug build. The post-commit hook trigger was reasoned, not run end to end.

**Issue:**
- The walk expands every path in the listing. The memo `verified` only spares the git reads, not
  the expansion.
- A legitimate tree in which level k holds `a` and `b`, both pointing to the tree of level k-1,
  needs only d+3 tree objects for 2^d leaf paths.
- Each git read is bounded by `LOCAL_TIMEOUT`, but the in-process loop, the `listing` `Vec`,
  `listing_records` and the `changes` `Vec` of `listing_diff` have no bound and never check a
  deadline.

Probe results: the commit tree is `templates/{f, bomb}`, with `bomb` built as above. The gate
refused correctly each time, but only after the times shown.

| depth d | listing bytes | listing time | gate time | peak RSS |
|--------:|--------------:|-------------:|----------:|---------:|
| 16 | 19.5 MB | 1.7 s | 2.6 s | not measured |
| 18 | 81 MB | 6.0 s | 9.4 s | not measured |
| 20 | 338 MB | 18.9 s | 33.1 s | 1.14 GB |

Each +2 in depth multiplies the numbers by about 4. By extrapolation, not run, d = 24 needs
about 18 GB of RAM and d = 30 several hundred GB. Whichever runs out first, the host's memory or
the user's patience, decides the outcome: an OOM kill, or a push that hangs with no deadline.

**How it is reached:**
- A post-commit hook runs despite `--no-verify`. It creates the tree with `git mktree` and
  points HEAD at a `commit-tree` whose parent is the fetched head and whose message is the sync
  message.
- `stage_publication` resolves HEAD to that commit (`:2167`). The parent, message and header
  checks pass, and the walk runs at `:2659` before the tree binding at `:2846` could refuse it.
- A remote-side trigger is not practical: ensure's checkout of such a fetched head would write
  2^d files and fail first (reasoned).

**Why this is a regression:**
- Before 02-24 and 02-25 the same inputs went through `git ls-tree` and `git diff-tree` under
  `run_local`, and each was killed at the 30 s local timeout (reasoned, not run).
- SYNC-03 requires bounded subprocess work.
- T-02-102 says the walk is mitigated because "each read is bounded by the local tier's timeout"
  and that a growing tree "slows only its own push". Neither holds for this shape: the time is
  spent outside any read, and the memory is UDE's own.

**Fix:** Bound the walk and fail closed with a refusal that names no tree text. For example,
count records and stop past a fixed cap, and check one deadline for the whole walk:
```rust
/// More listing records than any template repository holds; a larger walk refuses.
const MAX_LISTING_RECORDS: usize = 1 << 20;
// in verified_tree_listing, with `deadline = Instant::now() + self.limits.local`:
while let Some((path, mode, id)) = pending.pop() {
    records += 1;
    if records > MAX_LISTING_RECORDS || Instant::now() >= deadline {
        return Err(ListingDefect::TooLarge);
    }
    ...
}
```
- Map `TooLarge` to a fixed text on both routes, such as "the tree of ... is larger than the
  gate reads".
- Add a row with a d = 21 tree, more than 2^20 paths, that must refuse within the deadline, and
  a control at d = 4 that lists normally.
- Correct T-02-102's disposition in the 02-24 SUMMARY's threat flags.
- If the operator instead accepts this under NOT-14, widen NOT-14's wording to "a hook can make
  the gate exhaust memory or run without a deadline" so the closed list says it.

## Info

### IN-600: New tests chain several scenarios, so the first failing assertion hides the rest

**File:** `src/sync/mod.rs:15599-15719` (`test_verified_tree_listing_matches_ls_tree`), `src/sync/mod.rs:15744-15856` (`test_listing_diff`)
**Issue:** `test_verified_tree_listing_matches_ls_tree` runs four independent scenarios in
sequence with plain `assert!` and `assert_eq!`:
- the SHA-1 fixture;
- the empty commit;
- the SHA-256 repository;
- the forged-subtree negative control.

`test_listing_diff` chains 12 independent cases in the same way. A failure in an early case masks
every later one. This is the pattern the operator's test rules ask to avoid ("Atomic Test
Granularity"). `test_committed_gate_refuses_base_tree_not_matching_its_id` gathers its failures
before asserting, so it does not have this problem.
**Fix:** Split the scenarios into separate `#[test]` functions that share a fixture builder, or
collect the failures into a `Vec` and assert once, as the base-tree test does.

---

_Reviewed: 2026-11-04T19:59:24Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: deep_
