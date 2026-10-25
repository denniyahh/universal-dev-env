# Phase 02 plans — external adversarial review (2026-10-25)

Lanes: codex (gpt-6.1-sol, high, read-only sandbox) and agy (gemini-3.8-flash-high), both on a
`git archive` snapshot of workspace/denniyahh at 76c6c50. Both exited 0 with full output; no lane
dropped. Operator chose these two lanes only. Every finding below was checked against the plan text
and source by the host agent (Claude) before classification.

## Confirmed defects (verified against source)

| # | Lane | Sev | Finding | Verification |
|---|------|-----|---------|--------------|
| C1 | agy | BLOCKER | `=` in the R11 candidate alphabet (locked D-04) glues `NAME=` onto the value, so `.env`-style secrets mostly escape R11 | Independent Python re-implementation of 02-03 R11 (2000 samples, B64 values): bare 32-char value blocks 0.936 (matches plan's 0.951 measurement, control); `API_TOKEN=`+32 0.702; `CONFIG_PARAMETER_KEY=`+32 0.130; `MY_SERVICE_CLIENT_SECRET_VALUE=`+32 0.003 |
| C2 | codex | MAJOR | SRI exemption regex (02-03-PLAN.md:174) accepts any length after `sha256-`, so prefixing a credential exempts it | Plan text read; no length check exists |
| C3 | codex | MAJOR | Render-back gate 5 (02-08-PLAN.md:121) validates the candidate before `git add`; a clean filter can change the staged blob (existing harness src/sync/mod.rs:6264) and the later gates only scan for secrets | Code read; gate placement read |
| C4 | codex | MAJOR | Refusal text forwards MiniJinja errors (02-08-PLAN.md:120), which quote identifiers from the file (`filter {name} is unknown`, minijinja-2.24.0 src/vm/mod.rs:625); contradicts 02-08-PLAN.md:93 | Source line found in cargo registry |
| C5 | codex | MAJOR | 02-01-PLAN.md:179 acceptance check (`rg 'regex::Regex'` prints nothing) cannot pass while 02-01-PLAN.md:160 keeps `parameterize_paths` unchanged; it uses `Regex::new` (src/sanitizer/mod.rs:1,84,101) | Source read |
| C6 | codex | MAJOR | Parity negative control (02-06-PLAN.md:169) re-runs only the comparison loop, which reads the old report.json, so it cannot fail | Command at 02-06-PLAN.md:178 read |
| C7 | codex | MAJOR | UTF-16 files bypass every scanner; accepted as an unenforced assumption (02-01-PLAN.md:245) | Plan text read; codex ran gitleaks UTF-8 vs UTF-16LE control (1 vs 0 findings) |
| C8 | agy | MAJOR | ssh body exemption requires space/tab before the key type, so `"ssh-ed25519 AAAA…"` in JSON is scored and JSON has no comment for the `ude:allow-entropy` marker | Regex read at 02-03-PLAN.md:175; plan concedes it |
| C9 | agy | MINOR | Wave-4 plans 02-06 and 02-07 both write `target/mutants`, `target/mutants-run.{log,exit}`; parallelization is on in config.json | Both verify commands read |
| C10 | codex | MINOR | Boundary rule accepts `-`, so `/backup-/home/alice/x` is rewritten (02-07-PLAN.md:124, conceded at :271) | Plan text read |
| C11 | agy | MINOR | "count each distinct byte, divide by the byte length" (02-05-PLAN.md:141, 02-03-PLAN.md:130) reads as distinct/len; must say -Σ p·log2 p | Wording read; the 15-category positives would catch a wrong implementation only for entropy-gated rules in the table |
| C12 | agy | MINOR | Hex/UUID exemptions cannot change any R11 outcome (single-case hex ≤ 4.0 bits, mixed hex ≤ 4.46 < 4.76), so they are untestable and will show as surviving mutants | Arithmetic; plan concedes at 02-03-PLAN.md:167 |
| C13 | codex | MINOR | Only 15 of 222 vendored rules have behavioural fixtures (02-06-PLAN.md:139) | CONTEXT "Claude's Discretion" says "each rule category"; host decision: keep 15 + add a test listing the unexercised rule count, record as residual risk |

## Needs operator decision

- C1 changes locked decision D-04.
- agy-1 (02-08 refuses push of non-`.jinja` files whose parameterized text is not valid MiniJinja,
  e.g. a hook with `${#VAR}` plus a home path, or prose containing `{{`). This is a planner choice
  under CONTEXT "Claude's Discretion" with a user-visible cost; alternative is escaping on push.

## Rejected

- agy-8 (`git check-ignore` logic inverted): wrong. The idiom passes on exit 1 = not ignored, which
  is correct; 128 fails, as intended.
- agy-4 (`scan_project` uses host home via `dummy_ctx`): pre-existing Phase 1 behaviour, identical
  to the argument in production; only tests can differ. Logged as a todo, not a Phase 2 plan defect.
- codex suspected (render DoS via `range`): input is the pusher's own local file; self-inflicted.
- agy suspected (regex cost on large binaries): speculation, no measurement.
