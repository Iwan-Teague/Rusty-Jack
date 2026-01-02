# Docker Desktop Setup for Windows - Rustyjack ARM Cross-Compilation

This guide will help you set up Docker Desktop on Windows so you can build Rustyjack for Raspberry Pi.

## The Problem

You encountered this error:
```
ERROR: error during connect: Head "http://%2F%2F.%2Fpipe%2FdockerDesktopLinuxEngine/_ping": 
open //./pipe/dockerDesktopLinuxEngine: The system cannot find the file specified.
```

This means **Docker Desktop is not running** on your Windows machine.

## Quick Fix (5 minutes)

### Step 1: Install Docker Desktop

1. Download Docker Desktop for Windows:
   - Visit: https://www.docker.com/products/docker-desktop/
   - Click "Download for Windows"
   - Run the installer (`Docker Desktop Installer.exe`)

2. During installation:
   - Accept the default settings
   - Enable WSL 2 if prompted (recommended)
   - Restart your computer when prompted

### Step 2: Start Docker Desktop

1. After restart, launch "Docker Desktop" from the Start menu
2. Wait for Docker to start (this takes 1-2 minutes)
3. Look for the Docker whale icon in your system tray (bottom-right)
4. The icon should be **solid/green** when ready, not animated

### Step 3: Verify Docker is Running

Open PowerShell and run:
```powershell
docker ps
```

✅ **Success**: You'll see a table (even if empty):
```
CONTAINER ID   IMAGE     COMMAND   CREATED   STATUS    PORTS     NAMES
```

❌ **Still not working**: You'll see the pipe error again. Continue to troubleshooting below.

### Step 4: Enable ARM Emulation

Run this command once to enable ARM platform support:
```powershell
docker run --rm --privileged multiarch/qemu-user-static --reset -p yes
```

This downloads QEMU emulation support for building ARM binaries on your x86_64 Windows machine.

### Step 5: Test ARM Support

```powershell
# Test ARM64 support
docker run --rm --platform linux/arm64 alpine uname -m
# Should output: aarch64

# Test ARM32 support  
docker run --rm --platform linux/arm/v7 alpine uname -m
# Should output: armv7l
```

If both work, you're ready to build!

### Step 6: Build Rustyjack

```powershell
cd C:\Users\teagu\Desktop\Rustyjack
.\scripts\build_arm32.ps1
```

The first build will take **15-30 minutes** as it downloads the Rust toolchain and compiles all dependencies. Subsequent builds are much faster (2-5 minutes).

## Troubleshooting

### Docker Desktop Won't Start

**Check Windows version**: Docker Desktop requires Windows 10/11 Pro, Enterprise, or Education (64-bit). For Windows Home, you need WSL 2 enabled.

**Enable WSL 2**:
```powershell
# Run PowerShell as Administrator
wsl --install
# Restart computer
```

**Check Hyper-V/Virtualization**: 
- Open Task Manager → Performance tab
- Check if "Virtualization" is enabled
- If disabled, enable it in BIOS (consult your PC manufacturer)

### Docker Icon Shows "Docker Desktop Stopping..."

1. Close Docker Desktop completely
2. Open Task Manager and end any `Docker Desktop` or `dockerd` processes
3. Restart Docker Desktop from Start menu

### Error: "Hardware assisted virtualization and data execution protection must be enabled"

1. Restart your computer
2. Enter BIOS/UEFI settings (usually F2, F10, or Delete during boot)
3. Enable:
   - Intel VT-x / AMD-V (Virtualization Technology)
   - Intel VT-d / AMD-Vi (if available)
   - Execute Disable Bit
4. Save and exit BIOS

### WSL 2 is not installed

```powershell
# Run as Administrator
wsl --install
wsl --set-default-version 2
# Restart computer
```

Then in Docker Desktop:
- Settings → General → Enable "Use the WSL 2 based engine"
- Apply & Restart

### "docker ps" says "permission denied"

Run PowerShell or Docker Desktop as Administrator.

### Build is extremely slow

This is normal! QEMU emulation makes ARM builds 5-10x slower than native. First build takes longest:
- **First build**: 15-30 minutes
- **Incremental builds**: 2-5 minutes
- **Clean builds**: 10-15 minutes

To speed up:
1. Give Docker more resources:
   - Docker Desktop → Settings → Resources
   - Increase CPU to 4+ cores
   - Increase Memory to 8+ GB
2. Use `--release` builds only when deploying (debug is faster)

### Docker uses too much disk space

Docker images and build cache can use several GB:
```powershell
# Clean up unused Docker data
docker system prune -a
```

⚠️ This deletes all unused images and build cache. You'll need to re-download on next build.

## What Happens During Build

1. **Docker build phase** (first run only):
   - Downloads Rust ARM toolchain (~500 MB)
   - Creates a Docker image with cross-compilation tools
   - Takes 5-10 minutes

2. **Cargo build phase**:
   - Downloads and compiles Rust dependencies
   - First run: ~500 dependencies, takes 15-20 minutes
   - Subsequent runs: Only changed files, takes 2-5 minutes

3. **Output**:
   - Debug binaries: `target-32/armv7-unknown-linux-gnueabihf/debug/`
   - Copied to: `prebuilt/arm32/` for easy transfer to Pi

## Alternative: Use Mac or Linux

If Docker on Windows is problematic, the bash scripts work perfectly on:
- **Mac**: Just run `./scripts/build_arm32.sh`
- **Linux**: Just run `./scripts/build_arm32.sh`
- **WSL 2 on Windows**: Run bash scripts from Ubuntu/Debian terminal

The bash scripts are faster and have fewer compatibility issues.

## Next Steps

Once Docker is working:

1. **Build for your Pi**:
   - Pi Zero 2 W (32-bit OS): `.\scripts\build_arm32.ps1`
   - Pi Zero 2 W (64-bit OS): `.\scripts\build_arm64.ps1`
   - Pi 4/5 (64-bit): `.\scripts\build_arm64.ps1`

2. **Transfer binaries to Pi**:
   ```powershell
   # Using SCP (requires SSH enabled on Pi)
   scp prebuilt\arm32\* pi@raspberrypi.local:/tmp/
   ```

3. **Install on Pi**:
   ```bash
   # On the Pi
   cd /tmp
   sudo ./install_rustyjack.sh
   ```

## Why Docker?

Rustyjack is built for **ARM Linux** (Raspberry Pi) but you're developing on **x86_64 Windows**. Docker with QEMU emulation lets you:
- Build ARM binaries on Windows
- Test builds without a physical Pi nearby
- Avoid setting up complicated cross-compilation toolchains

It's slower than native, but it works reliably.

## Getting Help

If you're still stuck:
1. Check Docker Desktop logs: Settings → Troubleshoot → View logs
2. Verify Docker version: `docker --version` (should be 20.10+)
3. Check WSL 2: `wsl --status`
4. See full documentation: `scripts\BUILD_WINDOWS.md`

## TL;DR - One-Command Setup

```powershell
# Run PowerShell as Administrator
wsl --install
# Restart computer, then:
# Download and install Docker Desktop from docker.com
# Start Docker Desktop
docker run --rm --privileged multiarch/qemu-user-static --reset -p yes
# Now build works:
cd C:\Users\teagu\Desktop\Rustyjack
.\scripts\build_arm32.ps1
```

Good luck! 🚀
