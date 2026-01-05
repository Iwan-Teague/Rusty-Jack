# RUSTYJACK FIX PRIORITIES - Implementation Plan

## PHASE 1: Hardware Selection & Isolation (CRITICAL)
**Status**: Implemented; requires device validation

### Tasks:
1. Verify Hardware menu properly calls daemon set_active_interface RPC
2. Confirm isolation engine shuts down non-active interfaces  
3. Ensure hotspot exception allows AP + upstream simultaneously
4. Test interface changes require user action (no auto-switching)

**Progress**:
- Added daemon endpoints for active interface get/clear and interface status; UI now uses these.
- UI clears active interface on return to main menu.
- Hardware Detect now calls daemon `set_active_interface` and persists selection to GUI config.
- Removed auto-switching when entering menus; user must explicitly set the active interface.
- Hotspot service enforces hotspot exception for dual-interface mode; offline hotspot isolates AP only.

## PHASE 2: Preflight Checks (HIGH PRIORITY)
**Status**: Implemented for all UI operations; ready for device validation

### Tasks:
1. Add preflight checks to ALL wireless operations:
   - Scan: Check interface exists, is wireless
   - Deauth: Check monitor mode, injection capable
   - Evil Twin: Check AP mode capable
   - Handshake Capture: Check monitor + injection
   - PMKID: Check interface exists, wireless
   
2. Add preflight checks to ethernet operations:
   - Port Scan: Check interface is ethernet type
   - ARP operations: Check interface exists, is up
   
3. Add preflight checks to hotspot:
   - Check AP interface supports AP mode
   - Check upstream interface exists and is up
   - Check for IP conflicts
   
4. Implement persistent UI preflight warning dialog:
   - Show detailed failure reasons
   - Text wrapping for long messages
   - Require user button press to dismiss
   - Don't show if all checks pass

**Progress**:
- Wireless: scan, deauth, evil twin, handshake capture, PMKID capture, probe sniff now validate interface existence/capabilities.
- Hotspot preflight validates upstream interface exists/is up and checks subnet conflicts.
- Ethernet and MITM preflights validate interface exists/is up and IP presence; MITM now rejects wireless interfaces.
- Attack pipelines now enforce preflight on each step (scan/PMKID/deauth/probe sniff/karma/evil twin).
- Preflight dialog uses explicit user confirmation with wrapped, scrollable text.

## PHASE 3: WiFi Service Implementation (HIGH PRIORITY)  
**Status**: Implemented; connection now validates interface, scans with nl80211, and fails if DHCP cannot be acquired

### Tasks:
1. Implement wifi_scan() using rustyjack-wireless
2. Implement wifi_connect() using wpa_supplicant/nmcli
3. Implement wifi_disconnect() properly
4. Remove placeholder returns

**Progress**:
- RustWpa2 station backend now performs nl80211 scans and builds candidates with security parsing.
- Open-network connect path uses nl80211 connect without WPA attributes.
- Connect validates interface existence/type and errors if DHCP lease cannot be acquired.
- WiFi disconnect uses nl80211 disconnect + DHCP release.

## PHASE 4: Missing RPC Endpoints (MEDIUM PRIORITY)
**Status**: Completed

### Tasks:
1. Add get_active_interface RPC endpoint to daemon
2. Add clear_active_interface RPC endpoint to daemon
3. Add interface_status RPC endpoint with detailed info
4. Wire up UI CoreBridge to call these endpoints

**Progress**:
- Implemented all three endpoints and wired UI CoreBridge methods.
- Added explicit command-group RPCs for all core command variants; UI dispatch uses these directly.

## PHASE 5: Portal IP Configuration (LOW PRIORITY)
**Status**: Implemented; uses interface IP with fallback

### Task:
1. Make portal bind IP configurable or derive from AP interface

**Progress**:
- Portal service now derives bind IP from the selected interface and falls back to 0.0.0.0 if unavailable.

## PHASE 6: Comprehensive Testing
1. Test each operation end-to-end
2. Verify preflight checks block invalid operations
3. Verify hardware isolation works
4. Test hotspot two-interface exception
5. Verify UI shows proper errors
6. Repair or replace broken tests (currently Linux-only assumptions and recursive test client import)

---
## Current Focus: START WITH PHASE 2 (Preflight Checks)
User specifically requested systematic preflight check implementation.
