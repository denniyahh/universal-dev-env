---
phase: "02"
slug: "sanitizer-boundaries-security-hardening"
status: verified
# threats_open = count of OPEN threats at or above workflow.security_block_on severity (the blocking gate)
threats_open: 0
asvs_level: 1
created: "2026-10-27"
---

# Phase 02 — Security

> Per-phase security contract: threat register, accepted risks, and audit trail.

---

## Trust Boundaries

| Boundary | Description | Data Crossing |
|----------|-------------|---------------|
| Local managed file -> shared template repo (push) | Untrusted local content crosses into a repository other machines pull from | — (see description) |
| Staged index / committed HEAD -> remote | Content can change after the staging scan (clean filters, hooks) | — (see description) |
| Shared template repo -> `ude init` on a developer machine | Template text written by other pushers is rendered with this machine's context and written into the project | — (see description) |
| Developer-written marker in a managed file -> secret gate | A line marker in user-controlled content changes what the gate reports | — (see description) |
| Upstream rule file -> UDE binary (vendoring) | Third-party data compiled into the scanner | — (see description) |
| Local managed file -> shared template repo (push, CLI) | The user-visible refusal path | — (see description) |
| UDE port <-> gitleaks reference behaviour | A silent divergence would miss a category gitleaks catches | — (see description) |
| Pusher's machine paths -> shared template repo | Home and config paths carry the username and machine layout | — (see description) |
| Pusher's local file -> shared template repo -> every consumer's init and pull | A template that cannot render, or renders to different text, is a denial of service or a silent content change for all consumers | — (see description) |
| Validated file in the cache checkout -> git index and HEAD (git add, commit) | Clean filters and hooks configured in the cache checkout can change the bytes git stores after every check ran | — (see description) |
| Refusal text -> terminal, CI logs | Whatever the refusal prints leaves the machine in logs | — (see description) |
| Pusher's environment (HOME, XDG_CONFIG_HOME) -> shared template repo | The root strings decide what gets parameterized; a spelling the matcher cannot find publishes the username and machine layout | — (see description) |
| Shared template -> every consumer's init, status and pull | Placeholders are rendered with the consumer's roots; a different spelling than push used writes `//` paths or reports false drift | — (see description) |
| Pusher's rendered project files -> shared template repo | Values rendered on the pusher's machine (git email, user name, project name) must not replace a placeholder in a template every project consumes | — (see description) |
| Shared template -> every consumer's init | Each consumer's init must substitute its own values; a published value becomes everyone's value | — (see description) |

---

## Threat Register

Register authored at plan time across plans 02-01 to 02-10. Mitigations verified at ASVS L1 (grep depth): every test named in a mitigation exists as a `fn` in `src/` or `tests/`, and the named mechanisms exist in `src/`. The full `cargo test` run on 2026-10-27 at the gap-closure HEAD passed (218 unit, 88 e2e).

| Threat ID | Category | Component | Severity | Disposition | Mitigation | Status |
|-----------|----------|-----------|----------|-------------|------------|--------|
| T-02-01 | I | push_updates pass 1 (non-UTF-8 branch) | high | mitigate | scan_bytes runs on the published bytes in both branches; test_push_blocks_secret_in_non_utf8_file (plan 02-01-PLA) | closed |
| T-02-02 | T | stage_publication / validate_committed_tree | high | mitigate | Gates scan raw `git show` bytes with the same scan_bytes; existing reset_index and rollback kept; test_publication_gate_scans_raw_blob_bytes, test_post_commit_gate_scans_raw_blob_bytes (plan 02-01-PLA) | closed |
| T-02-03 | I | Finding::reason and every refusal string | high | mitigate | Finding carries rule + line only; tests assert the token body is absent from reasons and gate errors (plan 02-01-PLA) | closed |
| T-02-04 | T | materialize_template rendering of shared templates | medium | mitigate | Renders with minijinja through the existing TemplateRenderer (no loader is registered, so templates cannot include files; values come only from TemplateContext). Templates without `{{` or not UTF-8 are byte-copied (test_init_copies_templates_without_placeholders_byte_identical) (plan 02-02-PLA) | closed |
| T-02-05 | D | init on a template that cannot render | low | accept | Init stops with exit 2 naming the template, the same refusal pull already makes; fail closed is preferred over writing an unrenderable placeholder that every later scan would misread (test_init_refuses_unrenderable_template) (plan 02-02-PLA) | closed |
| T-02-06 | I | entropy::first_high_entropy_line via scan_bytes | high | mitigate | R11 runs on every outbound path (staging, both gates) through scan_bytes; seeded rate floors and the push tracer test (plan 02-03-PLA) | closed |
| T-02-07 | I | R11 residual misses | medium | accept | Measured limits, kept as a backstop by design (CONTEXT: "Entropy is a backstop, not the primary guard"): hex-only and single-case secrets are never caught (the named and gitleaks rules are the guard for those); about 3-7% of mixed-case and 18-23% of base36 keys at 20-40 chars pass; base36 tokens of exactly 64 chars are blocked only about 16-20% (RESEARCH probe 16.5% / 20.2%; plan-revision probe 17.1-20.7% over 21 seeds; not measured by CONTEXT). DECIDED (operator, 2026-10-23): accepted as a Phase 2 risk; the orchestrator logs a calibration todo. The D-05 constants stay locked and unchanged in this phase (plan 02-03-PLA) | closed |
| T-02-08 | D | R11 on large files | low | accept | Single linear pass over the bytes per line; no regex backtracking except the ssh-span regex (linear-time engine) (plan 02-03-PLA) | closed |
| T-02-09 | I | ALLOW_MARKER on a line holding a real credential | medium | mitigate | D-03: named rules (and gitleaks rules from 02-05) run before R11 and ignore the marker (test_marker_does_not_silence_named_rules; gitleaks half in 02-06). Residual: a prefix-less key on a marked line is not blocked, accepted by the operator in D-02 (plan 02-04-PLA) | closed |
| T-02-10 | T | marker look-alikes | low | mitigate | Exact case-sensitive byte match; near-miss spellings and adjacent-line markers do not suppress (test_marker_near_misses_do_not_silence) (plan 02-04-PLA) | closed |
| T-02-11 | I | scan_bytes coverage of provider categories | high | mitigate | All 222 vendored gitleaks rules run at staging and both gates; tracer tests at all three sites; the category table is in plan 02-06 (plan 02-05-PLA) | closed |
| T-02-12 | T | src/sanitizer/rules/gitleaks.toml | medium | mitigate | Fetched from the v8.30.1 tag URL; SHA-256 pinned by test_vendored_rules_load_all and by the pin-check command; deny_unknown_fields rejects a silently changed schema (plan 02-05-PLA) | closed |
| T-02-13 | D | regex compile and match cost | medium | mitigate | ASCII-mode `regex::bytes` (linear time, no backtracking); compiled once per process in a LazyLock; keyword prefilter before any regex runs (plan 02-05-PLA) | closed |
| T-02-14 | I | rule load failure | high | mitigate | from_toml returns Err naming the rule; the LazyLock expects a load proven by test_vendored_rules_load_all (222 / 221, 0 errors); test_invalid_rule_regex_is_error and test_unknown_key_is_error show errors are not filtered (plan 02-05-PLA) | closed |
| T-02-15 | I | CLI refusal output (src/main.rs SecretBlocked arm) | high | mitigate | e2e asserts the token is absent from stdout and stderr for 10 categories and the `--force` case (plan 02-06-PLA) | closed |
| T-02-16 | I | gitleaks port semantics | medium | mitigate | Mutation gate with no missed mutants over gitleaks.rs; parity with the gitleaks 8.30.1 binary on a 15-category corpus plus a benign file, with a scripted negative control that re-runs gitleaks into a fresh report (review concern 6) (plan 02-06-PLA) | closed |
| T-02-17 | I | encoded secrets (base64/hex-wrapped) | low | accept | gitleaks' decoding passes are not ported (RESEARCH A3); R11 catches most encoded blobs; a short wrapped secret that fails R11 is missed (plan 02-06-PLA) | closed |
| T-02-18 | I | parameterize_paths trailing and leading boundaries | medium | mitigate | Boundary predicate accepts `)`, `,`, `;`, `}`, sentence-final `.`, a leading `:-` and others; push tracer test and boundary table (plan 02-07-PLA) | closed |
| T-02-19 | T | partial-substring rewrite of neighbouring paths | medium | mitigate | Alphanumeric / `_` / `-` / `.word` neighbours block a match on both sides; a leading `-` counts only in `:-` (review concern 10), so `/backup-/home/alice/x` stays byte-identical; boundary table and prop_no_partial_substring_corruption (trailing and leading sides) (plan 02-07-PLA) | closed |
| T-02-20 | T | degenerate home (`/`, `.`, empty, relative) | high | mitigate | is_parameterizable_root guard; test_degenerate_home_not_parameterized (plan 02-07-PLA) | closed |
| T-02-21 | I | alias spellings of the home path (`/home/<user>` while $HOME is `/var/home/<user>`) and unparameterizable shapes such as `-I/home/<user>/include`, the colon-less shell default `${VAR-/home/<user>}` (a `-` after a name is not a boundary, review concern 10), and a home path inside a UTF-16 file (non-UTF-8 files are never parameterized; plan 02-01 still scans them for secrets) | medium | accept | Not parameterized and not refused (RESEARCH Pitfall 8); the shipped templates already contain such paths that did not come from push. Out of scope, DECIDED: the operator chose not to fold the home-path todo into Phase 2 (CONTEXT Deferred Ideas, "Reviewed Todos (not folded)"), so RESEARCH Open Question 1 is RESOLVED. The pending todo stays open and is re-checked after this phase (plan 02-07-PLA) | closed |
| T-02-22 | D | published templates consumed by pull and init | high | mitigate | Gate 5 renders the published text back with the push's own context through the real renderer and refuses on error or mismatch; tracer, e2e and irreversible-shape tests (plan 02-08-PLA) | closed |
| T-02-23 | I | ReversibilityBlocked reason text | medium | mitigate | Review concern 4: the engine's detail text quotes template identifiers (minijinja 2.24.0 src/vm/mod.rs:625), so the reason is built from fixed text, RenderFailure's category (ErrorKind's fixed description) and line numbers only, through render_str_checked; test_render_str_checked_reports_category_and_line_only (with render_str's leak as negative control), test_check_render_back_reason_has_no_template_text and e2e test_push_refusal_never_echoes_template_text (sentinel absent from stdout and stderr) (plan 02-08-PLA) | closed |
| T-02-24 | T | a proxy reversibility check (string replace) passing a broken template | high | mitigate | The check uses the real renderer (TemplateRenderer::render_str_checked, the same engine and environment as render_str) and template::should_render, the same rule init, scan and pull use; test_check_render_back_exact_contract pins the comparison (plan 02-08-PLA) | closed |
| T-02-25 | I | scan_bytes on UTF-16 / UTF-32 content (NUL-interleaved ASCII) | high | mitigate | Second scan of the NUL-stripped view whenever the bytes hold a NUL (review concern 7); test_scan_bytes_utf16 (LE with and without BOM, BE; raw-view miss as control), test_push_blocks_secret_in_utf16_file, test_publication_gate_scans_utf16_blob, test_post_commit_gate_scans_utf16_blob. Residual, accepted: UTF-16 text is not path-parameterized (non-UTF-8 files never are), so a home path in a UTF-16 file is published as written (same class as T-02-21) (plan 02-01-PLA) | closed |
| T-02-26 | I | D-02 shape exemptions used to smuggle a credential | medium | mitigate | Exact shapes (review concerns 2 and 12): SRI only at the digest's base64 length and padding, hex and UUID only single-case, ssh only the key body after a key type; wrong-length SRI controls block (test_r11_shape_exemption_is_whole_token) and every predicate branch is pinned (test_exempt_shape_predicate). Residual, accepted by D-02: a credential shaped exactly like a sha256/384/512 SRI body, or an `AAAA` body after a key type, is not scored (plan 02-03-PLA) | closed |
| T-02-27 | I | assignment-glued tokens (`NAME=value`) diluting the randomness test | high | mitigate | OD-1 tokenizer: `=` is a delimiter except 1-2 trailing padding `=`; test_r11_env_assignment_rates_seeded shows `.env`-style lines block at the bare rate (>= 0.90) while the glued scoring falls below it; test_r11_tokenizer_padding_and_delimiters pins the padding rule (plan 02-03-PLA) | closed |
| T-02-28 | T | stage_publication / validate_committed_tree: blob changed by a git clean filter or commit hook after gate 5 | high | mitigate | Review concern 3 and checker revision 1: push_updates records the fingerprint of every published file (ValidatedBlobs) and both gates compare each stored blob with that record, never with the working tree, refusing with exit 2 and the existing index reset or commit rollback; test_publication_gate_refuses_blob_changed_by_clean_filter (pass-through filter as control), test_publication_gate_refuses_rewritten_working_tree (index and working tree both rewritten; missing record refused), test_post_commit_hook_rewrite_is_refused (a real amending post-commit hook, through push_to_remote), test_publication_gate_allows_line_ending_normalization, test_push_updates_records_validated_digests, and the e2e remote pushes for the main.rs wiring. Residual: a pre-push hook runs after the last gate; git has already resolved the commit it sends (`HEAD:refs/heads/<branch>`, validated by the committed gate) and the hook can only abort, which this plan does not test. Accepted consequence: a template repository that relies on a content-changing filter (git-lfs, `ident`, `working-tree-encoding`), or a `text` attribute on a non-UTF-8 file with CRLF line endings, can no longer receive pushes for those paths (fail closed) (plan 02-08-PLA) | closed |
| T-02-29 | I | vendored rules with no behavioural fixture (207 of 222 with the 15-row table) | medium | accept | Review concern 13, host decision: keep 15 behavioural rows; test_report_rules_without_positive_fixture prints the uncovered count and the SUMMARY records it as residual risk. The loader test (222 rules, 0 errors) and the semantics tests in plan 02-05 cover the shared matcher, not each rule's regex; a vendored rule whose regex never matches real tokens would not be noticed (plan 02-06-PLA) | closed |
| T-02-30 | I | push_updates gate 3 with a home or XDG root ending in a separator (HOME=`/home/alice/`, XDG_CONFIG_HOME with a trailing `/`) | high | mitigate | normalize_root at push_updates entry (gate 3 and the gate 5 context) and inside parameterize_paths; test_push_parameterizes_home_with_trailing_separator (two spellings, neighbour-path control), e2e test_cli_push_parameterizes_trailing_separator_home (real CLI, byte equality with a plain-HOME push), boundary table over three spellings, round-trip property companion, mutation check (plan 02-09-PLA) | closed |
| T-02-31 | T | init, status, pull and push's scan rendering `{{ user.home }}` / `{{ user.xdg_config }}` with an unnormalized root (`{fake}//x` written into a project, a pushed file reported as changed upstream, `//` published back) | medium | mitigate | detect_for_project normalizes the same two roots with the same helper; e2e test_trailing_separator_home_renders_and_reads_in_sync (single separators, byte equality with a plain-HOME init, In Sync after push) (plan 02-09-PLA) | closed |
| T-02-32 | T | normalization widening a degenerate root (`/` trimmed to the empty string, a relative path treated as absolute) | high | mitigate | normalize_root returns its input when nothing would remain and never adds characters; test_normalize_root rows `/`, `//`, `///`, `.`, empty, `home/alice/`; degenerate-home test with `///`; test_push_does_not_parameterize_root_home (plan 02-09-PLA) | closed |
| T-02-33 | I | other non-canonical spellings of the pusher's home: `/home/alice/.`, `/home//alice`, alias or symlinked homes, and Windows trailing-separator behaviour | medium | accept | Not part of verification gap 1 (trailing separators only). Alias spellings are already accepted as T-02-21; Windows runtime is unverified for the whole phase (backlog Phase 999.1); a raw home left after parameterization (review WR-01) and the silent skip of a root like HOME=`/` (review IN-01) are not in the verification gaps (plan 02-09-PLA) | closed |
| T-02-34 | I | push gate 2 (`template_has_logic`) for a static (non-`.jinja`) managed template that the render rule renders: `gitconfig.repo`, the hooks, `check-in-container.sh` holding `{{ git_email }}` or other logic | high | mitigate | template_has_logic decides with template::should_render (operator decision 2026-10-27, option A); test_push_skips_static_template_with_logic (template bytes unchanged, rendered email absent), test_push_never_deletes_static_template_with_logic, e2e test_push_skips_edit_over_static_template_with_logic (status hint, exit 1, consumer init gets its own email), logic-table rows, mutation check (plan 02-10-PLA) | closed |
| T-02-35 | D | an over-broad gate 2 blocking legitimate edits: the shipped commit-msg hook (`${#FIRST_LINE}`: a `{#` with no `{{`), placeholder-only templates, non-UTF-8 non-`.jinja` templates | medium | mitigate | A UTF-8 template the render rule does not render, and a non-UTF-8 non-`.jinja` template, has no logic; the two path placeholders are removed before the marker check; logic-table false rows, test_push_publishes_over_placeholder_only_static_template, e2e test_push_publishes_edit_over_placeholder_only_static_template, test_push_refuses_unrenderable_parameterized_hook's commit-msg control (plan 02-10-PLA) | closed |
| T-02-36 | T | template_has_logic now reads every mapped upstream template, not only `.jinja` ones | low | accept | scan_project already reads every mapped template and fails the scan on any read error other than NotFound before gate 2 runs; template_has_logic keeps returning Err (IoFailed, exit 2) instead of guessing, so the change fails closed (plan 02-10-PLA) | closed |
| T-02-37 | T | push gate 2 for a non-UTF-8 `.jinja` template holding template logic: one project's local deletion would delete the shared logic for every project (Phase 01 D-14), which a "non-UTF-8 has no logic" rule would allow | medium | mitigate | template_has_logic keeps HEAD's answer for every non-UTF-8 template (`.jinja` with a marker left after placeholder removal on the raw bytes is logic); test_push_never_deletes_non_utf8_jinja_template_with_logic with its negative control test_push_deletes_non_utf8_jinja_template_without_logic, the non-UTF-8 logic-table rows, the mutation check. No such template ships today (every tracked template is UTF-8) (plan 02-10-PLA) | closed |
| T-02-38 | T | a post-commit hook wraps UDE's commit in a merge, stacks a commit on it or replaces it with a parentless commit | high | mitigate | validate_committed_tree reads the raw commit (raw_commit_parents) and requires exactly one parent, the fetched head, in both push modes; test_direct_push_refuses_hook_merge_over_amended_commit, test_pr_push_refuses_hook_merge_over_amended_commit, test_pr_push_refuses_hook_parentless_commit, test_pr_push_refuses_hook_added_commit (plan 02-11-PLA) | closed |
| T-02-39 | T | committed gate reading through `refs/replace/*` a hook created | high | mitigate | `--no-replace-objects` on each object read; test_push_refuses_hook_replaced_blob, test_push_refuses_hook_merge_replaced_by_validated_commit, test_push_refuses_hook_replaced_tree; per-read control recorded in the summaries (plan 02-11-PLA, 02-12-PLA) | closed |
| T-02-40 | T | PR mode pushing the sync branch by name, which a hook can move after validation | high | mitigate | Both modes push `<validated id>:refs/heads/<name>`; test_pr_push_publishes_the_validated_commit_not_the_branch_tip, test_direct_push_publishes_the_validated_commit_after_head_moves (plan 02-11-PLA) | closed |
| T-02-41 | D | a post-commit hook that legitimately adds commits or merges can no longer push (fail closed) | low | accept | Benign hooks still publish in both modes; see the Accepted Risks Log (plan 02-11-PLA) | closed |
| T-02-42 | I | what `git push` can send beyond the validated commit | high | mitigate | Closed for the probed channels by T-02-43 and T-02-47..T-02-51; this is the probed list, not a proven complete one; T-02-52 is the accepted exception (plan 02-12-PLA) | closed |
| T-02-43 | I | a hook-set `push.followTags` publishing an annotated tag and its unscanned message | high | mitigate | `--no-follow-tags` on both pushes; test_direct_push_does_not_publish_hook_tag, test_pr_push_does_not_publish_hook_tag (plan 02-11-PLA) | closed |
| T-02-44 | I | commit message written by a prepare-commit-msg or post-commit hook | high | mitigate | Message must equal SYNC_COMMIT_MESSAGE plus one LF, byte for byte (raw_commit_parts); test_committed_gate_requires_exact_sync_message and the hook tests in both modes (plan 02-12-PLA) | closed |
| T-02-45 | I | the rest of the commit header a hook can write | medium | mitigate | Header bytes go through scan_bytes; test_push_refuses_hook_secret_in_commit_header. A scan, not a bind: text the scanner does not flag still publishes (plan 02-12-PLA) | closed |
| T-02-46 | I | an empty directory whose name `diff-tree -r` never lists | medium | mitigate | Raw `ls-tree -r -t -z --full-tree` and count_empty_directories; test_push_refuses_hook_empty_directory, test_push_refuses_hook_replaced_tree_hiding_empty_directory (plan 02-12-PLA) | closed |
| T-02-47 | T | `.git/info/grafts` written by a commit hook making push send grafted objects | high | mitigate | refuse_grafts_file refuses while the file exists after the commit and hooks; test_direct_push_refuses_hook_grafts_file, test_pr_push_refuses_hook_grafts_file, test_refuse_grafts_file_outcomes (plan 02-12-PLA) | closed |
| T-02-48 | T | forged commit-graph (local, split chain, alternate object directory) | high | mitigate | `-c core.commitGraph=false` on both pushes; test_direct_push_sends_no_object_from_forged_commit_graph, test_pr_push_sends_no_object_from_forged_commit_graph, test_push_sends_no_object_from_forged_split_commit_graph, test_push_sends_no_object_from_forged_alternate_commit_graph (plan 02-12-PLA) | closed |
| T-02-49 | T | pack or multi-pack-index bitmap built while a graft existed | high | mitigate | `-c pack.useBitmaps=false` on both pushes; test_direct_push_sends_no_object_from_graft_built_bitmap, test_pr_push_sends_no_object_from_graft_built_bitmap, test_push_sends_no_object_from_graft_built_multi_pack_index_bitmap (plan 02-12-PLA) | closed |
| T-02-50 | D | refusing on the presence of files git itself writes would refuse legitimate caches | medium | mitigate | Disable on push instead (operator decision 2026-10-28); test_push_from_clone_of_gc_remote_publishes (plan 02-12-PLA) | closed |
| T-02-51 | T | inherited GIT_TEST_* variables re-enabling commit-graphs | medium | mitigate | remove_git_test_env on both pushes; test_remove_git_test_env, test_direct_push_ignores_inherited_git_test_commit_graph, test_pr_push_ignores_inherited_git_test_commit_graph. These tests set the variable through the engine's extra environment, not the real process environment (plan 02-12-PLA) | closed |
| T-02-52 | I | a pre-push hook writing a grafts file during `git push`, after the gate | low | accept | Not mitigated and not tested; operator-accepted residual, see the Accepted Risks Log (plan 02-12-PLA) | closed |
| T-02-53 | D | a user hook that rewrites every commit message makes every push refuse | medium | accept | Fail closed kept; the refusal names the likely hook; see the Accepted Risks Log (plan 02-12-PLA) | closed |
| T-02-SC | T | dependency installs | high | mitigate | No package is installed or changed: `regex` 1.13.1 is already in Cargo.lock; `git diff --exit-code -- Cargo.toml Cargo.lock` gates Task 2; `cargo deny check` stays green (plan 02-01-PLA, 02-02-PLA, 02-03-PLA, 02-04-PLA, 02-05-PLA, 02-06-PLA, 02-07-PLA, 02-08-PLA, 02-09-PLA, 02-10-PLA) | closed |

*Status: open · closed · open — below high threshold (non-blocking)*
*Severity: critical > high > medium > low — only open threats at or above workflow.security_block_on count toward threats_open*
*Disposition: mitigate (implementation required) · accept (documented risk) · transfer (third-party)*

---

## Accepted Risks Log

| Risk ID | Threat Ref | Rationale | Accepted By | Date |
|---------|------------|-----------|-------------|------|
| AR-02-01 | T-02-05 | Init stops with exit 2 naming the template, the same refusal pull already makes; fail closed is preferred over writing an unrenderable placeholder that every later scan would misread (test_init_refuses_unrenderable_template) | plan 02-02-PLA threat model (operator-reviewed plan) | 2026-10-27 |
| AR-02-02 | T-02-07 | Measured limits, kept as a backstop by design (CONTEXT: "Entropy is a backstop, not the primary guard"): hex-only and single-case secrets are never caught (the named and gitleaks rules are the guard for those); about 3-7% of mixed-case and 18-23% of base36 keys at 20-40 chars pass; base36 tokens of exactly 64 chars are blocked only about 16-20% (RESEARCH probe 16.5% / 20.2%; plan-revision probe 17.1-20.7% over 21 seeds; not measured by CONTEXT). DECIDED (operator, 2026-10-23): accepted as a Phase 2 risk; the orchestrator logs a calibration todo. The D-05 constants stay locked and unchanged in this phase | plan 02-03-PLA threat model (operator-reviewed plan) | 2026-10-27 |
| AR-02-03 | T-02-08 | Single linear pass over the bytes per line; no regex backtracking except the ssh-span regex (linear-time engine) | plan 02-03-PLA threat model (operator-reviewed plan) | 2026-10-27 |
| AR-02-04 | T-02-17 | gitleaks' decoding passes are not ported (RESEARCH A3); R11 catches most encoded blobs; a short wrapped secret that fails R11 is missed | plan 02-06-PLA threat model (operator-reviewed plan) | 2026-10-27 |
| AR-02-05 | T-02-21 | Not parameterized and not refused (RESEARCH Pitfall 8); the shipped templates already contain such paths that did not come from push. Out of scope, DECIDED: the operator chose not to fold the home-path todo into Phase 2 (CONTEXT Deferred Ideas, "Reviewed Todos (not folded)"), so RESEARCH Open Question 1 is RESOLVED. The pending todo stays open and is re-checked after this phase | plan 02-07-PLA threat model (operator-reviewed plan) | 2026-10-27 |
| AR-02-06 | T-02-29 | Review concern 13, host decision: keep 15 behavioural rows; test_report_rules_without_positive_fixture prints the uncovered count and the SUMMARY records it as residual risk. The loader test (222 rules, 0 errors) and the semantics tests in plan 02-05 cover the shared matcher, not each rule's regex; a vendored rule whose regex never matches real tokens would not be noticed | plan 02-06-PLA threat model (operator-reviewed plan) | 2026-10-27 |
| AR-02-07 | T-02-33 | Not part of verification gap 1 (trailing separators only). Alias spellings are already accepted as T-02-21; Windows runtime is unverified for the whole phase (backlog Phase 999.1); a raw home left after parameterization (review WR-01) and the silent skip of a root like HOME=`/` (review IN-01) are not in the verification gaps | plan 02-09-PLA threat model (operator-reviewed plan) | 2026-10-27 |
| AR-02-08 | T-02-36 | scan_project already reads every mapped template and fails the scan on any read error other than NotFound before gate 2 runs; template_has_logic keeps returning Err (IoFailed, exit 2) instead of guessing, so the change fails closed | plan 02-10-PLA threat model (operator-reviewed plan) | 2026-10-27 |
| AR-02-09 | T-02-41 | The cache checkout is UDE-managed and such a hook rewrites what UDE validated; hooks that leave UDE's commit as it is still publish (benign controls in both modes) | plan 02-11-PLA threat model | 2026-10-28 |
| AR-02-10 | T-02-52 | A hostile pre-push hook can already push anything to the same remote with the user's credentials, so this gives it no new capability; `push --no-verify` would disable every user's pre-push hooks. Reproduced on git 2.56.0 at plan time. The 02-08 prohibition carries this as its explicit exception | operator | 2026-10-28 |
| AR-02-11 | T-02-53 | Cost of the operator's decision to bind the message: a hook that rewrites messages makes every push refuse, with a refusal that names the likely hook. commit-msg hooks are skipped by `--no-verify` and are not affected | orchestrator, within the operator's decision | 2026-10-28 |

*Accepted risks do not resurface in future audit runs.*

---

## Security Audit Trail

| Audit Date | Threats Total | Closed | Open | Run By |
|------------|---------------|--------|------|--------|
| 2026-10-27 | 38 | 38 | 0 | /gsd-secure-phase 02 orchestrator (ASVS L1 short-circuit, grep depth; no auditor spawned) |
| 2026-11-01 | 54 | 54 | 0 | /gsd-secure-phase 02 orchestrator (ASVS L1 short-circuit, grep depth; no auditor spawned; 16 threats added from plans 02-11 and 02-12) |

---

## Sign-Off

- [x] All threats have a disposition (mitigate / accept / transfer)
- [x] Accepted risks documented in Accepted Risks Log
- [x] `threats_open: 0` confirmed
- [x] `status: verified` set in frontmatter

**Approval:** re-verified 2026-11-01 after plans 02-11 and 02-12 (ASVS L1 grep depth; does not establish that each mitigation is correct, only that its named tests and mechanisms exist and the suite passes)

## Security Audit 2026-11-01

| Metric | Count |
|---|---|
| Threats found | 54 |
| Closed | 54 |
| Open | 0 |
