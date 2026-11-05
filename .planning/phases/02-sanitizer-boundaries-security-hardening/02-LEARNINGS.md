---
phase: 02
phase_name: "Sanitizer, Boundaries & Security Hardening"
project: "Universal Development Environment (UDE)"
generated: "2026-11-05"
counts:
  decisions: 11
  lessons: 12
  patterns: 9
  surprises: 9
missing_artifacts:
  - "UAT.md"
---

# Phase 02 Learnings: Sanitizer, Boundaries & Security Hardening

## Decisions

### One byte-level scan entry point for every outbound check
`Sanitizer::scan_bytes` scans the raw bytes, then a NUL-stripped view when the bytes hold a NUL. Push staging and both publication gates call it, and staging scans the published bytes (after parameterization), so all three sites scan the same bytes. Later detectors (R11, gitleaks) were added inside `scan_view` only, so each reached all three call sites and both views without touching the call sites again.

**Rationale:** Non-UTF-8 and UTF-16 managed files were published unscanned before this plan. UTF-16 is scanned rather than refused, and the second view can only add findings.
**Source:** 02-01-SUMMARY.md

### Vendor the gitleaks v8.30.1 default rules verbatim, SHA-256 pinned
All 222 rules are vendored from the tag, pinned by hash in a unit test, compiled as ASCII-mode `regex::bytes`, and run between the named rules and R11. The loader returns `Err` naming the rule for any parse error, unknown key, empty allowlist, rule with neither regex nor path, or `secretGroup` beyond the regex's groups. Nothing is filtered away.

**Rationale:** Port gitleaks' plain-text matching semantics (`detect.go` `detectRule`) into UDE's own scanner, and fail closed when the configuration would silently weaken matching.
**Source:** 02-05-SUMMARY.md

### Accept R11's measured residual misses as a Phase 2 risk and keep the D-05 constants locked
Hex-only and single-case secrets are never caught by R11. About 3-7% of mixed-case and 18-23% of base36 keys at 20-40 chars pass, and 64-char base36 tokens are blocked only about 16-20% of the time. The D-05 constants stay unchanged this phase, and a calibration todo was logged.

**Rationale:** Entropy is a backstop, not the primary guard; the named and gitleaks rules guard the shapes R11 misses. The operator decided this on 2026-10-23.
**Source:** 02-SECURITY.md (AR-02-02), 02-03-SUMMARY.md

### Push gate 2 decides "has template logic" with the same render rule init uses (operator option A)
`template_has_logic` uses `template::should_render` for UTF-8 templates, so init and push protect the same set of templates. Non-UTF-8 templates keep exactly the previous answer (Phase 01 D-14).

**Rationale:** Init renders any template containing `{{` (since 02-02). A push decided by the `.jinja` extension alone let the pusher's rendered email replace a placeholder that every consumer's init must substitute itself.
**Source:** 02-10-SUMMARY.md, STATE.md

### Tighten the trailing-dot boundary rule, accepting that rare shapes publish raw
A `.` after the home path is a trailing boundary only at end of text, or when the next char is not alphanumeric, `_`, `-`, `.` or `/`.

**Rationale:** The earlier rule rewrote `/home/alice..bak` and `/home/alice./x`, which name other directories. The operator chose to tighten the rule and accepted that shapes such as a path followed by an ellipsis are published raw, with the username in them.
**Source:** 02-07-SUMMARY.md (post-plan amendment)

### Disable commit-graphs and bitmaps on push instead of refusing when they are present
Both pushes run with `-c core.commitGraph=false`, `-c pack.useBitmaps=false` and every `GIT_TEST_*` variable removed. The only presence refusal is the grafts file.

**Rationale:** Refusing on files git itself writes would refuse legitimate caches. The operator decided this on 2026-10-28, and `test_push_from_clone_of_gc_remote_publishes` pins that a gc'd remote still publishes.
**Source:** 02-12-SUMMARY.md, 02-SECURITY.md (T-02-50)

### Push to the caller's URL and return Ok only after an `ls-remote` read-back
Both modes push to `-- <RemoteCache.remote_url>` instead of `origin`. `confirm_published` must see the pushed ref at the validated commit (whole-string match, read from the filesystem root). On a mismatch, direct mode rolls back and PR mode removes the sync branch, and no ancestor is recorded. Residual redirects (hook-written `pushInsteadOf` and the other repository-level redirect keys, plus global redirect keys) were accepted by the operator.

**Rationale:** A hook-written `remote.origin.pushurl` could send the commit elsewhere while the push still returned Ok. A refusal before the push would need a list of config keys, and would still miss a process that writes a key later.
**Source:** 02-18-SUMMARY.md, 02-SECURITY.md (AR-02-12, AR-02-14, AR-02-15)

### The gate's promise is a closed list, closed by decision and not proven exhaustive
docs/ARCHITECTURE.md section 3 lists DEF-01..DEF-15 (defended channels, each with its tests) and NOT-01..NOT-15 (recorded non-promises). The gate's doc comments point at the list instead of claiming to stop "whatever git filter or commit hook" changed a blob.

**Rationale:** The operator decided this on 2026-11-02. New instances of a known class stay defects through the class rows; anything outside the list goes to the operator.
**Source:** 02-20-SUMMARY.md, 02-SECURITY.md (AR-02-16)

### Build the tree listing in process from hashed bytes instead of re-checking a second read
`verified_tree_listing` reads every tree, root included, through `read_verified_object`, and emits git's exact `ls-tree -r -t -z --full-tree` records. The existing parsers and refusal texts run unchanged on that output. The committed gate's change list became `listing_diff` over two verified listings, so neither `git ls-tree` nor `git diff-tree` runs on the publication routes any more.

**Rationale:** Keeping git's listing and checking each tree in a second read is WR-500 itself. No git option checks a subtree's bytes in the same read that produces the listing.
**Source:** 02-24-PLAN.md, 02-24-SUMMARY.md, 02-25-SUMMARY.md

### Bound the walk with one budget per gate call; leave the change loop outside the deadline
One `ListingBudget` per gate call holds 2^18 records, 64 MiB of listing bytes and one deadline at the local time limit (30 s). Records are taken before a tree's entries are pushed, and refusals are fixed text. The change loop stays outside the deadline (orchestrator decision; the operator delegated it), but the repeated-path check moved ahead of it. A repeat is therefore refused after 0 blob reads, and the loop reads at most one blob per allowlisted path.

**Rationale:** The operator chose to fix review WR-600 rather than widen NOT-14. A record limit alone does not bound memory, because git does not limit an entry name's length. The operator stated that template repositories are far below 1000 directories; the measured ceiling is about 880 distinct trees.
**Source:** 02-27-PLAN.md, 02-27-SUMMARY.md, STATE.md

### Low-severity threats with no operator answer stay OPEN and are never accepted by a plan
T-02-60, T-02-61, T-02-64, T-02-65 and T-02-67 stay `pending the operator, not accepted`. Executors copy the status cell verbatim and never accept a risk or stop to ask about one. Starting execution is not acceptance. These rows are below `security_block_on: high`, so they do not block the phase.

**Rationale:** Earlier Accepted Risks Log rows had listed a plan's threat model under "Accepted By". The plans forbid treating a plan as the acceptor.
**Source:** 02-15-PLAN.md (Operator answers), 02-SECURITY.md

## Lessons

### A grep-depth security audit read "closed" while bypasses existed
The 2026-11-01 `/gsd-secure-phase` run reported 54 of 54 threats closed at ASVS L1 grep depth. Re-verification then reproduced bypasses, and the register was corrected to 4 open, 3 of them at or above the block threshold. The final audit says outright that a grep-depth audit plus a registered-test check cannot see bypasses, and that behaviour under attack rests on the verifier's negative controls.

**Context:** With `asvs_level: 1` and `threats_open: 0`, the workflow skips the auditor and takes no flag to change that. The plan therefore told the orchestrator to run it only after re-verification, and to report its closure as grep depth.
**Source:** 02-SECURITY.md (Security Audit Trail, Sign-Off), 02-15-PLAN.md

### Green listed tests do not show that the object channels are exhausted
Every test plan 02-13 listed was green while a tag object at a validated path still published unread text: `cat-file blob` peels the tag. A probe aimed at `cat-file blob` found it, not any listed test. Later, CR-400 (stored bytes not matching their id) was again a channel the earlier tests could not see.

**Context:** The verifier's caveat: each control shows that the named tests depend on that check, and does not show that the list of channels is complete.
**Source:** 02-13-SUMMARY.md, 02-VERIFICATION.md

### An id check on a second read verifies different bytes from the ones decisions use
In 02-22 the gate took blob ids, modes and deletions from `ls-tree` while the id check hashed a separate `cat-file` read and threw those bytes away (review WR-500). A tree swapped between the two reads gave a false Ok. The fix makes decisions only from the bytes that were hashed.

**Context:** git does not check subtrees below the root on `cat-file`, `ls-tree` or `diff-tree` (02-22's measurements).
**Source:** 02-24-SUMMARY.md, 02-24-PLAN.md

### Bounding each read does not bound an in-process walk
T-02-102 (02-24) claimed that each bounded read bounds the walk. It was wrong: the cost is spent outside the reads, in UDE's own memory and time. A hook could make the gate list 2^d paths from d + 3 tree objects, or very long names. This needed record, byte and deadline budgets (T-02-110). Memory under a single very large tree object, the `verified` map and the change loop remain outside the bound.

**Context:** WR-600 was raised by code review after 02-24 to 02-26 replaced git's listings with an in-process walk.
**Source:** 02-SECURITY.md (Supersession notes), 02-27-SUMMARY.md, 02-VERIFICATION.md

### The per-task RED-commit gate cannot see Rust RED commits (#4379); use ignored-RED commits plus substitute evidence
The gate's pathspec does not see RED commits that touch only `src/*.rs`. It tripped (`TDD GATE TRIPPED: missing RED commit`) on 02-24, 02-25 and 02-27, although the RED commits existed. The same query without the test-file pathspecs found each RED commit, which showed the trip came from the pathspec. The scheme that followed: commit the RED rows as `#[ignore = "RED: ..."]` after a recorded failing run, un-ignore them with the fix, and continue under the operator-approved substitute evidence (the RED commit found by its exact subject, plus the `RED_EVIDENCE_OK` records). `last_gate_trip` could not be written, because STATE.md has no such field.

**Context:** RED rows cannot move to `tests/`, because integration tests build without `cfg(test)` and lose the seams and private helpers.
**Source:** 02-24-PLAN.md, 02-24-SUMMARY.md, 02-25-SUMMARY.md, 02-27-SUMMARY.md, STATE.md

### "Do not commit RED" produces a GREEN-before-RED history that the gate flags
Plan 02-22 forbade committing a red suite, so its RED rows ran uncommitted and were committed together with the fix. A log-order check then reports GREEN before RED. The operator accepted this on 2026-11-04 with no history rewrite, on the strength of the 11 uncommitted RED records and 02-23's controls on the final code.

**Context:** This led to the ignored-RED commit scheme of 02-24 onward.
**Source:** 02-22-SUMMARY.md, STATE.md

### A mutation filter written before later tests existed misses mutants the suite does catch
In 02-15 a mutant in `read_blob_by_id`'s type check survived. In 02-21 one survived in `require_fetched_head_holds`' deletion check. Both were killed by tests that already existed, but those tests were not in plan name lists written before them. Each time, the fix was a new test under a name the filter already selects, so the plan's command still runs unchanged.

**Context:** A missed mutant can be a gap in the filter, not in the suite. Re-read the filter against tests added since the plan was written.
**Source:** 02-15-SUMMARY.md, 02-21-SUMMARY.md

### cargo-mutants leaves string arguments, deleted statements and refusal texts unpinned
cargo-mutants does not mutate string arguments or delete statements. The push `-c` options and `--no-replace-objects` were therefore pinned only by control rows. The `the changes of the commit to publish {LISTING_PAST_DEADLINE}` arm was reached by no test, and remapping it would have passed the suite. Review IN-700 fixed that arm by extracting a pure `diff_refusal` mapping and unit-testing it.

**Context:** Mutation output with 0 missed covers only the operators cargo-mutants generates. Pair it with per-check control tables and with coverage.
**Source:** 02-15-SUMMARY.md, 02-28-SUMMARY.md, 02-REVIEW.md, 02-VERIFICATION.md

### Gating on a piped command checks the wrong exit code
One RED commit was gated on `cargo build ... | tail -2`, and one GREEN gate piped nextest into `rg Summary`. In both cases the chain saw the pipe tail's exit code. A control run printed `exit=0` for every case because `$?` was read after a `$(basename)` substitution. One verify piped into `head` reported 141 (SIGPIPE).

**Context:** No broken state was committed, but in each case the gate was wired wrong. Later commits were gated on the real exit code, with no pipe.
**Source:** 02-10-SUMMARY.md, 02-16-SUMMARY.md, 02-20-SUMMARY.md, 02-21-SUMMARY.md

### Proxy readings of tool output nearly recorded wrong numbers
Two near misses:
- Coverage summary rows carry no line list, so every function first showed 0 missed lines. A cross-check against process.rs's known 15 missed lines exposed the misreading.
- `rg` without context showed only the first line of each multi-line value, so a RED probe was first read as "not published". A second probe confirmed the hidden file was published.

**Context:** In both cases a second, independent reading caught the error before any number was recorded.
**Source:** 02-26-SUMMARY.md, 02-25-SUMMARY.md

### RED commits in a binary crate need lint workarounds to pass the commit gate
In a binary crate, stubs and fields used only by RED tests trip `dead_code` under `clippy -D warnings`. Workarounds used:
- a temporary `#[allow(dead_code)]` or `#[expect(dead_code, reason = ...)]`, removed in GREEN;
- wiring the stubs into the call site, still returning `None`;
- adding fields at first use.

In 02-27 the reverse happened: `#[expect(dead_code)]` on `cfg(test)` seam fields was itself an error (`unfulfilled_lint_expectations`).

**Context:** RED commits were gated on fmt plus build or clippy, not on the full test chain, because their tests fail by design.
**Source:** 02-01-SUMMARY.md, 02-03-SUMMARY.md, 02-05-SUMMARY.md, 02-09-SUMMARY.md, 02-27-SUMMARY.md

### A test that passes in RED, or on arrival, proves nothing until a planted fault makes it fail
Several pins and controls passed in RED by design. Examples:
- the byte-identical init control;
- the 02-08 no-echo and gate-order tests;
- the 02-11 boundary tests.

Each was then run against temporary wrong rules, restored byte-identical with a sha256 check. One such run found a real gap: the direct push by a `HEAD` refspec failed no test, so `test_direct_push_publishes_the_validated_commit_after_head_moves` was added. A related case: a CLI high-entropy row was refused by gitleaks' `generic-api-key` (the word `key`) instead of R11, and would have passed for the wrong reason.

**Context:** Pass-on-arrival is a weak signal. A planted fault must be able to turn the test red.
**Source:** 02-02-SUMMARY.md, 02-08-SUMMARY.md, 02-11-SUMMARY.md, 02-16-SUMMARY.md

## Patterns

### Per-check control table on the final code
Each row makes one edit by an exact single-match scripted replacement, dry-run first on scratch copies. Then the row runs the module tests, reads which tests failed, and restores the files, followed by `git diff --exit-code -- src` and a hash of the code before `mod tests {`. Each row has a state:
- `same`: exactly the named tests failed;
- `corrected`: the named tests failed plus recorded extras, each with its reason;
- `different`: a defect.

The verify refuses an invented name, and refuses exit 0 or 101. Corrected rows are named for re-run at re-verification. The runs were 02-15 (23 rows), 02-21 (46 rows), 02-23 (10), 02-26 (13) and 02-28 (20).

**When to use:** whenever a security check's evidence must be load-bearing. It covers string arguments and deleted statements that mutation testing cannot touch.
**Source:** 02-15-PLAN.md, 02-15-SUMMARY.md, 02-21-SUMMARY.md, 02-23-SUMMARY.md, 02-26-SUMMARY.md, 02-28-SUMMARY.md

### Scoped cargo-mutants with a negative control on the gate itself
The gate is scoped by `--file` and `--re`, or by `--in-diff`, with named test filters. No exclusions and no shared `.cargo/mutants.toml`. Survivors are killed with new tests, never excluded. The run is bounded with `timeout`, and afterwards `git diff --cached --quiet` and the local git config are compared with a saved copy. Negative control: the same invocation, restricted to one mutant and given a test filter that cannot see it, must report the mutant as missed (exit 2).

**When to use:** for critical algorithms and gate functions. The negative control separates "the tests catch everything" from "the filter runs nothing that matters".
**Source:** 02-04-SUMMARY.md, 02-12-SUMMARY.md, 02-15-SUMMARY.md, 02-28-SUMMARY.md

### Every refusal test paired with a delivering control
Each hook or redirect refusal row has a control that runs the same state through the unchecked path and shows the injected blob reaching the remote. Each test also asserts that the hook ran, and that the forge or bitmap was in effect, before its refusal assertion.

**When to use:** for any adversarial test. Without the control, a hook that never did anything would also pass.
**Source:** 02-17-SUMMARY.md, 02-12-SUMMARY.md, 02-16-SUMMARY.md

### A cfg(test) seam to make a race deterministic
A `cfg(test)` hook runs right after a route's listing and puts the honest bytes back. This reproduces a swap between reads without a racing process. The seam fields and builders are `cfg(test)` only, so non-test builds get an empty function. Integration tests under `tests/` run the binary built without them.

**When to use:** for time-of-check/time-of-use channels that a real hook can reach but cannot schedule reliably.
**Source:** 02-24-SUMMARY.md, 02-24-PLAN.md, 02-REVIEW.md

### JUnit-backed RED records classified by `gsd-tools check tdd-red-evidence`, with a classifier control
Each target test gets its own record, built from the nextest JUnit report and checked to be newer than the run start. Each record is classified `RED_EVIDENCE_OK`/`target_test_failed`, and the summary adds a semantic note that the failure was the planned assertion, not setup. Classifier negative control: a record naming a test that passed returns `INVALID_RED`/`no_target_test_failure`.

**When to use:** for every TDD RED step, and especially when the commit-order gate is blind (#4379).
**Source:** 02-01-SUMMARY.md, 02-04-SUMMARY.md, 02-27-SUMMARY.md

### Fixed refusal text that never echoes values, names or hook bytes
A refusal reports `<rule> at line <n>`. It never includes:
- the token or the engine's template detail;
- the raw `parent` text;
- a tree-chosen name.

An id mismatch names only a recorded, staged or allowlisted path. Tests assert that the token body or sentinel is absent from both CLI streams.

**When to use:** for any refusal that goes to the terminal or CI logs (the "refusal text -> terminal, CI logs" trust boundary).
**Source:** 02-01-SUMMARY.md, 02-08-SUMMARY.md, 02-19-SUMMARY.md, 02-27-SUMMARY.md, 02-SECURITY.md

### Byte equality with git's own output as an equivalence test, with a claims check
The in-process listing and diff are compared byte for byte with `git ls-tree -r -t -z --full-tree` and `git diff-tree` in both SHA-1 and SHA-256 repositories. The fixture first asserts that git's own output holds each claimed case, so the comparison is not vacuous. Claimed cases include a legacy mode, a gitlink, a symlink, a newline name and a 0xFF name.

**When to use:** when replacing a git command with in-process code.
**Source:** 02-24-SUMMARY.md, 02-25-SUMMARY.md, 02-28-SUMMARY.md

### Host-hermeticity checks after every git-spawning run
After runs:
- compare a hash of `git config --local -l` and the index state;
- run the hermeticity gate with a decoy `GIT_DIR` and list files under its `objects` and `refs`, not via `git for-each-ref`;
- keep TMPDIR in the session scratchpad.

`PROPTEST_DISABLE_FAILURE_PERSISTENCE=1` stops proptest writing `proptest-regressions/` into the checkout.

**When to use:** for any test or mutation run that spawns git near the real checkout.
**Source:** 02-15-PLAN.md, 02-19-SUMMARY.md, 02-21-SUMMARY.md, 02-07-SUMMARY.md

### Removals name literal paths, in a separate command
Claude Code's safety check refuses a `bash -c '... rm -rf "$T" ...'` whose target it cannot read. Mutation and control scripts therefore use a literal TMPDIR and remove it with a separate command that names the literal path.

**When to use:** in any plan command that cleans up scratch output under Claude Code.
**Source:** 02-12-SUMMARY.md, 02-15-PLAN.md, 02-15-SUMMARY.md, 02-07-SUMMARY.md

## Surprises

### `git cat-file blob` peels an annotated tag
A tree that listed a tag object at a validated path read as the validated bytes, while the push sent the tag object with its text. The gate returned Ok in both modes through the real `push_to_remote`. The fix: `read_blob_by_id` runs `cat-file -t` first and refuses anything that is not a blob.

**Impact:** an unplanned security fix inside 02-13, outside its threat register, and the first proof that the listed tests did not exhaust the object channels.
**Source:** 02-13-SUMMARY.md

### The `core.fsmonitor` index swap was real, not only reasoned
T-02-58 listed it as "reasoned, not run". In 02-13 a script swapped the index between the staged gate's last read and `git write-tree`, and the recorded tree id then matched.

**Impact:** binding to the recorded id alone was not enough. The gate has to validate the commit's own tree whatever the id says.
**Source:** 02-13-SUMMARY.md

### The old parameterization loop replaced a path twice
The fixpoint loop turned `/home/alice/home/alice` into `{{ user.home }}{{ user.home }}`: the `}` left by the first replacement became a boundary for the second. The RED run found this, and the plan had not listed it.

**Impact:** the new pass reads boundaries from the input only.
**Source:** 02-07-SUMMARY.md

### A git process inside a pre-push hook also sees `GIT_GRAFT_FILE` at the null device
The hook inherits the push's environment. A plain-git probe logged `hook:/dev/null`, while the remote side on a local path saw the variable unset.

**Impact:** the must-have "every other git process sees the variable unset" holds only for processes UDE spawns. No test pins either result.
**Source:** 02-14-SUMMARY.md

### Wall-clock assertions in the deadline-carry test failed under parallel load
These were the failures:
- At 5 s / 6 s, the control took 1.861 s under 43 parallel tests, against a 1.667 s bound.
- The orchestrator widened the test to 15 s / 16 s.
- Review WR-700 then failed 1 of 3 runs under artificial CPU load.

The timing bounds added no discrimination, because the refusal text already decides the test. The fix (57ed03a) removed the wall-clock asserts.

**Impact:** a timing assertion with a margin below about 3x is a source of flakiness, not evidence. Keep a hang guard far from real time, and let the asserted text decide.
**Source:** 02-27-SUMMARY.md, 02-REVIEW.md, 02-VERIFICATION.md

### Planner runs were stopped by a safety filter
Two planner runs for the byte-limit revision of 02-27 were stopped by a safety filter before editing. The orchestrator wrote the revision by hand and logged it in the GSD manual-fix incident log.

**Impact:** the planning step had to be done by hand. Expect content about resource-exhaustion attacks to trip filters.
**Source:** 02-27-PLAN.md

### GSD state verbs repeatedly scraped `milestone: v8.30.1` into STATE.md and state.json
The gitleaks version in ROADMAP plan text was written into the state files as the milestone. It was scrubbed by hand after close-out in plans 02-04 to 02-11 (logged as a known GSD defect; 02-04 records it as the fifth occurrence).

**Impact:** a manual fix after each plan's state update, and an incident entry each time.
**Source:** 02-04-SUMMARY.md, 02-09-SUMMARY.md, 02-10-SUMMARY.md, 02-11-SUMMARY.md

### git refuses a rewritten root tree at `git status`, before either gate route
A forged fetched-root tree failed at `git status` with `hash mismatch` (exit 128), not at `ls-tree` as the plan's measurement said. Its rows became pin rows for git's own check, and 02-24 added a direct test of the gate's own root check.

**Impact:** the push rows could not discriminate the gate's root check until WR-501 added `test_fetched_head_holds_refuses_root_tree_not_matching_its_id`.
**Source:** 02-22-SUMMARY.md, 02-24-SUMMARY.md

### Gap closure grew the phase from 8 plans to 28, and test-module timings swing about 25% between runs
02-08 was recorded as "the last plan of Phase 02". Plans 02-09 to 02-28 were gap and review closures, opened after verification and review reproduced gaps and bypasses. 02-21's control table and two mutation runs took 2h 40m. Module runs of `sync::tests::` swung about 25% between consecutive runs (79 s, 99.6 s, 76.6 s), which hid any per-tree cost of the new reads.

**Impact:** cost comparisons support only "no regression visible above noise". Budget long control and mutation plans as separate work.
**Source:** STATE.md (Per-Plan Metrics), 02-21-SUMMARY.md, 02-24-SUMMARY.md, 02-25-SUMMARY.md
