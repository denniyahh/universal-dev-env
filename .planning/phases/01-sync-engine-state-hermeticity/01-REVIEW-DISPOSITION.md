---
phase: 01
review: 01-REVIEW.md
titles: json
findings:
  - id: CR-01
    severity: critical
    disposition: fixed
    title: "`ude init` writes through symlinked managed paths, overwriting files outside the project"
  - id: CR-02
    severity: critical
    disposition: fixed
    title: "Init replaces an unreadable or non-UTF-8 `.gitignore` with only the graphify block (pre-existing)"
  - id: WR-01
    severity: warning
    disposition: deferred
    title: "One symlinked managed file disables every sync command, and init keeps a committed symlink as if sync could manage it"
  - id: WR-02
    severity: warning
    disposition: fixed
    title: "The fresh-clone keep is skipped for projects in a git subdirectory (monorepo), so init overwrites committed files"
  - id: WR-03
    severity: warning
    disposition: fixed
    title: "A plain re-run of `ude init` silently overwrites the recorded project type with the CLI default `rust`; the next pull rewrites `mise.toml`"
  - id: WR-04
    severity: warning
    disposition: fixed
    title: "The TypeSafe drift check reads `.agents/AGENTS.md` through symlinks and sends it to the external API before push's symlink refusal runs"
  - id: IN-01
    severity: info
    disposition: open
    title: "A local deletion of a templated file deletes the shared `.jinja` template; the CR-02 gate covers edits only"
  - id: IN-02
    severity: info
    disposition: open
    title: "The managed-file list is still duplicated, and init's copy now drives keep, tracked and missing decisions"
  - id: IN-03
    severity: info
    disposition: open
    title: "When init's scan fails, the missing-file warning falls back to the \"push will delete\" prediction"
  - id: IN-04
    severity: info
    disposition: open
    title: "Symlink refusal is not applied uniformly on read paths that feed decisions"
  - id: IN-05
    severity: info
    disposition: open
    title: "Supply-chain tooling pins versions but not artifacts, and `install-hooks` silently shadows any global hooks path"
  - id: CR-03
    severity: critical
    disposition: fixed
    title: "`ude init` keeps a custom `.gitignore` but records it as the ancestor, so the first pull silently overwrites it (pre-existing)"
  - id: IN-06
    severity: info
    disposition: open
    title: "`ude sync resolve ./.gitconfig` is refused as an \"unsafe path\""
  - id: IN-07
    severity: info
    disposition: open
    title: "Test hermeticity gaps (pre-existing pattern, now on new tests)"
open: 7
total: 14
recorded: 2026-10-20T16:38:47.095Z
---

# Phase 01: Code Review Disposition

| Finding | Severity | Disposition | Source |
|---------|----------|-------------|--------|
| CR-01 | critical | fixed | operator-approved fix ("fix now", 2026-10-20), bc0cce5 |
| CR-02 | critical | fixed | operator-approved fix ("fix now", 2026-10-20), bc0cce5 |
| WR-01 | warning | deferred | operator chose to log it (2026-10-20); todo 5b2841e, needs a per-file refused status |
| WR-02 | warning | fixed | operator-approved fix (2026-10-20), 8e7c6dd |
| WR-03 | warning | fixed | operator-approved fix (2026-10-20), 6d6b652 |
| WR-04 | warning | fixed | operator-approved fix (2026-10-20), 374a208 |
| IN-01 | info | open | - |
| IN-02 | info | open | - |
| IN-03 | info | open | - |
| IN-04 | info | open | - |
| IN-05 | info | open | - |
| CR-03 | critical | fixed | operator-approved fix, 52bc341, 984f0c7 (not in the current review) |
| IN-06 | info | open | - (not in the current review) |
| IN-07 | info | open | - (not in the current review) |

Dispositions: `open` (recorded, not yet triaged), `fixed`, `skipped`, `deferred`.
Set `deferred` by hand and put the reason in the Source cell; both are preserved. A `|` in the reason is kept as prose and escaped on the next run.
Re-running the gate keeps every row it can. A row the current review no longer reports is kept and its Source cell flagged, so a finding does not leave this record silently. ONE exception: when a finding id is REUSED by a different finding, the earlier decision cannot keep a row — the id is taken — and it is dropped. A RECORDED decision (anything but `open`) is named on the console when that happens; a row still at `open` is replaced silently, because `open` records no decision to lose.
