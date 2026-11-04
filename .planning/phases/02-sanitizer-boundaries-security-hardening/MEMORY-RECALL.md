# Memory Recall (MemPalace)

_Wing: universal-dev-env · Mode: augment · Transport: mcp_

Read two calls only: one targeted search and the KG query for `universal-dev-env`. The palace holds no drawer newer than 2026-11-02 10:33, so nothing here reflects plans 02-13 to 02-15 as executed or the latest re-verification. Those come from the phase directory.

## Prior decisions
- D-18: abort the push cleanly if upstream moved between fetch and push — kg fact, valid_from 2026-10-16
- D-03: subprocesses are bounded through process-wrap; the whole process group or JobObject is killed on timeout — kg fact, valid_from 2026-10-16
- D-10: push skips unresolved conflicts, publishes the rest, exits 1 — kg fact, valid_from 2026-10-16
- D-14 (amended 2026-10-21): push deletes upstream templates on local deletion, except templates containing logic — kg fact, valid_from 2026-10-21

## Patterns
- Hook test helpers from plan 02-11 (`push_with_post_commit_hook`, `HookedPush`, `remote_heads`, the `HOOK_*` tails) are the fixtures later gap plans extend — drawer_universal-dev-env_milestones_d4bd6d1804eeeaf766e2cd6f (02-11-SUMMARY.md)
- Both pushes send `<validated id>:refs/heads/<name>` with `--no-follow-tags` since 02-11. That form is the source of the shadow-ref route now open — same drawer

## Surprises / gotchas
- Plan-time probes for a push-side race used `remote.origin.receivepack` to delay `git receive-pack`, so a background write lands after the push starts and before the object walk — drawer_universal-dev-env_planning_572697d34970003590dae8d7 (02-14-PLAN.md)
