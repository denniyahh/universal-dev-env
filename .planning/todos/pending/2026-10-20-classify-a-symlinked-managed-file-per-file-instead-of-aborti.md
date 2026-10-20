---
created: 2026-10-20T18:49:48.869Z
title: Classify a symlinked managed file per file instead of aborting sync
area: general
severity: major
files:
  - src/sync/mod.rs:600
  - src/main.rs:217
---

## Problem

`scan_project` passes the symlink refusal up with `?` (`read_if_present(local_root, ..)?` at src/sync/mod.rs:600, through `refuse_symlink_under`). As a result, one linked managed file makes `ude sync status`, pull and push all exit 2, for all 11 managed files, and ude offers no per-file way out. A committed `.agents/AGENTS.md -> ../CLAUDE.md` is a common layout that triggers this.

Init also counts a committed symlink as kept: `kept_committed` at src/main.rs:217 accepts anything `symlink_metadata` can read. It prints "Kept committed .agents/AGENTS.md (not overwritten); sync status compares it with the template", but afterwards `sync status` never runs to completion.

Source: the phase 01 code review, finding WR-01; the reviewer reproduced it. On 2026-10-20 the operator chose to log it rather than fix it in phase 01, because it needs a new status design. Other changes in phase 01:
- bc0cce5 makes init refuse a symlinked *destination* it would write.
- Paths init keeps or that sync already tracks are exempt from that refusal, so this behavior is unchanged.

## Solution

Test-first:
- Keep refusing to follow the link. Classify it per file instead of aborting the whole scan, for example with `SyncStatus::Refused { reason }`.
- Status prints that classification. Push and pull skip the file and exit 1, the D-10 pattern for partial results.
- In init, exclude symlinks from `kept_committed`. Warn that sync will not manage that path.

Tests:
- A committed `AGENTS.md -> ../CLAUDE.md`: status lists the other 10 files and marks AGENTS.md as refused. Push and pull exit 1, and the link target is neither read nor published.
- Negative control: the same project with a regular file exits 0.
