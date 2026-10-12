---
last_mapped_commit: 61850bfdb6b4b42c0d948b51354d3dbd1ebe7fb9
last_mapped_at: 2026-10-12
---
# Technology Stack

**Analysis Date:** 2026-10-11

## Languages

**Primary:**
- Rust (Edition 2024) - All core CLI commands, synchronization engine, audit routines, template renderer, and boundary sanitizers (`src/`).

**Secondary:**
- Bash / POSIX Shell - Repository lifecycle hooks (`templates/projects/generic/hooks/`), scratch utilities (`templates/projects/agents/scratch/`), and container validation scripts (`templates/projects/generic/container/check-in-container.sh`).
- Jinja2 (MiniJinja) - Configuration templates for Git, Mise, MemPalace, Claude, Codex, and agent guidelines (`templates/`).
- TOML - Configuration manifests (`Cargo.toml`, `templates/projects/agents/mise.toml.jinja`, `.ude.toml`).

## Runtime

**Environment:**
- Target: Linux (Fedora Kinoite / Blue / immutable OS, Ubuntu/Debian), macOS, portable POSIX environments.
- Compiled binary: Single statically-linked or dynamic native executable `ude`.
- Host Toolchain: Managed via `mise` (`~/.local/share/mise/installs/`).

**Package Manager & Build:**
- Cargo (Rust package manager)
- Lockfile: `Cargo.lock` present and tracked.
- MiniJinja template syntax: Jinja2-compatible without external Python runtime dependencies.

## Frameworks & Core Libraries

**Core CLI & Ergonomics:**
- `clap` 4.5 (`derive`, `env`) - Command-line parsing with subcommands, environment variable fallback, and typed structures in `src/cli/mod.rs`.
- `colored` 3.0 - Terminal output styling, color-coded doctor diagnostics, and status reports.

**Serialization & Configuration:**
- `serde` 1.0 (`derive`) - Struct serialization/deserialization.
- `serde_json` 1.0 - JSON serialization for `.ude/state.json` and machine-readable doctor output (`--format json`).
- `toml` 0.8 - TOML parsing for project manifests and toolchain configurations.

**Template Engine:**
- `minijinja` 2.7 (`loader`) - In-memory and file-based Jinja template rendering for scaffolding files and global configurations.

**Filesystem & Path Utilities:**
- `walkdir` 2.5 - Recursive directory traversal for scanning repository templates and project state.
- `tempfile` 3.17 - Isolated temporary directories for atomic synchronization staging and hermetic unit/integration test fixtures.
- `directories` 6.0 - Standard cross-platform XDG base directory resolution (`XDG_CONFIG_HOME`, `XDG_DATA_HOME`).

**Hashing & Comparison:**
- `sha2` 0.10 - Cryptographic SHA-256 computation for ancestor ledger state tracking and file drift detection.
- `similar` 2.7 - Myers text diffing engine for 3-way reconciliation and conflict markers.

**Process & System:**
- `which` 7.0 - Runtime binary discovery on `$PATH` for doctor checks (`git`, `gh`, `mise`, `gitleaks`).
- `regex` 1.11 - Sanitizer expression matching, token boundary detection, and secret pattern screening in `src/sanitizer/mod.rs`.

**Testing Framework:**
- Rust built-in test harness (`cargo test`).
- `proptest` 1.6 (`dev-dependencies`) - Property-based testing for template parsing and path validation.

## Key Dependencies

**Critical:**
- `clap` 4.5 - Dictates CLI argument structure and subcommand routing (`doctor`, `init`, `project`, `sync`).
- `minijinja` 2.7 - Powers all baseline scaffolding rendering without requiring Python or external runtimes.
- `sha2` 0.10 - Forms the integrity foundation of `.ude/state.json` ancestor tracking.
- `similar` 2.7 - Generates unified diffs and detects conflict regions during sync operations.
- `tempfile` 3.17 - Ensures staging operations do not pollute production directories or leak partial states.

**Infrastructure:**
- `walkdir` 2.5 - Scans projects and template trees for change detection.
- `regex` 1.11 - Enforces credential sanitization before upstream synchronization.

## Configuration

**Environment:**
- Subprocess execution respects standard Git environment isolation (`GIT_DIR`, `GIT_WORK_TREE`, `GIT_CONFIG`).
- Host doctor inspects `XDG_CONFIG_HOME`, `SSH_AUTH_SOCK`, `GPG_AGENT_INFO`.

**Build Configuration:**
- `Cargo.toml` specifies edition 2024 and compiler profiles.
- Clippy rules enforce `-D warnings` across workspace targets.

## Platform Requirements

**Development:**
- Rust compiler 1.85+ (Edition 2024 support).
- Git 2.38+ (supports `diff-tree --root`, `sparse-checkout`, and safe directory configurations).
- External tools (optional/recommended): `gh`, `gitleaks`, `mise`.

**Production:**
- Native binary `ude` installed to user path (e.g. `~/.local/bin/ude`).
- File system access to target git repositories and user config paths.

---

*Stack analysis: 2026-10-11*
<!-- refreshed: 2026-10-11 -->
