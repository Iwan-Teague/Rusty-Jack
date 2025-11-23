#!/bin/bash
# Display Hardware Diagnostic Script for Raspberry Pi

echo "========================================"
echo "Rustyjack Display Diagnostics"
echo "========================================"
echo ""

# Check SPI
echo "[SPI Status]"
if ls /dev/spidev0.0 &> /dev/null; then
    echo "✓ SPI device found: $(ls -l /dev/spidev0.0)"
    echo "✓ SPI kernel module:"
    lsmod | grep spi || echo "  (module may be built-in)"
else
    echo "✗ SPI device NOT found (/dev/spidev0.0)"
    echo "  Fix: sudo raspi-config -> Interface Options -> SPI -> Enable"
fi
echo ""

# Check GPIO
echo "[GPIO Status]"
if [ -d /dev/gpiochip0 ] || [ -c /dev/gpiochip0 ]; then
    echo "✓ GPIO chip found"
    ls -l /dev/gpiochip0
else
    echo "✗ GPIO chip NOT found"
fi
echo ""

# Check permissions
echo "[User Permissions]"
echo "Current user: $USER"
echo "Groups: $(groups)"
if groups | grep -q "gpio"; then
    echo "✓ User is in gpio group"
else
    echo "✗ User NOT in gpio group"
    echo "  Fix: sudo usermod -a -G gpio $USER"
fi
if groups | grep -q "spi"; then
    echo "✓ User is in spi group"
else
    echo "✗ User NOT in spi group"
    echo "  Fix: sudo usermod -a -G spi $USER"
fi
echo ""

# Check rustyjack service
echo "[Rustyjack Service]"
if systemctl is-active --quiet rustyjack; then
    echo "✓ Service is running"
    systemctl status rustyjack --no-pager | head -n 10
else
    echo "✗ Service is NOT running"
    echo "  Status:"
    systemctl status rustyjack --no-pager | head -n 10
fi
echo ""

# Check binary
echo "[Rustyjack Binary]"
if [ -f /usr/local/bin/rustyjack-ui ]; then
    echo "✓ Binary found:"
    ls -lh /usr/local/bin/rustyjack-ui
else
    echo "✗ Binary NOT found at /usr/local/bin/rustyjack-ui"
fi
echo ""

# Recent logs
echo "[Recent Service Logs (last 20 lines)]"
sudo journalctl -u rustyjack -n 20 --no-pager
echo ""

# GPIO Pin Test
echo "[GPIO Pin Availability Check]"
PINS=(24 25 18)
PIN_NAMES=("RST" "DC" "BL")
for i in "${!PINS[@]}"; do
    PIN="${PINS[$i]}"
    NAME="${PIN_NAMES[$i]}"
    if [ -d "/sys/class/gpio/gpio$PIN" ]; then
        echo "  GPIO $PIN ($NAME): Currently exported"
    else
        echo "  GPIO $PIN ($NAME): Available"
    fi
done
echo ""

# Check config file
echo "[Config File]"
if [ -f ~/Rustyjack/gui_conf.json ]; then
    echo "✓ gui_conf.json found"
    echo "Pin configuration:"
    grep -A 10 "PINS" ~/Rustyjack/gui_conf.json || echo "  (pins section not found)"
else
    echo "✗ gui_conf.json NOT found"
fi
echo ""

# System info
echo "[System Information]"
echo "Hostname: $(hostname)"
echo "Kernel: $(uname -r)"
echo "Architecture: $(uname -m)"
if [ -f /proc/device-tree/model ]; then
    echo "Model: $(cat /proc/device-tree/model)"
fi
echo ""

echo "========================================"
echo "Diagnostic Complete"
echo "========================================"
echo ""
echo "Common issues:"
echo "1. White screen = wrong color inversion (fixed in latest code)"
echo "2. No display at all = SPI not enabled or wrong pins"
echo "3. Garbled display = wrong SPI speed or orientation"
echo ""
echo "Next steps:"
echo "1. Ensure SPI is enabled"
echo "2. Run: ./fix_display.sh to rebuild and deploy"
echo "3. Check logs: sudo journalctl -u rustyjack -f"
echo ""
