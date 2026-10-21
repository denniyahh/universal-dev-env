---
phase: 01
review: 01-REVIEW.md
titles: json
findings:
  - id: CR-01
    severity: critical
    disposition: fixed
    title: "Init with no templates directory found records the user's files as managed; the next pull deletes them"
  - id: CR-02
    severity: critical
    disposition: fixed
    title: "`sync pull` without a remote deletes every managed file when the template root is wrong or missing (predates the diff)"
  - id: CR-03
    severity: critical
    disposition: fixed
    title: "A monorepo subdirectory whose parent repo git cannot read gets its committed edits overwritten (exit 0)"
  - id: WR-01
    severity: warning
    disposition: fixed
    title: "`--type generic` (or `\"\"`) records a type that sync renders differently; mise.toml reads \"Upstream Updated\" right after init"
  - id: WR-02
    severity: warning
    disposition: fixed
    title: "pre-push now blocks every push when the advisory database cannot be fetched"
  - id: WR-03
    severity: warning
    disposition: fixed
    title: "A failed state save is only a warning (exit 0), which silently drops the new kept-tombstone markers"
  - id: IN-01
    severity: info
    disposition: fixed
    title: "Init hard failures exit 1, which this binary defines as \"drift\""
  - id: IN-02
    severity: info
    disposition: fixed
    title: "Fresh-clone default ignores project detection; `--type` is not validated"
  - id: IN-03
    severity: info
    disposition: fixed
    title: "`--mempalace` and `--graphify` cannot be turned off, so their pre-flight guards never skip"
  - id: IN-04
    severity: info
    disposition: deferred
    title: "\"No shared template\" is judged against the local templates, not the remote that sync uses (unverified)"
  - id: IN-05
    severity: info
    disposition: deferred
    title: "The supply-chain workflow has never run; push and pull_request both trigger it"
  - id: IN-06
    severity: info
    disposition: fixed
    title: "e2e tests that assume \"no git\" now depend on the tempdir not being inside a repository"
  - id: IN-07
    severity: info
    disposition: fixed
    title: "The pre-push hook checks the working tree, not the commits being pushed"
  - id: WR-04
    severity: warning
    disposition: fixed
    title: "The TypeSafe drift check reads `.agents/AGENTS.md` through symlinks and sends it to the external API before push's symlink refusal runs"
open: 0
total: 14
recorded: 2026-10-21T19:57:44.693Z
---

# Phase 01: Code Review Disposition

| Finding | Severity | Disposition | Source |
|---------|----------|-------------|--------|
| CR-01 | critical | fixed | 464bfc4: init exits 2 when no ude templates directory is found; verified with a templates-hidden copy build (control: same build with templates inits) |
| CR-02 | critical | fixed | 464bfc4: a templates dir counts only with projects/generic; sync without a remote exits 2 when none is found; e2e test_sync_ignores_a_project_templates_dir_that_is_not_ude |
| CR-03 | critical | fixed | 3249d70: a parent repo git recognises but cannot read stops init; e2e test_init_fails_when_parent_repo_index_is_unreadable (control: empty parent .git still ignored); dubious ownership untested (needs a second uid) |
| WR-01 | warning | fixed | db2e1a8: init records the rendered type; e2e test_init_records_the_detected_type_for_generic |
| WR-02 | warning | fixed | b18ffc9 (operator chose cache fallback 2026-10-21): advisories re-run with --offline when only the fetch fails, with a stale-cache warning; verified online 0, offline+cache 0, offline+no cache 1 |
| WR-03 | warning | fixed | 6b0a2d3: a failed state save exits 2; e2e test_init_fails_when_state_cannot_be_saved |
| IN-01 | info | fixed | 7e44cf1: every init failure exits 2 |
| IN-02 | info | fixed | 8b41fcb: --type validated (rust, node, polyglot, generic); the rust fallback for a fresh clone is the documented default and is unchanged |
| IN-03 | info | fixed | 8b41fcb: --mempalace=false / --graphify=false work; a bare flag still means true |
| IN-04 | info | deferred | unverified; init vs remote template root belongs with .ude.toml / central_repo work (Phase 4) |
| IN-05 | info | deferred | CI cannot run until the branch is pushed; operator said push nothing (2026-10-20) |
| IN-06 | info | fixed | 9bd2777: e2e helper sets GIT_CEILING_DIRECTORIES; verified with TMPDIR inside the checkout (2 failures before, 64/64 after) |
| IN-07 | info | fixed | d7d0848: documented in the hook comment |
| WR-04 | warning | fixed | operator-approved fix (2026-10-20), 374a208 (not in the current review) |

Dispositions: `open` (recorded, not yet triaged), `fixed`, `skipped`, `deferred`.
Set `deferred` by hand and put the reason in the Source cell; both are preserved. A `|` in the reason is kept as prose and escaped on the next run.
Re-running the gate keeps every row it can. A row the current review no longer reports is kept and its Source cell flagged, so a finding does not leave this record silently. ONE exception: when a finding id is REUSED by a different finding, the earlier decision cannot keep a row — the id is taken — and it is dropped. A RECORDED decision (anything but `open`) is named on the console when that happens; a row still at `open` is replaced silently, because `open` records no decision to lose.
