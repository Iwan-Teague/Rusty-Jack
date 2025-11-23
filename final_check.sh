#!/bin/bash
echo "=== FINAL HARDWARE CHECK ==="

echo "[1] Stopping rustyjack service to release pins..."
systemctl stop rustyjack

echo "[2] Checking if pins are free..."
# We look for "used" or "kernel" claims on our critical pins
gpioinfo | grep -E "\s(13|24|25|27)\s" | grep -v "unnamed"

echo "[3] Re-running Hardware Diagnostic..."
./diagnose_hat.sh
