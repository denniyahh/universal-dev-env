# Universal Development Environment & Project Bootstrapper

**Project Status:** DESIGN & EXTRACTION PHASE  
**Origin:** Extracted from DevFlow's development setup audit on 2026-09-13.

## Mission

Automate the capturing, provisioning, auditing, and maintenance of:
1. **Machine-Global Setup (`[GLOBAL]`):** Machine-wide developer configuration, SSH signing architectures, global git hooks, AI agent instructions, toolchain installations, and credential management across Fedora Blue/Kinoite immutable systems.
2. **Portable Project Baseline (`[PATTERN]`):** A zero-drift, drop-in repository scaffold providing git-flow branching, SSH signing includes, chained git hooks (Conventional Commits, secret scanning, protected branch guards), CI/devcontainer container parity, and AI workflow integration.

---

## Directory Structure

```text
universal-dev-env/
├── README.md                           # This overview and roadmap
├── docs/
│   ├── ARCHITECTURE.md                 # Two-tier architecture & design principles
│   ├── GLOBAL_MACHINE_BASELINE.md      # Tier 1: Complete machine-global specification
│   └── PORTABLE_PROJECT_BASELINE.md    # Tier 2: Reusable project baseline specification
└── templates/
    ├── global/                         # Machine-wide configuration templates
    │   ├── allowed_signers.example     # SSH key-to-email mapping for GitHub verification
    │   └── gitconfig.example           # Global ~/.gitconfig baseline
    └── repo/                           # Drop-in project scaffold templates
        ├── .gitconfig                  # Tracked project git-flow & signing include
        ├── .gitignore                  # Standard ignore with allowlist-based inclusions
        └── scripts/
            └── hooks/                  # Non-shadowing, chain-delegating git hooks
                ├── commit-msg          # Conventional Commits validator
                ├── pre-commit          # Branch protection & global hook chaining
                ├── pre-push            # Environment scrubbing & release key check
                └── post-commit         # Non-destructive warning hooks
```

---

## Project Roadmap

- [x] **Phase 1: Extraction & Inventory** — Extract all universal patterns, global specifications, and hook designs from DevFlow's historical audit into a clean baseline.
- [ ] **Phase 2: Project Bootstrapper CLI / Script** — Implement a one-line command (`bootstrap-project <repo-path>`) that initializes any new repository with the portable project baseline.
- [ ] **Phase 3: Machine-Global Drift Scanner** — Implement a CLI scanner (`audit-dev-env`) that asserts host tools, SSH signing keys, allowed signers, global git hooks, and agent configurations against the specification.
