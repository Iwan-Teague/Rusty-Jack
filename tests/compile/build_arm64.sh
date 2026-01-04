#!/usr/bin/env bash
set -euo pipefail

# Build Rustyjack for 64-bit ARM inside the arm64 container.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

cd "$REPO_ROOT"

./docker/arm64/run.sh env CARGO_TARGET_DIR=/work/target-64 cargo build --target aarch64-unknown-linux-gnu -p rustyjack-ui -p rustyjack-daemon -p rustyjack-portal --bin rustyjack --features rustyjack-core/cli

echo "Copying built binaries to prebuilt/arm64..."
mkdir -p "$REPO_ROOT/prebuilt/arm64"
bins=(rustyjack-ui rustyjack rustyjackd rustyjack-portal)
for bin in "${bins[@]}"; do
  src="$REPO_ROOT/target-64/aarch64-unknown-linux-gnu/debug/$bin"
  dst="$REPO_ROOT/prebuilt/arm64/$bin"
  cp -f "$src" "$dst" 2>/dev/null || true
  chmod +x "$dst" 2>/dev/null || true
done
