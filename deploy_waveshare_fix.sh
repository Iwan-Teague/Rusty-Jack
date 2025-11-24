#!/bin/bash
# Deploy Waveshare 1.44" LCD HAT GPIO pin fix to Raspberry Pi

set -e  # Exit on error

echo "=========================================="
echo "Waveshare 1.44\" LCD HAT Fix Deployment"
echo "=========================================="
echo ""

# Check if we're on the Pi
if [ ! -f /proc/device-tree/model ]; then
    echo "❌ Error: This script must be run on the Raspberry Pi"
    exit 1
fi

echo "📋 Current configuration:"
echo "   Display: Waveshare 1.44\" LCD HAT"
echo "   Controller: ST7735S (128×128)"
echo "   Pins: DC=GPIO25, RST=GPIO27, BL=GPIO24"
echo ""

# Check if SPI is enabled
if [ ! -e /dev/spidev0.0 ]; then
    echo "⚠️  WARNING: /dev/spidev0.0 not found!"
    echo "   SPI may not be enabled. Enable with:"
    echo "   sudo raspi-config → Interface Options → SPI → Yes"
    echo ""
    read -p "Continue anyway? (y/N) " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        exit 1
    fi
fi

# Stop the service
echo "🛑 Stopping rustyjack service..."
sudo systemctl stop rustyjack 2>/dev/null || echo "   (service not running or doesn't exist)"
echo ""

# Navigate to project
cd "$(dirname "$0")"
PROJECT_ROOT=$(pwd)
echo "📁 Project root: $PROJECT_ROOT"
echo ""

# Build the UI component
echo "🔨 Building rustyjack-ui with corrected GPIO pins..."
cd rustyjack-ui
cargo build --release

if [ $? -ne 0 ]; then
    echo "❌ Build failed!"
    exit 1
fi
echo "✅ Build successful"
echo ""

# Install the binary
echo "📦 Installing binary to /usr/local/bin..."
sudo cp target/release/rustyjack-ui /usr/local/bin/rustyjack-ui
sudo chmod +x /usr/local/bin/rustyjack-ui
echo "✅ Binary installed"
echo ""

# Check if service exists
if systemctl list-unit-files | grep -q rustyjack.service; then
    echo "🚀 Starting rustyjack service..."
    sudo systemctl start rustyjack
    echo "✅ Service started"
    echo ""
    
    # Wait a moment for initialization
    sleep 2
    
    # Check status
    echo "📊 Service status:"
    sudo systemctl status rustyjack --no-pager -n 5 || true
    echo ""
    
    echo "📜 Recent logs (checking for errors):"
    sudo journalctl -u rustyjack -n 20 --no-pager
    echo ""
else
    echo "⚠️  rustyjack.service not found in systemd"
    echo "   You can run manually with:"
    echo "   sudo /usr/local/bin/rustyjack-ui"
fi

echo ""
echo "=========================================="
echo "✅ Deployment Complete!"
echo "=========================================="
echo ""
echo "Expected behavior:"
echo "  1. Backlight ON → Screen BLACK (1 sec)"
echo "  2. GREEN BORDER appears around edges (test pattern)"
echo "  3. Splash screen → Main menu"
echo ""
echo "If you see the green border, the fix worked! 🎉"
echo ""
echo "Troubleshooting commands:"
echo "  • Watch logs: sudo journalctl -u rustyjack -f"
echo "  • Test manually: sudo /usr/local/bin/rustyjack-ui"
echo "  • Run diagnostics: RUSTYJACK_DISPLAY_DIAG=1 sudo /usr/local/bin/rustyjack-ui"
echo ""
