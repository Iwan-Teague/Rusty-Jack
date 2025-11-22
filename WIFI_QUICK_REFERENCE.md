# WiFi Manager - Quick Fix Reference

## Critical Fixes Applied ✅

### 1. Silent Errors → Comprehensive Logging
**Every WiFi operation now logs**:
- ℹ️ `log::info()` - Normal operations
- ⚠️ `log::warn()` - Non-critical issues  
- ❌ `log::error()` - Critical failures
- 🔍 `log::debug()` - Detailed debugging

**View logs**: `journalctl -u rustyjack -f`

---

### 2. Permission Checks
**Before**: Cryptic "Operation not permitted" errors  
**After**: Clear message: "Network operations require root privileges"

**Function**: `check_network_permissions()` checks UID == 0

---

### 3. Active Interface Indicator
**Before**: Couldn't tell which interface was routing traffic  
**After**: WiFi status shows `[ACTIVE]` marker on default route interface

**Example**:
```
Iface: wlan0 [ACTIVE]
IP: 192.168.1.100
Default via: wlan0
```

---

### 4. Graceful Failure & Cleanup
**Before**: Failed connections left interface broken  
**After**: Automatic cleanup on any error

**Function**: `cleanup_wifi_interface(interface)`
- Kills hanging wpa_supplicant
- Releases stale DHCP leases
- Ensures interface stays UP

---

### 5. Input Validation
**Profile Save Checks**:
- ✅ SSID not empty
- ✅ SSID ≤ 32 characters
- ✅ Interface name valid (alphanumeric + _ - only)
- ✅ Whitespace trimmed

---

### 6. Case-Insensitive Matching
**Before**: "MyNetwork" ≠ "mynetwork"  
**After**: All SSID matching uses `.to_lowercase()`

**Applies to**:
- Profile loading
- Profile deletion
- Profile searching
- Profile sorting

---

### 7. Retry Logic
**Operations with Retry**:
- pkill (wpa_supplicant cleanup): 3 attempts
- DHCP lease acquisition: 3 attempts with 2s delay
- Connection timeout: 20 seconds (was 10)

**Behavior**: Warns on retry, continues on success, fails gracefully after exhaustion

---

### 8. UI Progress Indicators
**Before**: UI froze during scan/connect  
**After**: "Please wait" message shown immediately

**Functions**:
- `show_progress()` - Non-blocking message
- `execute_with_progress()` - Operation wrapper

---

## Password Storage Notice 📝

**WiFi passwords stored as PLAINTEXT** in `wifi/profiles/*.json`

**This is intentional** for pentesting tool, but secure your device:
```bash
chmod 700 wifi/profiles/
chmod 600 wifi/profiles/*.json
```

**For future**: Consider encrypted storage using keyring/TPM

---

## Quick Troubleshooting

### WiFi Scan Not Working
1. Check permissions: `whoami` → must be `root`
2. Check interface is up: `ip link show wlan0`
3. Check logs: `journalctl -u rustyjack -n 50`

### Can't Connect to Network
1. Verify saved profile exists (case-insensitive)
2. Check password is correct
3. Ensure interface not busy (auto cleanup should handle)
4. Check DHCP server available on network
5. View connection logs for specific error

### Interface Stuck/Broken
1. Run manual cleanup:
   ```bash
   pkill -f wpa_supplicant
   dhclient -r wlan0
   ip link set wlan0 up
   ```
2. Or let code auto-cleanup on next operation attempt

### "Permission Denied" Errors
- Ensure running as root: `sudo systemctl restart rustyjack`
- Service file should have `User=root`

---

## Testing Commands

```bash
# Check service status
systemctl status rustyjack

# View live logs
journalctl -u rustyjack -f

# Check WiFi interfaces
ip link show

# Manual WiFi scan
iwlist wlan0 scan

# Check default route
ip route show default

# List saved profiles
ls -la /root/Rustyjack/wifi/profiles/

# Test permission
id -u  # Should be 0 for root
```

---

## Modified Files

**Core System**: `rustyjack-core/src/system.rs`
- scan_wifi_networks()
- connect_wifi_network()
- disconnect_wifi_interface()
- save_wifi_profile()
- load_wifi_profile()
- delete_wifi_profile()
- list_wifi_profiles()
- check_network_permissions() [NEW]
- cleanup_wifi_interface() [NEW]

**Operations**: `rustyjack-core/src/operations.rs`
- handle_wifi_scan()
- handle_wifi_status()
- handle_wifi_profile_*()
- handle_wifi_disconnect()

**UI**: `rustyjack-ui/src/app.rs`
- show_wifi_status_view()
- show_wifi_scan_menu()
- connect_named_profile()
- show_progress() [NEW]
- execute_with_progress() [NEW]

---

## Backward Compatibility ✅

- Existing profiles still work
- No config file changes needed
- No breaking API changes
- Old profile format auto-upgraded on use

---

**Last Updated**: 2025-11-22  
**Status**: All fixes tested and documented ✅
