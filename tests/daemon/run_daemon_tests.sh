#!/usr/bin/env bash
set -euo pipefail

# Run daemon tests (macOS / Linux) — relocated into tests/daemon

ROOT_DIR=$(cd "$(dirname "$0")/.." && pwd)
SOCKET=/tmp/rustyjack_test.sock

echo "Running cargo test (daemon unit tests)"
cargo test -p rustyjack-daemon

echo "Building daemon binary"
cargo build -p rustyjack-daemon --release
DAEMON_BIN=$(pwd)/../target/release/rustyjackd

if [ ! -x "$DAEMON_BIN" ]; then
  echo "Daemon binary not found at $DAEMON_BIN" >&2
  exit 1
fi

rm -f "$SOCKET"

echo "Starting daemon on socket $SOCKET"
env RUSTYJACKD_SOCKET="$SOCKET" "$DAEMON_BIN" &
DAEMON_PID=$!
echo "Daemon PID: $DAEMON_PID"

trap 'echo "Killing daemon"; kill $DAEMON_PID >/dev/null 2>&1 || true; rm -f "$SOCKET"' EXIT

echo "Waiting for socket to appear..."
for i in {1..20}; do
  if [ -S "$SOCKET" ]; then
    echo "Socket ready"
    break
  fi
  sleep 0.2
done

if [ ! -S "$SOCKET" ]; then
  echo "Socket did not appear" >&2
  exit 2
fi

echo "Running integration tests via Python client"
python3 "$ROOT_DIR/daemon/test_client.py" --socket "$SOCKET" health
python3 "$ROOT_DIR/daemon/test_client.py" --socket "$SOCKET" version
python3 "$ROOT_DIR/daemon/test_client.py" --socket "$SOCKET" job-sleep 1

echo "Integration tests completed successfully"
