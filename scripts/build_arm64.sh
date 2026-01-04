#!/usr/bin/env bash
set -euo pipefail

# Forwarding script — the canonical scripts are in tests/compile
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
exec "$REPO_ROOT/tests/compile/build_arm64.sh" "$@"
