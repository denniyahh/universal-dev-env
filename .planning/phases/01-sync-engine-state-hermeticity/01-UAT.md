---
status: testing
phase: 01-sync-engine-state-hermeticity
source: [01-VERIFICATION.md]
started: 2026-10-20T01:19:56Z
updated: 2026-10-20T01:19:56Z
---

## Current Test

number: 1
name: Windows — bounded runner kills a hung git (JobObject branch)
expected: |
  On a Windows host or CI runner: `cargo check --target x86_64-pc-windows-msvc` compiles; `ude sync status --remote <unreachable https URL>` returns within about 60 s with the "timed out after 60.0s and was terminated" message, and no git / git-remote-https process is left running.
awaiting: user response

## Tests

### 1. Windows — bounded runner kills a hung git (JobObject branch)
expected: Compiles; the command returns within ~60 s with the timeout message and no git / git-remote-https process is left running (WINDOWS.md #1, T-01-04 / AR-01-01)
result: [pending]

### 2. Windows — remote cache lock released on process death
expected: Two `ude sync` commands on the same remote and branch; killing the first while it holds the lock lets the second proceed after waiting (std File::try_lock / LockFileEx) (WINDOWS.md #2)
result: [pending]

### 3. Windows — state.json replaced over an existing file
expected: `ude sync pull` in an initialized project replaces .ude/state.json (NamedTempFile persist over an existing file) and it reloads as version 2 (WINDOWS.md #3)
result: [pending]

## Summary

total: 3
passed: 0
issues: 0
pending: 3
skipped: 0
blocked: 0

## Gaps
