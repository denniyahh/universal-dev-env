# Phase 2: Sanitizer, Boundaries & Security Hardening - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-10-23
**Phase:** 02-sanitizer-boundaries-security-hardening
**Areas discussed:** Entropy hits & overrides

Areas offered but not selected: Secret rule source, Placeholder round-trip, TypeSafe hardening scope.
Todo fold-in offered (home-path placeholder leak; generated-project gitleaks hook): **neither folded**.

---

## Entropy hits & overrides

| Option | Description | Selected |
|--------|-------------|----------|
| Block, overridable | Refuse like a named hit; narrow explicit override | ✓ |
| Block, no override | Treat exactly like a named secret | |
| Warn only | Print rule + file:line, publish anyway | |

| Option | Description | Selected |
|--------|-------------|----------|
| Built-in shapes + inline marker | ssh pubkeys, hex digests, UUIDs, SRI exempt; `ude:allow-entropy` per line | ✓ |
| Inline marker only | Every legit value needs a marker | |
| Reuse `gitleaks:allow` | One marker silences both scanners | |

| Option | Description | Selected |
|--------|-------------|----------|
| Entropy only | Marker never silences a named rule | ✓ |
| Any rule | Marker silences everything on the line | |

| Option | Description | Selected |
|--------|-------------|----------|
| Every token | Score every ≥20-char credential-alphabet run | ✓ |
| Assignment values only | Only key=value / parsed config scalars | |
| Both, tiered | Different thresholds per context | |

**Notes:** Research shown: gitleaks applies entropy per rule, not globally; uses `gitleaks:allow`,
per-rule allowlists, stopwords. UDE's shipped gitleaks baseline allowlists `templates/.*`.
Post-discussion measurement (Claude): 4.2 threshold catches only 18.6% of random 20-char Base64 and
flags a 39-char CamelCase English token (4.26) — threshold left to Claude's discretion with a
two-direction fixture requirement.

## Entropy rule calibration (follow-up, 2026-10-23)

Operator asked to resolve the entropy numbers before execution. Claude prototyped candidate rules in
Python against random tokens and the repo + host corpus.

| Option | Description | Selected |
|--------|-------------|----------|
| R11 class-relative | len-aware entropy + switch rate >= 0.75x expected for classes present; 93-100% mixed, 77-82% base36, 1 repo FP (JWT fixture, src/sanitizer tests) | ✓ |
| R7 fixed switch rate | switch >= 0.40; 97-99% mixed, ~48% base36, 1 repo FP | |
| R0 remediation 4.2 | 19% at 20 chars; 300 repo FPs | |

## TypeSafe hardening scope (follow-up, 2026-10-23)

Operator: move to Phase 3. Roadmap edit (SEC-05, criterion 5, plan 03-04) shown as diff and approved.

## Claude's Discretion

- Refusal output format
- Secret rule source for SEC-02
- Non-UTF-8 blob screening at the gate
- Placeholder round-trip design (SEC-03/04)

## Deferred Ideas

- TypeSafe System One hardening (remediation plan 2.3) — moved to Phase 3 (SEC-05)
