---
created: 2026-10-25T00:00:00.000Z
title: Auto-escape literal template syntax on push instead of refusing
area: sync
severity: minor
files:
  - src/sync/mod.rs
  - src/template/mod.rs
---

## Problem

Phase 2 (plan 02-08, operator decision OD-2 on 2026-10-25) refuses to push a file whose published
text does not render back, for example a hook with `${#VAR}` (MiniJinja reads `{#` as a comment)
plus a parameterized home path, or a `.jinja` file whose prose contains literal `{{ ... }}`. The
user must edit the file. Without the refusal such a file breaks every pull. No shipped template
hits it today (checked 2026-10-25: commit-msg and check-publish-boundary.sh use `${#` but carry no
home path; sync-personal-env.sh carries a home path but no `{#`/`{{`).

## Solution

On push, wrap literal template syntax that is not a ude placeholder in a raw block
(`{% raw %}...{% endraw %}`) so the file renders back byte-identical, then keep the render-back gate
as the proof. Needs a decision on how pull/merge handle the escaped form, and tests for `${#X}`,
`${{ secrets.T }}` and prose `{{ x }}` with and without a home path.
