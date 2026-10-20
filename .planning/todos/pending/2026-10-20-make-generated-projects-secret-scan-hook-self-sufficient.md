---
created: 2026-10-20T15:22:43.577Z
title: Make generated projects' secret-scan hook self-sufficient
area: tooling
severity: major
files:
  - templates/projects/generic/hooks/pre-commit:5-6
  - template/scripts/hooks/pre-commit:5-6
---

## Problem

The pre-commit hook that `ude` ships to generated projects does not scan for secrets itself. Both
templates (`templates/projects/generic/hooks/pre-commit` and `template/scripts/hooks/pre-commit`)
chain to the developer's global hook (`git config --global core.hooksPath`, falling back to
`~/.config/git/hooks`) and rely on it for gitleaks. The operator's global hook only warns and
continues when gitleaks is not on PATH ("pre-commit: warning: gitleaks not found on PATH; skipping
secret scan"), and on a fresh machine there may be no global hook at all. Either way a generated
project commits with no secret scan.

This breaks the operator's supply-chain policy:
- item 4: the repo's own pre-commit hook runs `gitleaks git --pre-commit --staged --redact`; do not
  rely on a developer's global hook;
- item 6: a missing scanner is a hard failure, never a skip.

Found while adding this repo's own supply-chain scanning (commit e1fcc77, which fixed it for the
`ude` repo itself via `scripts/hooks/pre-commit` + pinned gitleaks in `mise.toml`). Operator chose
"log as a todo" on 2026-10-20.

## Solution

Test-first:
1. RED: an e2e test that generates a project, plants a fake secret (e.g. a fake GitHub PAT built at
   runtime, never committed literally), and asserts the generated hook refuses the commit; plus a
   test that the hook exits non-zero when gitleaks is absent (subprocess PATH scrubbed via
   `cmd.env()`, no `set_var`).
2. GREEN: the template hook runs `gitleaks git --pre-commit --staged --redact` itself and fails
   loudly when gitleaks cannot be found. Decide how generated projects pin gitleaks (mirror this
   repo's `mise.toml` + `mise which` approach, which avoids an earlier-on-PATH binary winning).
3. Keep the existing main/develop direct-commit protection and decide whether chaining to the
   global hook stays (as an extra, not as the scan).
