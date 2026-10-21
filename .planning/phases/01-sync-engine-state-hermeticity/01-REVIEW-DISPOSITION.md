---
phase: 01
review: 01-REVIEW.md
titles: json
findings:
  - id: CR-01
    severity: critical
    disposition: open
    title: "Init with no templates directory found records the user's files as managed; the next pull deletes them"
  - id: CR-02
    severity: critical
    disposition: open
    title: "`sync pull` without a remote deletes every managed file when the template root is wrong or missing (predates the diff)"
  - id: CR-03
    severity: critical
    disposition: open
    title: "A monorepo subdirectory whose parent repo git cannot read gets its committed edits overwritten (exit 0)"
  - id: WR-01
    severity: warning
    disposition: open
    title: "`--type generic` (or `\"\"`) records a type that sync renders differently; mise.toml reads \"Upstream Updated\" right after init"
  - id: WR-02
    severity: warning
    disposition: open
    title: "pre-push now blocks every push when the advisory database cannot be fetched"
  - id: WR-03
    severity: warning
    disposition: open
    title: "A failed state save is only a warning (exit 0), which silently drops the new kept-tombstone markers"
  - id: IN-01
    severity: info
    disposition: open
    title: "Init hard failures exit 1, which this binary defines as \"drift\""
  - id: IN-02
    severity: info
    disposition: open
    title: "Fresh-clone default ignores project detection; `--type` is not validated"
  - id: IN-03
    severity: info
    disposition: open
    title: "`--mempalace` and `--graphify` cannot be turned off, so their pre-flight guards never skip"
  - id: IN-04
    severity: info
    disposition: open
    title: "\"No shared template\" is judged against the local templates, not the remote that sync uses (unverified)"
  - id: IN-05
    severity: info
    disposition: open
    title: "The supply-chain workflow has never run; push and pull_request both trigger it"
  - id: IN-06
    severity: info
    disposition: open
    title: "e2e tests that assume \"no git\" now depend on the tempdir not being inside a repository"
  - id: IN-07
    severity: info
    disposition: open
    title: "The pre-push hook checks the working tree, not the commits being pushed"
  - id: WR-04
    severity: warning
    disposition: fixed
    title: "The TypeSafe drift check reads `.agents/AGENTS.md` through symlinks and sends it to the external API before push's symlink refusal runs"
open: 13
total: 14
recorded: 2026-10-21T18:00:17.811Z
---

# Phase 01: Code Review Disposition

| Finding | Severity | Disposition | Source |
|---------|----------|-------------|--------|
| CR-01 | critical | open | - |
| CR-02 | critical | open | - |
| CR-03 | critical | open | - |
| WR-01 | warning | open | - |
| WR-02 | warning | open | - |
| WR-03 | warning | open | - |
| IN-01 | info | open | - |
| IN-02 | info | open | - |
| IN-03 | info | open | - |
| IN-04 | info | open | - |
| IN-05 | info | open | - |
| IN-06 | info | open | - |
| IN-07 | info | open | - |
| WR-04 | warning | fixed | operator-approved fix (2026-10-20), 374a208 (not in the current review) |

Dispositions: `open` (recorded, not yet triaged), `fixed`, `skipped`, `deferred`.
Set `deferred` by hand and put the reason in the Source cell; both are preserved. A `|` in the reason is kept as prose and escaped on the next run.
Re-running the gate keeps every row it can. A row the current review no longer reports is kept and its Source cell flagged, so a finding does not leave this record silently. ONE exception: when a finding id is REUSED by a different finding, the earlier decision cannot keep a row — the id is taken — and it is dropped. A RECORDED decision (anything but `open`) is named on the console when that happens; a row still at `open` is replaced silently, because `open` records no decision to lose.
