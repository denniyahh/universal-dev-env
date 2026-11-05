---
phase: 02-sanitizer-boundaries-security-hardening
reviewed: 2026-11-05T02:32:39Z
depth: deep
files_reviewed: 2
files_reviewed_list:
  - src/sync/mod.rs
  - docs/ARCHITECTURE.md
findings:
  critical: 0
  warning: 1
  info: 5
  total: 6
status: issues_found
---

# Phase 02: Code Review Report

**Reviewed:** 2026-11-05T02:32:39Z
**Depth:** deep
**Files Reviewed:** 2
**Status:** issues_found

## Narrative Findings (AI reviewer)

## Summary

Scope: `git diff c5e170e..HEAD -- src docs`, which covers plans 02-27 and 02-28. I traced these
call chains: `listing_budget`, `ListingBudget::{new, check_time, take}`, `is_past`,
`verified_tree_listing`, `push_tree_entries`, `parse_tree_object`, `listing_diff`,
`raw_diff_entries`, `tree_listing_blobs`, `count_empty_directories`, `validate_committed_tree`,
`require_fetched_head_holds`, `read_blob_by_id` and `read_verified_object`.

**I found no critical issue. The walk is bounded as the plan says.**
- **Bytes.** The byte charge equals the bytes the listing writes. I probed it on odd trees and
  checked each case with git:
  - gitlink, symlink and legacy modes;
  - a zero-padded mode `0000000100755`;
  - names with a tab, a newline or byte 0xFF;
  - SHA-1 and SHA-256 trees.

  A limit of exactly git's `ls-tree -r -t -z --full-tree` byte count listed the tree. A limit
  one byte lower refused it with `TooLarge`.
- **Pending stack.** Nothing is pushed until its records and bytes have been taken, so the
  listing and the pending stack together stay within the budget.
- **Overflow.** The sums are checked. The only unchecked `Instant + Duration` adds
  `limits.local`, which production code never sets above 30 s.
- **Deadline.** The deadline is checked before every tree, cached trees included, and at every
  pass of both loops of `listing_diff`. Between two checks the walk does one object read and at
  most one tree's worth of in-memory record writes. A tree with 2^18 blob entries therefore
  costs one read plus about 2^18 pops before the next check.
- **Reorder.** It changes refusal texts only. `tree_listing_blobs` ran on every Ok path before,
  and it still does, so the set of pushes that are accepted is unchanged.
- **Parent-only empty-directory count.** It equals the old count on names without `/`, and is
  never lower otherwise.
- **Test seams.** The `cfg(test)` seams do not reach a non-test build.

**One warning (WR-700).** The deadline-carry test has two wall-clock assertions with less than
a 3x margin. Neither adds discrimination the test does not already have. Under CPU
oversubscription, 1 of 3 runs failed.

**What argues against WR-700:**
- The load was artificial: 12 busy loops on 4 CPUs.
- The project's own parallel runs measured the control at up to 2.339 s, against the 5 s bound
  (02-27 SUMMARY).
- The test passed under llvm-cov and cargo-mutants (02-28 SUMMARY).
- I did not reproduce a failure under `cargo test` or `cargo nextest` themselves.

**How it was tested:**
- Probes ran as `#[ignore]` tests added to a `git archive` copy of HEAD under the session
  scratch directory, with its own `TMPDIR` and target directory.
- The copy and every busy loop are gone. The busy loops were stopped by their recorded PIDs.
- I did not run the full suite, the 20 control rows or the mutation run.

### Prior findings: status at HEAD (0630ff7)

- **WR-600 (the walk had no size or time limit): fixed.**
  - `ListingBudget` (`src/sync/mod.rs:3380-3419`) is shared by both listings of a gate call
    (`:2738`, `:2799-2804`) and the empty-status listing (`:3068`). It holds 2^18 records,
    64 MiB of listing bytes and one deadline of `limits.local`.
  - `listing_diff` takes the same deadline (`:2868`).
  - The refusals are fixed texts. The production-limit tests pin the constants.
  - What remains unbounded is listed in 02-27-PLAN.md under "Not claimed": the `verified` map,
    a single large tree object and the change loop. IN-701 below adds the deadline overrun.
- **IN-600 (chained tests): fixed** by 20d9a90.
  - `test_verified_tree_listing_matches_ls_tree` became 4 tests and `test_listing_diff`
    became 12.
  - I compared every assertion with c5e170e: the claims loop, all 12 labels, and the expected
    values. Each one is present once, and the arguments are unchanged.
  - `listing_diff(..)` is now `diff_unbounded(..)`. It panics if the deadline it is given,
    one hour away, ever passes, so it cannot hide a `PastDeadline`.

### Checked and found sound

- **Question 2, outcomes.**
  - The `listed_blobs` map is the same value it was. It is now computed earlier and used later.
  - The five tests that 02-27 updated are the only changed expectations.
  - No pair of inputs moves from refused to accepted, or from accepted to refused.
- **Question 3, seams.**
  - All four fields and their builders are `cfg(test)`.
  - In a non-test build, `listing_budget` reads only `MAX_LISTING_RECORDS`, `MAX_LISTING_BYTES`
    and `self.limits.local`.
  - `read_blob_by_id` counts reads only under `cfg(test)`.
  - The integration tests under `tests/` run the binary built without `cfg(test)`.
- **Question 5, tests that cannot fail.** None found. Each new row has a negative control in
  the same test, or a row in the 02-28 control table. The `!err.contains("bomb")` assertions can
  fail only if a refusal starts to print tree text, and that is what they are meant to catch.
- **Deadline tests at 1 ms.** These are deterministic. The first check after the root read is
  past 1 ms whatever the load, and a check failing at the root gives the same text.
- **`test_listing_diff_refuses_*_past_its_deadline`.** These are deterministic: `is_past(now)`
  is true for a monotonic clock.
- **Docs.** The DEF-09 and DEF-10 test names each match one test. No NOT row changed. NOT-14 is
  unchanged.

## Warnings

### WR-700: The deadline-carry test fails under CPU load on wall-clock assertions that add no discrimination

**File:** `src/sync/mod.rs:19532-19568` (`test_committed_gate_carries_the_deadline_into_the_fetched_head_listing`); assertions at `:19548-19551` and `:19567`
**Reproduced:** yes, under artificial load. Baseline: the test ran alone and serially, and its
control took 0.92 s. With 12 busy loops on 4 CPUs, I ran the same test binary 3 times:

| Run | Control time | Result |
|-----|-------------:|--------|
| 1 | 5.905 s | **FAIL**: `the control took 5.904710842s, so the margin under 15s is under 3x` |
| 2 | 3.844 s | pass; the whole test took 20.48 s, against the 21 s bound of the hooked call |
| 3 | 2.428 s | pass |

**Issue:**
- The test asserts two wall-clock bounds:
  - the unhooked control finishes in under `WALK / 3` = 5 s;
  - the hooked call returns within `HOOK_SLEEP + 5 s` = 21 s.

  In run 2 the hooked call passed with 0.52 s to spare.
- Neither bound is needed to prove what the test claims:
  - The hooked call's refusal must contain `the tree of the fetched head` and must not contain
    `the tree of the commit to publish`. That already proves the commit's listing finished
    inside the deadline and the fetched head's listing started past it.
  - The 16 s sleep after a 15 s walk makes the refusal deterministic.
- So the time bounds add only the chance of a spurious failure:
  - The project's own record already shows this pattern: the 5 s / 6 s version failed the
    same margin at 1.861 s.
  - The change to 15 s / 16 s widened the margin to about 2x the worst control time
    observed, not to a margin that removes the failure mode.

**Fix:** Drop the control's elapsed assertion, or keep it as a printed measurement only.
Replace the hooked call's upper bound with a hang guard far from any real time, so it still
fails a hang but not a slow host:
```rust
assert_eq!(control, Ok(()));
println!("control elapsed: {control_elapsed:?}");
// ... hooked call ...
assert!(err.contains(PAST_DEADLINE), "{err}");
assert!(err.contains("the tree of the fetched head"), "{err}");
assert!(!err.contains("the tree of the commit to publish"), "{err}");
// A hang guard, not a timing claim: the refusal is decided by the text above.
assert!(elapsed < HOOK_SLEEP + Duration::from_secs(60), "took {elapsed:?}");
```

## Info

### IN-700: The refusal for a diff past the deadline is reachable in production but pinned by no test

**File:** `src/sync/mod.rs:2871-2875`
**Issue:** The 02-28 SUMMARY reports this arm as uncovered. My conclusions:
- **It is reachable.** The fetched head's listing checks the deadline only before each tree
  read. The last tree read can start just before the deadline and run up to three local-tier
  git runs (IN-701). After that read, the next check is `listing_diff`'s first pass, which then
  refuses through this arm.
- **No existing seam reaches it.** `with_after_tree_listing` runs only after the commit's
  listing. The fetched head's root check then runs straight after that hook, so a sleep in the
  hook always trips the fetched-head arm first. A walk time set to expire during the last read
  would make a timing-dependent test.
- **Mapping it to another refusal would pass the suite.** Every such mapping still fails closed,
  so only the text is at risk.

**Fix:** No seam is needed. Move the `DiffDefect` to refusal mapping into a pure function and
unit-test it, as the other refusal texts are pinned:
```rust
fn diff_refusal(defect: DiffDefect) -> String {
    match defect {
        DiffDefect::Unparseable => UNPARSEABLE_ENTRY.to_string(),
        DiffDefect::PastDeadline => format!(
            "Post-commit publication gate violation: the changes of the commit to publish {LISTING_PAST_DEADLINE}"
        ),
    }
}
```
A `cfg(test)` hook run right before `listing_diff` would also pin it end to end. It costs one
more seam in production code.

### IN-701: The walk can run past its deadline by one object read, up to three local timeouts

**File:** `src/sync/mod.rs:3281-3283` (`push_tree_entries`); `read_verified_object` at
`:3174` makes three `run_local` calls, each with its own 30 s limit.
**Issue:**
- `check_time` runs before the tree read, never after it. A read that starts just before the
  deadline can finish up to 3 × `LOCAL_TIMEOUT` = 90 s later, before the next check fires.
- So the walk's wall-clock bound is the deadline plus one `read_verified_object`, not the
  deadline itself.
- DEF-09 states "one deadline, the local time limit from the start of the walk". The push
  still refuses, just later, so no promise is broken.

This is reasoned, not run. No hook I know of can slow `cat-file` or `hash-object` in a
controlled way.

**Fix:** Either check the deadline again after the read (`budget.check_time()?` right after
`verified.insert`), or say in the `ListingBudget` doc comment and DEF-09 that the deadline
stops new reads rather than bounding the walk's wall time.

### IN-702: A commit listing that uses up the shared budget is refused as "the tree of the fetched head"

**File:** `src/sync/mod.rs:2807-2811`; the text is pinned by
`test_committed_gate_refuses_commit_tree_over_the_byte_limit` (`:19311-19312`)
**Issue:**
- The commit listing is taken first from the shared budget. A hook-built commit tree that
  leaves less than the fetched head's size is therefore refused at the second listing.
- The refusal reads "the tree of the fetched head is larger than the publication gate lists
  for one push".
- The fetched head is the remote's tree, verified against its id. The oversized tree is the
  commit's.
- The text points the user at the wrong tree, though the trailing clause ("a tree in the cache
  checkout most likely ...") is generic. This is by design and tested, but it misdirects
  diagnosis.

**Fix:** For the fetched head's `TooLarge` arm, name both listings, for example "the trees of
the commit to publish and the fetched head together are larger than ...". Update the two tests
that assert `the tree of the fetched head` beside `TOO_LARGE`.

### IN-703: count_empty_directories' new doc comment claims a `/` in a name can never hide an empty tree; it can

**File:** `src/sync/mod.rs:3685-3693`
**Reproduced:** yes, at function level, in the scratch copy.
- Probe: a root tree, written with `hash-object --literally`, holds an empty tree `a` and a
  blob named `a/x`.
- `verified_tree_listing` lists it as `040000 tree <empty>\ta` and `100644 blob <x>\ta/x`.
  That is byte for byte the listing of a tree whose `a` holds `x`.
- `tree_listing_blobs` accepts it, and `count_empty_directories` returns `Some(0)`.
- Control: the same root without the `a/x` entry gives `Some(1)`.

**Issue:**
- The comment says "a name holding `/` can only add to the count, never hide an empty tree".
  That holds only relative to the old all-ancestors rule.
- The behaviour is old: the old rule hid it too. Only the claim is new.
- Impact is limited:
  - every non-tree record still goes through the change loop or matches the fetched head;
  - so the hidden empty tree's path is always the parent of a path that is already published
    or allowlisted, and no name a hook chose is published.

  This is reasoned, not run end to end.

**Fix:** Reword it, for example: "a name holding `/` can only add to the count compared with
counting every ancestor; neither rule can see an empty tree whose path is also spelled by a
`/`-named sibling".

### IN-704: DEF-09 says the deadline "also covers the changes", which reads as covering the change loop

**File:** `docs/ARCHITECTURE.md:107` (DEF-09, the "What happens" column)
**Issue:**
- "one deadline ... which also covers the changes" means the diff of the two listings
  (`listing_diff`).
- The change loop after it is outside the deadline by the orchestrator's decision of
  2026-11-04. That loop reads, scans and fingerprints each changed blob.
- The closed list nowhere says the loop is outside the deadline. Only 02-27-PLAN.md's "Not
  claimed" section says so.
- The Channel column's "listings and changes take longer than the local time limit" has the
  same ambiguity.

**Fix:** Write "which also covers the diff of the two listings; the blob reads of the changes
keep only their own per-read limit". This is a hand edit to the closed list, so it belongs with
the todo's other owed edits.

---

_Reviewed: 2026-11-05T02:32:39Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: deep_
