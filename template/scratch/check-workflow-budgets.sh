#!/usr/bin/env bash
# ADR-857 Phase 6: dynamically synced with tests/phase6-capstone-conformance.test.cjs
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"

ERRORS=0

echo "🔍 Checking workflow byte ceilings (ADR-857 Phase 6)..."

node -e "
const fs = require('fs');
const path = require('path');
const { lfByteCount } = require('./scripts/workflow-size.cjs');

// Dynamically extract the authoritative PRE_PHASE6 ceilings from upstream's
// tests/phase6-capstone-conformance.test.cjs so this pre-flight check never
// falls out of sync when upstream ratchets or raises them.
let prePhase6Ceilings = { 'plan-phase.md': 98300, 'execute-phase.md': 93600 };
try {
  const capstoneContent = fs.readFileSync('tests/phase6-capstone-conformance.test.cjs', 'utf-8');
  const match = capstoneContent.match(/const PRE_PHASE6 = (\{[^}]+\});/);
  if (match) {
    prePhase6Ceilings = Object.assign(prePhase6Ceilings, eval('(' + match[1] + ')'));
  }
} catch (e) {
  // Fall back to known defaults if the test file moves or is unreadable
}

const limits = {
  'gsd-core/workflows/execute-phase.md': {
    max: prePhase6Ceilings['execute-phase.md'] || 93600,
    name: 'execute-phase.md (ADR-857 Phase 6 ceiling)'
  },
  'gsd-core/workflows/plan-phase.md': {
    max: prePhase6Ceilings['plan-phase.md'] || 98300,
    name: 'plan-phase.md (ADR-857 Phase 6 ceiling)'
  }
};

let failed = false;
for (const [relPath, cfg] of Object.entries(limits)) {
  const fullPath = path.join(process.cwd(), relPath);
  if (fs.existsSync(fullPath)) {
    const bytes = lfByteCount(fullPath);
    // Upstream's test requires now < frozen ceiling (strictly less than)
    if (bytes >= cfg.max) {
      console.error(\`❌ \${cfg.name} exceeded byte ceiling: \${bytes} >= \${cfg.max} (overflow: +\${bytes - cfg.max + 1} bytes)\`);
      failed = true;
    } else {
      console.log(\`✅ \${cfg.name}: \${bytes} bytes (budget: <\${cfg.max}, margin: \${cfg.max - bytes} bytes)\`);
    }
  }
}

if (failed) process.exit(1);
" || ERRORS=$((ERRORS + 1))

if [ -f scripts/lint-emitted-drift-ack.cjs ]; then
  echo "🔍 Checking emitted drift acknowledgments..."
  node scripts/lint-emitted-drift-ack.cjs || ERRORS=$((ERRORS + 1))
fi

if [ "$ERRORS" -gt 0 ]; then
  echo "❌ Pre-flight checks failed ($ERRORS error(s))." >&2
  exit 1
fi

echo "✅ All workflow budget and drift ack checks passed!"
