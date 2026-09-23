# Universal Development Environment — Functional & CLI Specification

This specification formalizes the data models, directory hierarchy, CLI interface, and verification contracts for the UDE engine (`ude`).

---

## 1. Directory Layout & 2x2 Matrix Structure

The central configuration store organizes templates and configurations into the four quadrants:

```text
universal-dev-env/
├── docs/
│   ├── ARCHITECTURE.md                  # Design principles, 2x2 matrix, sync contracts
│   ├── SPECIFICATION.md                 # CLI commands, state schemas, and contracts
│   ├── GLOBAL_MACHINE_BASELINE.md       # Tier 1 detailed host specification
│   └── PORTABLE_PROJECT_BASELINE.md     # Tier 2 detailed repo specification
├── templates/
│   ├── universal/
│   │   ├── generic/                     # [UG] Machine-global, tool-agnostic
│   │   │   ├── allowed_signers.jinja    # Host SSH allowed signers mapping
│   │   │   ├── gitconfig.global.jinja   # Global ~/.gitconfig
│   │   │   └── security/
│   │   │       └── gitleaks.toml        # Global baseline gitleaks configuration
│   │   └── agents/                      # [UA] Machine-global, agent-tailored
│   │       ├── claude/
│   │       │   ├── settings.json.jinja  # PreToolUse guards & PostToolUse formatters
│   │       │   └── rules/               # Modular ~/.claude/rules/*.md
│   │       ├── codex/
│   │       │   └── hooks.json.jinja     # Codex hooks config
│   │       └── antigravity/
│   │           └── rules/               # Global antigravity agent directives
│   └── projects/
│       ├── generic/                     # [PG] Repo-local, tool-agnostic
│       │   ├── gitconfig.repo           # Tracked .gitconfig include (git flow + ssh sign)
│       │   ├── gitignore.repo           # High-assurance gitignore
│       │   ├── hooks/                   # scripts/hooks/ chained hook suite
│       │   │   ├── commit-msg           # Conventional commits validator
│       │   │   ├── pre-commit           # Protected branch guard + global chaining
│       │   │   └── pre-push             # Worktree environment scrubber & parity
│       │   └── container/
│       │       └── check-in-container.sh# Podman/Docker hermetic verification runner
│       └── agents/                      # [PA] Repo-local, agent-tailored
│           ├── agents.md.jinja          # .agents/AGENTS.md project instructions
│           ├── mise.toml.jinja          # Project workflow tasks (sync, wt, check)
│           ├── mempalace.yaml.jinja     # Local .planning/.mempalace-stage setup
│           └── scratch/                 # Project operational helper scripts
│               ├── sync-personal-env.sh # Worktree <-> personal workspace reconciler
│               ├── new-worktree.sh      # Capability-injected task worktree creator
│               └── check-publish-boundary.sh # Upstream open-source PR boundary guard
└── src/                                 # Rust UDE CLI engine (engine, doctor, sync)
```

---

## 2. CLI Command Interface

The `ude` CLI provides deterministic lifecycle operations across host and repositories:

### 2.1. Environment Auditing: `ude doctor`
Inspects host machine and target repository against the specification without mutating files.

```bash
ude doctor [--host] [--repo <path>] [--format text|json]
```

- **Host Assertions:**
  - Evaluates dual SSH key presence (`maintainer` vs `agent` signing keys).
  - Asserts `~/.config/git/allowed_signers` contains authorized signing fingerprints.
  - Asserts global Git config has `gpg.format = ssh`, `commit.gpgsign = true`.
  - Asserts canonical agent rules mesh (`~/.config/agents/AGENTS.md` symlinks).
  - Verifies presence of core toolchains (`cargo`, `zig`, `mise`, `gitleaks`, `podman`/`docker`).
- **Repo Assertions:**
  - Asserts local `.git/config` includes the root `.gitconfig`.
  - Asserts `core.hooksPath` points to `scripts/hooks`.
  - Verifies executable bit `100755` on tracked hooks in git index.
- **Exit Code Contract:**
  - `0`: All required assertions pass.
  - `1`: One or more drift warnings or missing non-critical items.
  - `2`: Critical security invariant failed (e.g. unencrypted private key, missing allowed signers, unsigned commits enabled).

### 2.2. Project Initialization: `ude project init`
Drops in the Tier 2 project baseline (`PG` + `PA`) into any new or existing repository.

```bash
ude project init [path] --type <rust|node|polyglot> [--mempalace] [--graphify]
```

- Generates tracked `.gitconfig`, `.gitignore`, and chained `scripts/hooks/`.
- Registers hooks with `git update-index --chmod=+x` ensuring portability across `core.fileMode=false`.
- Configures `mise.toml` with project tasks (`sync:env`, `start:wt`, `check`).
- Configures publish boundary guards preventing personal workflow leaks to upstream open-source repositories.

### 2.3. Bidirectional Sync: `ude sync`
Coordinates reconciliation between local files and the central configuration store.

#### `ude sync status`
Compares local configuration files against upstream templates and reports drift.
- Categorizes each file: `In Sync`, `Locally Modified`, `Upstream Updated`, `Conflicted`, `Overridden (.local)`.

#### `ude sync pull`
Pulls updates from central configuration store into the target environment.
- Applies clean 3-way updates.
- If conflicts exist, leaves local file untouched and writes `.conflict` report for operator decision.
- Preserves all `.local` and `.override` layer customizations.

#### `ude sync push`
Pushes refined local templates, prompts, or scripts back to the central repository.
- **Gate 1:** Checks path against `publish-boundary` rules.
- **Gate 2:** Runs Secret Sanitizer (rejects API keys and private keys).
- **Gate 3:** Runs Path Parameterizer (converts absolute paths to `{{ user.home }}`).
- **Gate 4 (Optional):** Evaluates semantic drift via TypeSafe Jev if configured.
- Pushes verified diff or creates upstream PR.

---

## 3. Configuration & State Schema

### 3.1. Manifest Schema (`.ude.toml` / `ude.manifest.json`)
Placed at root of projects or host `~/.config/ude/config.toml`:

```toml
[environment]
version = "1.0.0"
profile = "fedora-kinoite"

[matrix.universal]
generic = true
agents = ["claude", "codex", "antigravity"]

[matrix.project]
type = "rust"
features = ["mempalace", "graphify", "worktrees", "container-parity"]

[sync]
central_repo = "https://github.com/denniyahh/universal-dev-env"
strategy = "three-way"
allow_overrides = true
exclude = [
    "*.local.*",
    ".ude/overrides/**",
    "target/**",
    "node_modules/**"
]
```

---

## 4. Verification & Negative Controls

Every component in UDE must adhere to the high-assurance verification harness:
1. **Sanitizer Negative Controls:**
   - Test suite feeds dummy private keys, AWS tokens, and GitHub PATs to `sync push` sanitizer; test MUST assert rejection with exit code > 0.
   - Test suite feeds hardcoded `/home/username` paths; test MUST assert rejection or automatic tokenization into `{{ user.home }}`.
2. **Hook Chaining Negative Controls:**
   - Pre-commit test fixture attempts direct commit to `main`; test MUST fail closed with protected branch error.
   - Commit-msg test fixture feeds invalid commit string ("fixed bug."); test MUST fail with Conventional Commit format error.
3. **Worktree Environment Scrubbing:**
   - Pre-push test fixture sets `GIT_DIR=/tmp/polluted`; test MUST verify `GIT_DIR` is unset before child test runner executes.
