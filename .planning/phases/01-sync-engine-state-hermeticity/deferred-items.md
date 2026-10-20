## Deferred Items

- Repository baseline is not `cargo fmt --check` clean
  status: resolved (2026-10-18, db81851 — operator chose one-off repo-wide
  `cargo fmt` after wave 1, before 01-03; `cargo fmt --check` now exits 0 and is
  the real gate for 01-03 onward)
  **Found during:** 01-01 Task 1 (measured at HEAD eaa0afe, before any 01-01 change)
  **What:** `cargo fmt --check` exits 1 with 167 diff hunks across src/main.rs,
  src/audit/mod.rs, src/sanitizer/mod.rs, src/semantic/mod.rs, src/sync/merge.rs,
  src/sync/mod.rs, src/sync/state.rs, src/template/mod.rs and tests/e2e_lifecycle.rs.
  There is no rustfmt.toml; the existing code is wider than rustfmt's default
  100-column width. Every plan in this phase lists `cargo fmt --check` as a gate,
  so that gate cannot pass without a repo-wide reformat, which is outside any
  single plan's scope.
  **How 01-01 handled it:** gated on `rustfmt --check --edition 2024 src/process.rs`
  (new file) plus "workspace hunk count does not exceed the 167 baseline"; edited
  regions in main/template/audit were written rustfmt-clean (count ended at 166).
  **Options:** one `style:` commit running `cargo fmt` repo-wide (touches files
  that plans 01-02..01-11 edit, so do it between waves or before 01-03), or add a
  rustfmt.toml matching the existing width and re-measure.

- e2e tests inherit the host TYPESAFE_API_KEY
  status: resolved (2026-10-19, e285689 — operator chose fix-before-verification;
  every e2e ude spawn goes through ude_command, pinned by
  test_ude_command_drops_host_typesafe_key, negative-controlled)
  **Found during:** 01-10 Task 2 RED (the push output showed `TypeSafe System
  One (Jev) enabled`, so the test binary saw the operator's key)
  **What:** `src/semantic/mod.rs` reads `TYPESAFE_API_KEY` from the process env,
  and every `Command::new(bin)` in tests/e2e_lifecycle.rs except the new
  `sync_cli` helper (01-10) inherits it. A push whose project and upstream both
  have AGENTS.md rules that differ would send the local rules text to the
  TypeSafe API from a test run, and the verdict could change the exit code.
  No current test has both files, so no call was observed; that is unverified
  for runs on hosts with other fixtures.
  **Fix:** `env_remove("TYPESAFE_API_KEY")` on every e2e spawn (or route them all
  through `sync_cli`). 01-10 fixed only its own helper (scope boundary).

- Remote cache checkout is derived as `templates_dir.parent()`
  status: resolved (2026-10-19 — reproduced as push data loss by the security
  audit; operator chose fix-before-verification; 40c9c90..c737383: checkout root
  carried on RemoteCache, failed git status / diff --cached now a hard error)
  **Found during:** 01-11 Task 1 (reading `open_sync_templates` in src/main.rs)
  **What:** `ensure_remote_repo` returns the checkout root itself as
  `templates_dir` when the remote has no `templates/` directory, and
  `open_sync_templates` then takes its parent (`<cache>/ude`) as the checkout, so
  push would run git outside the cache repository. Read from code only, not
  reproduced; a remote without `templates/` is not a supported layout today.
  **Fix:** have `RemoteCache` carry the checkout root explicitly instead of
  deriving it in main.rs.

- init-kept files can be pushed over the shared template
  status: open (operator accepted for phase 01 on 2026-10-19; fix deferred)
  **Found during:** code-review fix for CR-03 (984f0c7)
  **What:** a custom `.gitignore` kept by `ude init` now reads as Locally Modified,
  so the next plain `ude sync push` publishes it as the shared `gitignore.repo` for
  every project.
  **Fix:** tracked as a GSD todo: .planning/todos/pending/2026-10-19-never-publish-files-that-init-kept-over-the-shared-template.md

- State-preserving re-init side effects (confirmed by 01-REVIEW-REINIT.md, both binaries)
  status: open (not fixed in phase 01; candidate for Phase 4, safe re-initialization)
  **Found during:** review of cdfa567/956c5e7 (re-init never writes a recorded file)
  **What:**
  1. `ude init --type <other>` on re-init no longer applies the new type's templates
     (e.g. node over rust leaves `mise.toml` as rust); status shows Upstream Updated and
     a separate pull is needed, but init reports success.
  2. Re-init still runs `git update-index --add --chmod=+x` for every hook, so an
     uncommitted local edit of a tracked hook is staged into the index.
  3. Re-init no longer restores a tracked hook's lost executable bit; sync cannot see a
     mode change, so status stays In Sync while git ignores the hook.

- `.ude/state.json` ignore rule never reaches a project that had its own .gitignore
  status: open (candidate for Phase 4, safe re-initialization)
  **Found during:** final phase 01 verification at cfa26b2 (reproduced with the binary)
  **What:** init keeps a pre-existing project `.gitignore` and never adds the
  `.ude/state.json` / `.ude/.tmp*` lines, so `git add -A` commits state.json; the doctor
  check only flags a state.json that is tracked AND ignored, so it reports Pass. Init's
  per-missing-file warning still fires, so an upstream deletion is warned, not silent.
  **Fix options:** init appends the `.ude/` rules to a kept `.gitignore`, or doctor flags
  a tracked state.json regardless of ignore status.
