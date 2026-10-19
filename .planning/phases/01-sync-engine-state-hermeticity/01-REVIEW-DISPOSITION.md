---
phase: 01
review: 01-REVIEW.md
titles: json
findings:
  - id: CR-01
    severity: critical
    disposition: fixed
    title: "A locally deleted boundary file blocks every push and the tool offers no way out (phase-introduced)"
  - id: CR-02
    severity: critical
    disposition: fixed
    title: "Push replaces `.jinja` templates with rendered output, erasing template logic for every consumer (pre-existing)"
  - id: CR-03
    severity: critical
    disposition: fixed
    title: "`ude init` keeps a custom `.gitignore` but records it as the ancestor, so the first pull silently overwrites it (pre-existing)"
  - id: WR-01
    severity: warning
    disposition: fixed
    title: "`test_remote_lock_not_inherited_by_children` is flaky (it races the child's exec and loader)"
  - id: IN-01
    severity: info
    disposition: open
    title: "A successful git command is reported as a hard failure when an escaped descendant holds the pipes"
  - id: IN-02
    severity: info
    disposition: open
    title: "Remote direct push exits 0 when the state save fails after publishing"
  - id: IN-03
    severity: info
    disposition: open
    title: "A v1 hash-only record whose file was edited migrates into a false conflict for CRLF templates"
  - id: IN-04
    severity: info
    disposition: open
    title: "The status scan falls back to the raw template when rendering fails, while pull aborts entirely (pre-existing)"
  - id: IN-05
    severity: info
    disposition: open
    title: "The managed-file list is duplicated in init"
  - id: IN-06
    severity: info
    disposition: open
    title: "`ude sync resolve ./.gitconfig` is refused as an \"unsafe path\""
  - id: IN-07
    severity: info
    disposition: open
    title: "Test hermeticity gaps (pre-existing pattern, now on new tests)"
open: 7
total: 11
recorded: 2026-10-19T17:53:06.283Z
---

# Phase 01: Code Review Disposition

| Finding | Severity | Disposition | Source |
|---------|----------|-------------|--------|
| CR-01 | critical | fixed | operator-approved fix, 98261e6, 9da142c |
| CR-02 | critical | fixed | operator-approved fix, 67b1c45, 3c9e79a |
| CR-03 | critical | fixed | operator-approved fix, 52bc341, 984f0c7 |
| WR-01 | warning | fixed | operator-approved fix, ee296c4 |
| IN-01 | info | open | - |
| IN-02 | info | open | - |
| IN-03 | info | open | - |
| IN-04 | info | open | - |
| IN-05 | info | open | - |
| IN-06 | info | open | - |
| IN-07 | info | open | - |

Dispositions: `open` (recorded, not yet triaged), `fixed`, `skipped`, `deferred`.
Set `deferred` by hand and put the reason in the Source cell; both are preserved. A `|` in the reason is kept as prose and escaped on the next run.
Re-running the gate keeps every row it can. A row the current review no longer reports is kept and its Source cell flagged, so a finding does not leave this record silently. ONE exception: when a finding id is REUSED by a different finding, the earlier decision cannot keep a row — the id is taken — and it is dropped. A RECORDED decision (anything but `open`) is named on the console when that happens; a row still at `open` is replaced silently, because `open` records no decision to lose.
