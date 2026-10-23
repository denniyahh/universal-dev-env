# Requirements: Universal Development Environment (UDE)

**Defined:** 2026-10-12
**Core Value:** Reliable, hermetic, bidirectional developer environment and agent baseline synchronization with cryptographic ancestor state tracking and zero credential leakage.

## v1 Requirements

Requirements for initial release, mapping directly to remediation tracks from [`docs/REMEDIATION_PLAN.md`](file:///home/denniyahh/Github/universal-dev-env/docs/REMEDIATION_PLAN.md).

### Sync Engine & State Hermeticity (Track 1)

- [x] **SYNC-01**: `pull_updates` tracks deletion tombstones in `.ude/state.json` to propagate upstream deletions cleanly without resurrecting deleted files.
- [x] **SYNC-02**: Synchronization materialization uses a two-phase transactional staging directory before atomically updating project working trees.
- [x] **SYNC-03**: All subprocess executions (`git`, `gh`) enforce bounded timeouts (30s-60s) with active process cancellation to prevent hangs.
- [x] **SYNC-04**: Cryptographic SHA-256 ancestor hash calculation normalizes CRLF line endings to prevent false drift detection across platforms.
- [x] **SYNC-05**: Divergent 3-way conflicts generate structured `.conflict` files with standard Myers conflict markers while preserving user modifications.
- [x] **SYNC-06**: `push_to_remote` reconciles push candidates safely and verifies branch head continuity before publishing.

### Sanitizer, Boundaries & Security (Track 2)

- [ ] **SEC-01**: `Sanitizer` calculates Shannon entropy to detect high-entropy tokens and raw Base64 credentials lacking obvious prefixes.
- [ ] **SEC-02**: Outbound content screening expands regex token patterns to cover full Gitleaks secret categories before git staging.
- [ ] **SEC-03**: Path parameterization enforces strict word and punctuation boundaries (`\b` and path delimiters) to prevent partial substring corruption.
- [ ] **SEC-04**: Sanitizer verifies token substitution reversibility between templated placeholders and local machine environments.
- [ ] **SEC-05**: TypeSafe semantic drift evaluation fails closed below 0.85 confidence or 0.20 top-two margin, and never transmits unscanned or local-only content.

### Doctor & CLI Hardening (Track 3)

- [ ] **CLI-01**: Host doctor implements strict OpenSSH public key parser validating `ssh-ed25519` key structures and comment metadata.
- [ ] **CLI-02**: Doctor audits both filesystem executable permissions and Git index mode flags (`100755` vs `100644`) for all repository hooks.
- [ ] **CLI-03**: Repo doctor canonicalizes relative include paths relative to the Git repository root (`.git/`) to prevent false warnings in subdirectories.
- [ ] **CLI-04**: All CLI subcommands enforce the strict POSIX exit code contract (`0` = clean, `1` = warning/drift, `2` = critical failure).

### Templates, Archetypes & Manifest (Track 4)

- [ ] **TMPL-01**: All hardcoded personal repository references (`github.com/d10475u5/...`) are decoupled into parameterized `.ude.toml` and manifest variables.
- [ ] **TMPL-02**: Project re-initialization detects existing customized files and creates timestamped backups before applying template updates.
- [ ] **TMPL-03**: Minijinja template rendering enables strict undefined variable checking, failing fast with descriptive context on missing variables.
- [ ] **TMPL-04**: System wires `.ude.toml` configuration loading, establishing explicit precedence between project settings and global defaults.

## v2 Requirements

Deferred to future releases.

### Advanced Sync & Mesh

- **SYNC-V2-01**: Multi-remote mesh synchronization across distributed developer forks.
- **SYNC-V2-02**: Sparse checkout synchronization for monorepo environments.

### Advanced Security

- **SEC-V2-01**: Hardware security key (FIDO2/YubiKey) touch-policy enforcement in doctor checks.
- **SEC-V2-02**: Automatic pre-receive hook installation on remote Git servers.

## Out of Scope

Explicitly excluded to maintain high assurance and zero complexity creep.

| Feature | Reason |
|---------|--------|
| Background Daemon / FS Watcher | Prevents process leaks, CPU spin, and file lock contention on immutable Linux hosts |
| Web UI / Cloud SaaS Backend | UDE is strictly decentralized and Git-backed; no centralized server dependencies |
| Interactive TUI Wizard | All workflows operate via standard CLI flags or structured JSON for agent and CI automation |

## Traceability

Which phases cover which requirements. Updated during roadmap creation.

| Requirement | Phase | Status |
|-------------|-------|--------|
| SYNC-01 | Phase 1 | Complete |
| SYNC-02 | Phase 1 | Complete |
| SYNC-03 | Phase 1 | Complete |
| SYNC-04 | Phase 1 | Complete |
| SYNC-05 | Phase 1 | Complete |
| SYNC-06 | Phase 1 | Complete |
| SEC-01 | Phase 2 | Pending |
| SEC-02 | Phase 2 | Pending |
| SEC-03 | Phase 2 | Pending |
| SEC-04 | Phase 2 | Pending |
| CLI-01 | Phase 3 | Pending |
| CLI-02 | Phase 3 | Pending |
| CLI-03 | Phase 3 | Pending |
| CLI-04 | Phase 3 | Pending |
| SEC-05 | Phase 3 | Pending |
| TMPL-01 | Phase 4 | Pending |
| TMPL-02 | Phase 4 | Pending |
| TMPL-03 | Phase 4 | Pending |
| TMPL-04 | Phase 4 | Pending |

**Coverage:**
- v1 requirements: 19 total
- Mapped to phases: 19
- Unmapped: 0

---
*Requirements defined: 2026-10-12*
*Last updated: 2026-10-12 after initialization*
