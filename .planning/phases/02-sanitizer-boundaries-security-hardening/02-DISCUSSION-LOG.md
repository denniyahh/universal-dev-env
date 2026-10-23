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

## Claude's Discretion

- Entropy threshold calibration (with fixture table + `templates/` negative control)
- Refusal output format
- Secret rule source for SEC-02
- Non-UTF-8 blob screening at the gate
- Placeholder round-trip design (SEC-03/04)

## Deferred Ideas

- TypeSafe System One hardening (remediation plan 2.3) — not in SEC-01..04
