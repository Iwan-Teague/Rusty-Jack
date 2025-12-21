#!/usr/bin/env bash
set -euo pipefail

# Build Rustyjack for 32-bit ARM (Pi Zero 2 W on 32-bit Pi OS) inside the arm32 container.
# Requires Docker Desktop with binfmt/qemu enabled.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$REPO_ROOT"

./docker/arm32/run.sh env CARGO_TARGET_DIR=/work/target-32 cargo build --target armv7-unknown-linux-gnueabihf -p rustyjack-ui

# After successful build, copy the produced binary into prebuilt/arm32 so it can be committed/pulled to the Pi.
echo "Copying built binary to prebuilt/arm32..."
mkdir -p "$REPO_ROOT/prebuilt/arm32"
cp -f "$REPO_ROOT/target-32/armv7-unknown-linux-gnueabihf/debug/rustyjack-ui" "$REPO_ROOT/prebuilt/arm32/rustyjack-ui" 2>/dev/null || true
chmod +x "$REPO_ROOT/prebuilt/arm32/rustyjack-ui" 2>/dev/null || true
if [ -f "$REPO_ROOT/prebuilt/arm32/rustyjack-ui" ]; then
  echo "Prebuilt binary placed at prebuilt/arm32/rustyjack-ui"
else
  echo "Warning: built binary not found to copy. Check build output." >&2
fi
