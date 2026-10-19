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
