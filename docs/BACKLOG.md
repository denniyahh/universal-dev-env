# UDE Project Backlog

This backlog tracks identified enhancements, test harness expansions, and integrations deferred from the core synchronization and security milestone.

---

## 1. Gate 4: Semantic Drift Auditing via TypeSafe Jev (In-Pipeline Integration)

* **Area**: `src/semantic/`, `src/main.rs`, `docs/SPECIFICATION.md:112`
* **Status**: Serialization & client models implemented; pipeline wiring placeholder active.
* **Context**:
  `src/semantic/mod.rs` implements `TypeSafeClient`, structured payload models, pre-transmission secret sanitization, and verdict classification (`PolicyShift`, `ProjectException`, `BenignRephrase`, `Uncertain`). `src/main.rs` inspects `typesafe.is_available()` before evaluating `AGENTS.md` drift.
* **Scope & Next Steps**:
  1. Add runtime configuration support in `.ude.toml` (`[sync.semantic]`) for endpoint URL, model selection (`jev`), and policy sensitivity thresholds.
  2. Implement secure credential resolution (e.g., `TYPESAFE_API_KEY` environment variable or local Jev sidecar container socket).
  3. Add network timeout and graceful degradation policies (e.g., `--require-semantic` vs non-blocking advisory warnings).
  4. Expand test coverage with mock HTTP server fixtures (e.g. `wiremock`) asserting on verdict handling during `sync push`.

---

## 2. Hook Chaining Automated Test Harness

* **Area**: `template/scripts/hooks/`, `tests/`, `docs/SPECIFICATION.md:155-160`
* **Status**: Hook installation and `100755` mode verification validated in `ude doctor`; direct hook script behavior tested manually / via host pre-commit.
* **Context**:
  `docs/SPECIFICATION.md` outlines negative controls for generated git hooks:
  - Pre-commit: Attempts direct commit to protected branches (`main`, `master`) fail closed.
  - Commit-msg: Non-conventional commit messages (e.g. "fixed bug.") fail closed with syntax guidance.
  - Pre-push: Secret detection and worktree environment scrubbing.
* **Scope & Next Steps**:
  1. Create a dedicated integration test suite (`tests/hooks_chaining.rs`).
  2. Test execution of `scripts/hooks/commit-msg` across valid Conventional Commit types (`feat:`, `fix:`, `chore:`, etc.) and invalid inputs, verifying exit code 1.
  3. Test execution of `scripts/hooks/pre-commit` against direct commits on `main` vs feature branches.
  4. Ensure shell dialect compatibility across Linux (`bash`/`sh`) and macOS without relying on external system pre-commit frameworks.

---

## 3. Live GitHub Integration E2E Suite (Credential-Gated)

* **Area**: `src/sync/mod.rs:push_to_remote`, CI/CD
* **Status**: Local git remote fixtures verified; live GitHub API / `gh` CLI behaviors unexercised in offline test suites.
* **Context**:
  Hermetic offline test suites intentionally do not make outbound network requests or require GitHub credentials. Real-world edge cases (GitHub API rate limiting, `gh` auth expiration, network disconnections during multi-part pack upload) can only be exercised against live remotes.
* **Scope & Next Steps**:
  1. Create credential-gated integration tests marked with `#[ignore]` by default.
  2. Enable tests via environment flag (e.g., `UDE_LIVE_GITHUB_TEST=1` and `GITHUB_TOKEN`).
  3. Validate real `gh pr create` response parsing and branch URL output.
  4. Verify graceful error messaging when `gh` CLI is installed but unauthenticated (`gh auth status` exit 1).
