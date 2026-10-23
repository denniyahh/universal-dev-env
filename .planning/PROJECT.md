# Universal Development Environment (UDE)

## What This Is

Universal Development Environment (UDE) is a modular, high-assurance CLI tool and configuration engine written in Rust that provisions, audits, and bidirectionally synchronizes portable developer environments, agent instructions, and security baselines across machines and repositories. It ensures local developer workflows, AI agent environments, and security guardrails stay consistent without manual friction or configuration drift.

## Core Value

Reliable, hermetic, bidirectional developer environment and agent baseline synchronization with cryptographic ancestor state tracking and zero credential leakage.

## Requirements

### Validated

- ✓ **Publication Secret Scanning Gate** — Committed tree inspected via `git diff-tree --root` and blob contents screened before push; automated commit rollback on detected secrets. (`src/sync/mod.rs`)
- ✓ **Safe PR Synchronization Mode** — Dedicated branch slugs, forced tracking checkout, and restoration of canonical branch state on completion. (`src/sync/mod.rs`)
- ✓ **Branch Lock Slug Sanitization** — Filesystem-safe slug translation preventing directory creation failures on slash-delimited branch locks. (`src/sync/mod.rs`)
- ✓ **Anti-Symlink Traversal Validation** — Target paths and parent directories validated to block writes through malicious external symlinks. (`src/sync/mod.rs`)
- ✓ **Binary & Non-UTF-8 State Tracking** — Raw byte SHA-256 fingerprinting and converged ancestor reconciliation in `.ude/state.json`. (`src/sync/mod.rs`, `src/sync/state.rs`)
- ✓ **Machine & Repository Doctor Audits** — System checks for SSH signing keys, tools (`git`, `gh`, `mise`, `gitleaks`), hooks, and global configs with text/JSON outputs and exit-code contracts. (`src/audit/mod.rs`)
- ✓ **Track 1: Sync Engine & State Hermeticity** — Deletion tombstones, two-phase staged materialization, push continuity checks, CRLF normalization, diff3 `.conflict` sidecars with `ude sync resolve`, and bounded subprocess timeouts. — Phase 1 (re-verified 18/18 on Linux 2026-10-22 after two code reviews and an adversarial review; Windows runtime checks deferred to Phase 999.1)
- ✓ **Jinja Archetype Scaffolding** — Project initialization across Rust, Node, and Polyglot archetypes using Minijinja with embedded hooks and agent guidelines. (`src/template/mod.rs`, `src/manifest/mod.rs`)

### Active

- [ ] **Track 2: Sanitizer, Boundaries & Security** — Implement Shannon entropy and Base64 credential screening, and strict word and punctuation boundary matching for path parameterization. (TypeSafe System One confidence guardrails moved to Phase 3 as SEC-05, 2026-10-23.)
- [ ] **Track 3: Doctor & CLI Hardening** — Implement strict OpenSSH public key parsing, dual disk/git-index executable bit auditing for hooks, relative include path resolution against repository roots, and strict exit code contracts (1 for warnings, 2 for critical errors).
- [ ] **Track 4: Templates, Archetypes & Manifest** — Decouple personal repository URLs into parameterized manifest variables, implement safe re-initialization with backup semantics, enforce strict Minijinja undefined variable checking, and wire `.ude.toml` policy loading.

### Out of Scope

- **Background Daemon / Continuous Watcher** — UDE is an on-demand, explicit CLI tool (`doctor`, `init`, `sync`). Running long-lived background daemons introduces process leaks and file lock contention on immutable OS hosts.
- **Hosted Web Dashboard / Cloud Service** — Environment configuration and synchronization are decentralized and Git-backed; no centralized web service or SaaS dependency is planned.
- **Interactive TUI / GUI** — Output is formatted for standard terminal emulators (ANSI color) and machine-readable JSON pipelines (`--format json`).

## Context

- **Host Environment:** Tested and optimized for modern POSIX environments including immutable Fedora Kinoite / Blue Linux and macOS.
- **Upstream Repository:** Stores central canonical configuration templates and archetypes; synchronization uses Git protocol with advisory file locks.
- **Prior Adversarial Review:** An external adversarial audit identified 25 confirmed defects across synchronization, sanitization, and CLI audits. P1 defects are verified and remediated; the remaining 20 defects form the four execution tracks of [`docs/REMEDIATION_PLAN.md`](file:///home/denniyahh/Github/universal-dev-env/docs/REMEDIATION_PLAN.md).

## Constraints

- **Language / Edition:** Rust Edition 2024 with zero clippy warnings (`-D warnings`).
- **Platform Compatibility:** POSIX compatibility without platform-specific shell dependencies; immutable OS compliance (no assumptions of mutable root filesystem or package managers like `dnf`).
- **Hermetic Testing:** Every test must run in isolated `tempfile::TempDir` fixtures, clearing Git environment variables (`GIT_DIR`, `GIT_WORK_TREE`, `GIT_CONFIG`) to avoid host pollution.
- **Security & Integrity:** Never emit raw credentials or bypass publication validation gates on automated remote pushes.

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Focus milestone on 4-track remediation plan | Systematic closure of all confirmed defects from adversarial review ensures production reliability | ✓ Good |
| Offline Git fixtures for sync verification | Eliminates network flakiness and allows high-assurance regression testing of sync protocols | ✓ Good |
| SHA-256 raw byte fingerprinting for state ledger | Disambiguates local modifications from upstream updates without parsing non-UTF-8 files | ✓ Good |
| Native Rust Minijinja instead of external template engine | Avoids external runtime dependencies (Python/Node) for core CLI scaffolding | ✓ Good |
| Fresh-clone init keeps committed managed files; one with no template is left Unmanaged (kept tombstone) | Without state.json a clone cannot tell a template deleted upstream from one that never existed; Unmanaged is the only state that neither deletes data nor resurrects a template | — Pending (Phase 1; stale copies stay on clones) |
| Init refuses any symlinked write destination (exit 2) before writing anything | Hermeticity: init must never write outside the project | ✓ Good (Phase 1) |
| Push never deletes a shared template that contains logic (D-14 amended) | A local deletion of a rendered file such as mise.toml would remove the template every project renders from | ✓ Good (Phase 1, operator) |
| With a remote, pull deletes a file only when the remote's history shows its template was deleted (D-12 amended); a template the remote never had is "Missing upstream" and kept | A missing template alone cannot tell a deletion from a remote that never carried it; deleting on that guess loses data | ✓ Good (Phase 1, operator) |
| Supply-chain advisories block pushes (pre-push and CI); Dependabot alerts only until git flow | A known-vulnerable dependency should not ship; security-fix PRs would bypass a future `develop` branch | — Pending |

## Evolution

This document evolves at phase transitions and milestone boundaries.

**After each phase transition** (via `/gsd-transition`):
1. Requirements invalidated? → Move to Out of Scope with reason
2. Requirements validated? → Move to Validated with phase reference
3. New requirements emerged? → Add to Active
4. Decisions to log? → Add to Key Decisions
5. "What This Is" still accurate? → Update if drifted

**After each milestone** (via `/gsd-complete-milestone`):
1. Full review of all sections
2. Core Value check — still the right priority?
3. Audit Out of Scope — reasons still valid?
4. Update Context with current state

---
*Last updated: 2026-10-22 after Phase 1 re-verification*
