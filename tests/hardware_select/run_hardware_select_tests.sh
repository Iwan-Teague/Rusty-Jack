#!/usr/bin/env bash
set -euo pipefail

# Hardware selection tests
# Calls daemon for wifi interfaces and capabilities

ROOT_DIR=$(cd "$(dirname "$0")/.." && pwd)
SOCKET=${SOCKET:-/tmp/rustyjack_test.sock}

if [ ! -S "$SOCKET" ]; then
  echo "Socket $SOCKET not found. Start daemon on this socket (e.g. via tests/daemon/run_daemon_tests.sh)" >&2
  exit 2
fi

echo "Querying wifi interfaces..."
python3 "$ROOT_DIR/daemon/test_client.py" --socket "$SOCKET" wifi-interfaces > /tmp/_wifi_if.json

ifs=$(jq -r '.body.data.interfaces[]' /tmp/_wifi_if.json 2>/dev/null || true)
if [ -z "$ifs" ]; then
  echo "No wifi interfaces found or parsing failed. Output:"
  cat /tmp/_wifi_if.json
  exit 1
fi

echo "Found interfaces:"; echo "$ifs"

for i in $ifs; do
  echo "--- Capabilities for $i ---"
  python3 "$ROOT_DIR/daemon/test_client.py" --socket "$SOCKET" wifi-capabilities "$i"
done

echo "Hardware selection tests completed"
