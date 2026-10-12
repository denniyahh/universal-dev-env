---
last_mapped_commit: 61850bfdb6b4b42c0d948b51354d3dbd1ebe7fb9
last_mapped_at: 2026-10-12
---
# Testing Patterns & Verification Practices

**Analysis Date:** 2026-10-11

## Test Structure Overview

The test suite consists of module-level unit tests embedded alongside implementation code and an end-to-end integration test harness.

```
universal-dev-env/
├── src/
│   ├── sync/mod.rs               # Unit tests for sync status, 3-way merge, security gates
│   ├── sanitizer/mod.rs          # Unit tests for token extraction and secret screening
│   ├── template/mod.rs           # Unit tests for template rendering and context injection
│   └── audit/mod.rs              # Unit tests for host and repo health audit checks
└── tests/
    └── e2e_lifecycle.rs          # Integration tests for full CLI lifecycles (init, doctor, sync)
```

## Test Types & Frameworks

### 1. Unit Tests (`cargo test --lib`)

- **Location:** Embedded in `#[cfg(test)]` modules within each source file.
- **Coverage Areas:**
  - `src/sync/mod.rs`: Branch slug sanitization, ancestor hash tracking, status classification (`InSync`, `LocallyModified`, `UpstreamModified`, `Overridden`), write target symlink traversal validation.
  - `src/sanitizer/mod.rs`: Path token replacement, secret token detection, reversible placeholder substitution.
  - `src/template/mod.rs`: Minijinja rendering, variable substitution, missing key handling.
  - `src/audit/mod.rs`: Tool existence checks, SSH/GPG signing validation, exit code assignments.

### 2. Integration & E2E Tests (`tests/e2e_lifecycle.rs`)

- **Execution:** `cargo test --test e2e_lifecycle`
- **Scenarios:**
  - Full project initialization (`test_cli_e2e_init_and_sync_lifecycle`): Creates a bare remote, clones a seed repo, runs `ude init`, and runs `ude sync status`.
  - Push failure handling (`test_sync_push_remote_failure_does_not_advance_ancestor_state`): Verifies that if remote push fails or is rejected, local `.ude/state.json` is NOT falsely updated.
  - PR mode ancestor immutability (`test_sync_push_pr_does_not_advance_ancestor_state`): Verifies that creating a PR branch does not advance main ancestor records until merged.

### 3. Property-Based Testing (`proptest`)

- **Framework:** `proptest` 1.6 (`dev-dependencies` in `Cargo.toml`).
- **Focus:** Validates boundary inputs for path sanitization, string tokenization, and non-UTF-8 binary handling across arbitrarily generated character sequences.

## Fixture Management & Environment Isolation

### 1. Temporary Repositories

- Every test uses `tempfile::TempDir` to generate independent, ephemeral directories:
  ```rust
  let temp = tempfile::tempdir().unwrap();
  let root = temp.path();
  ```
- No files are written to host configuration paths (`~/.gitconfig`, `~/.ssh`).

### 2. Git Subprocess Hermeticity

- Fixture helpers explicitly clear Git environment variables on spawned processes:
  ```rust
  let mut cmd = Command::new("git");
  cmd.current_dir(&repo_dir)
     .env_remove("GIT_DIR")
     .env_remove("GIT_WORK_TREE")
     .env_remove("GIT_INDEX_FILE")
     .env_remove("GIT_CONFIG");
  ```

## Negative Controls & Boundary Verification

- **Symlink Traversal Attack Test:**
  Verifies that attempting to write through an existing symlink or write outside the repo boundary returns an explicit `Err` and aborts file write.
- **Post-Commit Secret Rejection Test:**
  Verifies that injecting an apparent secret token into a staged commit triggers the publication gate, rolls back the commit, and prevents remote transmission.
- **Divergent Ancestor Conflict Test:**
  Verifies that concurrent edits on local and remote branches without shared ancestor generate a `.conflict` file and preserve user work.

## Running Tests

```bash

# Run all unit and integration tests

cargo test

# Run tests with output printed

cargo test -- --nocapture

# Run clippy with warnings denied

cargo clippy --workspace --all-targets -- -D warnings

# Run specific E2E integration test

cargo test --test e2e_lifecycle test_cli_e2e_init_and_sync_lifecycle
```

---

*Testing analysis: 2026-10-11*
<!-- refreshed: 2026-10-11 -->
