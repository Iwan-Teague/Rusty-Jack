#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
TARGET="aarch64-unknown-linux-gnu"
TARGET_DIR="/work/target-64"
HOST_TARGET_DIR="$REPO_ROOT/target-64"
DEFAULT_BUILD=0

if [ "$#" -gt 0 ]; then
    CMD=("$@")
else
    DEFAULT_BUILD=1
    CMD=(
        bash -c
        "set -euo pipefail; \
export PATH=/usr/local/cargo/bin:\$PATH; \
export CARGO_TARGET_DIR=$TARGET_DIR; \
cargo build --target $TARGET -p rustyjack-ui; \
cargo build --target $TARGET -p rustyjack-daemon; \
cargo build --target $TARGET -p rustyjack-portal; \
cargo build --target $TARGET -p rustyjack-core --bin rustyjack --features rustyjack-core/cli"
    )
fi

bash "$REPO_ROOT/docker/arm64/run.sh" "${CMD[@]}"

if [ "$DEFAULT_BUILD" -eq 1 ]; then
    DEST_DIR="$REPO_ROOT/prebuilt/arm64"
    mkdir -p "$DEST_DIR"
    for bin in rustyjack-ui rustyjackd rustyjack-portal rustyjack; do
        src="$HOST_TARGET_DIR/$TARGET/debug/$bin"
        if [ ! -f "$src" ]; then
            echo "Missing binary: $src" >&2
            exit 1
        fi
        cp -f "$src" "$DEST_DIR/$bin"
    done
    echo "Copied binaries to $DEST_DIR"
fi
