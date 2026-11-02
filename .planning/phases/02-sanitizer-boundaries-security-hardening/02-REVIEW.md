---
phase: 02-sanitizer-boundaries-security-hardening
reviewed: 2026-11-02T19:29:42Z
depth: deep
files_reviewed: 3
files_reviewed_list:
  - src/sync/mod.rs
  - src/process.rs
  - tests/e2e_lifecycle.rs
findings:
  critical: 2
  warning: 3
  info: 2
  total: 7
status: issues_found
---

# Phase 02: Code Review Report

**Reviewed:** 2026-11-02T19:29:42Z
**Depth:** deep
**Files Reviewed:** 3
**Status:** issues_found

## Narrative Findings (AI reviewer)

## Summary

Scope: `git diff 07ff152 HEAD -- src tests` (plans 02-13 to 02-15), at HEAD efc4117, with call chains
traced through `src/sanitizer/mod.rs` and `src/main.rs`.

**The gate's main claim is still false.** Two separate hooks published a blob holding a fake `ghp_`
token through the real `push_to_remote`, which returned Ok in direct and PR mode. Both blobs were on
the remote afterwards.
- CR-300: a ref named after the commit id makes `git push` send that ref's commit, not the commit the
  gate validated.
- CR-301: the "fetched head" is read from a local ref a hook can move, so an injected commit passes
  as the base and goes out underneath the validated one.

**What argues against these two findings:** both need a hostile hook, and a hostile hook can run
`git push` itself with the user's credentials. AR-02-10 used that argument to accept T-02-52. They are
still reported as critical because the code documents the opposite (`src/sync/mod.rs:2112` "never a
ref a hook can move"), phase 02 treats hook rewrites of refs as in scope, and each is a bypass that
returns Ok. If the operator applies AR-02-10's reasoning to them, that is a decision to narrow what
the gate promises, and the doc comments must change with it.

How the claims were tested: each `reproduced` below comes from a run in a scratch copy of HEAD, with
probe tests added to `mod tests` and driving the real `ensure_remote_repo` and `push_to_remote`
against local bare remotes on git 2.56.0, using the existing `push_with_post_commit_hook` fixture or
hooks installed the same way. The scratch copy has been removed.

What those runs do not show:
- No hosted remote and no https or ssh transport was tried, so whether a host's own checks would
  stop a blob is unverified. A host's secret scanning might refuse these tokens; a token shape it
  does not know would pass.
- `ude sync push` was not run end to end for any finding. The effect on `src/main.rs` is from reading
  the code.
- `GIT_GRAFT_FILE` reaching `pack-objects` under `git-remote-https` was not run. It is reasoned from
  environment inheritance only. The local-path case is what the existing tests cover.
- The list of channels below is what was tried, not a proof that no other exists.
- The full test suite was not run. 127 tests matching `push` ran only against the CR-300 fix (see
  there), not against unmodified HEAD.

### Standing findings

CR-200, CR-201, WR-200 and the other findings in `02-REVIEW-DISPOSITION.md` were not re-run. Reading
the code, the repeated-path, component and record checks are present as plans 02-13 describe. Their
disposition is the orchestrator's to record. IN-200 to IN-205 were not re-examined and are not
repeated here. T-02-45 (the commit header is scanned, not bound) is a recorded risk and is not
re-reported.

### Checked and found sound

Read from the code and from git's behaviour as recalled, not run, unless marked:
- `raw_commit_parents` is a superset of git's parser. A `parent` line after `author` counts for the
  gate but not for git; the tree binding still holds the content, so no new bytes go out.
- A tree entry with a non-canonical mode is canonicalized the same way by `ls-tree` and the object
  walk, and an unknown mode reads as a gitlink in both, so neither follows it.
- `ls-tree` fails on a tag or commit listed as a tree, so that form is refused.
- A loose object or pack rewritten after validation hashes to a different id on the remote, whose
  connectivity check should reject it.
- `Finding::reason` prints the rule name and line, never the value (read in `src/sanitizer/mod.rs:25`).
- A forced replace ref to a tag does not divert a `^{commit}` source (plain-git run: push refused
  without `--no-replace-objects`, validated commit sent with it).

## Critical Issues

### CR-300: A ref named after the commit id makes git push send another commit

**File:** `src/sync/mod.rs:2054` (PR), `src/sync/mod.rs:2182` (direct); false doc claims at `:2035` and `:2112`
**Reproduced:** yes, direct and PR mode.
**Issue:** `git push` resolves a refspec's source against local ref names first and treats it as an
object name only when no ref matches, so `refs/tags/<id>` or `refs/heads/<id>` takes precedence over
the id `<id>`. Both pushes use `{commit}:refs/heads/...`. A post-commit hook that creates such a ref
leaves HEAD, the tree, the message and the parent as validated, so every gate check passes. The ref
is not inspected, and it can be created after the gate has run.

Hook tail, in the existing fixture:
```sh
C=$(git rev-parse HEAD)
I=$(printf '<fake ghp_ token line>\n' | git hash-object -w --stdin)
export GIT_INDEX_FILE=.git/ude-probe-idx
git read-tree HEAD
git update-index --add --cacheinfo 100644,"$I","$F"
T=$(git write-tree)
unset GIT_INDEX_FILE
N=$(git commit-tree "$T" -p "$C" -m injected)
git update-ref "refs/tags/$C" "$N"
```
Output:
```
direct: res=Ok("Directly pushed changes to remote branch 'main'.")
        refs/heads/main managed file: <the fake token line>
PR:     res=Ok("Pushed changes to branch 'ude-sync-14f11ae4-...'. Create a PR on GitHub to merge into 'main'.")
        refs/heads/ude-sync-... managed file: <the fake token line>
```
Plain-git porcelain output for the same refspec shows the substitution:
`refs/tags/e059...:refs/heads/main fd344ce..7a7f829`. Control: with no such ref, the same plain-git
command leaves the remote at the validated commit and without the injected blob.

**Fix:** Make the source something no ref can be named, and read it past replace refs:

```rust
let refspec = format!("{commit}^{{commit}}:refs/heads/{branch}"); // same for sync_branch
push.args(["--no-replace-objects", "-c", "core.commitGraph=false", /* as today */]);
```
Applied in the scratch copy to both pushes: the shadow probes then found no injected blob on the
remote, in either mode. 126 of 127 tests matching `push` passed. The failure is
`test_direct_push_replaced_commit_refusal_does_not_say_upstream_moved`, which expects a replace ref
to get the push refused; with the flag the validated commit publishes, which is the safer outcome,
but that test and the comment at `:2207` need rewriting. Add hook tests in both modes that assert the
injected blob is absent from the remote, with a control that a plain-id refspec delivers it.

### CR-301: A hook can move the fetched head, and the injected commit goes out as the base

**File:** `src/sync/mod.rs:1092-1108` (`fetched_head`), `src/sync/mod.rs:1036-1064` (hooks run before it is read), `src/sync/mod.rs:2374-2375` (the parent check trusts it)
**Reproduced:** yes, direct and PR mode.
**Issue:** `fetched_sha` is whatever `refs/remotes/origin/<branch>` holds when `ensure_remote_repo`
ends, after `git checkout -B` has run the post-checkout hook. Nothing compares it with the remote. A
hook that moves that ref onto an injected commit makes `reset --hard` and `fetched_head` both land
there. UDE's commit then sits on the injected commit, its only parent equals the "fetched head", and
`diff-tree base commit` lists only the validated file. The push is a real fast-forward and carries
the injected commit and every object in it. No check reads what lies between the remote's head and
`base`, so the gate's "only commit the push adds on top of `base`" holds while it does not describe
what the push adds to the remote.

Hook, installed after a first `ensure_remote_repo` and run by the second:
```sh
I=$(printf '<fake ghp_ token line>\n' | git hash-object -w --stdin)
export GIT_INDEX_FILE=.git/ude-probe-idx
git read-tree refs/remotes/origin/main
git update-index --add --cacheinfo 100644,"$I",templates/leak.txt
T=$(git write-tree)
unset GIT_INDEX_FILE
N=$(git commit-tree "$T" -p refs/remotes/origin/main -m injected-base)
git update-ref refs/remotes/origin/main "$N"
```
Output (the first `ensure_remote_repo` in the same run reported the real head, which is the control):
```
real head c8f6f47..., fetched_sha reported 162d7f7...
direct: res=Ok("Directly pushed changes to remote branch 'main'.")
        refs/heads/main tree: templates/leak.txt, templates/projects/generic/gitconfig.repo
PR:     res=Ok("Pushed changes to branch 'ude-sync-14f11ae4-...'. ...")
        refs/heads/ude-sync-... tree: templates/leak.txt, templates/projects/generic/gitconfig.repo
```
A global `core.hooksPath` hook should do the same on the clone path, and a hook-written
`remote.origin.fetch` should reach the same ref. Both are reasoned, not run.

**Fix:** Take the base from the remote, not from a ref in the checkout. Before the fetch, run
`git ls-remote <remote_url> refs/heads/<branch>` with the caller's URL, from a directory outside the
checkout so hook-written config cannot redirect it. After the fetch, refuse when `fetched_head`
differs, with the "pull first" wording if the remote simply moved. This fix was not run. Note what it
leaves open: `url.*.insteadOf` in global config still applies. `-c core.hooksPath=/dev/null` is not
enough alone, since 02-11-PLAN records that config-defined hooks still run under it. Add tests in both
modes that assert `templates/leak.txt` and its blob are absent from the remote.

## Warnings

### WR-300: An index flag makes the push return Ok with nothing published

**File:** `src/sync/mod.rs:1812-1814`, `src/main.rs:1266-1295`
**Reproduced:** yes for `push_to_remote` (direct mode). The state write in `main.rs` is reasoned.
**Issue:** `git status --porcelain` printing nothing returns `Ok(None)` before any check of the
record, and `require_validated_content` never runs. `require_validated_content`'s doc says a push
returning Ok has published each recorded path. A post-index-change hook that sets `assume-unchanged`
on the managed file empties `status`:
```
hook_ran=true res=Ok("No changes to push to remote repository.")
before=c8f6f47... remote heads: [("refs/heads/main", "c8f6f47...")]
control (no index hook): res=Ok("Directly pushed changes to remote branch 'main'."), head moved to 8d32ea7...
```
In direct mode `main.rs` then records the validated hash as the new ancestor for each file, so local
state claims a publish that did not happen, and later syncs compare against it.

**Fix:** On an empty status, when `validated` is not empty, read the tree of `HEAD` and run
`require_validated_content` against it. Identical content still returns "No changes"; anything else
is refused. Also confirm no path in `deleted` is still in that tree.

### WR-301: A hook-written pushurl sends the commit elsewhere and the push returns Ok

**File:** `src/sync/mod.rs:2065`, `src/sync/mod.rs:2194`, `src/main.rs:1266-1295`
**Reproduced:** yes for `push_to_remote` (direct mode). The state write in `main.rs` is reasoned.
**Issue:** Both pushes name `origin`, so they use the checkout's config, which a hook can write. The
`remote_branch_is_at` doc already says this config cannot be trusted, but Ok depends on it. A
post-commit hook running `git config remote.origin.pushurl <other bare>`:
```
hook_ran=true res=Ok("Directly pushed changes to remote branch 'main'.")
origin heads: [("refs/heads/main", "1a5704b...")] fetched=1a5704b...
other heads: [("refs/heads/main", "134dc1c...")]
```
Origin never moved, and the validated commit plus the objects under it went to the other repository.
Direct mode still advances the ancestor state. PR mode should behave the same; that is reasoned.

**Fix:** Pass the URL from `ensure_remote_repo` through `RemoteCache` and push to it rather than to
`origin`, then confirm with `git ls-remote <url> refs/heads/<target>` that the ref is at `commit`
before returning Ok. Repository-level `url.*.pushInsteadOf` still needs a test or a refusal. Neither
part was run.

### WR-302: A refusal prints hook-chosen bytes from a `parent` header, including terminal escapes

**File:** `src/sync/mod.rs:2376-2379`, `src/sync/mod.rs:2630-2637`
**Reproduced:** yes, direct mode.
**Issue:** `raw_commit_parents` returns the raw text after `parent `, and the continuity refusal prints
it. Git's parser reads only `parent` lines directly after `tree`, so a later `parent` line resolves
fine and reaches this message. The text can hold any bytes except a newline, and
`src/main.rs:1306` prints it to the terminal unescaped. The comment at `:2437` says hook-chosen bytes
are not printed, and plan 02-12 says the same of the hook's text. A hook holding a secret could write
it here, or send terminal control sequences:
```
hook_ran=true res=Err(Failed("Branch continuity check failed: cache commit parent HOOK_CHOSEN_TEXT_\u{1b}[31m is not the fetched head c8f6f47..."))
```
Nothing reaches the remote through this. The exposure is the terminal and any log capturing it.

**Fix:** Drop `{parent}` from the message, and name only the fetched head. Add a test that asserts the
hook's text is absent from the refusal.

## Info

### IN-300: Some push failure paths leave the cache commit or the sync branch behind

**File:** `src/sync/mod.rs:2069-2071`, `src/sync/mod.rs:2076-2081`, `src/sync/mod.rs:2198-2200`, `src/sync/mod.rs:2220-2223`
**Reasoned.**
**Issue:** A push timeout or a failure other than non-fast-forward returns without `rollback_commit`
or `abandon_sync_branch`. On a timeout, PR mode also stays on the sync branch. The next
`ensure_remote_repo` runs `checkout -B` and `reset --hard`, so nothing wrong is published, but local
`ude-sync-*` branches accumulate and hold their commits.
**Fix:** Run the same cleanup on these paths.

### IN-301: The new e2e helper runs `ude init` with the host's HOME

**File:** `tests/e2e_lifecycle.rs:913`
**Reasoned.**
**Issue:** The push in `push_logging_git_graft_file` sets `HOME`, `XDG_CONFIG_HOME` and
`GIT_CONFIG_GLOBAL` to fixture paths, but the `ude init` before it does not, so its result can depend
on the host's config. `src/main.rs:726` reads the home directory. The same pattern appears at other
`ude init` calls in this file, so this helper follows an existing habit and is not a new class of
problem. Its limit is the same as any env-only assertion: the helper shows what the top-level `git`
process saw, not that git ignored a grafts file. Other tests in `mod tests` cover that.
**Fix:** Set the fixture variables on the init command as well.

---

_Reviewed: 2026-11-02T19:29:42Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: deep_
