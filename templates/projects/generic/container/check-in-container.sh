#!/usr/bin/env bash
# Hermes / Podman container parity verification runner
# Runs repository verification checks inside a pinned container to guarantee local-CI parity.
set -euo pipefail

SUITE="${1:-all}"
IMAGE="${UDE_CONTAINER_IMAGE:-rust:1.98-bookworm}"
CONTAINER_RUNTIME="$(command -v podman || command -v docker || true)"

if [ -z "$CONTAINER_RUNTIME" ]; then
    echo "ERROR: Neither podman nor docker found on host." >&2
    exit 1
fi

PROJECT_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"

echo "==> Running verification in container ($CONTAINER_RUNTIME using $IMAGE)..."
"$CONTAINER_RUNTIME" run --rm \
    -v "$PROJECT_ROOT:/workspace:z" \
    -w /workspace \
    "$IMAGE" \
    bash -c "cargo test --all-targets && cargo clippy --workspace --all-targets -- -D warnings"

echo "==> Container parity verification passed."
