#!/usr/bin/env bash
set -euo pipefail

# Forwarding script — the canonical scripts are in tests/compile.
# Use bash to avoid requiring the target script to be executable.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
exec bash "$REPO_ROOT/tests/compile/build_arm64.sh" "$@"
