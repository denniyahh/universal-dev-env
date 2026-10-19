# Roadmap: Universal Development Environment (UDE)

## Overview

Systematic execution of the 4-track remediation plan derived from external adversarial review findings. The roadmap establishes high-assurance bidirectional synchronization and security guardrails across 4 sequential engineering phases: hardening the core Git synchronization engine and state ledger, strengthening credential sanitizers and path boundaries, enforcing strict doctor audit contracts, and decoupling template archetypes into configurable manifests.

## Phases

- [ ] **Phase 1: Sync Engine & State Hermeticity** - Deletion tombstones, 2-phase shadow materialization, CRLF normalization, Myers diff conflicts, and bounded subprocess timeouts.
- [ ] **Phase 2: Sanitizer, Boundaries & Security Hardening** - Shannon entropy analysis, expanded Gitleaks-equivalent regex screening, boundary-safe path parameterization, and reversible token verification.
- [ ] **Phase 3: Doctor & CLI Hardening** - Strict OpenSSH public key parsing, dual disk/git-index hook mode audits, canonical relative include resolution against `.git/`, and strict POSIX exit codes.
- [ ] **Phase 4: Templates, Archetypes & Manifest Decoupling** - Parameterized `.ude.toml` manifest variables, safe re-initialization with backup semantics, strict Minijinja undefined checks, and config precedence.

## Phase Details

### Phase 1: Sync Engine & State Hermeticity

**Goal**: Ensure bidirectional synchronization is hermetic, crash-resilient, and atomic without phantom drift or zombie file resurrections.
**Depends on**: Nothing (first phase)
**Requirements**: SYNC-01, SYNC-02, SYNC-03, SYNC-04, SYNC-05, SYNC-06
**Success Criteria** (what must be TRUE):
  1. Deletion of an upstream template propagates locally on `ude sync pull` via state tombstones without resurrecting on subsequent pushes.
  2. Multi-file synchronization materializes in a temporary shadow folder before atomic update, preventing partial workspace writes on disk/permission errors.
  3. All `git` and `gh` subprocess calls enforce bounded timeouts (30s-60s) and terminate hung child processes automatically.
  4. SHA-256 ancestor fingerprints normalize CRLF line endings, preventing false drift reports across POSIX and Windows environments.
  5. Divergent concurrent modifications generate structured `.conflict` files with Myers conflict markers while preserving local developer changes.
  6. Outbound push verifies branch continuity and rejects divergent push candidates safely.

**Plans**: 11/11 plans executed

Plans:
**Wave 1**
- [x] 01-01: Bounded process-group runner (`src/process.rs`) and the 10 git calls outside `src/sync/` (SYNC-03).
- [x] 01-02: `GitFixture` helper making test git commands hermetic in sync unit and e2e tests (SYNC-03).

**Wave 2** *(blocked on Wave 1 completion)*
- [x] 01-03: Sync's 31 git/gh calls on the runner, test-only env/timeout injection, std file lock (D-04), inherited-`GIT_DIR` e2e test, hermeticity check (SYNC-03).

**Wave 3** *(blocked on Wave 2 completion)*
- [x] 01-04: Corrupt or newer `state.json` fails loudly (D-06, forward-version guard), atomic staged state save, exit codes 1/2 (SYNC-02).

**Wave 4** *(blocked on Wave 3 completion)*
- [x] 01-05: CRLF-insensitive fingerprints, classification by hash, state schema v2 migration (D-07), line-ending-preserving writes (SYNC-04).

**Wave 5** *(blocked on Wave 4 completion)*
- [x] 01-06: Stage-validate-commit for pull/push writes, explicit file modes, `state.json` written last (D-05) (SYNC-02).

**Wave 6** *(blocked on Wave 5 completion)*
- [x] 01-07: Deletion statuses, tombstones, pull/push deletions, publication gate that accepts deletions (D-12–D-16) (SYNC-01).

**Wave 7** *(blocked on Wave 6 completion)*
- [x] 01-08: diffy 3-way merge (D-02), LOCAL/ANCESTOR/UPSTREAM markers, sidecar renderer; `similar` removed (SYNC-05).

**Wave 8** *(blocked on Wave 7 completion)*
- [x] 01-09: Pull applies clean files plus `.conflict` sidecars, conflict records, convergence auto-clear, `*.conflict` gitignore rule (D-08, D-11) (SYNC-05).

**Wave 9** *(blocked on Wave 8 completion)*
- [x] 01-10: `ude sync resolve <file>` (D-09); push skips conflicted and merged files and exits 1 (D-10, D-17) (SYNC-05, SYNC-06).

**Wave 10** *(blocked on Wave 9 completion)*
- [x] 01-11: Fetched-head continuity check, `--porcelain` race detection, rollback and exit 1, never force-push (D-18) (SYNC-06).

### Phase 2: Sanitizer, Boundaries & Security Hardening

**Goal**: Screen and redact all confidential tokens, raw credentials, and user-specific paths before any remote git staging or push.
**Depends on**: Phase 1
**Requirements**: SEC-01, SEC-02, SEC-03, SEC-04
**Success Criteria** (what must be TRUE):
  1. High-entropy strings and raw Base64 tokens without standard prefixes are flagged by Shannon entropy scoring before git staging.
  2. Outbound commit inspection catches all Gitleaks secret categories across committed blobs and auto-rolls back commits on detection.
  3. Path parameterization matches only full word and punctuation boundaries (`\b`, path delimiters), preventing corruption of partial substring tokens.
  4. Sanitizer round-tripping verifies that templated tokens substitute back cleanly to local machine environments on initialization.

**Plans**: 3 plans

Plans:
- [ ] 02-01: Implement Shannon entropy calculation and Base64 credential screening in `Sanitizer` (`src/sanitizer/mod.rs`).
- [ ] 02-02: Expand secret regex patterns to mirror full Gitleaks rule categories and integrate into the pre-push publication gate (`src/sanitizer/mod.rs`, `src/sync/mod.rs`).
- [ ] 02-03: Enforce word and delimiter boundaries for path parameterization and add reversible substitution unit tests (`src/sanitizer/mod.rs`).

### Phase 3: Doctor & CLI Hardening

**Goal**: Make host and repository health diagnostics foolproof with strict parsing, dual permission audits, and standard POSIX exit codes.
**Depends on**: Phase 2
**Requirements**: CLI-01, CLI-02, CLI-03, CLI-04
**Success Criteria** (what must be TRUE):
  1. Host doctor verifies OpenSSH public keys against standard `ssh-ed25519` wire formats and catches malformed or corrupted keys.
  2. Repository doctor audits both filesystem executable bits (`0o755`) and Git index mode flags (`100755` vs `100644`) for all hook templates.
  3. Include directives in `.gitconfig.repo` are resolved canonically relative to the Git repository root (`.git/`), preventing false failures in subdirectories.
  4. All CLI invocations return POSIX exit code `0` on clean success, `1` on non-critical warning/drift, and `2` on critical security/system failures.

**Plans**: 3 plans

Plans:
- [ ] 03-01: Implement strict OpenSSH public key parsing for SSH signing verification in host doctor (`src/audit/mod.rs`).
- [ ] 03-02: Implement dual filesystem and Git index mode flag audit for repository hooks (`src/audit/mod.rs`).
- [ ] 03-03: Implement canonical repository-relative path resolution for git includes and enforce exit code contracts (`src/audit/mod.rs`, `src/main.rs`).

### Phase 4: Templates, Archetypes & Manifest Decoupling

**Goal**: Parameterize and decouple all personal configurations into clean manifests with safe re-initialization protections.
**Depends on**: Phase 3
**Requirements**: TMPL-01, TMPL-02, TMPL-03, TMPL-04
**Success Criteria** (what must be TRUE):
  1. All personal upstream repository URLs (`github.com/d10475u5/...`) are eliminated from templates and resolved dynamically from `.ude.toml` or manifest variables.
  2. Re-running `ude init` on an existing workspace detects modified files and creates timestamped `.bak` files before applying updates.
  3. Minijinja rendering fails fast with descriptive diagnostics whenever an undefined template variable is encountered.
  4. `.ude.toml` is parsed and establishes deterministic precedence over global machine defaults.

**Plans**: 3 plans

Plans:
- [ ] 04-01: Decouple hardcoded personal repository references in templates and scripts into parameterized manifest context (`templates/`, `src/template/mod.rs`).
- [ ] 04-02: Implement safe re-initialization backup semantics creating `.bak` archives for locally modified files before template overwrites (`src/template/mod.rs`, `src/main.rs`).
- [ ] 04-03: Configure Minijinja strict undefined behavior and wire `.ude.toml` configuration loading (`src/template/mod.rs`, `src/manifest/mod.rs`).

## Progress

**Execution Order:**
Phases execute in numeric order: 1 → 2 → 3 → 4

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. Sync Engine & State Hermeticity | 11/11 | In Progress | - |
| 2. Sanitizer, Boundaries & Security Hardening | 0/3 | Not started | - |
| 3. Doctor & CLI Hardening | 0/3 | Not started | - |
| 4. Templates, Archetypes & Manifest Decoupling | 0/3 | Not started | - |

---
*Roadmap created: 2026-10-12*
*Last updated: 2026-10-12*
