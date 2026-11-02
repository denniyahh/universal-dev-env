---
phase: 02-sanitizer-boundaries-security-hardening
verified: 2026-11-02T19:44:32Z
status: gaps_found
score: 43/55 must-haves verified
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
covered_digest: "v4:sha256:94c9eb144e749f0489fa4a8b2230e80253e22cd772fc644138ae13b354a61f65"
behavior_unverified: 0
overrides_applied: 0
re_verification:
  previous_status: gaps_found
  previous_score: 38/44
  gaps_closed:
    - "Grafts file written after the gate (review WR-203): 02-14's deterministic tests pass in both modes for a pre-push hook and a process a post-commit hook left running, each with a negative control that delivers the blob without the null grafts file"
    - "The channel of a hook moving HEAD to a duplicate-entry tree (review CR-200, including the token-shaped second entry): refused in both modes"
    - "The channel of a hook moving HEAD to a commit that deletes or reverts the validated file (review CR-201): refused in both modes"
    - "The channel of a hook moving HEAD to a tree with an entry named `.` (review WR-200): refused in both modes"
  gaps_remaining:
    - "02-08 blob prohibition and ROADMAP SC2: fail through two channels the previous round did not know, CR-300 and CR-301"
    - "A push returning Ok has published the validated content: fails through WR-300 and WR-301, and through CR-300"
    - "A hook cannot add an unbound name to the published tree: fails through CR-300 and CR-301"
  regressions: []
gaps:
  - truth: "02-08 prohibition, restated by 02-13 and 02-14 (SEC-04): MUST NOT publish a blob whose content differs, beyond CRLF/LF line endings of UTF-8 text, from the bytes push_updates validated, whatever git clean filter or commit hook produced it. 02-14 scopes it to what UDE's own push sends; both channels below are UDE's own push"
    status: failed
    reason: "Review CR-300 and CR-301, reproduced here through push_to_remote and through `ude sync push`, direct and PR. CR-300: a post-commit hook creates refs/tags/<validated id> (also refs/heads/<id> and refs/remotes/<id>) pointing at an injected commit; git push resolves the refspec source to that ref, returns Ok, and the remote holds the injected blob. CR-301: a post-checkout hook moves refs/remotes/origin/main onto an injected commit during ensure_remote_repo; fetched_sha reports it, UDE's commit sits on it, the parent check passes, and the push carries it. Nothing refused either one; a consumer's `ude sync pull` wrote the fake token into mise.toml in the CR-300 CLI run. Controls: same hook without the shadow ref, no injected blob on the remote; same hook moving no ref, fetched_sha equals the real head; the same token with no hook is refused with exit 1."
    artifacts:
      - path: "src/sync/mod.rs"
        issue: "Refspecs at lines 2054 and 2182 use `{commit}:refs/heads/...`, whose source a local ref of the same name takes precedence over; the doc comment at 2112 and the comment at 2035 claim the opposite"
      - path: "src/sync/mod.rs"
        issue: "fetched_head (lines 1092-1108) reads a local ref after hooks have run (1036-1064), and validate_committed_tree (2374-2375) trusts it; nothing compares it with the remote"
    missing:
      - "Make the push source something no ref can be named (review's fix, `{commit}^{commit}` plus `--no-replace-objects`), rewriting test_direct_push_replaced_commit_refusal_does_not_say_upstream_moved and the comment at 2207"
      - "Take the base from the remote: `git ls-remote <remote_url> refs/heads/<branch>` from outside the checkout before the fetch, refuse when fetched_head differs (not run by anyone yet)"
      - "Hook tests in both modes for each channel asserting the injected blob is absent from the remote, with a control that delivers it"
      - "Or an operator decision to narrow what the gate promises, recorded in the plan and SECURITY docs, with the doc comments changed"
  - truth: "ROADMAP SC2: Outbound commit inspection catches all Gitleaks secret categories across committed blobs and auto-rolls back commits on detection"
    status: failed
    reason: "Same root causes as the gap above. A GitHub PAT shape, flagged by the scanner in a control, reached the remote in direct and PR mode with no refusal and no rollback. SC2 holds for every commit the gate actually inspects; it does not hold for what the push then sends."
    artifacts:
      - path: "src/sync/mod.rs"
        issue: "The gate inspects the commit named by the id; the push can send another (CR-300) or a commit below it (CR-301)"
    missing:
      - "Closed by the same changes as the gap above; add a token-shaped case to the hook tests"
  - truth: "02-11 prohibition MUST NOT push any commit other than the one the committed gate validated; 02-11 prohibition on a commit reachable from the published ref; 02-12 prohibition MUST NOT publish commit text a hook wrote; 02-13 prohibition MUST NOT publish a tree another machine cannot clone or read unambiguously; 02-11 truth that every blob the push adds is checked; 02-13 gap-1 clause that no blob reaches the remote unread"
    status: failed
    reason: "CR-300 re-opens each of these. Run here with the hook building the same rebuilt commits the plan's tests refuse, then putting HEAD back and shadowing the id: the remote tip is the injected commit (Ok, direct); a duplicate-entry tree with its injected blob published (Ok, direct and PR); a `.` tree published and `git clone` exit 128 (Ok, direct); a message with INJECTED_BY_HOOK published (Ok, direct). The plans' refusals are real and fire when HEAD moves; none of them sees this route. Previously graded VERIFIED or closed. That is new evidence, not a code regression: the plain-id refspec dates from plan 02-11."
    artifacts:
      - path: "src/sync/mod.rs"
        issue: "Same refspecs, lines 2054 and 2182"
    missing:
      - "Closed by the push-source fix; add a shadow-ref case to the existing hook tests for at least the duplicate tree"
  - truth: "A hook cannot add an unbound name to the published tree (derived in the previous report from review WR-200; 02-13's staged-tree binding was meant to close it)"
    status: failed
    reason: "CR-301 published `templates/leak.txt` beside the validated file, in direct and PR mode. The staged-tree binding holds, because the staged tree itself is built on the injected base. CR-300 publishes any tree the hook builds."
    artifacts:
      - path: "src/sync/mod.rs"
        issue: "The binding and the allowlist compare against `base`, and nothing checks base against the remote"
    missing:
      - "Closed by the two fixes above"
  - truth: "02-13 prohibition MUST NOT report success for a push whose commit lacks validated content: a push returning Ok means every validated path holds its recorded content in the published commit"
    status: failed
    reason: "Three routes, all reproduced. WR-300: a post-index-change hook that sets assume-unchanged during ensure_remote_repo empties `git status`; push_to_remote returns Ok(\"No changes...\") before any check, the remote stays at the seed, and the CLI exits 0 and advances the ancestor state. WR-301: a post-commit hook writing remote.origin.pushurl sends the commit to another repository; origin never moves, Ok in both modes, CLI exit 0, ancestor advances. CR-300 with a rebuilt delete or revert: Ok, the published tree has no template, or the old blob. Controls: no flag and no pushurl publish normally. With the hook installed after ensure_remote_repo, as the fixture does, this run got `git commit failed` (fail closed), so the flag has to be set before `git status` runs; installing the hook before ensure_remote_repo gives that, and a global core.hooksPath hook gave it through the CLI."
    artifacts:
      - path: "src/sync/mod.rs"
        issue: "stage_publication returns Ok(None) on empty status (lines 1812-1814) without calling require_validated_content; both pushes name `origin` (lines 2065, 2194), which resolves through hook-writable config"
      - path: "src/main.rs"
        issue: "Direct mode records ancestors on any Ok (lines 1266-1283)"
    missing:
      - "On empty status with a non-empty record, read HEAD's tree and run require_validated_content; confirm deleted paths are gone"
      - "Push to the caller's URL rather than `origin`, then confirm with `git ls-remote <url>` that the ref is at the commit before returning Ok"
      - "Hook tests for both; repository-level url.*.pushInsteadOf needs a test or a refusal"
  - truth: "02-13 prohibition MUST NOT print a name or line a hook chose in the committed gate's refusals"
    status: partial
    reason: "Review WR-302, reproduced in both modes and through the CLI. A hook-built commit with a later `parent` line reaches the continuity refusal, which prints the raw bytes. CLI stderr contained a raw ESC byte. A token-shaped value there is printed unredacted, because that check runs before the header scan. Nothing reaches the remote; the exposure is the terminal and any log. The prohibition's enumerated list omits this refusal, its lead clause does not."
    artifacts:
      - path: "src/sync/mod.rs"
        issue: "raw_commit_parents (line 2630) keeps raw text; the refusal at lines 2376-2379 prints it"
      - path: "src/main.rs"
        issue: "Prints the error unescaped (line 1306)"
    missing:
      - "Drop `{parent}` from the message and name only the fetched head; add a test that the hook's text is absent from the refusal"
---

# Phase 2: Sanitizer, Boundaries & Security Hardening Verification Report

**Phase Goal:** Screen and redact all confidential tokens, raw credentials, and user-specific paths before any remote git staging or push.
**Verified:** 2026-11-02T19:44:32Z
**Status:** gaps_found
**Re-verification:** Yes, after gap closure (plans 02-13, 02-14, 02-15)

## Bottom line

The phase's main promise is still false. Fake token-shaped bytes reached the remote through UDE's own
push, through the real CLI, with exit 0 and no refusal; a consumer's pull then wrote them into a project.
All five review findings reproduced here. No recorded decision covers them.

**What argues against this verdict.** Every failure needs code running in the cache checkout: a hook there
or a global `core.hooksPath`, running with the user's privileges. Such code could run `git push` itself.
With no hook, the same token and a high-entropy value are refused (exit 1, remote unchanged) and a benign
edit publishes. So ordinary use behaves as designed; the failure is the gate's claim against hooks, which
this phase took on as scope.

What did close: the grafts-after-the-gate gap, and the HEAD-moving route to duplicate entries, deletes,
reverts and `.` entries. 02-13's binding works on the commit it inspects. CR-300 and CR-301 go around it,
so the same bad trees and blobs still publish by that other route.

Green checks pass again next to these bypasses: full suite, all 23 controls (`same`), 83 caught and 0
missed. They exercise the commit the gate reads, and none of them can see which commit the push sends.

## Goal Achievement

### Ruling on each review finding

| Finding | Reproduced here? | Ruling | What was observed |
|---------|------------------|--------|-------------------|
| CR-300, ref named after the commit id | **Yes.** Engine: tag in direct (4 runs) and PR; `refs/heads/<id>` both modes. Added: `refs/remotes/<id>` direct. CLI: direct and PR | **FAILED**, blocks | Ok; remote tip is the injected commit; token in the managed file on the remote. CLI exit 0, ancestors advance in direct mode; consumer's pull writes the token to mise.toml. PR: token on the `ude-sync-*` branch, main untouched. `refs/<id>` did not shadow (published the validated commit) |
| CR-301, hook moves the fetched head | **Yes.** Engine direct and PR. CLI direct and PR, cached path; CLI direct on the clone path too (the review only reasoned this one) | **FAILED**, blocks | `fetched_sha` differs from the real head; Ok; `templates/leak.txt` and the injected blob on the remote. Clone-path hook also runs `git reset --hard` to the injected commit |
| WR-300, index flag, Ok with nothing published | **Yes.** Engine direct and PR, CLI direct. Hook installed before the second `ensure_remote_repo`. Installed after ensure, as the fixture does, this run got `git commit failed` instead, so the review's exact form was not reproduced here | **FAILED**, blocks 02-13's success prohibition | `Ok("No changes to push...")`, remote at the seed, probe edit absent; CLI exit 0 and the ancestor state changed |
| WR-301, hook-written pushurl | **Yes.** Engine direct and PR (the review only reasoned PR), CLI direct | **FAILED**, blocks the same prohibition | Ok; origin stays at the fetched head; the other repository holds the commit. CLI exit 0, ancestors advance |
| WR-302, parent bytes printed | **Yes.** Engine direct and PR, CLI direct | **PARTIAL**, nothing reaches a remote | CLI stderr has a raw ESC byte; a token-shaped value prints unredacted. `git hash-object --literally` built the commit |
| IN-300, IN-301 | Not run; the reviewer also reasoned them | Not goal gaps | Local branch residue, and a test helper's HOME |

### Does AR-02-10 cover CR-300 or CR-301? No.

AR-02-10's text, in 02-SECURITY.md, threat ref T-02-52: "A hostile pre-push hook can already push anything
to the same remote with the user's credentials, so this gives it no new capability; `push --no-verify` would
disable every user's pre-push hooks. Reproduced on git 2.56.0 at plan time. The 02-08 prohibition carries
this as its explicit exception".

T-02-52's channel in that file: "a pre-push hook writing a grafts file during `git push`, after the gate".
CR-300 is a post-commit hook creating a ref. CR-301 is a post-checkout hook moving a tracking ref. Neither is
a pre-push hook and neither writes grafts, so the acceptance does not reach them. 02-CONTEXT.md records no
hook decision; its only hook mention is deferred template work.

02-14's prohibition also removes the exception: "With plans 02-13 and 02-14 the T-02-52 exception for a grafts
file written by a pre-push hook is removed, because both tests above hold". Its scope sentence puts these two
findings in scope: "this rule is about what UDE's own push sends. A hostile hook can still run its own `git
push`". CR-300 and CR-301 are UDE's own push.

AR-02-10's *reasoning* does extend to them, as the review says. Applying it would be a new operator decision
narrowing what the gate promises, not something already decided.

The hand edit (T-02-52 to `mitigate` naming T-02-62, AR-02-10 superseded) is still owed. 02-15-PLAN makes it
conditional on re-verification passing, so it waits. On grafts alone the evidence supports it.

### Roadmap Success Criteria

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| SC1 | High-entropy strings and raw Base64 tokens without prefixes flagged before staging | VERIFIED | CLI, no hook: refused `high-entropy at line 26`, exit 1, remote unchanged. Named tests in the full suite. `git diff --stat 07ff152 HEAD -- src tests docs` lists no sanitizer file |
| SC2 | Outbound commit inspection catches gitleaks categories across committed blobs and rolls back | **FAILED** | CR-300, CR-301: token shape on the remote, Ok, no rollback. Holds for commits the gate inspects (`GitHub Personal Access Token at line 26` with no hook) |
| SC3 | Path parameterization matches only full word and punctuation boundaries | VERIFIED | Regression: full suite; sanitizer unchanged since 07ff152 |
| SC4 | Templated tokens substitute back cleanly on initialization | VERIFIED | Regression: full suite; template code unchanged since 07ff152. CLI control: a consumer's pull exits 0 with no token, which does not itself prove the round trip |

### Plan must-have groups

| Plan | Truths (summary) | Status | Evidence |
|------|------------------|--------|----------|
| 02-01..02-10 | As in the previous report | VERIFIED (10) | Regression: full suite passes; their code outside src/sync/mod.rs and src/process.rs unchanged since 07ff152 |
| 02-11 | Bound to the fetched head; raw reads; validated id pushed; no tags | **FAILED** | "every blob the push adds is allowlist-checked, secret-scanned and compared" is false under CR-300, CR-301. "Validated id pushed" is true of the refspec's text and false of what git sends |
| 02-12 | Message bound; header scanned; empty directories refused; raw reads; grafts refused at the gate; graphs and bitmaps disabled | VERIFIED as written | About the gate's reads and push options; named tests pass. Its prohibition on commit text fails, see below |
| 02-13 | Staged-tree binding; blobs by id; repeated-path, component and held-content checks; name-free refusals; legitimate states publish | **FAILED** | Gate behaviour holds: HEAD-moving hook refused in both modes for duplicate, delete, revert, `.`; benign amend publishes; code read at :2445-2491, :2521-2567. False: gap-1 "no blob reaches the remote unread", gap-4 "a push returning Ok means every validated path holds its recorded content" |
| 02-14 | Both pushes use the null grafts file; narrow carve-out; hint off; upstream blamed only when ls-remote disagrees | VERIFIED | Code read: process.rs:135-144, `.run_push` at both pushes. test_direct and test_pr `_sends_no_object_from_pre_push_grafts` and `_from_grafts_written_during_push`, each with a delivering negative control read in `assert_no_injected_blob_with_control`. CLI shim tests pass in both modes |
| 02-15 | Controls, mutation, gates, hermeticity, no production change, trigger for hand edits | VERIFIED | Stored results read, not re-run: 23 rows `same` in target/controls-02-15.tsv; outcomes.json 96 total, 83 caught, 0 missed, 0 timeout, 13 unviable; no .cargo/mutants.toml. Gates re-run below |

### Prohibitions

| Plan | Prohibition | Status | Evidence |
|------|-------------|--------|----------|
| 02-01..02-10, except the row below (18) | As in the previous report | VERIFIED | Regression: full suite |
| 02-08, restated by 02-12, 02-13, 02-14 | Never publish a blob differing from the validated bytes | **FAILED** | CR-300, CR-301 |
| 02-11 | Never publish a commit reachable from the published ref carrying a differing blob | **FAILED** | CR-300 tip; CR-301 base |
| 02-11 | Never push a commit other than the validated one | **FAILED** (was VERIFIED) | CR-300 |
| 02-11 | Never read committed objects through replacement refs | VERIFIED | Named tests; `--no-replace-objects` at each read |
| 02-11 | No ref beyond the refspec's branch update | VERIFIED | Named tests; `--no-follow-tags` at both pushes. A shadow tag is a local ref and did not itself publish |
| 02-11 | Never refuse only because a hook ran | VERIFIED | Benign controls here: kept amend, no-op hooks |
| 02-12 | Never publish hook-written commit text | **FAILED** (was VERIFIED) | CR-300 with the plan's amend: remote message holds INJECTED_BY_HOOK, Ok |
| 02-12 | Never refuse because a hook ran or git maintains the object store | VERIFIED | Named tests |
| 02-13 | Never report success when the commit lacks validated content | **FAILED** | WR-300, WR-301; CR-300 with delete or revert |
| 02-13 | Never publish a tree another machine cannot clone or read unambiguously | **FAILED** | CR-300 with a duplicate tree (direct, PR) and a `.` tree (`git clone` exit 128) |
| 02-13 | Never refuse only because a hook ran | VERIFIED | Benign controls |
| 02-13 | Never print a name or line a hook chose in the gate's refusals | **FAILED** (partial) | WR-302 |
| 02-14 | Inherited GIT_GRAFT_FILE never reaches git; only the pushes get the null device | VERIFIED | test_cli_direct and test_cli_pr `_push_alone_gets_the_null_grafts_file` pass |
| 02-14 | Never refuse only because a hook ran | VERIFIED | Named tests |
| 02-15 | Control rows recorded honestly | VERIFIED | The stored tsv, 23 `same`, 0 `corrected`; not re-run |
| 02-15 | No missed mutant killed by exclusion | VERIFIED | No .cargo/mutants.toml; 0 missed |

All are test-tier, wired to tests. The seven FAILED rows have tests that pass and are contradicted by the
probes. Two rows graded VERIFIED last time are FAILED now on new evidence, not a code regression.

### Derived in the previous report, re-checked

| Truth | Status | Evidence |
|-------|--------|----------|
| A push that returns Ok has published the validated content | **FAILED** | WR-300, WR-301, CR-300 with delete or revert. The original delete and revert route is closed |
| A hook cannot add an unbound name to the published tree | **FAILED** | CR-301 `templates/leak.txt`; CR-300. The original `.` route is closed |

**Score:** 43/55 truths verified (SC 3/4; plan groups 13/15; prohibitions 27/34; derived 0/2). 0 present but
behavior-unverified.

### Deferred Items

None. Phases 3 (doctor, CLI) and 4 (templates, manifests) do not cover the push path.

### Advisory (New Scope, Unevidenced)

None. Every finding here has a reproduced run, and src/sync/mod.rs changed since the previous report.

### Required Artifacts

| Artifact | Status | Details |
|----------|--------|---------|
| 02-13: `StagedPublication`, `record_staged_tree`, `raw_commit_tree`, `raw_diff_entries`, `read_blob_by_id`, `tree_listing_blobs`, hidden-blob test | VERIFIED | `verify.artifacts` 7/7, exit 0; read and exercised |
| 02-14: `run_bounded_ignoring_grafts`, `run_push`, hint option, tests, `remote_branch_is_at` | VERIFIED | `verify.artifacts` 6/6, exit 0 |
| 02-15: SUMMARY, hand-edits todo, third | VERIFIED | `verify.artifacts` 3/3, exit 0 |

### Key Link Verification

`verify.key-links`: 4/4, 4/4, 3/3. Patterns also counted by hand with `rg`.

| From | To | Status | Details |
|------|----|--------|---------|
| stage_publication | record_staged_tree | WIRED | 1 site |
| validate_committed_tree | raw_commit_tree, read_blob_by_id, tree_listing_blobs | WIRED | 1 site each |
| push_direct | remote_branch_is_at | WIRED | 1 site |
| run_push | process::run_bounded_ignoring_grafts | WIRED | 1 site |
| both pushes | run_push | WIRED | 2 sites |
| run_bounded_ignoring_grafts | `GIT_GRAFT_FILE` = NULL_DEVICE | WIRED | 1 site |
| **refspec source** | **the validated commit** | **BROKEN** | Not a plan key link. Text names the id; git resolves it against local refs first (CR-300) |
| **fetched head** | **the remote's head** | **NOT WIRED** | Not a plan key link. Nothing compares them (CR-301) |

### Data-Flow Trace (Level 4)

Not applicable: no rendered dynamic data. The equivalent trace, the bytes from the gate to the remote, is the
ruling table above.

### Behavioral Spot-Checks

Scratch copy of HEAD, git 2.56.0, local bare remotes, probe tests added to `mod tests` and
`tests/e2e_lifecycle.rs`. Every case records that its hook ran. The token is built at runtime in the hook and
is not reproduced in this report.

| Case | Mode | Result | Injected blob on remote |
|------|------|--------|-------------------------|
| Scanner on the probe token (control) | n/a | flagged | n/a |
| Injected commit, no shadow ref (control) | direct, PR | Ok | no |
| Injected commit, HEAD moved to it (control) | direct | refused, continuity | no |
| Injected commit, `refs/tags/<id>` | direct, PR | **Ok** | **yes** |
| Same, `refs/heads/<id>` | direct, PR | **Ok** | **yes** |
| Same, `refs/remotes/<id>` | direct | **Ok** | **yes** |
| Same, `refs/<id>` | direct | Ok | no |
| Post-checkout hook moves no ref (control) | direct, PR | Ok; fetched = real | no |
| Post-checkout hook moves origin/main | direct, PR | **Ok**; fetched differs | **yes**, plus `templates/leak.txt` |
| Duplicate tree, HEAD moved | direct, PR | refused, `differs from the validated content` | no |
| Delete, revert, HEAD moved | direct, PR | refused, `does not hold the validated content` | n/a |
| `.` tree, HEAD moved | direct, PR | refused, path component | n/a |
| Kept-content amend (control) | direct | Ok | n/a |
| Duplicate tree through the shadow tag | direct, PR | **Ok** | **yes** |
| `.` tree through the shadow tag | direct | **Ok**; `git clone` exit 128 | n/a |
| Delete through the shadow tag | direct | **Ok**; published tree empty | n/a |
| Revert through the shadow tag | direct | **Ok**; old blob published | n/a |
| Amended message through the shadow tag | direct | **Ok**; INJECTED_BY_HOOK in the message | n/a |
| assume-unchanged set during ensure | direct, PR | **`Ok("No changes...")`**, remote at seed | n/a |
| Same hook without the flag (control) | direct | Ok, remote moved | n/a |
| assume-unchanged set after ensure | direct, PR | refused, `git commit failed` | n/a |
| Hook-written pushurl | direct, PR | **Ok**; origin at fetched, other repository holds it | n/a |
| No pushurl (control) | direct | Ok; origin moved, other empty | n/a |
| Parent line with ESC, and with a token shape | direct, PR (ESC), direct (token) | refused, **bytes printed raw** | no |
| Exec bit set before the id is recorded (T-02-60, not a gap) | direct | Ok, published mode 100755 | n/a |

`ude sync push`, direct unless noted:

| Case | Exit | Ancestor state | Remote | Consumer's pull |
|------|------|----------------|--------|-----------------|
| No hook, token line | 1 | n/a | unchanged | n/a |
| No hook, high-entropy line | 1 | n/a | unchanged | n/a |
| No hook, benign line (control) | 0 | n/a | moved | n/a |
| Post-commit hook, no shadow ref (control) | 0 | changed | no token | no token |
| Post-commit hook, shadow tag | 0 | changed | **token** | **token in mise.toml** |
| Same, `--pr` | 0 | unchanged | **token on the sync branch** | not run |
| Post-checkout hook, cached path; same with `--pr` | 0, 0 | changed; unchanged | **token** both | not run |
| Post-checkout hook, clone path | 0 | changed | **token** | not run |
| assume-unchanged hook | 0 | **changed** | at seed, no edit | not run |
| No-op index hook (control) | 0 | changed | moved | not run |
| pushurl hook | 0 | **changed** | commit in the other repository | not run |
| Parent-line hook | 2 | unchanged | unchanged; raw ESC on stderr | n/a |

Gates, one run each in a clean scratch copy of HEAD (952c889), sources touched so everything rebuilt, no probes present:

| Gate | Result |
|------|--------|
| `cargo fmt --check` | exit 0 |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 |
| `cargo test` | exit 0; 333 unit passed, 1 ignored; 90 e2e passed |
| `cargo deny check advisories bans sources` | exit 0 |
| `cargo machete` | exit 0 |
| Decoy GIT_DIR, `cargo test --bin ude sync::` | exit 0, 252 passed; decoy 0 objects, 0 refs |

Not re-run here: the 23 controls (0 `corrected` rows, so 02-15's rule requires none) and the mutation run. Stored results read.

### Probe Execution

No `scripts/*/tests/probe-*.sh` is tracked or declared by plans 02-13 to 02-15. Step 7c: not applicable.

### Requirements Coverage

REQUIREMENTS.md's checkboxes and status table were not counted as evidence.

| Requirement | Source Plans | Status | Evidence |
|-------------|--------------|--------|----------|
| SEC-01 entropy and raw Base64 | 02-03, 02-04, 02-06 | SATISFIED | SC1 |
| SEC-02 gitleaks categories before staging | 02-01, 02-05, 02-06, 02-13, 02-14, 02-15 | PARTIAL | The requirement's text holds before staging: the token is refused with no hook. Plans 02-13 to 02-15 also file SC2's committed-blob truth here, and that fails |
| SEC-03 strict boundaries | 02-07, 02-09 | SATISFIED | SC3 |
| SEC-04 placeholder reversibility | 02-02, 02-07 to 02-15 | PARTIAL | SC4 satisfied; the binding prohibitions filed under SEC-04 fail |

All four IDs are claimed by plans and exist in REQUIREMENTS.md; none is orphaned. The file does not match this
report: it marks SEC-02 and SEC-04 Complete and SEC-01 and SEC-03 Gaps Found. Not edited here.

### Decision Coverage

`check.decision-coverage-verify`, exit 0: 5 of 5 trackable CONTEXT.md decisions honored. A substring heuristic;
it says nothing about the gaps.

### Test Quality Audit

| Test File | Linked Req | Skipped | Circular | Assertion Level | Verdict |
|-----------|-----------|---------|----------|-----------------|---------|
| src/sync/mod.rs `mod tests` | SEC-02, SEC-04 | 0 | no | Behavioural: real push to a bare remote, object presence checked | Sound for what each names; no test has a ref named after the id or a hook during `ensure_remote_repo` |
| tests/e2e_lifecycle.rs | SEC-02, SEC-04 | 0 | no | Behavioural, CLI | Same |
| src/sanitizer/gitleaks.rs | SEC-02 | 1 `#[ignore]`, writes the parity corpus, not a requirement's only test | no | Value | Not a gap |

Disabled tests on requirements: 0. Circular patterns: 0. Insufficient assertions: 0.

### Anti-Patterns Found

No `TBD`, `FIXME`, `XXX`, `TODO`, `HACK`, `todo!` or `unimplemented!` in src/sync/mod.rs or src/process.rs (negative
control: a planted `FIXME` line counts 1).

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| src/sync/mod.rs | 2112 | Doc: "never a ref a hook can move" | Blocker, with gap 1 | False: CR-300 |
| src/sync/mod.rs | 2035 | Comment: "The validated id, never the branch name" | Warning | True of the text, misleading about what goes out |
| src/sync/mod.rs | 2437-2438 | Comment: refusals never print a listed name | Info | True as worded; the parent refusal is outside it, WR-302 |
| 02-13-PLAN.md | objective | "whatever ran in the cache checkout" | Warning | The plan's purpose overstates what the gate delivers |

### Human Verification Required

None. What is left is a fix or an operator decision, not a check a person must perform.

### Open operator items (not gaps)

Each is recorded as "pending the operator, not accepted" in 02-15-PLAN `## Operator answers`. None is in
02-SECURITY.md's register yet, since `/gsd-secure-phase` has not run since.

- T-02-60: a file mode set before the tree id is recorded publishes. Reproduced: Ok, mode 100755, blob exact.
- T-02-61: the binding refuses some legitimate-looking states. Not re-run.
- T-02-64, T-02-65: replace refs, shallow, loose-object overwrite during the push. Not re-run.
- T-02-67: `NUL` on Windows. Unreachable here.

### Gaps Summary

Six gaps, four causes.

1. **The push source is not the validated commit (gaps 1, 2, 3, 4).** A local ref can stand in for the id.
   This also re-opens every tree and blob route 02-13 closed, so it is the one to fix first. The review ran its
   fix in a scratch copy; it was not re-run here.
2. **The fetched head is read from a local ref (gaps 1, 2, 4).** The review's fix is unrun by anyone and leaves
   global `url.*.insteadOf` open.
3. **Ok does not depend on the remote (gap 5).** An early return skips the record check, and `origin` is
   hook-writable.
4. **A refusal prints hook-chosen bytes (gap 6).** A one-line fix.

Decision for the operator on causes 1 and 2: fix them, or apply AR-02-10's reasoning and narrow what the gate
promises, changing the doc comments and prohibitions with it. Fixing is recommended for cause 1: small, reviewed
in scratch, and it restores what 02-13 built. Cause 2 costs a network read per push and has a known residual.
An override is not suggested unless the operator chooses to narrow.

### What these checks do not establish

- Local bare remotes, git 2.56.0, Linux. **No hosted remote, no https or ssh, no Windows: unreachable here.** A
  host's secret scanning might refuse a `ghp_` token; a shape it does not know would pass, reasoned and not run.
  A bare remote with `receive.fsckObjects=true` refused the duplicate tree last round; that does not bear on
  CR-300 with a well-formed tree, or on CR-301.
- CR-300 and CR-301 are deterministic in these runs, one to four runs per case. This says the route works here,
  not what fraction of hostile hooks would use it.
- The channels here are the ones tried, not a proven complete list. Reasoned and not run: a hook-written
  `remote.origin.fetch`, a `reference-transaction` hook on the CR-301 route, repository-level
  `url.*.pushInsteadOf`, a clean filter on these routes.
- The free-running grafts race (5 of 60 last round) was not re-run. 02-14's tests force the order, which is
  stronger for the channel and says nothing of rate.
- No proposed fix was applied or tested in this run.
- WR-300, WR-301, WR-302 CLI runs are direct only; PR for those is at engine level. Consumer pulls ran for CR-300
  only.
- Passing gates and stored mutation and control results show the gate's checks are load-bearing on what they
  read. They do not show the push sends what was read.

---

_Verified: 2026-11-02T19:44:32Z_
_Verifier: Claude (gsd-verifier)_
