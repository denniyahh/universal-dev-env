<!-- GSD:project-start source:PROJECT.md -->

## Project

**Universal Development Environment (UDE)**

Universal Development Environment (UDE) is a modular, high-assurance CLI tool and configuration engine written in Rust that provisions, audits, and bidirectionally synchronizes portable developer environments, agent instructions, and security baselines across machines and repositories. It ensures local developer workflows, AI agent environments, and security guardrails stay consistent without manual friction or configuration drift.

**Core Value:** Reliable, hermetic, bidirectional developer environment and agent baseline synchronization with cryptographic ancestor state tracking and zero credential leakage.

### Constraints

- **Language / Edition:** Rust Edition 2024 with zero clippy warnings (`-D warnings`).
- **Platform Compatibility:** POSIX compatibility without platform-specific shell dependencies; immutable OS compliance (no assumptions of mutable root filesystem or package managers like `dnf`).
- **Hermetic Testing:** Every test must run in isolated `tempfile::TempDir` fixtures, clearing Git environment variables (`GIT_DIR`, `GIT_WORK_TREE`, `GIT_CONFIG`) to avoid host pollution.
- **Security & Integrity:** Never emit raw credentials or bypass publication validation gates on automated remote pushes.

<!-- GSD:project-end -->

<!-- GSD:stack-start source:codebase/STACK.md -->

## Technology Stack

## Languages

- Rust (Edition 2024) - All core CLI commands, synchronization engine, audit routines, template renderer, and boundary sanitizers (`src/`).
- Bash / POSIX Shell - Repository lifecycle hooks (`templates/projects/generic/hooks/`), scratch utilities (`templates/projects/agents/scratch/`), and container validation scripts (`templates/projects/generic/container/check-in-container.sh`).
- Jinja2 (MiniJinja) - Configuration templates for Git, Mise, MemPalace, Claude, Codex, and agent guidelines (`templates/`).
- TOML - Configuration manifests (`Cargo.toml`, `templates/projects/agents/mise.toml.jinja`, `.ude.toml`).

## Runtime

- Target: Linux (Fedora Kinoite / Blue / immutable OS, Ubuntu/Debian), macOS, portable POSIX environments.
- Compiled binary: Single statically-linked or dynamic native executable `ude`.
- Host Toolchain: Managed via `mise` (`~/.local/share/mise/installs/`).
- Cargo (Rust package manager)
- Lockfile: `Cargo.lock` present and tracked.
- MiniJinja template syntax: Jinja2-compatible without external Python runtime dependencies.

## Frameworks & Core Libraries

- `clap` 4.5 (`derive`, `env`) - Command-line parsing with subcommands, environment variable fallback, and typed structures in `src/cli/mod.rs`.
- `colored` 3.0 - Terminal output styling, color-coded doctor diagnostics, and status reports.
- `serde` 1.0 (`derive`) - Struct serialization/deserialization.
- `serde_json` 1.0 - JSON serialization for `.ude/state.json` and machine-readable doctor output (`--format json`).
- `toml` 0.8 - TOML parsing for project manifests and toolchain configurations.
- `minijinja` 2.7 (`loader`) - In-memory and file-based Jinja template rendering for scaffolding files and global configurations.
- `walkdir` 2.5 - Recursive directory traversal for scanning repository templates and project state.
- `tempfile` 3.17 - Isolated temporary directories for atomic synchronization staging and hermetic unit/integration test fixtures.
- `directories` 6.0 - Standard cross-platform XDG base directory resolution (`XDG_CONFIG_HOME`, `XDG_DATA_HOME`).
- `sha2` 0.10 - Cryptographic SHA-256 computation for ancestor ledger state tracking and file drift detection.
- `similar` 2.7 - Myers text diffing engine for 3-way reconciliation and conflict markers.
- `which` 7.0 - Runtime binary discovery on `$PATH` for doctor checks (`git`, `gh`, `mise`, `gitleaks`).
- `regex` 1.11 - Sanitizer expression matching, token boundary detection, and secret pattern screening in `src/sanitizer/mod.rs`.
- Rust built-in test harness (`cargo test`).
- `proptest` 1.6 (`dev-dependencies`) - Property-based testing for template parsing and path validation.

## Key Dependencies

- `clap` 4.5 - Dictates CLI argument structure and subcommand routing (`doctor`, `init`, `project`, `sync`).
- `minijinja` 2.7 - Powers all baseline scaffolding rendering without requiring Python or external runtimes.
- `sha2` 0.10 - Forms the integrity foundation of `.ude/state.json` ancestor tracking.
- `similar` 2.7 - Generates unified diffs and detects conflict regions during sync operations.
- `tempfile` 3.17 - Ensures staging operations do not pollute production directories or leak partial states.
- `walkdir` 2.5 - Scans projects and template trees for change detection.
- `regex` 1.11 - Enforces credential sanitization before upstream synchronization.

## Configuration

- Subprocess execution respects standard Git environment isolation (`GIT_DIR`, `GIT_WORK_TREE`, `GIT_CONFIG`).
- Host doctor inspects `XDG_CONFIG_HOME`, `SSH_AUTH_SOCK`, `GPG_AGENT_INFO`.
- `Cargo.toml` specifies edition 2024 and compiler profiles.
- Clippy rules enforce `-D warnings` across workspace targets.

## Platform Requirements

- Rust compiler 1.85+ (Edition 2024 support).
- Git 2.38+ (supports `diff-tree --root`, `sparse-checkout`, and safe directory configurations).
- External tools (optional/recommended): `gh`, `gitleaks`, `mise`.
- Native binary `ude` installed to user path (e.g. `~/.local/bin/ude`).
- File system access to target git repositories and user config paths.

<!-- GSD:stack-end -->

<!-- GSD:conventions-start source:CONVENTIONS.md -->

## Conventions

## Rust Language & Edition Idioms

- **Rust Edition 2024:**

## Error Handling & Exit Codes

- **Result Propagation:**
- **Process Exit Code Contract:**
- **Terminal Messaging:**

## Path Safety & Filesystem Hygiene

- **Anti-Symlink Traversal (`validate_safe_write_target`):**
- **Atomic Operations:**
- **Non-UTF-8 & Binary Support:**

## Git Subprocess Execution Guardrails

- **Environment Hermeticity:**
- **Automation Flags:**
- **Post-Commit Publication Gate:**

## Testing Conventions & Negative Controls

- **Hermetic Test Environments:**
- **Negative Controls Mandatory:**

<!-- GSD:conventions-end -->

<!-- GSD:architecture-start source:ARCHITECTURE.md -->

## Architecture

## Pattern Overview

- **Single Executable:** Compiled native binary dispatching subcommands (`doctor`, `init`, `project`, `sync`).
- **Filesystem & Subprocess Isolation:** Staging operations occur in isolated temporary workspaces (`tempfile::TempDir`) before atomic commit or materialization.
- **Stateful Convergence Ledger:** Tracks cryptographic file hashes in `.ude/state.json` to enable reliable 3-way reconciliation without requiring persistent daemon processes.
- **Fail-Safe Sanitization Gate:** All outbound pushes to central repositories pass through deterministic pattern and regex filtering to block secret and path leakage.

## Conceptual Layers

```

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

### 5. Sanitizer Subsystem (`src/sanitizer/mod.rs`)

- **Purpose:** Redact personal identifiers, hardcoded paths, and sensitive secrets before updates are pushed to shared upstream repositories.
- **Mechanism:** Bidirectional token mapping (e.g. `{{ developer_home }}` <-> `/home/user`).

### 6. Semantic Policy & Rules Subsystem (`src/semantic/mod.rs`)

- **Purpose:** Enforce structural project invariants and policy compliance rules (e.g. `RULE_ZERO.md` enforcement).

## Execution Flows

### Flow 1: Doctor Audit (`ude doctor`)

### Flow 2: Project Initialization (`ude init <path> -t <type>`)

### Flow 3: Upstream Sync (`ude sync pull` / `ude sync push`)

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

<!-- GSD:architecture-end -->

<!-- GSD:skills-start source:skills/ -->

## Project Skills

No project skills found. Add skills to any of: `.agents/skills/`, `.agents/skills/`, `.cursor/skills/`, `.github/skills/`, or `.codex/skills/` with a `SKILL.md` index file.
<!-- GSD:skills-end -->

<!-- GSD:workflow-start source:GSD defaults -->

## GSD Workflow Enforcement

Before using Edit, Write, or other file-changing tools, start work through a GSD command so planning artifacts and execution context stay in sync.

Use these entry points:
- `$gsd-fast` for a trivial task inline, with no subagents and no PLAN.md
- `$gsd-quick` for small fixes, doc updates, and ad-hoc tasks
- `$gsd-debug` for investigation and bug fixing
- `$gsd-execute-phase` for planned phase work

Do not make direct repo edits outside a GSD workflow unless the user explicitly asks to bypass it.
<!-- GSD:workflow-end -->

<!-- GSD:profile-start -->

## Developer Profile

> Profile not yet configured. Run `$gsd-profile-user` to generate your developer profile.
> This section is managed by `generate-claude-profile` -- do not edit manually.
<!-- GSD:profile-end -->
