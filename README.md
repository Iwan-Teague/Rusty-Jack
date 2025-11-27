<p align="center">
  <img src="https://img.shields.io/badge/platform-Raspberry%20Pi-red?style=flat-square&logo=raspberry-pi">
  <img src="https://img.shields.io/badge/language-Rust-orange?style=flat-square&logo=rust">
  <img src="https://img.shields.io/badge/LCD-ST7735%20128x128-blue?style=flat-square">
  <img src="https://img.shields.io/badge/license-Educational%20Use-green?style=flat-square">
</p>

<div align="center">
  <h1>🦀 Rustyjack</h1>

  <img src="img/rustyjack.bmp" alt="Rustyjack Logo" width="300"/>

  <p>
    <strong>Portable Network Offensive Security Toolkit</strong><br>
    100% Rust • LCD Interface • Raspberry Pi<br>
    <em>Pure Rust offensive security toolkit</em>
  </p>

> ⚠️ **LEGAL DISCLAIMER**
> 
> This tool is for **authorized security testing and educational purposes ONLY**.
> 
> Unauthorized access to computer systems is **ILLEGAL** under:
> - Computer Fraud and Abuse Act (USA)
> - Computer Misuse Act (UK)
> - Similar laws worldwide
> 
> **Always obtain written permission before testing any network or system.**
> 
> The authors accept **NO LIABILITY** for misuse or illegal activities.
> You are **solely responsible** for your actions.

</div>

---

## 📋 Table of Contents

- [What is Rustyjack?](#-what-is-rustyjack)
- [Features](#-features)
- [Hardware Requirements](#-hardware-requirements)
- [Installation](#-installation)
- [Usage](#-usage)
- [Loot & Data](#-loot--data)
- [Troubleshooting](#-troubleshooting)
- [Credits](#-credits)

---

## 🎯 What is Rustyjack?

**Rustyjack** is a self-contained offensive security platform that runs on a Raspberry Pi with an LCD screen. It provides penetration testing tools accessible through a simple joystick interface—no keyboard, monitor, or external computer needed.

### What Makes It Special?

- **🦀 100% Rust** - Standalone Rust toolkit
- **📟 Standalone** - LCD screen, buttons, all tools built-in
- **🔋 Portable** - Pocket-sized, battery-powered option
- **🛡️ Complete** - Recon, credential capture, phishing, and other integrated tools

### Architecture

A high-level, current view of the project's structure. Rustyjack is split into two focused components — a compact UI for the on-device display and a separate core that runs orchestration and long-lived tasks.

```
┌────────────────────────────────────────┐
│               Rustyjack                │
├──────────────┬─────────────────────────┤
│ rustyjack-ui │     rustyjack-core      │
│ (LCD + GPIO) │     (Orchestration)     │
│ - Menu / UX  │ - Task runner / CLI     │
│ - Display    │ - Autopilot & tasks     │
│ - Button I/O │ - Loot management       │
│              │ - System actions        │
└──────────────┴─────────────────────────┘

UI → dispatches commands to → Core → interacts with OS and filesystem

Notes:
- The UI process (rustyjack-ui) owns the display, button handling and menu presentation.
- The core process (rustyjack-core) executes longer-running actions, maintains loot, and performs system operations.
```

---

## ✨ Features


### 💾 Loot & Data

**On-device loot viewer:**
- Browse captured output and logs (Wireless and other supported outputs)
- View device-generated logs and export files
- Navigate and open files using LCD buttons

**Discord integration:**
- Manual upload of ZIP loot archives via the UI when configured
- Formatted embeds for notifications and metadata
- File attachments up to configured size limits


### 🎨 User Interface

**3 View Modes:**
- **List View** - Classic vertical list (7 items)
- **Grid View** - 2×4 grid (8 items)
- **Carousel View** - Single-item focus with wraparound

**Customization:**
- RGB color themes (background, border, text, selection)
- Save/restore configurations
- Temperature monitoring
- System controls (restart, shutdown, update)

---

## 🛠️ Hardware Requirements

### Required Components

| Item | Specification | Purpose |
|------|---------------|---------|
| **Raspberry Pi** | Zero 2 W (recommended)<br>or Pi 4 / Pi 5 | Main computer |
| **LCD HAT** | Waveshare 1.44" LCD HAT<br>ST7735, 128×128, SPI | Display + buttons |
| **MicroSD Card** | 16GB+ (Class 10) | Operating system |
| **Power Supply** | 5V 2.5A USB-C/Micro-USB | Power |

### Optional Components

| Item | Purpose |
|------|---------|
| **Ethernet HAT** | 3 USB + 1 Ethernet (Pi Zero) |
| **Battery Pack** | Portable power |
| **USB Storage / Drive** | Attach external storage for backup or loot export |

### Pin Configuration

**Waveshare 1.44" LCD HAT Pinout:**

| Component | GPIO Pin | Function |
|-----------|----------|----------|
| LCD Data/Command | GPIO 25 | DC (data/command select) |
| LCD Reset | GPIO 24 | RST (hardware reset) |
| LCD Backlight | GPIO 18 | BL (PWM control) |
| LCD SPI | `/dev/spidev0.0` | Data transfer |
| Joystick Up | GPIO 6 | Button input |
| Joystick Down | GPIO 19 | Button input |
| Joystick Left | GPIO 5 | Button input |
| Joystick Right | GPIO 26 | Button input |
| Joystick Press | GPIO 13 | Center button |
| Key 1 | GPIO 21 | View mode toggle |
| Key 2 | GPIO 20 | Reserved |
| Key 3 | GPIO 16 | Back button |

**All pins configured in:** `/root/Rustyjack/gui_conf.json`

---

## 🔘 Button controls

The on-HAT buttons and joystick map directly to menu navigation and actions. Below is a concise mapping of logical controls (and the GPIO pins used — see the Pin Configuration table above).

- **Up / Down** (Joystick Up / Down) — Navigate lists and menus.
- **Left** (Joystick Left) — Back / return to previous menu.
- **Right** (Joystick Right) — Select / activate highlighted item.
- **Center press** (Joystick Press) — Confirm / secondary select.
- **Key 1** (View toggle) — Cycle UI view modes (List / Grid / Carousel).
- **Key 2** (Reserved) — Reserved for future use / custom mapping.
- **Key 3** (Back) — Quick back to menu or cancel dialogs.

How these controls behave:
- Short-press Right or Center confirms selections and triggers actions.
- Left always behaves as a 'Back' while on dialogs or submenus.
- Key 1 cycles the display mode to let you change how the menu is rendered.

> Tip: button and view behaviour can be customized by editing `gui_conf.json` (see `config` module in the UI source).


---

## 📥 Installation

### Part 1: Flash Operating System

**1. Download Raspberry Pi Imager**

Download from: https://www.raspberrypi.com/software/

**2. Configure in Imager**

- **Device:** Raspberry Pi Zero 2 W (or your model)
- **OS:** Raspberry Pi OS Lite (32-bit or 64-bit)
- **Settings (⚙️):**
  - Enable SSH
  - Set username: `pi` (or your choice)
  - Set password
  - Configure WiFi (optional)
  - Set hostname: `rustyjack` (optional)

**3. Flash to SD Card**

- Insert microSD card
- Select storage device
- Click "Write"
- Wait for completion (~5 minutes)

**4. Boot Raspberry Pi**

- Insert SD card into Pi
- Connect LCD HAT
- Connect power
- Wait for first boot (~2 minutes)

### Part 2: Install Rustyjack

**1. SSH into Raspberry Pi**

```bash
ssh pi@rustyjack.local
# Or use IP address: ssh pi@192.168.1.XXX
```

**2. Switch to Root User**

```bash
sudo su -
```

**3. Clone Repository**

```bash
cd /root
git clone https://github.com/Iwan-Teague/Rusty-Jack.git Rustyjack
cd Rustyjack
```

**4. Run Installation Script**

```bash
chmod +x install_rustyjack.sh
./install_rustyjack.sh
```

**What it does:**
- ✅ Installs Rust toolchain (rustup)
- ✅ Installs optional system tooling as needed by configured features
- ✅ Enables SPI and I2C in boot configuration
- ✅ Compiles `rustyjack-core` (orchestration engine)
- ✅ Compiles `rustyjack-ui` (LCD interface)
- ✅ Installs binaries to `/usr/local/bin/`
- ✅ Creates systemd service `rustyjack.service`
- ✅ Sets up needed directories (e.g. `loot/`)
- ✅ Starts service automatically

**Time:** ~10-15 minutes (Rust compilation is CPU-intensive)

**5. Reboot**

```bash
reboot
```

**6. Verify Installation**

After reboot, the LCD should display the Rustyjack menu.

Check service status via SSH:

```bash
systemctl status rustyjack
```

Expected output:
```
● rustyjack.service - Rustyjack UI Service (100% Rust)
   Loaded: loaded (/etc/systemd/system/rustyjack.service; enabled)
   Active: active (running)
   Main PID: 456
```

**View logs:**
```bash
journalctl -u rustyjack -f
```

### Part 3: Configure (Optional)

**Discord Webhook Setup:**

1. Create webhook in Discord (Server → Channel → Integrations → Webhooks)
2. Copy webhook URL
3. Edit file:
   ```bash
   nano /root/Rustyjack/discord_webhook.txt
   ```
4. Paste URL, save, exit
5. Restart service:
   ```bash
   systemctl restart rustyjack
   ```

**Network configuration:**

Configure WiFi and network interfaces using the host OS or your preferred tooling (SSH / NetworkManager / wpa_supplicant).

### Updating

```bash
cd /root/Rustyjack
git pull
./install_rustyjack.sh
reboot
```

⚠️ **Backup loot before updating:**
```bash
tar -czf ~/loot_backup_$(date +%Y%m%d).tar.gz loot/
```

---

## 🎮 Usage

### Button Controls

| Button | Action |
|--------|--------|
| **↑ Up** | Scroll up / Previous item |
| **↓ Down** | Scroll down / Next item |
| **→ Right** | Select / Enter submenu / Confirm |
| **← Left** | Back / Cancel / Previous menu |
| **○ Select** | Same as Right (center button) |
| **KEY1** | Toggle view mode (List/Grid/Carousel) |
| **KEY2** | Reserved |
| **KEY3** | Alternative back button |

### Menu Structure (current)

```
Main Menu
│
├─ Hardware Detect
├─ Crack Passwords
├─ View Dashboards
├─ Autopilot
│  ├─ Start Standard
+│  ├─ Start Aggressive
+│  ├─ Stop Autopilot
+│  └─ View Status
├─ Settings
│  ├─ Options
│  └─ System
├─ Loot
│  ├─ Transfer to USB
│  └─ Wireless
└─ (Other utilities)
```

### Quick Start Examples

Here are a few things you can do from the device's main menu (keeps the current trimmed feature set in mind):

- Run hardware detection: Main Menu → Hardware Detect
- Start password cracking workflows: Main Menu → Crack Passwords
- View dashboards for system and attack metrics: Main Menu → View Dashboards
- Launch Autopilot to run automated sequences: Main Menu → Autopilot → Start ...
- Browse loot captured from wireless attacks and transfer to USB: Main Menu → Loot → Transfer to USB

---

## ⚙️ Core Capabilities

Rustyjack is now focused on providing a compact UI for the device and a core orchestration process. The following capabilities are maintained in the trimmed-down project:

- UI / LCD-driven menu and controls (rustyjack-ui)
- Hardware detection (interface and peripheral detection)
- Autopilot — automated sequences and task orchestration
- Native wireless attacks (rustyjack-wireless - pure Rust, no external tools)
- Loot viewer and transfer utilities (view and export wireless attack outputs)
- Discord webhook integration for uploading loot and notifications
- System management (configuration, updates, service control)

These components are the primary surface of the current Rustyjack project.
```

 

---

## 📊 Performance

### Boot Time

| Device | Rust UI |
|--------|---------|
| Raspberry Pi Zero 2 W | (boot time varies) |
| Raspberry Pi 4 (4GB) | ~8 seconds |
| Raspberry Pi 5 (8GB) | ~6 seconds |

### Memory Usage

| Component | RAM |
|-----------|-----|
| rustyjack-ui (idle) | ~12 MB |
| rustyjack-core (idle) | ~5 MB |
| **Total (idle)** | **~17 MB** |

### Response Time

| Action | Time |
|--------|------|
| Button press → LCD update | ~20 ms |
| Menu navigation | ~30 ms |
| Button press → LCD update | ~20 ms |
| Menu navigation | ~30 ms |

### Binary Sizes

| Binary | Size |
|--------|------|
| rustyjack-core | ~5.2 MB |
| rustyjack-ui | ~6.8 MB |
| **Total** | **~12 MB** |

---

## 🐛 Troubleshooting

### LCD Not Working

**Symptom:** Blank, white, or garbled display

**Checks:**
```bash
# 1. SPI enabled?
ls -l /dev/spidev0.0
# Should exist

# 2. Boot config correct?
grep spi /boot/firmware/config.txt
# Should show: dtparam=spi=on

# 3. Service running?
systemctl status rustyjack
```

**Fix:**
```bash
echo "dtparam=spi=on" >> /boot/firmware/config.txt
echo "dtoverlay=spi0-2cs" >> /boot/firmware/config.txt
reboot
```

### Buttons Not Responding

**Symptom:** Pressing buttons does nothing

**Checks:**
```bash
# GPIO device exists?
ls -l /dev/gpiochip0

# Service running as root?
systemctl status rustyjack | grep User
# Should show: User=root
```

**Fix:** Service must run as root (configured by install script)

### Service Won't Start

**Checks:**
```bash
# View errors
journalctl -u rustyjack -xe

# Test binary manually
sudo /usr/local/bin/rustyjack-ui
```

**Fix:**
```bash
# Rebuild from source
cd /root/Rustyjack/rustyjack-ui
cargo build --release
sudo install target/release/rustyjack-ui /usr/local/bin/
sudo systemctl restart rustyjack
```

### Network Issues

If you encounter networking problems, prefer using the host OS to configure interfaces and routing. The UI focuses on presentation and orchestration — network configuration and advanced wireless tooling should be managed via the OS and CLI tools on the device.

<!-- Removed legacy attack feature references -->

### Out of Disk Space

**Clean up:**
```bash
# Remove old logs
journalctl --vacuum-time=7d

# Clean Rust build cache
cd /root/Rustyjack
cd rustyjack-core && cargo clean
cd ../rustyjack-ui && cargo clean

# Archive loot
tar -czf ~/loot_$(date +%Y%m%d).tar.gz loot/
rm -rf loot/*
mkdir -p loot/Wireless
```

---

## 📜 Credits

### Original Project

**Rustyjack** - Created by [@Iwan-Teague](https://github.com/Iwan-Teague)  
Repository: https://github.com/Iwan-Teague/Rusty-Jack.git

### Rust Libraries

- **embedded-graphics** - James Waples
- **st7735-lcd** - Various contributors
- **gpio-cdev** - posborne
- **clap** - Kevin K.
- **serde** - David Tolnay

---

## 📄 License

**Educational and Authorized Testing Only**

This software is provided for:
- ✅ Security education and research
- ✅ Authorized penetration testing
- ✅ CTF competitions
- ✅ Personal lab environments

This software is **NOT** for:
- ❌ Unauthorized access
- ❌ Malicious activities
- ❌ Breaking laws
- ❌ Causing harm

**You are solely responsible for your use of this tool.**

**The authors provide NO WARRANTY and accept NO LIABILITY for misuse.**

---

## ⚠️ Legal Warning

```
╔═══════════════════════════════════════════════════════╗
║                                                       ║
║   ⚠️  UNAUTHORIZED ACCESS IS A FEDERAL CRIME  ⚠️      ║
║                                                       ║
║   Violating computer security laws can result in:    ║
║   • Heavy fines ($100,000+)                          ║
║   • Prison sentences (up to 20 years)                ║
║   • Permanent criminal record                        ║
║   • Civil lawsuits                                   ║
║                                                       ║
║   ALWAYS obtain written permission before testing.   ║
║                                                       ║
╚═══════════════════════════════════════════════════════╝
```

---

<div align="center">

**🦀 Rustyjack - Portable Offensive Security 🦀**

*Built with Rust • Powered by Raspberry Pi • Made for Security Professionals*

</div>
