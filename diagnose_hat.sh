#!/bin/bash
echo "=== Rustyjack HAT Diagnostic ==="

# 1. Test Button (to verify HAT connection)
echo "[1] Testing Joystick Press (GPIO 13)"
echo "    Setting GPIO 13 to Input with Pull-Up..."
pinctrl set 13 ip pu
echo "    >>> PLEASE PRESS THE JOYSTICK CENTER BUTTON NOW <<<"
echo "    Waiting 5 seconds..."

detected=0
for i in {1..50}; do
    # pinctrl get output looks like: "13: ip    pu | lo // GPIO13 = input"
    # We look for "lo" which means pressed (grounded)
    if pinctrl get 13 | grep -q "| lo"; then
        echo "    [SUCCESS] Button press detected! The HAT is connected correctly."
        detected=1
        break
    fi
    sleep 0.1
done

if [ $detected -eq 0 ]; then
    echo "    [WARNING] No button press detected. Is the HAT seated properly?"
fi

echo ""
echo "[2] Testing Backlight Candidates"

# Test GPIO 24 (Standard Waveshare)
echo "    Testing GPIO 24 (Standard)..."
pinctrl set 24 op
echo "    -> ON"
pinctrl set 24 dh
sleep 1
echo "    -> OFF"
pinctrl set 24 dl
sleep 1
echo "    -> ON"
pinctrl set 24 dh

# Test GPIO 18 (Alternative)
echo "    Testing GPIO 18 (Alternative)..."
pinctrl set 18 op
echo "    -> ON"
pinctrl set 18 dh
sleep 1
echo "    -> OFF"
pinctrl set 18 dl
sleep 1
echo "    -> ON"
pinctrl set 18 dh

echo ""
echo "Diagnostic complete."
