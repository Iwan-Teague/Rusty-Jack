// Alternative Display Configurations for ST7735 Troubleshooting
// Replace the Display::new() function in display.rs with these variants to test

// ============================================================
// VARIANT 1: RGB mode with inverted colors (CURRENT/DEFAULT)
// ============================================================
// This is the most common fix for white/blank screens
pub fn new(colors: &ColorScheme) -> Result<Self> {
    let mut spi = SpidevDevice::open("/dev/spidev0.0")
        .context("opening SPI device")?;
    
    let options = SpidevOptions::new()
        .bits_per_word(8)
        .max_speed_hz(12_000_000)
        .mode(SpiModeFlags::SPI_MODE_0)
        .build();
    spi.configure(&options)
        .context("configuring SPI")?;

    let mut chip = Chip::new("/dev/gpiochip0").context("opening GPIO chip")?;
    
    let dc_line = chip.get_line(25).context("getting DC line")?;
    let dc_handle = dc_line.request(LineRequestFlags::OUTPUT, 0, "rustyjack-dc")
        .context("requesting DC line")?;
    let dc = CdevPin::new(dc_handle).context("creating DC pin")?;
    
    let rst_line = chip.get_line(24).context("getting RST line")?;
    let rst_handle = rst_line.request(LineRequestFlags::OUTPUT, 0, "rustyjack-rst")
        .context("requesting RST line")?;
    let rst = CdevPin::new(rst_handle).context("creating RST pin")?;
    
    let bl_line = chip.get_line(18).context("getting backlight line")?;
    let bl_handle = bl_line.request(LineRequestFlags::OUTPUT, 1, "rustyjack-bl")
        .context("requesting backlight line")?;
    let _backlight = CdevPin::new(bl_handle).context("creating backlight pin")?;

    let mut delay = Delay {};
    let mut lcd = ST7735::new(spi, dc, rst, true, true, LCD_WIDTH as u32, LCD_HEIGHT as u32);
    //                                        ^^^^ ^^^^
    //                                        RGB  INVERTED
    lcd.init(&mut delay).map_err(|_| anyhow::anyhow!("LCD init failed"))?;
    lcd.set_orientation(&Orientation::Portrait).map_err(|_| anyhow::anyhow!("LCD orientation failed"))?;
    lcd.set_offset(LCD_OFFSET_X, LCD_OFFSET_Y);
    lcd.clear(Rgb565::BLACK).map_err(|_| anyhow::anyhow!("LCD clear failed"))?;
    
    // ... rest of initialization
}


// ============================================================
// VARIANT 2: BGR mode, not inverted
// ============================================================
// Try this if colors appear but are wrong (red/blue swapped)
    let mut lcd = ST7735::new(spi, dc, rst, false, false, LCD_WIDTH as u32, LCD_HEIGHT as u32);
    //                                        ^^^^^ ^^^^^
    //                                        BGR   NOT INVERTED


// ============================================================
// VARIANT 3: RGB mode, not inverted (ORIGINAL CONFIG)
// ============================================================
// This was the original configuration
    let mut lcd = ST7735::new(spi, dc, rst, true, false, LCD_WIDTH as u32, LCD_HEIGHT as u32);
    //                                        ^^^^ ^^^^^
    //                                        RGB  NOT INVERTED


// ============================================================
// VARIANT 4: BGR mode with inverted colors
// ============================================================
// Try this if Variant 1 shows inverted colors
    let mut lcd = ST7735::new(spi, dc, rst, false, true, LCD_WIDTH as u32, LCD_HEIGHT as u32);
    //                                        ^^^^^ ^^^^
    //                                        BGR   INVERTED


// ============================================================
// VARIANT 5: Reduced SPI speed (for unstable connections)
// ============================================================
// If you see garbled/corrupted display, reduce SPI speed
    let options = SpidevOptions::new()
        .bits_per_word(8)
        .max_speed_hz(4_000_000)  // Reduced from 12MHz
        .mode(SpiModeFlags::SPI_MODE_0)
        .build();


// ============================================================
// VARIANT 6: Different offsets for various ST7735 modules
// ============================================================
// After initialization, try different offsets:

    lcd.set_offset(0, 0);    // No offset
    // OR
    lcd.set_offset(1, 2);    // Alternative 1
    // OR
    lcd.set_offset(2, 3);    // Alternative 2
    // OR
    lcd.set_offset(26, 1);   // For ST7735S 0.96" displays
    // OR
    lcd.set_offset(2, 1);    // Current default


// ============================================================
// VARIANT 7: Different orientations
// ============================================================
// Try these if display works but is rotated or mirrored:

    lcd.set_orientation(&Orientation::Landscape)?;
    // OR
    lcd.set_orientation(&Orientation::LandscapeSwapped)?;
    // OR
    lcd.set_orientation(&Orientation::PortraitSwapped)?;
    // OR
    lcd.set_orientation(&Orientation::Portrait)?;  // Current default


// ============================================================
// VARIANT 8: Alternative GPIO pins (for different HATs)
// ============================================================
// Common pin configurations for various ST7735 HATs:

// Adafruit ST7735 (current config):
// DC=25, RST=24, BL=18

// Waveshare 1.44" LCD HAT:
// DC=25, RST=27, BL=24

// Pimoroni Display HAT Mini:
// DC=9, RST=25, BL=13

// To change pins, modify these lines:
    let dc_line = chip.get_line(25)?;   // Change 25 to your DC pin
    let rst_line = chip.get_line(24)?;  // Change 24 to your RST pin
    let bl_line = chip.get_line(18)?;   // Change 18 to your backlight pin


// ============================================================
// TESTING PROCEDURE
// ============================================================
/*
1. Edit rustyjack-ui/src/display.rs
2. Find the Display::new() function (around line 60-110)
3. Modify the ST7735::new() call with one of the variants above
4. Rebuild and deploy:
   cd ~/Rustyjack/rustyjack-ui
   cargo build --release
   sudo cp target/release/rustyjack-ui /usr/local/bin/
   sudo systemctl restart rustyjack
5. Check the display - if it doesn't work, try the next variant
6. View logs: sudo journalctl -u rustyjack -n 50
*/
