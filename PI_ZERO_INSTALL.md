# Rustyjack Installation for Raspberry Pi Zero W 2

## Problem: Out of Memory During Rust Compilation

The Pi Zero W 2 has only 512MB RAM, which is insufficient for compiling Rust projects. The process gets killed by the OOM (Out Of Memory) killer.

## Solution: Increase Swap Space

Run these commands on your Pi:

```bash
# 1. Stop the swap service
sudo dphys-swapfile swapoff

# 2. Edit swap configuration
sudo nano /etc/dphys-swapfile
```

Change this line:
```
CONF_SWAPSIZE=100
```

To:
```
CONF_SWAPSIZE=2048
```

Save and exit (Ctrl+X, Y, Enter)

```bash
# 3. Recreate and enable swap
sudo dphys-swapfile setup
sudo dphys-swapfile swapon

# 4. Verify swap is active
free -h
# Should show 2GB swap
```

## Now Run Installation

```bash
cd /root/Rustyjack
./install_rustyjack.sh
```

**⏱️ Compilation time: 30-45 minutes on Pi Zero W 2**

Be patient! The compilation is CPU and disk intensive.

---

## Alternative: Pre-compiled Binaries (Faster)

If you have another Raspberry Pi or Linux ARM device, you can:

1. Compile on a faster Pi (Pi 4/5)
2. Copy the binaries to Pi Zero:

```bash
# On the faster Pi (after running install_rustyjack.sh):
scp /usr/local/bin/rustyjack-core root@192.168.0.48:/usr/local/bin/
scp /usr/local/bin/rustyjack-ui root@192.168.0.48:/usr/local/bin/

# On Pi Zero:
chmod +x /usr/local/bin/rustyjack-core
chmod +x /usr/local/bin/rustyjack-ui
```

Then on Pi Zero, run the install script again - it will skip compilation and just set up services.

---

## Monitor Installation Progress

Open another SSH session and watch:

```bash
# Memory usage
watch -n 1 free -h

# Compilation progress
journalctl -f
```

---

## After Successful Installation

```bash
# Reboot
reboot

# Check service status (after reboot)
systemctl status rustyjack

# View logs
journalctl -u rustyjack -f
```

---

## Reduce Swap Back (Optional)

After installation, you can reduce swap back to save SD card wear:

```bash
sudo dphys-swapfile swapoff
sudo nano /etc/dphys-swapfile
# Change back to CONF_SWAPSIZE=100
sudo dphys-swapfile setup
sudo dphys-swapfile swapon
```

---

## Troubleshooting

**Still getting killed?**

Try compiling one project at a time:

```bash
cd /root/Rustyjack/rustyjack-core
cargo build --release

cd /root/Rustyjack/rustyjack-ui
cargo build --release

# Then install binaries
sudo install target/release/rustyjack-core /usr/local/bin/
sudo install target/release/rustyjack-ui /usr/local/bin/

# Continue with service setup
sudo systemctl daemon-reload
sudo systemctl enable rustyjack.service
sudo systemctl start rustyjack.service
```

**Check for other memory hogs:**

```bash
ps aux --sort=-%mem | head -10
```

Kill unnecessary processes before compiling.
