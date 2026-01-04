## IMPLEMENTATION STATUS SUMMARY

Based on the daemon verification report, here are the remaining implementation stages:

### ✅ COMPLETED (from verification report):
1. JobStart validation enforcement
2. Per-job authorization tier enforcement
3. Policy-based mount/unmount
4. SystemLogsGet response capping
5. Blocking ops moved to spawn_blocking
6. Retention no longer evicts running jobs

### 🔴 STAGE 1 - UDS Robustness (Security P0) - READY TO IMPLEMENT
**What:** Frame read/write timeouts for all daemon<->client communication
**Why:** Prevents DoS from clients that connect and stall mid-frame
**Work:**
- Add RUSTYJACKD_READ_TIMEOUT_MS and RUSTYJACKD_WRITE_TIMEOUT_MS config
- Wrap all read_frame/write_frame in tokio::time::timeout
- Add helpers: read_frame_timed, write_frame_timed
- Log timeout errors with peer+endpoint+request_id

### 🔴 STAGE 2 - Real Cancellation (Security P1) - READY TO IMPLEMENT  
**What:** Proper cancellation that stops blocking work and subprocesses
**Why:** Current cancel checks once then blocks; work continues after cancel
**Work:**
- Implement run_blocking_cancellable(cancel, f) helper
- Refactor job kinds: mount/unmount, portal, scan, update, logs
- Add cancellable subprocess runner for update pipeline
- Fix wifi_connect use-after-move bug

### 🟡 STAGE 3 - Group-Based Authorization (P1)
**What:** Derive tier from supplementary groups instead of just uid==0
**Why:** Allows UI to run unprivileged but gain Admin via group membership
**Work:**
- Implement authorization_for_peer reading /proc/<pid>/status Groups:
- Add admin_group and operator_group to config
- Update systemd socket policy

### 🟡 STAGE 4 - Observability & Tests (P2)
**What:** Tests for critical guardrails + structured tracing + feature negotiation
**Work:**
- Tests for validation, mount policy, retention, tier mapping
- Switch from env_logger to tracing spans with request_id/peer/duration
- Implement HelloAck.features advertising

### 🟡 STAGE 5 - Attack Surface Reduction (P2)
**What:** WiFi migration to daemon boundary, portal isolation, systemd hardening
**Work:**
- Move real wifi ops into daemon-callable services
- Consider portal in separate unprivileged process
- Add CapabilityBoundingSet, ProtectSystem, ProtectKernel* to systemd units

---

## BINARIES BUILT

The project has these binary crates:
1. **rustyjackd** (rustyjack-daemon) - Main privileged daemon
2. **rustyjack-ui** (rustyjack-ui) - Display UI for Waveshare HAT  
3. **rustyjack-portal** (rustyjack-portal) - Captive portal web server

Current build scripts build all three correctly.

---

## BUILD SCRIPT ANALYSIS

### Docker Build Scripts:
- **build_arm32.ps1/sh**: ✅ Builds rustyjackd, rustyjack-ui, rustyjack-portal
- **build_arm64.ps1/sh**: Need to verify builds same binaries

### Install Scripts:
Need to verify these install the three binaries correctly:
- install_rustyjack.sh (production)
- install_rustyjack_dev.sh (debug)
- install_rustyjack_prebuilt.sh (prebuilt binaries)

