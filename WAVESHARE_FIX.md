# Waveshare 1.44″ LCD HAT - White Screen Fix

## Problem Identified ✅

Your Rustyjack code was using **incorrect GPIO pins** for the Waveshare 1.44″ LCD HAT, causing communication failures between the Pi and display controller.

## Root Cause

The code was configured for **Adafruit ST7735** displays, not the Waveshare HAT.

### ❌ BEFORE (Wrong Pins):
```rust
RST Pin:  GPIO 24  // WRONG - this is BL on Waveshare!
DC Pin:   GPIO 25  // Correct
BL Pin:   GPIO 18  // WRONG - not connected on Waveshare!
```

### ✅ AFTER (Correct for Waveshare 1.44″ HAT):
```rust
RST Pin:  GPIO 27  // Correct per Waveshare spec
DC Pin:   GPIO 25  // Correct per Waveshare spec
BL Pin:   GPIO 24  // Correct per Waveshare spec
```

## Official Waveshare 1.44″ LCD HAT Pinout

According to [Waveshare Wiki](https://www.waveshare.com/wiki/1.44inch_LCD_HAT):

| Function | BCM GPIO | Physical Pin | Description |
|----------|----------|--------------|-------------|
| SCLK | GPIO 11 | Pin 23 | SPI clock |
| MOSI | GPIO 10 | Pin 19 | SPI data |
| CS | GPIO 8 | Pin 24 | Chip select |
| **DC** | **GPIO 25** | **Pin 22** | Data/Command control |
| **RST** | **GPIO 27** | **Pin 13** | Reset |
| **BL** | **GPIO 24** | **Pin 18** | Backlight |

## Why You Were Getting White Screen

1. **Wrong Reset Pin**: RST was on GPIO 24 instead of GPIO 27
   - The controller never received proper reset signals
   - Remained in undefined state after power-on

2. **Wrong Backlight Pin**: BL was on GPIO 18 instead of GPIO 24
   - Backlight might have appeared to work due to pull-ups
   - But wasn't properly controlled by software

3. **Result**: Controller never initialized → display showed white (uninitialized LCD state)

## Rust Driver Status ✅

Your choice of **`st7735-lcd = "0.10"`** is **CORRECT**:
- ✅ Supports ST7735S controller (used in Waveshare HAT)
- ✅ Compatible with embedded-hal 1.0
- ✅ Supports 128×128 resolution with proper offset
- ✅ Works with SPI Mode 0 (CPOL=0, CPHA=0)

## Current Configuration Status

### ✅ Already Correct:
- **SPI Device**: `/dev/spidev0.0` ✓
- **SPI Speed**: 4 MHz (conservative, good for stability) ✓
- **SPI Mode**: MODE_0 (CPOL=0, CPHA=0) ✓
- **Display Size**: 128×128 pixels ✓
- **Offset**: (2, 1) - correct for ST7735S 132×162 addressing ✓
- **Color Mode**: RGB with inversion enabled ✓
- **DC Pin**: GPIO 25 ✓

### 🔧 Fixed:
- **RST Pin**: Changed from GPIO 24 → **GPIO 27** ✓
- **BL Pin**: Changed from GPIO 18 → **GPIO 24** ✓

## Files Changed

1. **`rustyjack-ui/src/display.rs`**
   - Lines 102-120: Updated GPIO pin assignments
   - Lines 245-250: Updated diagnostic routine pin assignments
   - Added comments clarifying Waveshare HAT pinout

2. **`DISPLAY_DIAGNOSTIC.md`**
   - Updated pin reference to reflect Waveshare specs

3. **`DISPLAY_FIX_README.md`**
   - Corrected pin configuration section

## Next Steps - Deploy to Your Pi

### Option 1: Git Push/Pull (Recommended)
```bash
# On your Windows machine:
cd C:\Users\teagu\Desktop\Rustyjack
git add .
git commit -m "Fix GPIO pins for Waveshare 1.44\" LCD HAT"
git push

# Then on your Pi Zero 2 W:
cd ~/Rustyjack
git pull
./fix_display.sh  # This will rebuild and restart the service
```

### Option 2: Manual Copy via SSH
```powershell
# On your Windows machine:
scp C:\Users\teagu\Desktop\Rustyjack\rustyjack-ui\src\display.rs pi@<your-pi-ip>:~/Rustyjack/rustyjack-ui/src/

# Then SSH into Pi and rebuild:
ssh pi@<your-pi-ip>
cd ~/Rustyjack
./fix_display.sh
```

### Option 3: Full Rebuild Script
Create and run this on your Pi:
```bash
#!/bin/bash
cd ~/Rustyjack
sudo systemctl stop rustyjack
cd rustyjack-ui
cargo build --release
sudo cp target/release/rustyjack-ui /usr/local/bin/
sudo systemctl start rustyjack
sudo journalctl -u rustyjack -f
```

## Expected Behavior After Fix

1. **0-1 sec**: Backlight turns on, screen clears to black
2. **1-2 sec**: Green test border appears around edges
3. **2-3 sec**: "RUSTYJACK" splash screen or logo
4. **3+ sec**: Main menu with temperature and stats

If you see the **green border**, the display is working correctly!

## Troubleshooting (If Still White)

### 1. Verify SPI is Enabled
```bash
ls -l /dev/spidev0.0  # Should exist
lsmod | grep spi      # Should show spi modules
```

### 2. Check Service Logs
```bash
sudo journalctl -u rustyjack -n 50 --no-pager
```
Look for:
- ✅ "LCD init succeeded" or similar success messages
- ❌ "Failed to open /dev/spidev0.0" (SPI not enabled)
- ❌ "GPIO line busy" (another process using GPIO)

### 3. Test GPIO Manually
```bash
# Install gpio tools if needed
sudo apt-get install gpiod

# Test backlight control
gpioset gpiochip0 24=1  # Backlight ON
gpioset gpiochip0 24=0  # Backlight OFF
```

### 4. Run Diagnostics
```bash
cd ~/Rustyjack
RUSTYJACK_DISPLAY_DIAG=1 sudo ./rustyjack-ui/target/release/rustyjack-ui
```
This will cycle through different display configurations with colored borders.

## Hardware Verification

Your Waveshare 1.44″ LCD HAT should be:
- **Seated properly** on GPIO header (pins 1-40)
- **Power LED on** (if present)
- **No loose connections** or bent pins
- **HAT fully inserted** (not offset by one pin row)

### Quick Hardware Test
1. Look at the HAT from the side - should sit flat on GPIO header
2. Check if any other hardware is connected that might use GPIOs 24, 25, or 27
3. Ensure no wiring projects are conflicting with the HAT pins

## Technical Summary

**Controller**: ST7735S (132×162 addressing 128×128 display)  
**Interface**: 4-wire SPI (MOSI, SCLK, CS, DC) + Reset + Backlight  
**Protocol**: SPI Mode 0, up to 32 MHz (using 4 MHz for stability)  
**Color Format**: RGB565 (5-6-5 bit color)  
**Initialization**: RGB mode with color inversion enabled  

The Waveshare wiki confirms these specs and the fix aligns your code with the official hardware design.

## Support Links

- [Waveshare 1.44″ LCD HAT Wiki](https://www.waveshare.com/wiki/1.44inch_LCD_HAT)
- [ST7735S Datasheet](https://files.waveshare.com/upload/e/e2/ST7735S_V1.1_20111121.pdf)
- [Raspberry Pi GPIO Pinout](https://pinout.xyz/)

---

**Status**: Pin configuration corrected. Ready for deployment and testing.
