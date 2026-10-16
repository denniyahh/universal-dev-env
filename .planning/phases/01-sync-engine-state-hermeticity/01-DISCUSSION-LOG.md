# Phase 1: Sync Engine & State Hermeticity - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-10-16
**Phase:** 01-sync-engine-state-hermeticity
**Areas discussed:** Pull with conflicts, Deletion behavior, Push of merged files
**Not selected (left to Claude):** Timeout limits

Pre-discussion (same session): the scope bar was lowered to the remediation plan's Track 1 tests. The libraries chosen were diffy, process-wrap, std file locking, and tempfile with stage-then-rename. redb, tfio, fs4, the on-disk journal, fsync, and tombstone TTL were rejected or deferred.

---

## Pull with conflicts

| Option | Description | Selected |
|--------|-------------|----------|
| Apply clean, sidecar rest | Clean files update; conflicted files untouched with `.conflict`; exit 1 | ✓ |
| Apply nothing | Write all `.conflict` files, update nothing, exit 1 | |
| You decide | | |

| Option | Description | Selected |
|--------|-------------|----------|
| Delete the .conflict file | Deleting the sidecar marks it resolved | |
| Explicit resolve command | `ude sync resolve <file>` | ✓ |
| You decide | | |

| Option | Description | Selected |
|--------|-------------|----------|
| Skip that file only | Push publishes the rest, lists unresolved, exit 1 | ✓ |
| Block the whole push | Nothing published until all resolved | |
| You decide | | |

| Option | Description | Selected |
|--------|-------------|----------|
| Add *.conflict to gitignore | Never committed by accident | ✓ |
| Leave them visible | Shows in `git status` as a reminder | |
| You decide | | |

**Notes:** The user chose the explicit command over the recommended "delete the sidecar" option.

---

## Deletion behavior

| Option | Description | Selected |
|--------|-------------|----------|
| Delete it and report it | Upstream deleted, local unchanged → delete locally | ✓ |
| Keep it, stop managing it | Leave the file as unmanaged | |
| You decide | | |

| Option | Description | Selected |
|--------|-------------|----------|
| Treat as a conflict | Upstream deleted, local edited → conflict, resolve via `ude sync resolve` | ✓ |
| Keep it, stop managing it | Local edits win automatically | |
| You decide | | |

| Option | Description | Selected |
|--------|-------------|----------|
| Delete upstream automatically | Local deletion propagates on push | ✓ |
| Require a flag | e.g. `--propagate-deletes` | |
| You decide | | |

| Option | Description | Selected |
|--------|-------------|----------|
| Keep it deleted | Pull doesn't resurrect; conflict if upstream changed | ✓ |
| Restore it | Pull re-creates from upstream | |
| You decide | | |

---

## Push of merged files

| Option | Description | Selected |
|--------|-------------|----------|
| Skip it, say "pull first" | Push publishes the rest, exit 1; never rewrites local files | ✓ |
| Merge and publish in one go | Push writes the merge locally and publishes it | |
| You decide | | |

| Option | Description | Selected |
|--------|-------------|----------|
| Abort cleanly, say "pull first" | Upstream moved → publish nothing, undo cache commit, exit 1 | ✓ |
| Re-fetch and retry once | Re-scan and push if nothing conflicts | |
| You decide | | |

---

## Claude's Discretion

- Timeout limits (area not selected): ~30 s local / 60 s network and `gh`, process-group kill, no override.
- Line-ending normalization approach, `.conflict` file content/format, exit codes 1 and 2.

## Deferred Ideas

- Crash/power-loss durability (journal, fsync); redb; tombstone TTL; re-fetch-and-retry; timeout overrides; bounding the `curl` call in `src/semantic/mod.rs` (Phase 2).
