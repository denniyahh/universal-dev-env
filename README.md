# Universal Development Environment & Configuration System (`ude`)

An auditable, iterable, and bidirectionally synchronizable AI development environment system. Configures host-wide developer baselines and bootstraps zero-drift, high-assurance repositories across Linux, macOS, and Windows.

---

## 1. Core Architecture: The 2x2 Matrix

UDE separates configuration state cleanly across two independent axes:
- **Scope Axis:** **Universal (Machine-Global)** vs. **Project-Specific (Repo-Local)**
- **Actor Axis:** **Agent-Generic (Human & Toolchain Baseline)** vs. **Agent-Specific (AI Agent Adaptations)**

```text
                        AGENT-GENERIC                          AGENT-SPECIFIC
             ┌───────────────────────────────────┬───────────────────────────────────┐
             │ [UG] Universal Generic            │ [UA] Universal Agent-Specific     │
             │ • Dual SSH signing infrastructure │ • ~/.claude/settings.json hooks   │
  UNIVERSAL  │ • ~/.config/git/allowed_signers   │ • ~/.codex/hooks.json formatters  │
  (Host-Wide)│ • Global commit attribution       │ • Canonical AGENTS.md mesh        │
             │ • Gitleaks pre-commit guard       │ • PreToolUse destructive guards   │
             ├───────────────────────────────────┼───────────────────────────────────┤
             │ [PG] Project Generic              │ [PA] Project Agent-Specific       │
             │ • Public-safe root .gitconfig     │ • .agents/AGENTS.md instructions  │
   PROJECT   │ • Chained scripts/hooks/          │ • mise tasks (sync, wt, check)    │
 (Repo-Local)│ • Worktree GIT_* env scrubbing    │ • .planning/.mempalace-stage      │
             │ • Container test parity runner    │ • check-publish-boundary guards   │
             └───────────────────────────────────┴───────────────────────────────────┘
```

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for detailed architecture and design contracts.

---

## 2. Bidirectional Sync & Drift Reconciliation

UDE connects local host environments and project checkouts with the central configuration repository:
- **Downstream Sync (`ude sync pull` / `ude project init`)**: Idempotently materializes and updates environment baselines.
- **Upstream Sync (`ude sync push`)**: Captures local rule/prompt/tool refinements back upstream with automated sanitization:
  - **Conflict Strategy**: Option A (3-way merge with explicit conflict rejection) by default; Option B (Layered `.local` / `.override` layers) for permanent unpushed local adaptations.
  - **Sanitization Gate**: Scrubbing of high-entropy secrets and automatic parameterization of user paths (`{{ user.home }}`).
  - **Semantic Intelligence**: Optional TypeSafe System One (Jev) semantic drift classification and credential screening.

---

## 3. Repository Structure

```text
universal-dev-env/
├── README.md                           # Project mission and roadmap
├── copier.yaml                         # Copier project template configuration
├── docs/
│   ├── ARCHITECTURE.md                 # 2x2 matrix, sync engine, and design contract
│   ├── SPECIFICATION.md                # CLI commands, schemas, and test invariants
│   ├── GLOBAL_MACHINE_BASELINE.md      # Tier 1: Host specification
│   └── PORTABLE_PROJECT_BASELINE.md    # Tier 2: Repository baseline
├── templates/                          # Canonical 2x2 configuration matrix
│   ├── universal/                      # [UG] and [UA] host-level configurations
│   └── projects/                       # [PG] and [PA] repo-level templates
└── template/                           # Copier baseline template tree
```

---

## 4. Project Roadmap

- [x] **Phase 1: 2x2 Matrix & Sync Contract Specification** — Define the four quadrants, bidirectional reconciliation lifecycle, sanitization gate, and TypeSafe judgment integration.
- [x] **Phase 2: Canonical Template Matrix Reorganization** — Align `templates/` into the four-quadrant structure with Minijinja parameterization.
- [x] **Phase 3: Rust CLI Engine (`ude`) Scaffolding** — Implement core engine crate with manifest parsing, path resolution, and template renderer.
- [x] **Phase 4: Non-Destructive Auditor (`ude doctor`)** — Implement host and project integrity scanner asserting signing keys, hooks, and agent rules mesh.
- [x] **Phase 5: Bidirectional Reconciler (`ude sync`)** — Implement 3-way diffing, `.local` overlay handling, secret sanitization, and upstream push with high-assurance negative controls.

## 5. Development: Supply-Chain Scanning

Scanner versions are pinned in `mise.toml`; the dependency policy is `deny.toml`. On a new clone:

```bash
mise trust && mise install && mise run install-hooks
```

- `scripts/hooks/pre-commit` scans staged changes for secrets (gitleaks). It fails if mise or the pinned gitleaks is missing.
- `scripts/hooks/pre-push` runs the full `cargo deny check` (advisories, bans, licenses, sources); any failure blocks the push. An advisory that cannot be fixed yet goes under `[advisories] ignore` in `deny.toml` with a reason.
- `mise run supply-chain` runs the same check by hand.
- `.github/workflows/supply-chain.yml` runs both scans on every push and pull request, and weekly from the default branch.
