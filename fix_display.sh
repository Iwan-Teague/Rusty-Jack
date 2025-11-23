#!/bin/bash
# Fix and redeploy Rustyjack display on Raspberry Pi

set -e

echo "========================================"
echo "Rustyjack Display Fix Deployment"
echo "========================================"

# Change to project directory
cd "$(dirname "$0")"

echo ""
echo "[1/6] Stopping rustyjack service..."
sudo systemctl stop rustyjack || true

echo ""
echo "[2/6] Building rustyjack-ui with display fix..."
cd rustyjack-ui
cargo build --release

echo ""
echo "[3/6] Installing updated binary..."
sudo cp target/release/rustyjack-ui /usr/local/bin/rustyjack-ui
sudo chmod +x /usr/local/bin/rustyjack-ui

echo ""
echo "[4/6] Verifying SPI is enabled..."
if ! ls /dev/spidev0.0 &> /dev/null; then
    echo "WARNING: /dev/spidev0.0 not found!"
    echo "SPI may not be enabled. Run: sudo raspi-config"
    echo "Navigate to: Interface Options -> SPI -> Enable"
else
    echo "SPI device found: $(ls -l /dev/spidev0.0)"
fi

echo ""
echo "[5/6] Checking GPIO permissions..."
if ! groups | grep -q "gpio"; then
    echo "Adding current user to gpio group..."
    sudo usermod -a -G gpio,spi $USER
    echo "NOTE: You may need to logout and login for group changes to take effect"
fi

echo ""
echo "[6/6] Starting rustyjack service..."
sudo systemctl start rustyjack

echo ""
echo "========================================"
echo "Deployment Complete!"
echo "========================================"
echo ""
echo "Check status with:"
echo "  sudo systemctl status rustyjack"
echo ""
echo "View logs with:"
echo "  sudo journalctl -u rustyjack -f"
echo ""
echo "If display still doesn't work, check DISPLAY_FIX_ALTERNATIVES.md"
echo ""
