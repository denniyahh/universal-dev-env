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
