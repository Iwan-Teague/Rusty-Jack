#!/usr/bin/env bash
set -euo pipefail

# Build Rustyjack for 64-bit ARM (Pi Zero 2 W on 64-bit Pi OS / other ARM64 Pis) inside the arm64 container.
# Requires Docker Desktop with binfmt/qemu enabled.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$REPO_ROOT"

./docker/arm64/run.sh env CARGO_TARGET_DIR=/work/target-64 cargo build --target aarch64-unknown-linux-gnu -p rustyjack-ui
./docker/arm64/run.sh env CARGO_TARGET_DIR=/work/target-64 cargo build --target aarch64-unknown-linux-gnu -p rustyjack-core

# After successful build, copy the produced binaries into prebuilt/arm64 so they can be committed/pulled to the Pi.
echo "Copying built binaries to prebuilt/arm64..."
mkdir -p "$REPO_ROOT/prebuilt/arm64"
cp -f "$REPO_ROOT/target-64/aarch64-unknown-linux-gnu/debug/rustyjack-ui" "$REPO_ROOT/prebuilt/arm64/rustyjack-ui" 2>/dev/null || true
chmod +x "$REPO_ROOT/prebuilt/arm64/rustyjack-ui" 2>/dev/null || true
cp -f "$REPO_ROOT/target-64/aarch64-unknown-linux-gnu/debug/rustyjack-core" "$REPO_ROOT/prebuilt/arm64/rustyjack-core" 2>/dev/null || true
chmod +x "$REPO_ROOT/prebuilt/arm64/rustyjack-core" 2>/dev/null || true
if [ -f "$REPO_ROOT/prebuilt/arm64/rustyjack-ui" ] && [ -f "$REPO_ROOT/prebuilt/arm64/rustyjack-core" ]; then
  echo "Prebuilt binaries placed at prebuilt/arm64/rustyjack-ui and prebuilt/arm64/rustyjack-core"
else
  echo "Warning: built binaries not found to copy. Check build output." >&2
fi
