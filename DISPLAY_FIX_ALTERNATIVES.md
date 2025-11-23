# ST7735 Display Fix Alternatives

## Issue: White/Blank Screen with Backlight On

### Fix #1: Inverted Colors (APPLIED)
**File**: `rustyjack-ui/src/display.rs` line ~93
```rust
let mut lcd = ST7735::new(spi, dc, rst, true, true, LCD_WIDTH as u32, LCD_HEIGHT as u32);
//                                       ^^^^ Changed from false to true
```

---

### Fix #2: BGR Color Mode (if Fix #1 doesn't work)
If the display still doesn't work, try BGR mode instead of RGB:
```rust
let mut lcd = ST7735::new(spi, dc, rst, false, false, LCD_WIDTH as u32, LCD_HEIGHT as u32);
//                                       ^^^^^ RGB=false means BGR mode
```

---

### Fix #3: Different Orientation (if display is garbled)
Try different orientations:
```rust
lcd.set_orientation(&Orientation::Landscape)?;
// or
lcd.set_orientation(&Orientation::LandscapeSwapped)?;
// or
lcd.set_orientation(&Orientation::PortraitSwapped)?;
```

---

### Fix #4: Different Offset Values
Some ST7735 displays have different offsets:
```rust
// Common alternatives:
lcd.set_offset(0, 0);    // No offset
lcd.set_offset(1, 2);    // Alternative 1
lcd.set_offset(2, 3);    // Alternative 2
lcd.set_offset(26, 1);   // For some ST7735S variants
```

---

### Fix #5: Check SPI Speed
If display shows garbage/noise, try reducing SPI speed:
```rust
let options = SpidevOptions::new()
    .bits_per_word(8)
    .max_speed_hz(4_000_000)  // Reduced from 12MHz to 4MHz
    .mode(SpiModeFlags::SPI_MODE_0)
    .build();
```

---

## Testing After Changes

After modifying `rustyjack-ui/src/display.rs`:

1. **Rebuild on Pi**:
```bash
cd ~/Rustyjack/rustyjack-ui
cargo build --release
```

2. **Install updated binary**:
```bash
sudo cp target/release/rustyjack-ui /usr/local/bin/
```

3. **Restart service**:
```bash
sudo systemctl restart rustyjack
```

4. **Check logs**:
```bash
sudo journalctl -u rustyjack -f
```

---

## Quick Test Without Service

To test display changes quickly without systemd:
```bash
sudo systemctl stop rustyjack
cd ~/Rustyjack/rustyjack-ui
sudo ./target/release/rustyjack-ui
# Press Ctrl+C to stop, then:
sudo systemctl start rustyjack
```

---

## Hardware Checklist

If software fixes don't work, verify hardware:

1. **Connections are secure** (no loose wires)
2. **Display is getting 3.3V** (not 5V which can damage it)
3. **SPI is enabled**: `ls /dev/spi* should show /dev/spidev0.0`
4. **GPIO pins match your HAT/display**

Common ST7735 HAT pin configurations:
- **Waveshare 1.44" LCD HAT**: DC=25, RST=27, BL=24
- **Adafruit ST7735**: DC=25, RST=24, BL=18 (current config)
- **Pimoroni Display HAT Mini**: DC=9, RST=25, BL=13

If your display uses different pins, update lines 72-86 in `display.rs`.
