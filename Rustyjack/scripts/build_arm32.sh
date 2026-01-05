#!/usr/bin/env bash
set -euo pipefail

# This script has been relocated to tests/compile/build_arm32.sh for organizational purposes.
# Delegate via bash for backward compatibility without requiring the target to be executable.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
exec bash "$REPO_ROOT/tests/compile/build_arm32.sh" "$@"
