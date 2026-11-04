---
phase: 02-sanitizer-boundaries-security-hardening
verified: 2026-11-04T07:09:47Z
status: gaps_found
score: 70/75 must-haves verified
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
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-13-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-13-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-14-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-14-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-15-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-15-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-16-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-16-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-17-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-17-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-18-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-18-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-19-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-19-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-20-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-20-SUMMARY.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-21-PLAN.md"
  - ".planning/phases/02-sanitizer-boundaries-security-hardening/02-21-SUMMARY.md"
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
covered_digest: "v4:sha256:392002a035d846855b5b1a6c31044534b5d752cb69827d794772acacb78bcd26"
behavior_unverified: 0
overrides_applied: 0
re_verification:
  previous_status: gaps_found
  previous_score: 43/55
  gaps_closed:
    - "02-08 blob prohibition and ROADMAP SC2 (CR-300, CR-301): both routes refused or ignored in both modes; the token cases pass; removing the `^{commit}` source fails 18 tests and removing the head check fails 10 (negative controls run here)"
    - "02-11, 02-12, 02-13 prohibitions re-opened by CR-300 (shadow ref): the rebuilt duplicate, `.`, delete, revert and message commits behind a shadow ref stay off the remote in both modes"
    - "A hook cannot add an unbound name to the published tree (CR-301): tracking-ref tests assert templates/leak.txt absent; a forged base tree carrying a token blob was probed here and git itself refuses it (`hash mismatch`, `git status` exit 128), nothing published"
    - "02-13 prohibition on success without validated content, for WR-300 (index flag) and WR-301 (pushurl): refused; removing the empty-status check fails 4 tests and making confirm_published always pass fails 4"
    - "02-13 prohibition on printing hook-chosen bytes (WR-302): the continuity refusal names only the fetched head; putting the raw parent back into the message fails 4 tests"
  gaps_remaining:
    - "A push returning Ok means the remote holds every recorded path: re-opened by a new channel, review CR-400 (a local object whose bytes do not match its id), reproduced here on both routes and in both modes"
  regressions: []
gaps:
  - truth: "02-13 prohibition, scoped by 02-20 to DEF-11: MUST NOT report success for a push whose commit lacks validated content; a push returning Ok means every validated path holds its recorded content in the published commit. Also the previous report's derived truth: a push that returns Ok has published the validated content"
    status: failed
    reason: "Review CR-400, reproduced here at engine level, direct and PR. Two recorded paths; one changed, the other (gitconfig.repo) kept at the seed blob in the index by assume-unchanged, and the seed blob's loose object file in the cache checkout replaced with the bytes of the validated content. The committed gate reads that blob by id with `cat-file blob`, which does not check the bytes against the id, so it reads the validated content and passes. Result: Ok(\"Directly pushed changes...\") and Ok(\"Pushed changes to branch 'ude-sync-...'\"); the remote's main (and the sync branch) hold the seed bytes at gitconfig.repo, not the record. Control, same steps without the object change: refused, `does not hold the validated content of templates/projects/generic/gitconfig.repo`. The bare remote still reads the seed id as the seed bytes, so the probe did not corrupt the remote. Nothing unvalidated reached the remote; the defect is a false Ok, and direct mode then records the validated hash as the ancestor (src/main.rs:1263-1289)."
    artifacts:
      - path: "src/sync/mod.rs"
        issue: "read_blob_by_id (2879-2891) returns stored bytes without checking that they hash to the id; first_path_not_held (2775-2788) and so require_validated_content (2756-2769) decide 'held' from those bytes, for recorded paths the push never sends because the remote already holds them"
    missing:
      - "Check each blob these checks rely on against its id: hash `blob <len>\\0<bytes>` (or run `git hash-object -t blob --no-filters --stdin`) and refuse on mismatch; or record each validated file's git blob id at staging and require the tree to list exactly that id at each recorded path"
      - "Rows in both modes where a local object no longer matches its id, each refused, keeping the existing controls; add them to DEF-11 in docs/ARCHITECTURE.md"
      - "Or an operator ruling that this channel is outside the promise, recorded by widening NOT-10 (see Needs operator decision)"
  - truth: "02-19 prohibition: MUST NOT report success for a push whose validated content is not on the remote's head when status is empty (DEF-12, a class row). 02-19 truth: 'Ok here means the remote's head held the content when read'. ARCHITECTURE promise 2: 'Ok with nothing to push means the head read that way holds every recorded path'"
    status: failed
    reason: "CR-400 on the empty-status route, reproduced here direct and PR: validated content written, assume-unchanged set, the seed blob's loose object replaced with the validated bytes. Local `cat-file blob <seed id>` returns the validated bytes. Result: Ok(\"No changes to push to remote repository.\") in both modes, no remote head moved, 0 remote heads hold the record. Control without the object change: refused, `git status reports nothing to push, but the fetched head does not hold the validated content`. The head check (02-17) confirms the fetched head's id, not the content the checkout stores under it."
    artifacts:
      - path: "src/sync/mod.rs"
        issue: "require_fetched_head_holds (2797-2873) reads the fetched head's blobs from the checkout's object store through first_path_not_held and read_blob_by_id"
    missing:
      - "Closed by the same id check as the gap above; add empty-status rows in both modes and add the channel to DEF-12"
---

# Phase 2: Sanitizer, Boundaries & Security Hardening Verification Report

**Phase Goal:** Screen and redact all confidential tokens, raw credentials, and user-specific paths before any remote git staging or push.
**Verified:** 2026-11-04T07:09:47Z
**Status:** gaps_found
**Re-verification:** Yes, after gap plans 02-16 to 02-21

## Bottom line

The secret-screening goal now holds on every route tried here: no token or unvalidated blob reached a remote,
and all five earlier findings (CR-300, CR-301, WR-300, WR-301, WR-302) are closed, each fix shown to be
load-bearing by a negative control run here.

One gap remains, and it is about what Ok means, not about leaks. Review CR-400 reproduced here on both
routes and in both modes: when a loose object in the cache checkout no longer matches its id, the gate reads
the validated content where the remote holds the old content, and the push returns Ok. In direct mode the
ancestor state then advances for content the remote does not have.

**What argues against this verdict.** CR-400 needs code that writes `.git/objects` in the cache checkout and
knows the exact validated bytes; no hook-driven or CLI run of it was made (the object change was a test step,
the same model DEF-12's own `_after_ensure` test uses). Nothing unvalidated is published. NOT-10 already
lists a loose object overwritten *during the push, after the gate read it* as pending the operator; CR-400 is
the same object class *before* the gate reads it. Read literally, NOT-10 does not cover it, so under the
doc's rule (1) it is a defect against DEF-11 and DEF-12. If the operator reads NOT-10 as covering every
forged local object, this becomes an operator decision instead (rule 3), and the phase would pass.

## Goal Achievement

### Ruling on the review findings (02-REVIEW.md, 88bb986)

| Finding | Reproduced here? | Ruling | Evidence |
|---------|------------------|--------|----------|
| CR-400, local object not matching its id | **Yes**, engine level: committed route direct and PR; empty-status route direct and PR. Controls refused | **FAILED, blocks** (DEF-11, DEF-12, 02-13 and 02-19 prohibitions) | See gaps. Added here: the tree variant the review reasoned was probed with a forged base tree hiding a token blob. git refused it (`hash mismatch`, `bad tree object HEAD`, `git status` exit 128); the push failed closed, the token blob is not on the remote. So on git 2.56.0 the channel is blobs read by `cat-file`, not trees |
| WR-400, relative remote path from two directories | **Yes**, engine level: `../x` from two directories naming two repositories, one cache | Warning, not a must-have failure | First call Ok; every later call from the second directory: `fetched head is not the commit the remote branch holds; a hook or config ... most likely moved or redirected it`. Fails closed, wrong cause named, persists until the cache is removed. 02-17's relative-path truth is about one working directory and holds |
| IN-400 | Read | Info | open_push_templates (src/main.rs:1615) returns a checkout path its only caller binds as `_remote_checkout_root` (1042) |
| IN-401 | Read | Warning (doc) | DEF-02 says "Every gate read ... run past replace refs"; stage_publication's `status`, `diff --cached`, `show :<path>`, `write-tree` do not. The committed gate reads raw and binds, so no bypass is known |
| IN-402 | Read | Info | src/sync/mod.rs:1201-1208, Err from the read-back gets the "not at the validated commit" text |
| IN-403 | Read | Info | src/sync/mod.rs:2861 `unwrap_or(staged)`; staged paths come from under the checkout today |
| CR-300, CR-301, WR-300, WR-301, WR-302 | Fix code read; named tests run; a negative control per fix run | **Closed** | Table below |
| IN-300, IN-301 | Not re-run | Not goal gaps | NOT-14 records IN-300 as open |

### Negative controls on the earlier fixes (scratch copy of HEAD, each edit reverted after)

| Fix removed | Tests run | Result |
|-------------|-----------|--------|
| Push source back to `{commit}:refs/heads/` (CR-300) | `named_after_the_commit`, `behind_shadow_ref` | 18 failed, 2 passed (the `refs/<id>` pair, which never shadowed) |
| Head check always passes (CR-301) | `tracking_ref_moved` | 10 of 10 failed |
| Empty-status check removed (WR-300) | `assume_unchanged_set` | 4 of 4 failed |
| confirm_published always Ok (WR-301 family) | `hook_written_origin_pushurl`, `push_instead_of` | 4 failed; the 2 pushurl tests pass, as expected, because the push no longer reads `origin` |
| Raw parent text back in the continuity refusal (WR-302) | `parent_line` | 4 of 4 failed |

Unmodified, the same filters: 47 passed; token cases 12 passed; e2e `test_cli_` 29 passed.

### Roadmap Success Criteria

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| SC1 | High-entropy strings and raw Base64 tokens flagged before staging | VERIFIED | `git diff --quiet b8c54e9 HEAD -- src/sanitizer src/template` exit 0 (control: same over src/sync exit 1); full suite green (orchestrator's run) |
| SC2 | Outbound commit inspection catches gitleaks categories across committed blobs, rolls back | VERIFIED | Token behind shadow ref, on moved tracking ref, in duplicate entry, hidden in index: refused or absent from the remote, both modes, with delivering controls; gitleaks-only category and hook-injected secret rollback tests pass. CR-400 publishes no blob; its tree variant fails closed (probed) |
| SC3 | Path parameterization on word and punctuation boundaries only | VERIFIED | Regression only: code unchanged since b8c54e9 |
| SC4 | Templated tokens substitute back on initialization | VERIFIED | Regression only: code unchanged since b8c54e9 |

### Plan must-have groups

| Plan | Status | Evidence |
|------|--------|----------|
| 02-01..02-10 | VERIFIED (10) | Regression; sanitizer and template code unchanged |
| 02-11, 02-12 | VERIFIED (2) | Were FAILED via CR-300; shadow-ref rows now pass with controls |
| 02-13 | **FAILED** | Gate behaviour holds, and CR-300 no longer bypasses it. Gap-4 clause "a push returning Ok means every validated path holds its recorded content" fails under CR-400 (committed route) |
| 02-14, 02-15 | VERIFIED (2) | Unchanged code paths; tests pass |
| 02-16 | VERIFIED | Source `{commit}^{{commit}}` with `--no-replace-objects` at mod.rs:2195-2210, 2349-2365; resolve_commit past replace refs (2415); 25 DEF-01 tests; control above |
| 02-17 | VERIFIED | ensure_remote_repo_for_push (1133-1163), remote_head exact refname (1168-1188); 29 DEF-06 tests; control above. WR-400 is a warning outside the truth's wording |
| 02-18 | VERIFIED | Push to `target.remote_url`, confirm_published (1195-1209) at 2227 and 2396; control above |
| 02-19 | **FAILED** | WR-300 and WR-302 closed (controls above). Truth "Ok here means the remote's head held the content when read" fails under CR-400 (empty-status route) |
| 02-20 | VERIFIED | DEF rows byte-identical in plan and doc (15); NOT rows identical once the Record column is dropped (control: one changed byte gives a diff); 214 distinct DEF test names, each defined exactly once (control: an invented name counts 0); 02-CONTEXT.md and plans 02-01..02-15 unchanged since b8c54e9 |
| 02-21 | VERIFIED | Stored results read: controls-02-21.tsv 35 `same`, 11 `corrected`, 46 rows; outcomes.json 122 mutants, 103 caught, 0 missed, 19 unviable; missed.txt empty; no .cargo/mutants.toml. Code before `mod tests {` unchanged since 5ce1bee (first changed line 14514, tests start 3398). Gates re-run here, below |

### Prohibitions

| Plan | Prohibition | Status | Evidence |
|------|-------------|--------|----------|
| 02-01..02-10 (18) | As before | VERIFIED | Regression |
| 02-08 as scoped by 02-20 | No blob differing from the validated bytes, through UDE's push and a DEF channel | VERIFIED | Was FAILED; CR-300/301 closed. CR-400 sends no blob |
| 02-11 (5) | Validated commit only; no reachable differing blob; raw reads; no extra ref; no refusal for a hook alone | VERIFIED | Shadow-ref and tracking-ref rows; benign-hook rows |
| 02-12 (2) | No hook-written commit text; no refusal for hooks or maintenance | VERIFIED | Amended message behind shadow ref, both modes |
| 02-13 | Never report success when the commit lacks validated content | **FAILED** | CR-400 committed route |
| 02-13 | No unclonable or ambiguous tree | VERIFIED | Duplicate and `.` trees behind shadow ref, both modes |
| 02-13 | No hook-chosen name or line in refusals | VERIFIED | Was partial; parent-line rows and CLI zero-ESC rows |
| 02-13 | No refusal for a hook alone | VERIFIED | Benign rows |
| 02-14 (2), 02-15 (2) | As before | VERIFIED | Unchanged |
| 02-16 (2) | No other commit or ref-built tree; hooks keep running | VERIFIED | DEF-01 rows; benign pre-push rows |
| 02-17 (2) | Base never from the checkout; no refusal for a hook alone | VERIFIED | DEF-06 rows; benign post-checkout rows |
| 02-18 (2) | No Ok unless the URL reports the ref at the commit; hooks still run | VERIFIED | DEF-13 rows. CR-400's committed route does put the ref at the commit, so this rule holds there |
| 02-19 | No Ok on empty status unless the remote's head holds the record | **FAILED** | CR-400 empty-status route |
| 02-19 (2) | No hook bytes in committed-gate refusals; legitimate no-change still Ok | VERIFIED | Parent-line rows; `_publishes_nothing_when_content_is_held` rows |
| 02-20 (3) | No blob differing from validated; no row on reasoning alone; plans 01-15 unedited | VERIFIED | As above |
| 02-21 (2) | Control rows honest; no mutant killed by exclusion | VERIFIED | Stored tsv; no mutants.toml |

All test-tier and wired to tests. The two FAILED rows have passing tests, and the probes here contradict
them through a channel no test covers.

### Derived in the previous report, re-checked

| Truth | Status | Evidence |
|-------|--------|----------|
| A push that returns Ok has published the validated content | **FAILED** | WR-300, WR-301 routes closed; CR-400 re-opens it |
| A hook cannot add an unbound name to the published tree | VERIFIED | CR-301 rows assert templates/leak.txt absent; forged base tree with a hidden name fails closed (probed) |

**Score:** 70/75 (SC 4/4; plan groups 19/21; prohibitions 46/48; derived 1/2). 0 present but behaviour-unverified.

### Deferred Items

None. Phases 3 and 4 do not cover the push path.

### Advisory (New Scope, Unevidenced)

None. CR-400 and WR-400 were reproduced here; IN-400 to IN-403 are recorded as info or warning, not blockers.

### Required Artifacts

| Plan | `verify.artifacts` | `verify.key-links` |
|------|--------------------|--------------------|
| 02-16 | 3/3, exit 0 | 1/1, exit 0 |
| 02-17 | 4/4, exit 0 | 2/2, exit 0 |
| 02-18 | 3/3, exit 0 | 2/2, exit 0 |
| 02-19 | 3/3, exit 0 | 1/1, exit 0 |
| 02-20 | 2/2, exit 0 | 1/1, exit 0 |
| 02-21 | 2/2, exit 0 | 1/1, exit 0 |

### Key Link Verification

| From | To | Status | Details |
|------|----|--------|---------|
| refspec source | the validated commit | WIRED | Was BROKEN; `^{commit}` plus `--no-replace-objects`; control fails 18 tests |
| fetched head | the remote's head (id) | WIRED | Was NOT WIRED; control fails 10 tests |
| push destination | caller's URL, read back from `/` | WIRED | confirm_published at both pushes |
| empty status | require_fetched_head_holds | WIRED | Both early returns (1936, 1941) |
| **stored blob bytes** | **the blob id** | **NOT WIRED** | Not a plan key link. Nothing checks that bytes read by id hash to that id (CR-400) |

### Data-Flow Trace (Level 4)

Not applicable: no rendered dynamic data. The bytes-to-remote trace is the ruling tables above.

### Behavioral Spot-Checks

Scratch `git archive` copy of HEAD (5064edc; code at 8570971), own target dir, git 2.56.0, local bare remotes,
probe tests added to `mod tests`. Fake token built at runtime; not reproduced here.

| Case | Mode | Result | Remote |
|------|------|--------|--------|
| Empty status, seed blob forged to validated bytes | direct, PR | **Ok("No changes...")** | 0 heads hold the record, no head moved |
| Same, no forgery (control) | direct, PR | refused, empty-status `does not hold the validated content` | unchanged |
| Committed, unchanged recorded path forged | direct, PR | **Ok** | main (and sync branch) hold the seed at gitconfig.repo |
| Same, no forgery (control) | direct, PR | refused, `does not hold the validated content` | unchanged |
| Forged base tree hiding a token blob | direct, PR | refused, `git status` exit 128, `hash mismatch` | token blob absent |
| Same, no forgery (control) | direct | Ok, normal publish | token blob absent |
| WR-400: `../x` from a second directory | engine | refused twice, hook named as cause | n/a |

Gates, run once each in the repository: `cargo fmt --check` exit 0; `cargo clippy --workspace --all-targets
-- -D warnings` exit 0; `cargo machete` exit 0; `cargo deny check advisories bans sources` exit 0. Full
`cargo test` not re-run by me: the orchestrator's run at this code (417 unit, 1 ignored; 114 e2e) is cited,
plus my filtered runs above. Cargo.toml and Cargo.lock unchanged since b8c54e9.

### Probe Execution

No `scripts/*/tests/probe-*.sh` is tracked or declared by plans 02-16 to 02-21. Step 7c: not applicable.

### Requirements Coverage

| Requirement | Source Plans | Status | Evidence |
|-------------|--------------|--------|----------|
| SEC-01 entropy and raw Base64 | 02-03, 02-04, 02-06, carried by 02-21 | SATISFIED | SC1 |
| SEC-02 gitleaks categories before staging | 02-01, 02-05, 02-06, 02-13..02-21 | SATISFIED | SC2; no token reached a remote on any route tried |
| SEC-03 strict boundaries | 02-07, 02-09, carried by 02-21 | SATISFIED | SC3 |
| SEC-04 placeholder reversibility | 02-02, 02-07..02-21 | PARTIAL | SC4 satisfied; the two Ok-meaning prohibitions filed under SEC-04 fail (CR-400) |

All four IDs are claimed by plans and exist in REQUIREMENTS.md; none orphaned. REQUIREMENTS.md does not match:
it marks SEC-01 and SEC-03 unchecked and "Gaps Found", SEC-02 and SEC-04 "Complete". Not edited here.

### Anti-Patterns Found

No `TBD`, `FIXME`, `XXX`, `TODO`, `HACK`, `todo!` or `unimplemented!` in src/sync/mod.rs, src/main.rs,
src/process.rs, tests/e2e_lifecycle.rs or docs/ARCHITECTURE.md (control: a planted `FIXME` line counts 1).

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| src/sync/mod.rs | 2794-2795 | Doc: "Ok here means the remote's head held the content when read" | Blocker, with gap 2 | False under CR-400 |
| src/sync/mod.rs | 2752-2754 | Doc: "a push returning Ok has published it" | Blocker, with gap 1 | False under CR-400 |
| docs/ARCHITECTURE.md | DEF-02 | "Every gate read ... past replace refs" | Warning | Staged gate's reads do not (IN-401) |
| src/sync/mod.rs | 1157-1161 | Refusal blames a hook | Warning | Wrong cause for WR-400, reproduced |

### Human Verification Required

None. What is left is a fix or an operator ruling, not a check a person must perform.

### Open operator items (not gaps)

Still recorded as "pending the operator, not accepted" in 02-15-PLAN `## Operator answers`: T-02-60, T-02-61,
T-02-64, T-02-65, T-02-67. Not decided here. 02-SECURITY.md has not been updated since these plans, so its
`threats_open` is stale. The hand edits in `.planning/todos/pending/2026-11-01-hand-edits-owed-for-phase-02-after-re-verification.md`
stay conditional on re-verification passing, so they wait.

### Needs operator decision

Whether CR-400 is a defect (fix) or outside the promise (widen NOT-10). Recommended: fix. It is small (one
id check in read_blob_by_id, or comparing recorded blob ids), it makes the existing doc comments true, and it
closes the false ancestor advance. If the operator instead rules it out of scope, record that by widening
NOT-10 and add this override:

```yaml
overrides:
  - must_have: "MUST NOT report success for a push whose validated content is not on the remote's head when status is empty"
    reason: "A local object whose bytes do not match its id is outside the gate's promise (NOT-10 widened)"
    accepted_by: "{operator}"
    accepted_at: "{ISO timestamp}"
  - must_have: "MUST NOT report success for a push whose commit lacks validated content"
    reason: "Same ruling, committed route"
    accepted_by: "{operator}"
    accepted_at: "{ISO timestamp}"
```

### Gaps Summary

One cause, two gaps. The gate decides "held" from bytes in the cache checkout's object store, and git does not
check those bytes against the id when `cat-file blob` reads them. On the committed route this hits recorded
paths the commit leaves unchanged; on the empty-status route it hits the fetched head. The fix is the same for
both.

### What these checks do not establish

- CR-400 was driven by test steps, not an installed hook, and not through `ude sync push`. The data-loss chain
  (the next pull taking the remote's old content over the user's edit) is reasoned, not run.
- Only loose objects were forged. A packed object, commit objects and subtree-only forgeries were not tried. The
  tree result (git refuses) is for a forged root tree on git 2.56.0; it does not prove every tree read
  verifies.
- Local bare remotes, git 2.56.0, Linux only. No hosted remote, https, ssh or Windows: unreachable here.
- The negative controls show each earlier fix is load-bearing for its named tests; they do not show the list of
  channels is complete. CR-400 is exactly a channel those tests cannot see.
- The control table and mutation run were read from stored files, not re-run; 0 `corrected` rows were added
  since, and the code before `mod tests {` is unchanged since they ran.
- My count of distinct DEF test names is 214 (217 occurrences); the review says 215. Each of the 214 is defined
  exactly once.

---

_Verified: 2026-11-04T07:09:47Z_
_Verifier: Claude (gsd-verifier)_
