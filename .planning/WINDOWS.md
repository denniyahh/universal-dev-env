---
schema_version: 1
open_count: 3
waived_count: 0
fixed_count: 0
total_count: 3
last_updated: 2026-10-18T19:23:59.347Z
---

# Broken Windows Ledger

> Cross-phase defect register. With `workflow.windows_enforce` enabled, `/gsd-ship` blocks while `open_count > 0`.
> Waive with `gsd-tools windows waive <id> "<reason>"` (reason required).
> Mark fixed with `gsd-tools windows fixed <id>`.

| id | phase | kind | file | line | description | status | reason | recorded_at | resolved_at |
|----|-------|------|------|------|-------------|--------|--------|-------------|-------------|
| 1 | 01 | unrun-verify | src/process.rs |  | 01-01 Task 2 human-check not run: Windows JobObject branch of run_bounded never compiled or executed (no Windows target/host); needs cargo check --target x86_64-pc-windows-msvc and a timeout run on Windows | open |  | 2026-10-18T17:33:35.943Z |  |
| 2 | 01 | unrun-verify | src/sync/mod.rs |  | 01-03: RemoteLockGuard std File::try_lock path (and the removed non-Unix no-op branch) never compiled or run on Windows; lock tests are cfg(target_os = linux) or ran only on Linux | open |  | 2026-10-18T19:12:45.428Z |  |
| 3 | 01 | unrun-verify | src/sync/state.rs |  | Windows: staged state.json write (NamedTempFile persist over an existing file, no mode handling) never compiled or run; test_state_save_mode and test_state_save_refuses_symlinked_ude_dir are Unix-only | open |  | 2026-10-18T19:23:59.347Z |  |

````json
[
  {
    "id": 1,
    "kind": "unrun-verify",
    "phase": "01",
    "file": "src/process.rs",
    "line": null,
    "description": "01-01 Task 2 human-check not run: Windows JobObject branch of run_bounded never compiled or executed (no Windows target/host); needs cargo check --target x86_64-pc-windows-msvc and a timeout run on Windows",
    "status": "open",
    "reason": "",
    "recorded_at": "2026-10-18T17:33:35.943Z",
    "resolved_at": null,
    "milestone": null
  },
  {
    "id": 2,
    "kind": "unrun-verify",
    "phase": "01",
    "file": "src/sync/mod.rs",
    "line": null,
    "description": "01-03: RemoteLockGuard std File::try_lock path (and the removed non-Unix no-op branch) never compiled or run on Windows; lock tests are cfg(target_os = linux) or ran only on Linux",
    "status": "open",
    "reason": "",
    "recorded_at": "2026-10-18T19:12:45.428Z",
    "resolved_at": null,
    "milestone": null
  },
  {
    "id": 3,
    "kind": "unrun-verify",
    "phase": "01",
    "file": "src/sync/state.rs",
    "line": null,
    "description": "Windows: staged state.json write (NamedTempFile persist over an existing file, no mode handling) never compiled or run; test_state_save_mode and test_state_save_refuses_symlinked_ude_dir are Unix-only",
    "status": "open",
    "reason": "",
    "recorded_at": "2026-10-18T19:23:59.347Z",
    "resolved_at": null,
    "milestone": null
  }
]
````
