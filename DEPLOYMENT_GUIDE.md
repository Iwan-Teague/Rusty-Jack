# Building and Deploying Daemon Enhancements

## Overview
This guide explains how to build and deploy the daemon integration enhancements on a Raspberry Pi Zero 2 W.

## Prerequisites

On the Raspberry Pi:
```bash
# Ensure Rust toolchain is installed
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install system dependencies (if not already done)
sudo apt update
sudo apt install -y \
    build-essential \
    pkg-config \
    libssl-dev \
    libdbus-1-dev \
    network-manager \
    wpasupplicant \
    rfkill \
    wireless-tools \
    hostapd \
    dnsmasq
```

## Building

### Option 1: Build on Raspberry Pi (Recommended for testing)

```bash
cd /path/to/Rustyjack

# Build the daemon
cargo build --release --package rustyjack-daemon

# Build the client
cargo build --release --package rustyjack-client

# Build the UI
cargo build --release --package rustyjack-ui

# Binaries will be in target/release/
ls -lh target/release/rustyjackd
ls -lh target/release/rustyjack-ui
```

### Option 2: Cross-compile from x86_64 Linux

```bash
# Install cross-compilation toolchain
rustup target add armv7-unknown-linux-gnueabihf

# Install cross-compilation tools
sudo apt install -y gcc-arm-linux-gnueabihf

# Build
cargo build --release --target armv7-unknown-linux-gnueabihf \
    --package rustyjack-daemon \
    --package rustyjack-client \
    --package rustyjack-ui

# Copy to Raspberry Pi
scp target/armv7-unknown-linux-gnueabihf/release/rustyjackd pi@rustyjack.local:/tmp/
scp target/armv7-unknown-linux-gnueabihf/release/rustyjack-ui pi@rustyjack.local:/tmp/
```

## Deployment

### Step 1: Stop existing services

```bash
sudo systemctl stop rustyjack-ui rustyjackd
```

### Step 2: Backup existing binaries

```bash
sudo cp /usr/local/bin/rustyjackd /usr/local/bin/rustyjackd.backup
sudo cp /usr/local/bin/rustyjack-ui /usr/local/bin/rustyjack-ui.backup
```

### Step 3: Install new binaries

```bash
# If built on Pi:
sudo cp target/release/rustyjackd /usr/local/bin/
sudo cp target/release/rustyjack-ui /usr/local/bin/

# If cross-compiled:
sudo mv /tmp/rustyjackd /usr/local/bin/
sudo mv /tmp/rustyjack-ui /usr/local/bin/

# Set permissions
sudo chown root:root /usr/local/bin/rustyjackd
sudo chown root:root /usr/local/bin/rustyjack-ui
sudo chmod 755 /usr/local/bin/rustyjackd
sudo chmod 755 /usr/local/bin/rustyjack-ui
```

### Step 4: Update service files

```bash
cd /path/to/Rustyjack

# Copy updated service files
sudo cp rustyjackd.service /etc/systemd/system/
sudo cp rustyjack-ui.service /etc/systemd/system/
sudo cp rustyjack.service /etc/systemd/system/

# Reload systemd
sudo systemctl daemon-reload
```

### Step 5: Verify configuration

```bash
# Check daemon service hardening
systemctl cat rustyjackd.service | grep -E "(Protect|Restrict|Lock|Memory)"

# Check UI service hardening  
systemctl cat rustyjack-ui.service | grep -E "(Protect|Restrict|Lock|Memory)"

# Verify socket configuration
systemctl cat rustyjackd.socket
```

Expected output should include:
- `ProtectSystem=strict`
- `ProtectHome=true`
- `RestrictRealtime=true`
- `MemoryDenyWriteExecute=true`
- `SystemCallArchitectures=native`
- `LockPersonality=true`

### Step 6: Start services

```bash
# Start daemon first
sudo systemctl start rustyjackd
sudo systemctl status rustyjackd

# Check daemon is responding
sudo ls -la /run/rustyjack/rustyjackd.sock

# Start UI
sudo systemctl start rustyjack-ui
sudo systemctl status rustyjack-ui
```

## Testing

### Test 1: Client Retry Logic

```bash
# Terminal 1: Monitor UI logs
sudo journalctl -u rustyjack-ui -f

# Terminal 2: Stop daemon
sudo systemctl stop rustyjackd

# In UI, try an operation - should see retry attempts
# Then restart daemon
sudo systemctl start rustyjackd

# UI should automatically reconnect
```

### Test 2: Input Validation

Test via UI or direct socket connection that invalid inputs are rejected:

```bash
# These should all fail with BadRequest errors:
# - Empty interface name
# - SSID over 32 bytes
# - PSK under 8 characters
# - Invalid channel (0 or >165)
# - Port number <1024
# - Device path with ".." 
```

### Test 3: Service Hardening

```bash
# Check UI process capabilities (should be minimal)
sudo cat /proc/$(pgrep rustyjack-ui)/status | grep Cap

# Verify UI cannot access /home or /root
sudo -u rustyjack-ui cat /root/.bashrc
# Expected: Permission denied

# Verify UI cannot write to /usr
sudo -u rustyjack-ui touch /usr/bin/test
# Expected: Permission denied or read-only filesystem

# Verify daemon has required capabilities
sudo cat /proc/$(pgrep rustyjackd)/status | grep Cap
```

### Test 4: Connection Resilience

```bash
# Start a long-running operation from UI
# During operation, use iptables to temporarily block loopback:
sudo iptables -A OUTPUT -o lo -j DROP
sleep 2
sudo iptables -D OUTPUT -o lo -j DROP

# Operation should retry and eventually succeed
```

### Test 5: CoreDispatch Restriction

```bash
# Try to use CoreDispatch (should fail)
# Can be tested via UI if it still has legacy dispatch code

# Check environment
systemctl show rustyjackd --property=Environment
# Should NOT show RUSTYJACKD_ALLOW_CORE_DISPATCH=true
```

## Monitoring

### Check daemon health

```bash
# View daemon logs
sudo journalctl -u rustyjackd -n 100

# View UI logs
sudo journalctl -u rustyjack-ui -n 100

# Check job status
# (via UI or direct socket connection)

# Monitor system resource usage
sudo systemd-cgtop
```

### Performance baseline

```bash
# Measure request latency
time echo '{"v":1,"request_id":1,"endpoint":"Health","body":{"type":"Health"}}' | \
    socat - UNIX-CONNECT:/run/rustyjack/rustyjackd.sock

# Should complete in <10ms under normal conditions
```

## Rollback Procedure

If issues arise:

```bash
# Stop services
sudo systemctl stop rustyjack-ui rustyjackd

# Restore old binaries
sudo cp /usr/local/bin/rustyjackd.backup /usr/local/bin/rustyjackd
sudo cp /usr/local/bin/rustyjack-ui.backup /usr/local/bin/rustyjack-ui

# Restore old service files (if you backed them up)
sudo cp /path/to/backup/rustyjackd.service /etc/systemd/system/
sudo cp /path/to/backup/rustyjack-ui.service /etc/systemd/system/
sudo systemctl daemon-reload

# Start services
sudo systemctl start rustyjackd rustyjack-ui
```

## Troubleshooting

### Daemon won't start

```bash
# Check logs
sudo journalctl -u rustyjackd -xe

# Common issues:
# - Socket permission errors: Check /run/rustyjack ownership
# - Missing dependencies: Verify all system packages installed
# - Binary architecture mismatch: Ensure correct ARM binary
```

### UI can't connect to daemon

```bash
# Verify socket exists and is accessible
ls -la /run/rustyjack/rustyjackd.sock

# Check UI user is in rustyjack group
groups rustyjack-ui

# Verify socket group ownership
stat /run/rustyjack/rustyjackd.sock

# Test socket manually
echo '{"v":1,"request_id":1,"endpoint":"Health","body":{"type":"Health"}}' | \
    socat - UNIX-CONNECT:/run/rustyjack/rustyjackd.sock
```

### Validation errors

```bash
# If operations fail with BadRequest:
# - Check input lengths (SSID ≤32, interface ≤64)
# - Verify PSK is 8-64 characters
# - Ensure channel is 1-165
# - Verify port is ≥1024
# - Check device paths are absolute with no ".."
```

### Service hardening issues

```bash
# If UI can't access required resources:
# - Check ReadWritePaths includes /var/lib/rustyjack
# - Check ReadOnlyPaths includes /run/rustyjack
# - Verify SupplementaryGroups includes gpio, spi for hardware access

# If daemon can't perform privileged operations:
# - Daemon runs as root - check User= line isn't present in rustyjackd.service
# - Verify NoNewPrivileges doesn't prevent required capabilities
```

## Performance Notes

On Raspberry Pi Zero 2 W:
- Daemon startup: ~200-500ms
- UI startup: ~300-800ms  
- Request latency: 1-10ms (local socket)
- Retry overhead: ~100-800ms per retry (only on failures)
- Validation overhead: <1ms per request

## Security Notes

### Default Configuration (Secure)
- CoreDispatch: **DISABLED**
- Dangerous operations: Controlled by RUSTYJACKD_DANGEROUS_OPS
- UI: Fully unprivileged (user rustyjack-ui)
- Daemon: Root but sandboxed with systemd

### Audit Recommendations
- Keep CoreDispatch disabled (secure by default)
- Enable dangerous_ops only when needed
- Review logs regularly for validation failures (may indicate attacks)
- Monitor for excessive retry attempts (may indicate network issues)

## Next Steps

After successful deployment:

1. Run full integration tests with actual hardware
2. Test WiFi operations (scan, connect, disconnect)
3. Test hotspot operations (start, stop, client management)
4. Test portal operations
5. Test mount/unmount operations
6. Monitor for 24 hours to ensure stability
7. Update any automation/scripts that relied on CoreDispatch

## Support

For issues, check:
1. This deployment guide
2. DAEMON_COMPLETION_IMPLEMENTATION.md for architecture details
3. IMPLEMENTATION_SUMMARY.txt for feature overview
4. Daemon logs: `sudo journalctl -u rustyjackd`
5. UI logs: `sudo journalctl -u rustyjack-ui`
