---
last_mapped_commit: 61850bfdb6b4b42c0d948b51354d3dbd1ebe7fb9
last_mapped_at: 2026-10-12
---
# Integrations & External Systems

**Analysis Date:** 2026-10-11

## Git Subsystem

**Direct Process Integration:**
- Execution: Subprocess invocation (`std::process::Command`) invoking system `git` binary.
- Purpose:
  - Repository inspection (`git status --porcelain`, `git diff-tree --root -r`, `git rev-parse`).
  - Remote repository coordination (`git clone --bare`, `git fetch`, `git checkout -B`, `git pull`, `git push`).
  - Branch lifecycle management (`git checkout`, `git branch -D`, `git reset --hard`, `git clean -fd`).
  - Configuration verification (`git config --get`, `git config --global`).
- Safety Mechanisms:
  - Force `--no-verify` and `--no-gpg-sign` flags during automated synchronization staging commits to avoid recursive hook triggers or interactive PIN prompts.
  - Verification of HEAD tree blobs post-commit to detect unintended file pollution or leaked secrets before remote pushes (`src/sync/mod.rs`).

## GitHub CLI (`gh`)

**PR Mode Integration:**
- Binary: `gh` on `$PATH`.
- Purpose: When `ude sync push --pr` is requested, `ude` creates a remote branch, pushes commits, and invokes `gh pr create` with automated title, body, and head/base branch specifications.
- Error Handling: Detects presence of `gh` via `which::which("gh")`. Warns and provides actionable fallback instructions when `gh` is unauthenticated or missing.

## Security & Secret Scanners

**Gitleaks Integration:**
- Binary: `gitleaks` on `$PATH`.
- Purpose: Pre-commit hook checks (`templates/projects/generic/hooks/pre-commit`) and doctor audits (`src/audit/mod.rs`).
- Rules Configuration: Project-local template `templates/universal/generic/security/gitleaks.toml` defining custom secret detection rules and allowlists.

**SSH & GPG Signing Subsystems:**
- Integration: Direct audit of `ssh-agent`, `gpg-agent`, `allowed_signers` file, and Git commit signing configuration (`commit.gpgsign`, `gpg.format`, `user.signingkey`).
- Verified via `src/audit/mod.rs` against host configuration and repo settings.

## Developer Memory & Knowledge Systems

**MemPalace:**
- Integration: Optional project scaffolding stage (`--mempalace`, default enabled).
- Artifacts: Renders `templates/projects/agents/mempalace.yaml.jinja` into the project root and initializes `.mempalace/` structure for cross-session AI assistant context.

**Graphify:**
- Integration: Optional knowledge graph scaffolding (`--graphify`, default enabled).
- Artifacts: Scaffolds `.graphify/` configuration and directory markers for AST/graph-based codebase indexing.

## Toolchain & Runtime Managers

**Mise:**
- Integration: Toolchain declaration rendering (`templates/projects/agents/mise.toml.jinja`).
- Purpose: Enforces deterministic toolchain versions across environments (e.g. pinned compiler versions, linters, and formatters).
- Doctor Verification: Audits whether `mise` is installed, active, and managing the project environment.

## AI Assistant Environments

**Claude / Anthropic Desktop:**
- Scaffolding: `templates/universal/agents/claude/settings.json.jinja`.
- Guidelines: `templates/universal/generic/rules/RULE_ZERO.md`.

**OpenAI Codex / Hermetic Agents:**
- Scaffolding: `templates/universal/agents/codex/hooks.json.jinja`.
- Instructions: `templates/projects/agents/agents.md.jinja`.

## Central Configuration Store

**Remote Git Upstream:**
- Location: Configurable via `.ude.toml` (`central_repo`), CLI flag `--remote <URL>`, or cached in `.ude/state.json`.
- Protocol: SSH (`git@github.com:...`) or HTTPS.
- Concurrency: Coordinated via advisory lock files on the upstream repository cache (`src/sync/mod.rs`).

---

*Integrations analysis: 2026-10-11*
<!-- refreshed: 2026-10-11 -->
