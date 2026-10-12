---
last_mapped_commit: 61850bfdb6b4b42c0d948b51354d3dbd1ebe7fb9
last_mapped_at: 2026-10-12
---
# Architecture

**Analysis Date:** 2026-10-11

## Pattern Overview

**Overall:** Modular Monolithic Rust CLI Application (`ude`).

**Key Characteristics:**
- **Single Executable:** Compiled native binary dispatching subcommands (`doctor`, `init`, `project`, `sync`).
- **Filesystem & Subprocess Isolation:** Staging operations occur in isolated temporary workspaces (`tempfile::TempDir`) before atomic commit or materialization.
- **Stateful Convergence Ledger:** Tracks cryptographic file hashes in `.ude/state.json` to enable reliable 3-way reconciliation without requiring persistent daemon processes.
- **Fail-Safe Sanitization Gate:** All outbound pushes to central repositories pass through deterministic pattern and regex filtering to block secret and path leakage.

## Conceptual Layers

```
┌─────────────────────────────────────────────────────────────┐
│                    CLI & Dispatcher                         │
│               (src/main.rs, src/cli/mod.rs)                 │
└──────────────┬───────────────────────────────┬──────────────┘
               │                               │
       ┌───────▼────────┐             ┌────────▼────────┐
       │     Doctor     │             │    Scaffolder   │
       │ (src/audit/)   │             │ (src/template/, │
       └────────────────┘             │  src/manifest/) │
                                      └────────┬────────┘
                                               │
       ┌───────────────────────────────────────▼────────┐
       │               Sync & Reconciliation            │
       │           (src/sync/mod.rs, state.rs,          │
       │                    merge.rs)                   │
       └──────────────┬────────────────────────┬────────┘
                      │                        │
              ┌───────▼────────┐       ┌───────▼────────┐
              │   Sanitizer    │       │ Semantic Rules │
              │(src/sanitizer/)│       │(src/semantic/) │
              └────────────────┘       └────────────────┘
```

### 1. CLI & Command Dispatcher (`src/main.rs`, `src/cli/mod.rs`)

- **Purpose:** Parse command-line flags and environment variables via `clap`, format terminal output, and route commands to specific domain handlers.
- **Dependencies:** Subsystem modules (`audit`, `sync`, `template`, `manifest`).

### 2. Audit Subsystem (`src/audit/mod.rs`)

- **Purpose:** Inspect host machine configuration (SSH keys, GPG signing, global gitconfig, tools) and repository baseline compliance (pre-commit hooks, gitconfig.repo inclusion, allowed signers).
- **Output:** Structured `AuditReport` vectors for human-readable terminal rendering or JSON serialization.

### 3. Template & Scaffolding Engine (`src/template/mod.rs`, `src/manifest/mod.rs`)

- **Purpose:** Render Jinja templates (`templates/`) into target directories using `minijinja`.
- **Manifest:** Maps template archetypes (`rust`, `node`, `polyglot`) to required file structures, hooks, and configuration files.

### 4. Bidirectional Sync Engine (`src/sync/mod.rs`, `src/sync/state.rs`, `src/sync/merge.rs`)

- **Purpose:** Coordinate bidirectional template distribution between developer working trees and the central upstream configuration repository.
- **Components:**
  - `SyncManager`: Handles git clone/fetch/checkout, branch creation, lockfiles, and push publication gates.
  - `StateLedger`: Manages `.ude/state.json` records storing converged ancestor hashes.
  - `MergeEngine`: Performs 3-way text reconciliation with `similar` and generates `.conflict` markers.

### 5. Sanitizer Subsystem (`src/sanitizer/mod.rs`)

- **Purpose:** Redact personal identifiers, hardcoded paths, and sensitive secrets before updates are pushed to shared upstream repositories.
- **Mechanism:** Bidirectional token mapping (e.g. `{{ developer_home }}` <-> `/home/user`).

### 6. Semantic Policy & Rules Subsystem (`src/semantic/mod.rs`)

- **Purpose:** Enforce structural project invariants and policy compliance rules (e.g. `RULE_ZERO.md` enforcement).

## Execution Flows

### Flow 1: Doctor Audit (`ude doctor`)

1. User invokes `ude doctor [--host] [--repo <path>] [--format json]`.
2. `Auditor::new()` initializes check registry.
3. Host checks execute: binary existence, SSH signing keys, global gitconfig format, tool versions.
4. Repo checks execute: hook symlinks, tracked config includes, gitleaks rules.
5. Exit status returned: `0` (clean), `1` (warnings/non-critical drift), `2` (critical failures).

### Flow 2: Project Initialization (`ude init <path> -t <type>`)

1. User invokes `ude init <target_dir> --type rust`.
2. `TemplateEngine` loads archetype manifest.
3. Templates rendered into temporary staging directory: `.gitconfig.repo`, hooks, `.gitignore`, agent guidelines.
4. Target repository initialized (`git init` if needed) and staging files copied safely.
5. Baseline ancestor state recorded in `.ude/state.json`.

### Flow 3: Upstream Sync (`ude sync pull` / `ude sync push`)

1. `SyncManager` obtains advisory file lock on local upstream cache.
2. Central repository fetched / pulled in isolated worktree.
3. **Pull:** Compares local working tree, upstream template, and recorded ancestor in `.ude/state.json`. If 3-way conflict exists, writes `.conflict` files; otherwise safely writes updates and updates ledger.
4. **Push:** Scans local modified templates, applies `Sanitizer`, stages in upstream branch, commits with `--no-verify --no-gpg-sign`.
5. Publication Gate: Inspects `git diff-tree --root` of committed commit; checks blob contents for secrets. If verification passes, pushes upstream or creates PR via `gh`.

## Key Abstractions

- **`AuditReport` & `CheckStatus`** (`src/audit/mod.rs`): Encapsulates individual check results, severity levels (`Pass`, `Warn`, `Fail`, `Critical`), remediation advice, and JSON mapping.
- **`SyncStatusReport` & `SyncFileStatus`** (`src/sync/mod.rs`): Classifies file state as `InSync`, `LocallyModified`, `UpstreamModified`, `Overridden`, `AddedLocally`, or `Deleted`.
- **`StateLedger` & `AncestorRecord`** (`src/sync/state.rs`): Serialized JSON state storing SHA-256 hashes of converged files to disambiguate local modifications from upstream updates.
- **`Sanitizer` & `TokenMap`** (`src/sanitizer/mod.rs`): Parameterizes personal tokens into templated Jinja placeholders.

## Entry Points

- **`src/main.rs`**: Single program entry point. Dispatches to CLI commands, formats terminal and JSON outputs, and terminates with POSIX exit codes.

## Error Handling Strategy

- All internal fallible operations return `Result<T, Box<dyn std::error::Error>>` or domain-specific error types.
- Critical operations (pushes, merges, materializations) employ automatic rollback if any validation step fails.
- Failures print actionable diagnostics in colored stderr text.

---

*Architecture analysis: 2026-10-11*
<!-- refreshed: 2026-10-11 -->
