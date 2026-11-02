---
phase: 02
review: 02-REVIEW.md
titles: json
findings:
  - id: CR-300
    severity: critical
    disposition: open
    title: "A ref named after the commit id makes git push send another commit"
  - id: CR-301
    severity: critical
    disposition: open
    title: "A hook can move the fetched head, and the injected commit goes out as the base"
  - id: WR-300
    severity: warning
    disposition: open
    title: "An index flag makes the push return Ok with nothing published"
  - id: WR-301
    severity: warning
    disposition: open
    title: "A hook-written pushurl sends the commit elsewhere and the push returns Ok"
  - id: WR-302
    severity: warning
    disposition: open
    title: "A refusal prints hook-chosen bytes from a `parent` header, including terminal escapes"
  - id: IN-300
    severity: info
    disposition: open
    title: "Some push failure paths leave the cache commit or the sync branch behind"
  - id: IN-301
    severity: info
    disposition: open
    title: "The new e2e helper runs `ude init` with the host's HOME"
  - id: CR-200
    severity: critical
    disposition: open
    title: "A tree listing the managed path twice publishes unvalidated bytes, and consumers check them out"
  - id: CR-201
    severity: critical
    disposition: open
    title: "A hook can delete or revert a validated file and the push still returns Ok"
  - id: WR-200
    severity: warning
    disposition: open
    title: "A tree entry named `.` passes the allowlist and makes the remote unclonable"
  - id: WR-201
    severity: warning
    disposition: open
    title: "`rollback_commit` leaves the cache checkout wrong after a refusal, and no test checks it"
  - id: WR-202
    severity: warning
    disposition: open
    title: "`refuse_grafts_file` passes when the printed path is not one absolute path"
  - id: WR-203
    severity: warning
    disposition: open
    title: "A post-commit hook's background process can write grafts after the gate has passed"
  - id: WR-204
    severity: warning
    disposition: open
    title: "Tests named for inherited `GIT_TEST_*` variables never exercise inheritance"
  - id: IN-200
    severity: info
    disposition: open
    title: "A hook-written `push.pushOption` reaches the remote"
  - id: IN-201
    severity: info
    disposition: open
    title: "The push command and its comment block are written out twice"
  - id: IN-202
    severity: info
    disposition: open
    title: "`validate_committed_tree` is about 200 lines"
  - id: IN-203
    severity: info
    disposition: open
    title: "The `GIT_TEST_` prefix match is case-sensitive, and Windows environment names are not"
  - id: IN-204
    severity: info
    disposition: open
    title: "The `commit_command` doc comment says \"no hooks\""
  - id: IN-205
    severity: info
    disposition: open
    title: "Refusals print hook-chosen path names"
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
    title: "The vendored rule pin is enforced only under `cargo test`, and line-ending conversion would break it"
  - id: WR-101
    severity: warning
    disposition: open
    title: "Nothing checks for the raw home path after parameterization, so every rejected occurrence is published silently"
  - id: WR-102
    severity: warning
    disposition: open
    title: "The post-commit binding gate passes vacuously on a merge HEAD, and PR mode never checks ancestry"
  - id: IN-101
    severity: info
    disposition: open
    title: "A home path that cannot be parameterized is skipped with no diagnostic"
  - id: IN-102
    severity: info
    disposition: open
    title: "Init has two template-render paths with different rules"
  - id: IN-103
    severity: info
    disposition: open
    title: "The vendored rule pin is enforced only under `cargo test`, and line-ending conversion would break it"
open: 32
total: 34
recorded: 2026-11-02T19:31:26.995Z
---

# Phase 02: Code Review Disposition

| Finding | Severity | Disposition | Source |
|---------|----------|-------------|--------|
| CR-300 | critical | open | - |
| CR-301 | critical | open | - |
| WR-300 | warning | open | - |
| WR-301 | warning | open | - |
| WR-302 | warning | open | - |
| IN-300 | info | open | - |
| IN-301 | info | open | - |
| CR-200 | critical | open | - (not in the current review) |
| CR-201 | critical | open | - (not in the current review) |
| WR-200 | warning | open | - (not in the current review) |
| WR-201 | warning | open | - (not in the current review) |
| WR-202 | warning | open | - (not in the current review) |
| WR-203 | warning | open | - (not in the current review) |
| WR-204 | warning | open | - (not in the current review) |
| IN-200 | info | open | - (not in the current review) |
| IN-201 | info | open | - (not in the current review) |
| IN-202 | info | open | - (not in the current review) |
| IN-203 | info | open | - (not in the current review) |
| IN-204 | info | open | - (not in the current review) |
| IN-205 | info | open | - (not in the current review) |
| WR-01 | warning | open | - (not in the current review) |
| WR-02 | warning | open | - (not in the current review) |
| IN-01 | info | open | - (not in the current review) |
| IN-02 | info | open | - (not in the current review) |
| IN-03 | info | open | - (not in the current review) |
| CR-01 | critical | fixed | plan 02-09; confirmed fixed by re-review ed8dc0d (not in the current review) |
| CR-02 | critical | fixed | plan 02-10; confirmed fixed by re-review ed8dc0d (not in the current review) |
| IN-04 | info | open | - (not in the current review) |
| IN-05 | info | open | - (not in the current review) |
| WR-101 | warning | open | WR-01 of first review a32cd36; id reused by re-review ed8dc0d, restored (not in the current review) |
| WR-102 | warning | open | WR-02 of first review a32cd36; id reused by re-review ed8dc0d, restored (not in the current review) |
| IN-101 | info | open | IN-01 of first review a32cd36; id reused by re-review ed8dc0d, restored (not in the current review) |
| IN-102 | info | open | IN-02 of first review a32cd36; id reused by re-review ed8dc0d, restored (not in the current review) |
| IN-103 | info | open | IN-03 of first review a32cd36; id reused by re-review ed8dc0d, restored (not in the current review) |

Dispositions: `open` (recorded, not yet triaged), `fixed`, `skipped`, `deferred`.
Set `deferred` by hand and put the reason in the Source cell; both are preserved. A `|` in the reason is kept as prose and escaped on the next run.
Re-running the gate keeps every row it can. A row the current review no longer reports is kept and its Source cell flagged, so a finding does not leave this record silently. ONE exception: when a finding id is REUSED by a different finding, the earlier decision cannot keep a row — the id is taken — and it is dropped. A RECORDED decision (anything but `open`) is named on the console when that happens; a row still at `open` is replaced silently, because `open` records no decision to lose.
