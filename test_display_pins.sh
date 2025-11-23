#!/bin/bash
echo "=== Rustyjack Display Diagnostic ==="
echo "This script will manually toggle GPIO pins to test the hardware."
echo ""

# Ensure we can control the pins
echo "[*] Setting GPIO 24 (Backlight) to Output..."
pinctrl set 24 op

echo "[*] Toggling Backlight (GPIO 24)..."
echo "    -> ON"
pinctrl set 24 dh
sleep 1
echo "    -> OFF"
pinctrl set 24 dl
sleep 1
echo "    -> ON"
pinctrl set 24 dh
echo ""
echo "DID THE SCREEN FLASH? (Even a white/black flicker counts)"
echo "If YES: GPIO 24 is correct."
echo "If NO:  We have the wrong pin or a hardware issue."
echo ""

echo "[*] Setting GPIO 27 (Reset) to Output..."
pinctrl set 27 op
echo "    -> Reset HIGH"
pinctrl set 27 dh
sleep 0.1
echo "    -> Reset LOW (Resetting...)"
pinctrl set 27 dl
sleep 0.1
echo "    -> Reset HIGH"
pinctrl set 27 dh
echo ""
echo "Diagnostic complete."
