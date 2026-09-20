# Tier 1: Machine-Global Setup Specification

This specification documents the host-level developer environment extracted from the DevFlow audit. These items apply machine-wide across all repositories and are not tracked inside individual projects.

---

## 1. Git Identity & Dual-Key SSH Signing

### Key Separation Architecture
To enable AI agents to commit autonomously while maintaining distinct cryptographic attribution and release authority, two SSH keys are configured:

| Key Role | Purpose | Typical Path | Stored In |
|---|---|---|---|
| **Maintainer Key** | Personal human commits, release tagging, `main` branch merges | `~/.ssh/github_ed25519.pub` | Global `~/.gitconfig` or local `devflow.releaseSigningKey` |
| **Agent Signing Key** | Autonomous AI agent commits on development/feature branches | `~/.ssh/devflow_signing_ed25519.pub` | Repo-local `.git/config` (`user.signingkey`) |

### GitHub "Verified" Attribution (`allowed_signers`)
Both keys share the maintainer's email address (`user.email`). Git verifies signatures against `~/.config/git/allowed_signers`:
```text
<maintainer-email> ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAA... <maintainer-comment>
<maintainer-email> ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAA... <agent-comment>
```
Because both keys are mapped to `<maintainer-email>`, commits pushed to GitHub by the agent display the maintainer's "Verified" badge, while local reflogs and pre-push hooks can distinguish them by fingerprint.

### Global Git Configuration (`~/.gitconfig`)
Global settings required across all repos:
```ini
[gpg]
    format = ssh
[gpg "ssh"]
    allowedSignersFile = ~/.config/git/allowed_signers
[commit]
    gpgsign = true
[tag]
    gpgsign = true
[init]
    defaultBranch = main
```

---

## 2. Global Git Hooks & Secret Scanning

### Global Pre-Commit Hook (`~/.config/git/hooks/pre-commit`)
Every commit across every project on the machine passes through a global pre-commit secret scanner before any repo-local hook runs:
- **Tool:** `gitleaks` (`gitleaks protect --staged --verbose`, installed via brew).
- **Delegation Protocol:**
  1. The global hook scans staged content for high-entropy secrets and credentials.
  2. If clean, the global hook detects if a repo-local hook exists (or if `core.hooksPath` was set) and chains execution to it.
  3. A failure in either gate rejects the commit.

---

## 3. GitHub CLI (`gh`) Authentication & Least-Privilege Scopes

Authentication is managed via system keyring with least-privilege token scopes:
- **Active Scopes:** `repo`, `workflow`, `gist`, `read:org`.
- **Deliberately Excluded:** `admin:ssh_signing_key` is **not** granted by default. It is refreshed on-demand (`gh auth refresh -h github.com -s admin:ssh_signing_key`) only when auditing or provisioning signing keys, preventing automated agents from reading or modifying account keys.

---

## 4. Unified AI Agent Rules Topology

To eliminate ruleset drift and prevent different tools from executing contradictory policies:
- **Canonical Source of Truth:** `~/.config/agents/AGENTS.md` (stores RULE ZERO, verification standards, negative-control requirements, and coding methodology).
- **Symlink Mesh:**
  - `~/.claude/CLAUDE.md` -> `~/.config/agents/AGENTS.md`
  - `~/.codex/AGENTS.md` -> `~/.config/agents/AGENTS.md`
- **Claude Code Specific Rules:** `~/.claude/rules/` contains modular context:
  - `git-workflow.md`
  - `code-style.md`
  - `context7.md`
  - `effort-routing.md`
  - `claude-subagent-worktrees.md` (rules for worktree root pinning during sequential dispatch)

---

## 5. Agent Safety & Formatting Hooks

Global hooks configured in `~/.claude/settings.json` and `~/.codex/hooks.json`:
- **PreToolUse Destructive Command Guard:** Rejects destructive terminal commands (`rm -rf /`, `mkfs.*`, direct force-pushes `git push --force`).
- **PostToolUse Mechanical Formatters:** Automatically lints and formats modified files on disk without burning LLM reasoning turns:
  - Python: `ruff check --fix` and `ruff format`
  - Rust: `cargo fmt`
  - Zig: `zig fmt`
  - TypeScript/JavaScript: `prettier --write`
- **Graphify PreToolUse Guards:** Intercepts search/read tools to steer queries toward knowledge graph structures when available.
- **Desktop Notifications:** Fires desktop alert on completion via `notify-send`.

---

## 6. CLI Toolchains & Utilities

Installed globally via Homebrew, Cargo, or Flatpak (no `dnf` on immutable Fedora):
- **Rust AI Development & Verification:**
  - `cargo-deny` (license and supply-chain auditing)
  - `cargo-machete` (unused dependency detector)
  - `cargo-llvm-cov` (line and branch coverage)
  - `cargo-mutants` (mutation testing)
  - `cargo-nextest` (high-performance test runner)
  - `cargo-semver-checks` (API compatibility enforcement)
  - `miri` (Rust nightly unsafe memory validation)
- **Documentation Lookup:** `npx ctx7@latest` CLI (per `~/.claude/rules/context7.md`).
- **Token & Usage Tracking:** `npx ccusage@latest claude session` (audits token costs from JSONL transcripts).
- **Memory Infrastructure:** `mempalace` CLI (v3.6.0+).
