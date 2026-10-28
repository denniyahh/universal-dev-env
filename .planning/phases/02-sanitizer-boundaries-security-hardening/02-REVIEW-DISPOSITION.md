---
phase: 02
review: 02-REVIEW.md
titles: json
findings:
  - id: WR-01
    severity: warning
    disposition: open
    title: "`ude sync resolve` promises a publish or deletion that gate 2 now refuses for static templates"
  - id: WR-02
    severity: warning
    disposition: open
    title: "The unit test for a placeholder-only static template passes on a push that erases the placeholder"
  - id: IN-01
    severity: info
    disposition: open
    title: "Two roots are still not single-spelled"
  - id: IN-02
    severity: info
    disposition: open
    title: "`template_has_logic` reads templates differently from `read_if_present`"
  - id: IN-03
    severity: info
    disposition: open
    title: "Upgrading re-renders every placeholder file for users whose HOME or XDG root ended in a separator"
  - id: CR-01
    severity: critical
    disposition: fixed
    title: "If HOME ends in `/`, parameterization does nothing and the raw home path is published"
  - id: CR-02
    severity: critical
    disposition: fixed
    title: "Gate 2 only treats `.jinja` templates as having logic, so push can overwrite logic in other rendered templates and publish the pusher's values"
  - id: IN-04
    severity: info
    disposition: open
    title: "ARCHITECTURE.md overstates what the gate guarantees"
  - id: IN-05
    severity: info
    disposition: open
    title: "The TypeSafe drift check sends unparameterized AGENTS.md text to an external API (pre-existing, outside the phase diff)"
  - id: R1-WR-01
    severity: warning
    disposition: open
    title: "Nothing checks for the raw home path after parameterization, so every rejected occurrence is published silently"
  - id: R1-WR-02
    severity: warning
    disposition: open
    title: "The post-commit binding gate passes vacuously on a merge HEAD, and PR mode never checks ancestry"
  - id: R1-IN-01
    severity: info
    disposition: open
    title: "A home path that cannot be parameterized is skipped with no diagnostic"
  - id: R1-IN-02
    severity: info
    disposition: open
    title: "Init has two template-render paths with different rules"
  - id: R1-IN-03
    severity: info
    disposition: open
    title: "The vendored rule pin is enforced only under `cargo test`, and line-ending conversion would break it"
open: 12
total: 14
recorded: 2026-10-28T00:42:47.564Z
---

# Phase 02: Code Review Disposition

| Finding | Severity | Disposition | Source |
|---------|----------|-------------|--------|
| WR-01 | warning | open | - |
| WR-02 | warning | open | - |
| IN-01 | info | open | - |
| IN-02 | info | open | - |
| IN-03 | info | open | - |
| CR-01 | critical | fixed | plan 02-09; confirmed fixed by re-review ed8dc0d |
| CR-02 | critical | fixed | plan 02-10; confirmed fixed by re-review ed8dc0d |
| IN-04 | info | open | - (not in the current review) |
| IN-05 | info | open | - (not in the current review) |
| R1-WR-01 | warning | open | first review a32cd36 (id reused by ed8dc0d review; restored) |
| R1-WR-02 | warning | open | first review a32cd36 (id reused by ed8dc0d review; restored) |
| R1-IN-01 | info | open | first review a32cd36 (id reused by ed8dc0d review; restored) |
| R1-IN-02 | info | open | first review a32cd36 (id reused by ed8dc0d review; restored) |
| R1-IN-03 | info | open | first review a32cd36 (id reused by ed8dc0d review; restored) |

Dispositions: `open` (recorded, not yet triaged), `fixed`, `skipped`, `deferred`.
Set `deferred` by hand and put the reason in the Source cell; both are preserved. A `|` in the reason is kept as prose and escaped on the next run.
Re-running the gate keeps every row it can. A row the current review no longer reports is kept and its Source cell flagged, so a finding does not leave this record silently. ONE exception: when a finding id is REUSED by a different finding, the earlier decision cannot keep a row — the id is taken — and it is dropped. A RECORDED decision (anything but `open`) is named on the console when that happens; a row still at `open` is replaced silently, because `open` records no decision to lose.
