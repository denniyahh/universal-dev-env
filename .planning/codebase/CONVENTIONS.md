---
last_mapped_commit: 61850bfdb6b4b42c0d948b51354d3dbd1ebe7fb9
last_mapped_at: 2026-10-12
---
# Coding Conventions & Standards

**Analysis Date:** 2026-10-11

## Rust Language & Edition Idioms

- **Rust Edition 2024:**
  - Modern syntax and language features enabled (`edition = "2024"` in `Cargo.toml`).
  - Strict FFI rules: Any external C interface must use `unsafe extern "C"` syntax.
  - Zero compiler warnings: Enforced via `cargo clippy --workspace --all-targets -- -D warnings`.
  - Format adherence: Strictly formatted with standard `cargo fmt`.

## Error Handling & Exit Codes

- **Result Propagation:**
  - Internal operations return standard `Result<T, Box<dyn std::error::Error>>` or specific module errors.
  - Panic avoidance: Functions avoid `unwrap()` or `expect()` in production codepaths; explicit pattern matching or `?` error propagation is used.
- **Process Exit Code Contract:**
  - `0`: Success (all checks passed or command executed cleanly).
  - `1`: Non-critical failure or detected drift (e.g., doctor found non-critical warnings, sync found drift).
  - `2`: Critical failure (security violation, fatal system error, missing critical dependency).
- **Terminal Messaging:**
  - Color styling via `colored` crate:
    - Green (`.green()`): Successful verification or pass status.
    - Yellow (`.yellow()`): Warnings, non-critical drift, skipped items.
    - Red (`.red()`): Errors, security alerts, failed checks.
    - Cyan (`.cyan()`): Informational headers, category dividers.

## Path Safety & Filesystem Hygiene

- **Anti-Symlink Traversal (`validate_safe_write_target`):**
  - All file write targets during synchronization are verified before materialization.
  - Ensures targets and their parent directories are canonicalized and do not resolve through unauthorized symlinks or traverse outside the destination workspace.
- **Atomic Operations:**
  - Multi-file updates use temporary workspaces (`tempfile::TempDir`) before copying to final destinations.
  - Failed operations clean up staging directories automatically on drop.
- **Non-UTF-8 & Binary Support:**
  - File reading and hash generation operate on raw byte slices (`Vec<u8>`), not lossy UTF-8 conversions, ensuring binary files are tracked cleanly without corrupting line endings.

## Git Subprocess Execution Guardrails

- **Environment Hermeticity:**
  - Any child process running `git` scrubs workspace-polluting environment variables:
    `GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE`, `GIT_CONFIG`, `GIT_COMMON_DIR`.
  - Commands use explicit working directories (`cmd.current_dir(...)`).
- **Automation Flags:**
  - Automated staging commits pass `--no-verify` and `--no-gpg-sign` to avoid recursive hook invocation or hanging on interactive GPG/SSH agent prompts.
- **Post-Commit Publication Gate:**
  - Every commit created for remote push is inspected immediately using `git diff-tree --root -r`.
  - Committed blob contents are screened for unexpected files and leaked credentials.
  - If any unexpected file or secret pattern is detected, the commit is rolled back (`git reset --hard HEAD~1`) before any push is attempted.

## Testing Conventions & Negative Controls

- **Hermetic Test Environments:**
  - Every test fixture creates isolated temporary repositories via `tempfile::TempDir`.
  - Never mutate process-global environment variables (`std::env::set_var` is avoided to prevent test runner races).
- **Negative Controls Mandatory:**
  - Tests verify both the happy path and the rejection/error path (e.g. testing that valid branches push cleanly, AND testing that malformed slash branches or commits with secrets are rejected).
  - Boundary conditions (empty files, non-UTF-8 bytes, conflicting ancestors) are explicitly tested.

---

*Conventions analysis: 2026-10-11*
<!-- refreshed: 2026-10-11 -->
