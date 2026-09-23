# Personal Fork Instructions & AI Agent Rules

> **Repository Context**: Personal fork (`denniyahh/gsd-core`) of OpenGSD (`open-gsd/gsd-core`).

## 1. Branching & PR Rules
* **Integration Target**: Almost all Pull Requests target **`upstream/next`** (not `main`).
* **Branch Creation**: Always branch off `upstream/next` (`git checkout next && git pull upstream next`).
* **Pull Requests**: Open PRs targeting `upstream/next` using `gh pr create --base next --repo open-gsd/gsd-core`.

## 2. Environment & Tooling
* **Tool Manager**: Use `mise` for Node 22 and task management.
* **Remote CI Runner & CI Execution Requirement (MANDATORY)**:
  * Whenever CI, tests, or pre-PR verification is requested or needed, **ALWAYS run the Mac-ci script located in the scratch directory (`scratch/ci-mac.sh` or `mise run check` / `mise run ci:mac` / `mise run test:mac`), NEVER run the local Aurora CI/test commands directly on this host**.
  * The Mac runner executes in a hermetic clean-room environment (`ssh -T mac`), protecting against host environment leakage and process-hanging issues on Aurora.
  * Running full CI: `./scratch/ci-mac.sh` (or `mise run check` to check budget + run full CI).
  * Running focused/targeted tests on Mac: `./scratch/ci-mac.sh "<command>"` (e.g. `./scratch/ci-mac.sh "node --test tests/config.test.cjs"` or `mise run test:mac`).
* **Branch & Worktree Setup Helper**: Use `mise run start:wt <type> <issue-number> <slug>` (e.g. `mise run start:wt fix 2783 wedged-pr-note`) to create task worktrees off `upstream/next` populated with personal workflow capabilities.
* **Pre-flight & Pre-push Commands (STRICT ENFORCEMENT)**:
  * Check environment & workflow budgets: `mise run check:budget` (checks ADR-857 Phase 6 ceilings dynamically against `tests/phase6-capstone-conformance.test.cjs`, and drift acks)
  * Pre-push / pre-PR verification rule: Run `mise run check` (or `./scratch/check-workflow-budgets.sh && ./scratch/ci-mac.sh`). Do not run `npm test` or `npm run lint:ci` directly on the local Aurora host for verification.
  * **Workflow Size Ceiling & Delegation Rule**: When modifying host workflows (`execute-phase.md`, `plan-phase.md`), delegate multi-line logic to step fragments (`gsd-core/workflows/<workflow>/steps/*.md` or `capabilities/*/fragments/`) and keep host workflow byte counts strictly under the ADR-857 Phase 6 ceilings (`execute-phase.md` < 93,600 bytes, `plan-phase.md` < 98,300 bytes).

## 3. Worktree-Safe Contribution Flow
* **Sync, then isolate**: Run `mise run sync` in this primary checkout. For AI-assisted work, create the task branch in a separate worktree via `mise run start:wt <type> <issue-number> <slug>`.
* **Validation**: Run `mise run check` from the task worktree (which automatically syncs and verifies on the Mac runner) before requesting review or opening a PR. Use narrower suites (e.g. `mise run test:mac`) during the edit loop when appropriate.
* **Publishing**: Push task branches to `origin`; open the upstream PR only after the approved issue, required test evidence, changeset (when applicable), and PR template are ready.
* **Personal-fork files**: `.agents/`, `mise.toml`, and `scratch/` are personal workflow material. They may be committed to `denniyahh/gsd-core`, but must be excluded from PRs to `open-gsd/gsd-core` unless their content is explicitly in scope.
* **Local state**: Never stage `.local/`; it is intentionally locally ignored and can contain machine-specific GitHub state.

### Contribution Boundary Automation

The `start:wt` helper always bases contribution branches on `upstream/next`; do not branch an
upstream contribution from `personal/workspace`, whose private commits would otherwise enter the
PR ancestry. The helper then injects personal tooling into the new worktree and installs a
worktree-local hook path, preserving the upstream hooks while adding the personal checks.

The pre-push hook rejects any committed path under `.agents/`, `.planning/`, or `scratch/`, plus
`mise.toml`, when compared with `upstream/next`. These files are development capabilities and
planning state, not upstream contribution files. Do not bypass the hook with `--no-verify`.

Before pushing, inspect the exact PR surface with:

```bash
git diff --name-only upstream/next...HEAD
git diff --check upstream/next...HEAD
```

If a private customization needs to change during development, edit it in the injected worktree
but stage only the intended upstream files. The helper's publish guard is the final safety net,
not a substitute for reviewing the staged diff.

## 4. Reference Notes
* See [scratch/FORK_NOTES.md](file:///home/denniyahh/Github/gsd-core/scratch/FORK_NOTES.md) for local environment notes and shortcuts.
* Keep the shared DevFlow dogfooding ledger at [scratch/UPSTREAM-GSD-ISSUES.md](file:///home/denniyahh/Github/gsd-core/scratch/UPSTREAM-GSD-ISSUES.md). DevFlow links to this file; record only upstream GSD issues there.

## 5. PR & Contribution Formatting (CRITICAL)
* **PR Title Format**: Must strictly follow `type(#<issue>): short summary` (e.g. `fix(#3158): branch protection checks`). Do not use brackets like `[fix]`.
* **Changesets Required**: Every PR with user-facing changes MUST have a `.changeset/*.md` fragment created via `npm run changeset`. The fragment MUST include `pr: <NNN>` in its frontmatter.
* **PR Templates**: You must use the correct GitHub template for the PR type (e.g. `.github/PULL_REQUEST_TEMPLATE/fix.md`). Do NOT use the default template or overwrite it with a blank body.
* **Documentation**: If your changeset type is `Added`, `Changed`, `Deprecated`, or `Removed`, you must update the relevant file in `docs/` OR add the `<!-- docs-exempt: <reason> -->` marker in the changeset fragment.
* **CI Verification Rules**:
  * ALWAYS query status via `gh run list --workflow Tests` or `gh pr checks <PR_NUMBER>`. Never rely on un-filtered `gh run list` (which returns single-job sidecars).
  * NEVER post a comment to maintainers stating that a PR is ready for review until ALL GitHub Actions CI checks have completed successfully (100% green).
