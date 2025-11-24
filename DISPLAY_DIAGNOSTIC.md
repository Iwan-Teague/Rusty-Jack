# Display Diagnostic for Raspberry Pi Zero W 2

## Issue: Backlight on, but blank white screen

### Current Configuration (from display.rs):
- **Display Type**: ST7735S 128x128 LCD (Waveshare 1.44" HAT)
- **SPI Device**: /dev/spidev0.0
- **DC Pin**: GPIO 25
- **RST Pin**: GPIO 27 (CORRECTED for Waveshare HAT)
- **Backlight Pin**: GPIO 24 (CORRECTED for Waveshare HAT)
- **Offset**: X=2, Y=1

### Common Causes & Fixes:

## 1. SPI Not Enabled
Check if SPI is enabled:
```bash
lsmod | grep spi
ls -l /dev/spidev*
```

If SPI is not enabled:
```bash
sudo raspi-config
# Navigate to: Interface Options -> SPI -> Enable
sudo reboot
```

## 2. Wrong Display Variant
The ST7735 has multiple variants (ST7735R, ST7735S, ST7735B). The current code uses:
- `ST7735::new(spi, dc, rst, true, false, 128, 128)`
- Parameters: `rgb=true`, `inverted=false`

**Try this fix** - the display might need inverted colors or BGR mode.

## 3. Pin Connection Issues
Verify your Waveshare 1.44" LCD HAT connections (per official spec):
- **VCC** → 3.3V (Pin 1 or 17)
- **GND** → Ground (Pin 6, 9, 14, 20, 25, 30, 34, or 39)
- **SCL/SCLK** → GPIO 11 (Pin 23) - SPI0 SCLK
- **SDA/MOSI** → GPIO 10 (Pin 19) - SPI0 MOSI
- **RES/RST** → GPIO 27 (Pin 13) - CORRECTED FOR WAVESHARE
- **DC/RS** → GPIO 25 (Pin 22)
- **CS** → GPIO 8 (Pin 24) - SPI0 CE0
- **BL/BLK** → GPIO 24 (Pin 18) - CORRECTED FOR WAVESHARE

## 4. Permissions
Ensure the rustyjack-ui binary has GPIO access:
```bash
sudo usermod -a -G gpio,spi $USER
# Or run with sudo (your systemd service should already do this)
```

## 5. Check Display Initialization Logs
Since the process is running, check journalctl for errors:
```bash
sudo journalctl -u rustyjack -n 100 --no-pager
```

## Recommended Fix:

The most likely issue is the **display variant parameters**. Apply the fix below.
