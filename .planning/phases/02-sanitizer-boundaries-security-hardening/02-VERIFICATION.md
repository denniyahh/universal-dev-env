---
phase: 02-sanitizer-boundaries-security-hardening
verified: 2026-11-01T16:28:16Z
status: gaps_found
score: 38/44 must-haves verified
covered_files:
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-01-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-01-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-02-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-02-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-03-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-03-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-04-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-04-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-05-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-05-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-06-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-06-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-07-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-07-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-08-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-08-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-09-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-09-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-10-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-10-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-11-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-11-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-12-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-12-SUMMARY.md"
  - "docs/ARCHITECTURE.md"
  - "src/main.rs"
  - "src/process.rs"
  - "src/sanitizer/entropy.rs"
  - "src/sanitizer/gitleaks.rs"
  - "src/sanitizer/mod.rs"
  - "src/sanitizer/rules/LICENSE-gitleaks"
  - "src/sanitizer/rules/gitleaks.toml"
  - "src/sync/mod.rs"
  - "src/template/mod.rs"
  - "tests/e2e_lifecycle.rs"
covered_digest: "v4:sha256:0b4b6a102dfac5ee1cccb46156ca502113c6e85f03c3cb88b17dce6ad64ba554"
behavior_unverified: 0
overrides_applied: 0
re_verification:
  previous_status: gaps_found
  previous_score: 32/33
  gaps_closed:
    - "The merge-HEAD and PR-mode stacked-commit channels of the 02-08 prohibition (review WR-102): the hook from the previous report is refused in both modes"
  gaps_remaining:
    - "02-08 prohibition (SEC-04): MUST NOT publish a blob whose content differs, beyond CRLF/LF line endings of UTF-8 text, from the bytes push_updates validated, whatever git clean filter or commit hook produced it. It still fails, through two other channels: a duplicate tree entry and a grafts file written after the gate by a process a hook left running"
  regressions: []
gaps:
  - truth: "02-08 prohibition, restated in full by 02-12 (SEC-04): MUST NOT publish a blob whose content differs, beyond CRLF/LF line endings of UTF-8 text, from the bytes push_updates validated, whatever git clean filter or commit hook produced it. Also fails the 02-11 prohibition on a commit reachable from the published ref, and the 02-11 truth that every blob the push adds is allowlist-checked, secret-scanned and compared"
    status: failed
    reason: "Review CR-200, reproduced through SyncEngine::push_to_remote in direct and PR mode and through the CLI. A post-commit hook rebuilds UDE's commit, keeping its parent and message bytes, with the managed file listed twice in one tree: the validated blob first, an injected blob second. diff-tree lists both, both paths pass the allowlist, and `git show <commit>:<path>` returns the first entry both times, so the second blob is never scanned or compared. push_to_remote returns Ok, the bare remote holds the injected blob, and a fresh ensure_remote_repo and a consumer's `ude sync pull` both get the injected bytes. Controls: a benign hook and the same rebuild with one validated entry publish with no injected blob on the remote; the same rebuild with one injected entry is refused with `differs from the validated content`. So the duplicate entry is the channel. A bare remote with receive.fsckObjects=true refused this tree in a plain git push."
    artifacts:
      - path: "src/sync/mod.rs"
        issue: "validate_committed_tree reads each listed blob by path (line 2369), which returns the first of two entries with the same name; count_empty_directories (line 2435) checks only for empty directories, so nothing refuses a repeated path"
    missing:
      - "Refuse a tree whose raw `ls-tree -r -t -z` listing repeats a path, including a repeated directory name (a duplicate directory holding injected bytes also passed), never printing the name"
      - "Read each listed blob by its own object id (`diff-tree -r -z --raw`, then `cat-file blob <oid>`) so a path lookup cannot hide an entry"
      - "Hook tests in both modes asserting the refusal and that the remote does not hold the injected blob, with a control showing a push without the check delivers it, and a clean-push control"
  - truth: "ROADMAP SC2: Outbound commit inspection catches all Gitleaks secret categories across committed blobs and auto-rolls back commits on detection"
    status: failed
    reason: "Same root cause as the gap above. The second entry of a duplicate tree held a GitHub PAT-shaped value built at runtime; push_to_remote returned Ok in direct and PR mode with no rollback, the remote held the blob, and a fresh ensure_remote_repo left that value in the managed file. SC2 holds for every commit UDE builds and for every other hook shape tested; the suite's gate tests all pass. It does not hold for a committed blob behind a duplicate tree entry."
    artifacts:
      - path: "src/sync/mod.rs"
        issue: "scan_bytes runs on the bytes `git show` returned by path (lines 2369-2389), so the second entry is never scanned"
    missing:
      - "Closed by the same change as the gap above; add a hook test with a token-shaped second entry"
  - truth: "02-08 prohibition, restated in full by 02-12 (SEC-04), whose one accepted exception (T-02-52) names only a pre-push hook: it also covers a grafts file written after the gate by a process a post-commit hook left running"
    status: failed
    reason: "Review WR-203, reproduced through push_to_remote. A post-commit hook leaves a background process that waits for UDE's `git push` to start, then writes `.git/info/grafts`. 60 runs (30 direct, 30 PR): all 60 returned Ok; the remote held the injected blob in 5 (2 direct, 3 PR), with the ref on the validated commit. Controls: the same background process without the grafts write delivered it in 0 of 20; a grafts file written inside the hook was refused 3 of 3. The window between the gate's check (line 2266) and push's object walk is unprotected, and the exception's wording does not reach this case."
    artifacts:
      - path: "src/sync/mod.rs"
        issue: "refuse_grafts_file runs once before the push; both push commands run without GIT_GRAFT_FILE set to the null device"
      - path: "src/process.rs"
        issue: "prepare_command removes GIT_GRAFT_FILE, so the push cannot set it without a carve-out"
    missing:
      - "Either run both pushes with GIT_GRAFT_FILE set to the null device (needs a carve-out in process::prepare_command; a plain-git check here, with a grafts file present at push time, sent the injected blob without the variable and did not send it with the variable; that check does not cover the race), with a hook test"
      - "Or an operator decision extending the T-02-52 acceptance to any process a hook started, recorded in plan and SECURITY docs"
  - truth: "A push that returns Ok has published the validated content (validate_committed_tree's documented contract: every changed file is identical to what `validated` recorded). Derived in this re-verification from review CR-201; distinct from the blob prohibition, because no unvalidated bytes are sent"
    status: partial
    reason: "Review CR-201, reproduced through push_to_remote in direct and PR mode (delete and revert, 4 of 4 Ok) and through the CLI in direct mode. Delete: `git rm --cached` plus amend; remote loses the template, CLI prints the push as done, exit 0, ancestor advances. With another managed template on the remote, the pusher's and a consumer's next `ude sync pull` both remove mise.toml; with no other managed template, every later sync fails with `not a ude template repository`. Revert: `git checkout HEAD~1 -- <file>` plus amend; remote keeps the old bytes, CLI prints the push as done, exit 0, ancestor advances, status then says Upstream Updated, and the pusher's next pull discards the pusher's own edit. Control: the same amend with content kept publishes the validated bytes."
    artifacts:
      - path: "src/sync/mod.rs"
        issue: "the D entry is skipped for every allowlisted path (lines 2365-2367), including one `validated` holds a digest for; nothing checks that validated paths hold their recorded content in the commit, and a revert leaves nothing in the listing to check"
    missing:
      - "Refuse a deletion of a path that has a validated record (real deletions have no record), and after the loop require every validated path to hold its recorded content in the commit"
      - "Hook tests for delete and revert in both modes, with the no-op amend as control"
  - truth: "A hook cannot add an unbound name to the published tree. T-02-46 in plan 02-12 claims other tree names are bound because a new name always contains a blob, which the allowlist refuses. Derived in this re-verification from review WR-200"
    status: partial
    reason: "Review WR-200, reproduced through push_to_remote in direct and PR mode: a subtree named `.` holding the validated blob returns Ok, because PathBuf equality ignores the `.` component. `git clone` of the remote and a fresh ensure_remote_repo then fail with exit 128, `invalid path`, so one hook breaks later clones of the shared repository. No unvalidated bytes: the same subtree holding the injected blob is refused, and a subtree named `sub` is refused as unvalidated."
    artifacts:
      - path: "src/sync/mod.rs"
        issue: "`paths.contains(&p)` (line 2358) compares PathBuf values built at line 2456, which treat `a/./b` as `a/b`"
    missing:
      - "Compare listed paths as raw bytes, or refuse any path component that is empty, `.` or `..`, in the same listing pass as the duplicate check; add a hook test"
---

# Phase 2: Sanitizer, Boundaries & Security Hardening Verification Report

**Phase Goal:** Screen and redact all confidential tokens, raw credentials, and user-specific paths before any remote git staging or push.
**Verified:** 2026-11-01T16:28:16Z
**Status:** gaps_found
**Re-verification:** Yes, after gap closure (plans 02-11, 02-12)

## Bottom line

The prohibition plan 02-12 records as `status: resolved` does not hold. A post-commit hook can still
publish bytes nothing validated, and a token shape nothing scanned, and `push_to_remote` returns Ok in
direct and PR mode. Through the real CLI a consumer's `ude sync pull` then writes the injected bytes
into a project.

Every test the plans wrote passes, and they are not wrong: they cover the channels they name, and
this report re-ran their controls and mutation run, which also pass. The holes are in channels no test
exercises. A clean 49 of 49 mutation score sits next to a reproduced bypass, so it says nothing about
those channels.

What the plans did close: the merge-HEAD and stacked-commit hook from the previous report, replace
refs, tag following, hook-written commit text, forged commit-graphs and bitmaps, and a grafts file
present when the gate runs.

All of this needs a hook in the cache checkout or a global `core.hooksPath`, which runs with the
user's privileges. The operator accepted one such case on that reasoning (T-02-52), and did not accept
these.

## Goal Achievement

### Focus items

| Item | Reproduced? | Outcome for the prohibition | What was observed |
|------|-------------|-----------------------------|-------------------|
| Review CR-200, duplicate tree entry | Yes, direct and PR, engine and CLI | **FAILED** | Ok; remote `ls-tree -r` lists both blobs under one path; injected blob on remote; `git show` returns the validated bytes; fresh `ensure_remote_repo` file = `INJECTED_BY_HOOK`; consumer pull writes it to `mise.toml` |
| Review CR-201, delete or revert | Yes, 4 of 4 through the engine, plus CLI | Blob prohibition's letter holds: no unvalidated bytes are sent. Separate gap, partial | Ok and a success message while the validated file is absent or old; ancestor advances; later pull removes the file or discards the pusher's edit |
| Review WR-200, entry named `.` | Yes, direct and PR | Blob prohibition's letter holds: bytes are the validated ones. Separate gap, partial | Ok; `git clone` exit 128 `invalid path`; variant with injected blob and control named `sub` both refused |
| Review WR-203, grafts after the gate | Yes, 5 of 60 | **FAILED**: the exception names only a pre-push hook | All 60 Ok; injected blob on the remote in 2 of 30 direct and 3 of 30 PR; background process with no grafts write: 0 of 20; synchronous grafts: refused 3 of 3 |

### Roadmap Success Criteria

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| SC1 | High-entropy strings and raw Base64 tokens without prefixes flagged before staging | VERIFIED | Regression: full `cargo test` exit 0. No file under src/sanitizer has changed since the review base ed8dc0d (`git diff --stat ed8dc0d HEAD -- src tests docs` lists only src/sync/mod.rs). The previous independent CLI probe was not re-run |
| SC2 | Outbound commit inspection catches gitleaks categories across committed blobs and rolls back | **FAILED** (was VERIFIED with a caveat) | A PAT-shaped second entry of a duplicate tree: Ok in both modes, no rollback, value on the remote. Holds for commits UDE builds and every other hook shape tested. See gap 2 |
| SC3 | Path parameterization matches only full word and punctuation boundaries | VERIFIED | Regression: full suite; `src/sanitizer/mod.rs` unchanged since ed8dc0d |
| SC4 | Templated tokens substitute back cleanly on initialization | VERIFIED | Regression: full suite; template code unchanged since ed8dc0d; CLI control with no hook: push, then a consumer's pull gets the pusher's edit |

### Plan must-have groups

| Plan | Truths (summary) | Status | Evidence |
|------|------------------|--------|----------|
| 02-01..02-10 | As in the previous report | VERIFIED | Regression: the full suite passes and their code outside src/sync/mod.rs is unchanged since ed8dc0d |
| 02-11 | Bound to the fetched head in both modes; raw reads; validated id pushed; no tags; continuity helper gone | **FAILED** on one clause | Code read at src/sync/mod.rs:2276-2300, 2328-2338, 1995, 2109, and named tests pass. The clause "every blob the push adds to the remote is allowlist-checked, secret-scanned and compared" is false under a duplicate entry |
| 02-12 | Message bound; header scanned; empty directories refused; raw reads; grafts refused at the gate; graphs and bitmaps disabled; GIT_TEST_* removed; gc'd remote publishes; controls and mutation | VERIFIED as written | Code read at src/sync/mod.rs:2266, 2302-2326, 2412-2444, 1997-2007, 2111-2122; named tests pass. Controls re-run here: all 8 removals failed exactly the expected tests, the 2 unmodified rows failed none, sha256 restored. Mutation re-run here: 49 caught, 0 missed, 0 timeouts, exit 0. WR-200 and WR-203 sit in what the truths' wording leaves open; they are carried by gaps 3 and 5 |

### Prohibitions

| Plan | Prohibition | Status | Evidence |
|------|-------------|--------|----------|
| 02-01..02-10, except the row below (18) | As in the previous report | VERIFIED | Regression: full suite |
| 02-08, restated by 02-12 | Never publish a blob differing from the validated bytes, whatever hook produced it, except T-02-52 | **FAILED** (carried forward) | CR-200 and WR-203 |
| 02-11 | Never publish a commit reachable from the published ref carrying a differing blob | **FAILED** | CR-200: the commit is the published ref's tip |
| 02-11 | Never push a commit other than the validated one | VERIFIED | test_pr_push_publishes_the_validated_commit_not_the_branch_tip; refspecs at :1995, :2109 |
| 02-11 | Never read committed objects through replacement refs | VERIFIED | Three replace-ref tests; per-read control for `cat-file` re-run here |
| 02-11 | No ref beyond the refspec's branch update | VERIFIED | test_direct_push_does_not_publish_hook_tag and PR twin; `--no-follow-tags` at both pushes |
| 02-11 | Never refuse only because a hook ran | VERIFIED | Benign hook controls in both modes, repeated in these probes |
| 02-12 | Never publish hook-written commit text: message bound, header scanned, no empty directory | VERIFIED as written | Named tests pass; per-read control for `ls-tree` re-run here. The statement does not cover a non-empty directory named `.`, see gap 5 |
| 02-12 | Never refuse because a hook ran or git maintains the object store | VERIFIED | Benign prepare-commit-msg controls; test_push_from_clone_of_gc_remote_publishes |

All 26 are test-tier with wired tests, except that the two FAILED rows are contradicted by the probes
below.

### Derived in this re-verification

| Truth | Status | Evidence |
|-------|--------|----------|
| A push that returns Ok has published the validated content | **FAILED** (partial) | CR-201, gap 4 |
| A hook cannot add an unbound name to the published tree | **FAILED** (partial) | WR-200, gap 5 |

**Score:** 38/44 verified (SC 3/4; plan groups 11/12; prohibitions 24/26; derived 0/2); 0 present but behavior-unverified.

SC2 was graded VERIFIED with a caveat last time and is FAILED now. That is a stricter reading on new
evidence, not a code regression: the same text covers committed blobs, and a blob now reproduces past it.

### Advisory (New Scope, Unevidenced)

None.

### Required Artifacts

| Artifact | Status | Details |
|----------|--------|---------|
| 02-01..02-10 artifacts | VERIFIED | `verify.artifacts` 26/26 (each plan exit 0) |
| `src/sync/mod.rs` (02-11) | VERIFIED | `verify.artifacts` 3/3 |
| `src/sync/mod.rs` (02-12) | VERIFIED | `verify.artifacts` 7/7 |

### Key Link Verification

`verify.key-links` exits 1 with 0/4 and 0/5 for plans 02-11 and 02-12, with the detail "Source file not
found". That is the tool rejecting the plans' `from:` wording (a path plus a symbol), not missing wiring,
so each link's pattern was checked by hand over the non-test, non-comment code.

| From | To | Status | Details |
|------|----|--------|---------|
| validate_committed_tree | raw_commit_parents, raw_commit_parts | WIRED | Definition and call each, :2287, :2302 |
| push_to_remote | push_pr_branch, push_direct with fetched_sha | WIRED | Call at :1743-1749 spans lines, so the plan's one-line pattern matches nothing; read directly |
| push_direct, push_pr_branch | push of the validated id, no tags | WIRED | Refspecs at :1995, :2109; `"--no-follow-tags"` 2 lines |
| validate_committed_tree | count_empty_directories | WIRED | :2435 |
| validate_committed_tree | refuse_grafts_file | WIRED | :2266 |
| both pushes | `-c core.commitGraph=false`, `-c pack.useBitmaps=false` | WIRED | 2 lines each |
| both pushes | remove_git_test_env | WIRED | 2 calls, :2007, :2122 |

Presence does not prove behaviour here; the per-option controls do.

### Behavioral Spot-Checks

Scratch copy of HEAD (403b86b) with temporary tests added inside `mod tests`, driving the real
`push_to_remote` on local bare remotes. Every case asserts the hook's marker exists.

| Case | Mode | push_to_remote | Remote holds injected blob | Fresh ensure_remote_repo reads | `git clone` |
|------|------|----------------|----------------------------|--------------------------------|-------------|
| Benign hook (control) | direct, PR | Ok | n/a | validated bytes | exit 0 |
| Rebuild, one validated entry (control) | direct, PR | Ok | no | validated bytes | exit 0 |
| Rebuild, one injected entry (control) | direct, PR | Refused, `differs from the validated content` | no | n/a | n/a |
| Duplicate entry | direct, PR | **Ok** | **yes** | **`INJECTED_BY_HOOK`** | exit 0 |
| Duplicate entry, PAT shape built at runtime | direct, PR | **Ok** | **yes** | **the token shape** | exit 0 |
| Duplicate directory (not in the review) | direct | **Ok** | **yes** | **`INJECTED_BY_HOOK`** | exit 0 |
| Amend, content kept (control) | direct, PR | Ok | n/a | validated bytes | exit 0 |
| Delete | direct, PR | **Ok** | n/a | error, `not a ude template repository` | exit 0 |
| Revert | direct, PR | **Ok** | n/a | old bytes | exit 0 |
| `.` subtree, validated blob | direct, PR | **Ok** | no | error, `invalid path` | **exit 128** |
| `.` subtree, injected blob (variant) | direct | Refused, `differs from the validated content` | no | n/a | n/a |
| `sub` subtree, validated blob (control) | direct | Refused, `unvalidated file committed` | no | n/a | n/a |

Duplicate-entry commit, plain `git push` to a new bare remote: exit 1 with `duplicateEntries` under
`receive.fsckObjects=true`; exit 0 without it. `git fsck` on the remote that accepted it: exit 1,
`duplicateEntries`. On the `.` remote: exit 0 with a `hasDot` warning.

Real CLI, direct mode, hook reached through `core.hooksPath` in the fake user's global git config:

| Case | `ude sync push` | Remote | Pusher's next status and pull | Consumer's pull |
|------|-----------------|--------|-------------------------------|-----------------|
| No hook (control) | exit 0, ancestor advances | edit | In Sync; nothing to pull | gets the edit |
| Benign hook (control) | exit 0, ancestor advances | edit | In Sync; nothing to pull | gets the edit |
| Duplicate entry | exit 0, ancestor advances | **both blobs** | Upstream Updated; **pull replaces the edit with `INJECTED_BY_HOOK`** | **`INJECTED_BY_HOOK`** |
| Delete, no other managed template | exit 0, ancestor advances | **template gone** | status and pull exit 2 `not a ude template repository` | same error |
| Delete, another managed template kept | exit 0, ancestor advances | **template gone** | Deleted upstream; **pull removes mise.toml** | **pull removes mise.toml** |
| Revert | exit 0, ancestor advances | **old bytes** | Upstream Updated; **pull discards the pusher's edit** | no update |

Grafts race, post-commit hook with a background process that polls for the push and then writes the
grafts file: see the focus table. `GIT_GRAFT_FILE=/dev/null` on a plain push with a grafts file present:
injected blob absent; same push without the variable: present.

Gates, one run each in a scratch copy of HEAD with a fresh target directory:

| Gate | Result |
|------|--------|
| `cargo fmt --check` | exit 0 |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 |
| `cargo test` | exit 0; 263 unit passed, 1 ignored; 88 e2e passed |
| `cargo deny check advisories bans sources` | exit 0 |
| `cargo machete` | exit 0 |
| Decoy GIT_DIR, `cargo test --bin ude sync::` | exit 0, 183 passed; decoy has 0 objects and 0 refs |
| Per-option and per-read controls | all 8 removals exact; 2 unmodified rows failing none |
| Scoped mutation run | 49 caught, 0 missed, 0 timeouts, exit 0, plan's test list plus test_refuse_grafts_file_outcomes, as the SUMMARY recorded |

### Probe Execution

No `scripts/*/tests/probe-*.sh` exists or is declared for plans 02-11 and 02-12. Step 7c: not applicable.

### Requirements Coverage

| Requirement | Source Plans | Status | Evidence |
|-------------|--------------|--------|----------|
| SEC-01 entropy and raw Base64 | 02-03, 02-04, 02-06 | SATISFIED | SC1 |
| SEC-02 gitleaks categories before staging | 02-01, 02-05, 02-06 | SATISFIED as worded | The staging gate scans every file; the hole in SC2 is after staging |
| SEC-03 strict boundaries | 02-07, 02-09 | SATISFIED | SC3 |
| SEC-04 placeholder reversibility | 02-02, 02-07, 02-08, 02-09, 02-10, 02-11, 02-12 | PARTIAL | SC4 satisfied; the binding prohibition filed under SEC-04 fails |

All four IDs are claimed by plans and all exist in REQUIREMENTS.md; none is orphaned. REQUIREMENTS.md and
02-SECURITY.md no longer match this report: REQUIREMENTS.md marks SEC-04 Complete and SEC-01..03 Gaps
Found, and 02-SECURITY.md says `threats_open: 0`, `status: verified`, with T-02-38 and T-02-42 closed.
Neither was edited here.

### Anti-Patterns Found

No `TBD`, `FIXME`, `XXX`, `TODO`, `HACK`, `todo!` or `unimplemented!` in src/sync/mod.rs or src/process.rs
(negative control: a planted `FIXME` line counts 1).

The other findings in 02-REVIEW.md were not reproduced here and are not goal gaps:

| Item | Verdict |
|------|---------|
| WR-201 rollback leaves the cache wrong after a refusal | Warning. The reviewer's run shows nothing published and the next ensure restoring it |
| WR-202 grafts path check with older git | Warning. Shim only in the review; no old git was run |
| WR-204 the inherited-variable tests use the engine's extra environment | Warning. Test strength only; 02-SECURITY records the setup |
| IN-200 hook-written `push.pushOption` | Info. Not stored in the repository |
| IN-201..IN-205 | Info. Not goal gaps |

### Human Verification Required

None. What is left is a fix or an operator decision, not a check a person must perform.

### Gaps Summary

Five gaps, three causes.

1. **The gate reads blobs by path (gaps 1, 2).** A duplicate tree entry shows it the validated blob while
   the push sends both. This is the serious one: unvalidated and unscanned bytes reach consumers.
2. **The gate checks only what changed, one way (gaps 4, 5).** Nothing requires validated content to be in
   the commit, and PathBuf equality accepts `.`. No unvalidated bytes, but a false success, lost edits,
   and a shared repository that cannot be cloned. Same listing pass as cause 1.
3. **The grafts refusal runs once, before the push (gap 3).** A process a hook left running can write
   the file after it. The prohibition's exception does not cover this.

None is covered by later phases (3 is doctor and CLI, 4 is templates and manifests), so nothing is deferred.

Decision for the operator on gap 3 only: fix it with `GIT_GRAFT_FILE` on both pushes, or extend the T-02-52
acceptance to any process a hook started. The fix is recommended if the carve-out in
`process::prepare_command` stays small, since it also targets the pre-push grafts residual; that wider
effect was not run. An override is not suggested for gaps 1 and 2: they are incomplete implementation,
not an intentional deviation.

### What these checks do not establish

- Local bare remotes only, on git 2.56.0 on Linux. **No hosted remote was tried.** A remote that runs fsck on
  receive refused the duplicate-entry tree in a plain git push, so whether CR-200 publishes on GitHub or
  another host is unverified; pushing a malformed tree there is an outward-facing action no run here took.
  A rejected push should make `ude sync push` fail, not publish (reasoned from the code, not run). Whether such a host accepts the `.` entry,
  which `git fsck` only warns about, was not run either.
- 5 of 60 is a weak bound on the race: roughly 4% to 18% for this harness at 95%, local paths, one polling
  hook. It shows the race happens, not its rate over a network or for another hook.
- CLI runs were direct mode. PR mode was driven through `push_to_remote`, including checks of the remote branch,
  not through `ude sync push --pr`.
- The proposed fixes were not applied or tested in UDE; only the `GIT_GRAFT_FILE` plain-git check ran.
- Gaps 1 to 5 are the channels probed, not a proven complete list. The passing regression suite covers the
  cases it names, and the earlier CLI and gitleaks-binary parity probes behind SC1 to SC4 were not repeated.
- Mutation runs use function-level mutants; they do not show that a channel is covered.
- Windows is unverified (backlog Phase 999.1).

---

_Verified: 2026-11-01T16:28:16Z_
_Verifier: Claude (gsd-verifier)_
