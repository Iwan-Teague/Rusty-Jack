#!/usr/bin/env bash
set -euo pipefail

# This script has been relocated to tests/compile/build_arm32.sh for organizational purposes.
# Delegate to the canonical script under tests/compile when invoked from scripts/ for backward compatibility.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
exec "$REPO_ROOT/tests/compile/build_arm32.sh" "$@"
