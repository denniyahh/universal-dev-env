---
last_mapped_commit: 61850bfdb6b4b42c0d948b51354d3dbd1ebe7fb9
last_mapped_at: 2026-10-12
---
# Directory Structure & Organization

**Analysis Date:** 2026-10-11

## Repository Layout

```
universal-dev-env/
├── Cargo.toml                    # Package manifest, dependencies, and edition settings
├── Cargo.lock                    # Pinned dependency lockfile
├── README.md                     # Project overview and usage guide
├── docs/                         # System architecture, specs, and remediation roadmap
│   ├── ARCHITECTURE.md           # High-level architecture specification
│   ├── GLOBAL_MACHINE_BASELINE.md # Machine-level configuration requirements
│   ├── PORTABLE_PROJECT_BASELINE.md # Repository-level configuration standard
│   ├── REMEDIATION_PLAN.md       # Defect remediation roadmap across 4 tracks
│   └── SPECIFICATION.md          # Functional specifications and CLI contracts
├── reviews/                      # External code review artifacts and audits
│   └── adversarial/
│       └── review-codex.md       # Deep adversarial review findings (P1-P3)
├── src/                          # Rust application source tree
│   ├── main.rs                   # Executable entry point and command dispatcher
│   ├── cli/                      # Command line interface argument parsing
│   │   └── mod.rs                # Clap structs, enums, and argument derivations
│   ├── audit/                    # Host and repository drift auditing subsystem
│   │   └── mod.rs                # Auditor, check runner, and report formatting
│   ├── manifest/                 # Archetype and template manifest parsing
│   │   └── mod.rs                # Template file mapping and manifest models
│   ├── sanitizer/                # Token parameterization and secret screening
│   │   └── mod.rs                # Sanitizer rules, token mapping, and regex filters
│   ├── semantic/                 # Semantic rule and policy enforcement
│   │   └── mod.rs                # Policy compliance checkers
│   ├── sync/                     # Bidirectional synchronization engine
│   │   ├── mod.rs                # SyncManager, git operations, and publication gates
│   │   ├── merge.rs              # 3-way text reconciliation and conflict generation
│   │   └── state.rs              # Ancestor ledger (.ude/state.json) persistence
│   └── template/                 # Minijinja template renderer
│       └── mod.rs                # Scaffolding rendering and file emission
├── templates/                    # Jinja templates and archetype baselines
│   ├── projects/                 # Project-level templates
│   │   ├── generic/              # Common project hooks and configs
│   │   │   ├── container/        # Container check scripts
│   │   │   ├── hooks/            # Git hook templates (commit-msg, pre-commit, pre-push)
│   │   │   ├── gitconfig.repo    # Tracked repo git configuration
│   │   │   └── gitignore.repo    # Standard ignore rules
│   │   └── agents/               # AI agent scaffolding templates
│   │       ├── scratch/          # Helper scripts (new-worktree, sync-personal-env)
│   │       ├── agents.md.jinja   # AGENTS.md instructions template
│   │       ├── mempalace.yaml.jinja # MemPalace memory configuration
│   │       └── mise.toml.jinja   # Mise toolchain configuration
│   └── universal/                # Global machine configuration templates
│       ├── generic/              # Global git and security baselines
│       │   ├── gitconfig.global.jinja # Global git configuration template
│       │   ├── allowed_signers.jinja  # SSH commit signing allowed signers
│       │   ├── rules/                 # Global agent behavior rules (RULE_ZERO)
│       │   └── security/              # Security configurations (gitleaks.toml)
│       └── agents/               # IDE and agent settings templates
│           ├── claude/           # Claude settings templates
│           └── codex/            # Codex hook templates
├── tests/                        # Integration test suite
│   └── e2e_lifecycle.rs          # End-to-end CLI workflow tests
└── .planning/                    # GSD project management and codebase intel
    └── codebase/                 # Codebase map documents (STACK, ARCHITECTURE, etc.)
```

## Key Module Responsibilities

| Path | Primary Responsibility | Key Types / Functions |
| :--- | :--- | :--- |
| `src/main.rs` | CLI execution routing, output formatting, process exits | `main`, `format_report` |
| `src/cli/mod.rs` | Argument definition, subcommands, flags | `Cli`, `Commands`, `DoctorArgs`, `InitArgs`, `SyncArgs` |
| `src/audit/mod.rs` | System and repository health checks | `Auditor`, `AuditReport`, `CheckStatus` |
| `src/sync/mod.rs` | Git upstream interactions, advisory locking, push/pull | `SyncManager`, `SyncFileState`, `pull_updates`, `push_to_remote` |
| `src/sync/state.rs` | State ledger loading and saving | `StateLedger`, `AncestorRecord`, `.ude/state.json` |
| `src/sync/merge.rs` | 3-way diffing and conflict handling | `MergeEngine`, `MergeResult`, `resolve_3way` |
| `src/sanitizer/mod.rs` | Secret redaction, path tokenization | `Sanitizer`, `TokenMapping`, `sanitize_content` |
| `src/template/mod.rs` | Jinja template evaluation and filesystem writing | `TemplateEngine`, `render_template`, `write_scaffold` |
| `src/manifest/mod.rs` | Template archetype manifest definitions | `ProjectManifest`, `TemplateFile` |

## Naming & Organization Conventions

- **Submodules:** Each major domain resides in its own subdirectory under `src/` with a `mod.rs` entry point.
- **Error Handling:** Return idiomatic `Result<T, Box<dyn std::error::Error>>` with context.
- **Tests:**
  - Unit tests: Located in `mod.rs` or adjacent `tests.rs` modules using `#[cfg(test)]`.
  - Integration tests: Placed in `tests/*.rs`, executing the compiled binary or public interfaces.
- **Templates:** Jinja templates use `.jinja` suffix if dynamic, or raw filename if static (e.g. `gitleaks.toml`).

---

*Structure analysis: 2026-10-11*
<!-- refreshed: 2026-10-11 -->
