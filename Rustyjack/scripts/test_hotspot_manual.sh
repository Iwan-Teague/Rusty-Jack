#!/bin/bash
# Manual hotspot test (Rust-native AP + DHCP/DNS)

set -euo pipefail

AP_IFACE="${1:-wlan1}"
SSID="${2:-rustyjack-test}"
CHANNEL="${3:-6}"
PASSWORD="${4:-rustyjack-test}"
UPSTREAM="${5:-eth0}"

if [ "$UPSTREAM" = "none" ] || [ "$UPSTREAM" = "-" ]; then
  UPSTREAM=""
fi

find_core() {
  if [ -n "${RUSTYJACK_CORE:-}" ] && [ -x "$RUSTYJACK_CORE" ]; then
    echo "$RUSTYJACK_CORE"
    return 0
  fi
  if command -v rustyjack-core >/dev/null 2>&1; then
    command -v rustyjack-core
    return 0
  fi
  if [ -x /usr/local/bin/rustyjack-core ]; then
    echo /usr/local/bin/rustyjack-core
    return 0
  fi
  if [ -x /root/Rustyjack/rustyjack-core/target/release/rustyjack-core ]; then
    echo /root/Rustyjack/rustyjack-core/target/release/rustyjack-core
    return 0
  fi
  if [ -x /root/Rustyjack/rustyjack-core/target/debug/rustyjack-core ]; then
    echo /root/Rustyjack/rustyjack-core/target/debug/rustyjack-core
    return 0
  fi
  return 1
}

CORE_BIN="$(find_core || true)"
if [ -z "$CORE_BIN" ]; then
  echo "[ERROR] rustyjack-core not found. Build or install it first." >&2
  exit 1
fi

run_core() {
  sudo "$CORE_BIN" --output text "$@"
}

echo "=========================================="
echo "Manual Hotspot Start Test (Rust-native)"
echo "=========================================="
echo "Core: $CORE_BIN"
echo "AP Interface: $AP_IFACE"
echo "Upstream: ${UPSTREAM:-<none>}"
echo "SSID: $SSID"
echo "Channel: $CHANNEL"
echo "Password: $PASSWORD"
echo "=========================================="
echo

echo "1. Stopping any running hotspot..."
run_core hotspot stop || true
sleep 1

echo
echo "2. Starting hotspot..."
if [ -n "$UPSTREAM" ]; then
  run_core hotspot start \
    --ap-interface "$AP_IFACE" \
    --upstream-interface "$UPSTREAM" \
    --ssid "$SSID" \
    --password "$PASSWORD" \
    --channel "$CHANNEL"
else
  run_core hotspot start \
    --ap-interface "$AP_IFACE" \
    --upstream-interface "" \
    --ssid "$SSID" \
    --password "$PASSWORD" \
    --channel "$CHANNEL"
fi

echo
echo "3. Hotspot status:"
run_core hotspot status || true

echo
echo "=========================================="
echo "SUCCESS (if status shows running)"
echo "=========================================="
echo "To stop:"
echo "  sudo $CORE_BIN hotspot stop"
echo
echo "Logs:"
echo "  journalctl -u rustyjack -f"
echo "=========================================="
