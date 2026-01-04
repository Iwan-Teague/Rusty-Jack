# Rustyjack Daemon Implementation Roadmap
## Based on Senior Rust Developer Security Review

---

## Executive Summary

This document provides a clear roadmap for completing the Rustyjack daemon security implementation based on the comprehensive verification report in `rustyjack_daemon_verification_report.txt`.

**Current Status:** Core security boundaries (P0) are implemented. Key robustness features (UDS timeouts, real cancellation) remain incomplete.

---

## ✅ COMPLETED SECURITY FIXES

The following P0 security issues from the original review have been addressed:

1. **JobStart Validation** - No longer bypasses validation; all jobs validated before execution
2. **Per-Job Authorization** - Required tier enforcement in request loop; SystemUpdate is admin-only
3. **Policy-Based Mounting** - Switched to mount syscalls with core policy; prevents internal device mounting
4. **Log Bundle Capping** - SystemLogsGet enforces size caps to prevent max_frame overflow
5. **Async Offloading** - Heavy blocking work moved to spawn_blocking via helper
6. **Retention Safety** - Only evicts terminal jobs; never evicts running jobs

---

## 🔴 STAGE 1: UDS Robustness (Security P0)
**Priority:** CRITICAL - Prevents daemon DoS
**Status:** NOT IMPLEMENTED

### Problem
Only the Hello handshake has a timeout. All subsequent read_frame/write_frame calls can stall indefinitely, allowing a local client to DoS the daemon by connecting and stopping mid-frame.

### Implementation Plan

#### 1. Add Configuration Constants
**File:** `rustyjack-daemon/src/config.rs`

```rust
pub struct DaemonConfig {
    // ... existing fields ...
    pub read_timeout_ms: u64,   // default: 5000
    pub write_timeout_ms: u64,  // default: 5000
}
```

**Environment Variables:**
- `RUSTYJACKD_READ_TIMEOUT_MS` (default: 5000)
- `RUSTYJACKD_WRITE_TIMEOUT_MS` (default: 5000)

#### 2. Create Timeout Helpers
**File:** `rustyjack-daemon/src/framing.rs` (new or in server.rs)

```rust
async fn read_frame_timed(
    stream: &mut UnixStream,
    max_size: usize,
    timeout_ms: u64,
    peer: &PeerCred,
    endpoint: Option<&str>,
) -> Result<Vec<u8>> {
    match tokio::time::timeout(
        Duration::from_millis(timeout_ms),
        read_frame(stream, max_size)
    ).await {
        Ok(Ok(data)) => Ok(data),
        Ok(Err(e)) => Err(e),
        Err(_) => {
            log::warn!(
                "Read timeout ({}ms) from peer uid={} pid={} endpoint={:?}",
                timeout_ms, peer.uid, peer.pid, endpoint
            );
            Err(anyhow!("Read timeout"))
        }
    }
}

async fn write_frame_timed(
    stream: &mut UnixStream,
    data: &[u8],
    max_size: usize,
    timeout_ms: u64,
    peer: &PeerCred,
    endpoint: Option<&str>,
) -> Result<()> {
    match tokio::time::timeout(
        Duration::from_millis(timeout_ms),
        write_frame(stream, data, max_size)
    ).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(e),
        Err(_) => {
            log::warn!(
                "Write timeout ({}ms) to peer uid={} pid={} endpoint={:?}",
                timeout_ms, peer.uid, peer.pid, endpoint
            );
            Err(anyhow!("Write timeout"))
        }
    }
}
```

#### 3. Update Request Loop
**File:** `rustyjack-daemon/src/server.rs`

Replace all `read_frame` calls with `read_frame_timed`:
```rust
// Before
let req_bytes = read_frame(&mut stream, state.config.max_frame).await?;

// After
let req_bytes = read_frame_timed(
    &mut stream,
    state.config.max_frame,
    state.config.read_timeout_ms,
    &peer,
    None,
).await?;
```

Replace all `write_frame` calls with `write_frame_timed` (include endpoint when available).

#### 4. Error Response on Timeout
When timeout occurs, attempt to send DaemonError before closing socket:
```rust
Err(e) if e.to_string().contains("timeout") => {
    let err_resp = DaemonError {
        code: ErrorCode::Timeout,
        message: "Request timed out".into(),
        request_id: request_id.unwrap_or_default(),
    };
    // Best-effort write; ignore if it also times out
    let _ = write_frame_timed(&mut stream, &serde_json::to_vec(&err_resp)?, ...).await;
    break;
}
```

### Acceptance Criteria
- [ ] Config accepts RUSTYJACKD_READ_TIMEOUT_MS and RUSTYJACKD_WRITE_TIMEOUT_MS
- [ ] All read_frame calls wrapped in timeout with logging
- [ ] All write_frame calls wrapped in timeout with logging
- [ ] Timeout errors log peer credentials + endpoint + request_id
- [ ] Test: Client connects and stalls → daemon drops within timeout
- [ ] Test: Client sends request then stops reading → daemon times out on write

---

## 🔴 STAGE 2: Real Cancellation (Security P1)
**Priority:** HIGH - Current cancellation is ineffective
**Status:** PARTIALLY IMPLEMENTED (checks token but doesn't stop blocking work)

### Problem
Most job kinds check `cancel.is_cancelled()` once, then `await spawn_blocking(...)`. Cancellation won't interrupt the actual work. This wastes resources and prevents timely cleanup.

### Implementation Plan

#### 1. Create Cancellable Blocking Runner
**File:** `rustyjack-daemon/src/jobs/helpers.rs` (new)

```rust
use std::future::Future;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use anyhow::Result;

/// Run blocking work with cancellation support.
/// Returns Err on cancellation before completion.
pub async fn run_blocking_cancellable<F, T>(
    cancel: &CancellationToken,
    f: F,
) -> Result<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let handle: JoinHandle<T> = tokio::task::spawn_blocking(f);
    
    tokio::select! {
        result = handle => {
            result.map_err(|e| anyhow!("Task panicked: {}", e))
        }
        _ = cancel.cancelled() => {
            handle.abort();
            Err(anyhow!("Cancelled"))
        }
    }
}
```

#### 2. Refactor Job Kinds
**Files to update:**
- `rustyjack-daemon/src/jobs/mount.rs`
- `rustyjack-daemon/src/jobs/portal.rs`
- `rustyjack-daemon/src/jobs/scan.rs`
- `rustyjack-daemon/src/jobs/update.rs`
- `rustyjack-daemon/src/jobs/logs.rs`

**Example (mount_device):**
```rust
// Before
if ctx.cancel.is_cancelled() {
    return Err(anyhow!("Cancelled before mount"));
}
tokio::task::spawn_blocking(move || {
    // ... mount logic ...
}).await??;

// After
if ctx.cancel.is_cancelled() {
    return Err(anyhow!("Cancelled before mount"));
}
run_blocking_cancellable(&ctx.cancel, move || {
    // ... mount logic ...
}).await?
```

#### 3. Cancellable Subprocess Runner
**File:** `rustyjack-core/src/subprocess.rs` (new)

```rust
use std::process::{Command, Stdio};
use tokio_util::sync::CancellationToken;
use anyhow::{Result, anyhow};

pub async fn run_command_cancellable(
    cancel: &CancellationToken,
    mut cmd: Command,
) -> Result<std::process::Output> {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    
    let mut child = cmd.spawn()
        .map_err(|e| anyhow!("Failed to spawn: {}", e))?;
    
    loop {
        tokio::select! {
            status = child.wait() => {
                let output = child.wait_with_output()
                    .map_err(|e| anyhow!("Failed to collect output: {}", e))?;
                return Ok(output);
            }
            _ = cancel.cancelled() => {
                log::info!("Killing child process due to cancellation");
                let _ = child.kill();
                let _ = child.wait();
                return Err(anyhow!("Command cancelled"));
            }
        }
    }
}
```

#### 4. Update SystemUpdate Job
**File:** `rustyjack-daemon/src/jobs/update.rs`

Replace all `Command::output()` calls with `run_command_cancellable(&ctx.cancel, cmd).await?`

#### 5. Fix wifi_connect Use-After-Move
**File:** `rustyjack-daemon/src/jobs/wifi.rs`

```rust
// Before (BROKEN)
let result = spawn_blocking(move || {
    // request moved here
    core::wifi_connect(&request)
}).await?;
if ctx.cancel.is_cancelled() {
    core::wifi_disconnect(&request.interface)?; // ERROR: request already moved
}

// After (FIXED)
let iface = request.interface.clone();
let result = run_blocking_cancellable(&ctx.cancel, move || {
    core::wifi_connect(&request)
}).await;
if result.is_err() && ctx.cancel.is_cancelled() {
    // Disconnect only if this job established the connection
    core::wifi_disconnect(&iface)?;
}
```

### Acceptance Criteria
- [ ] `run_blocking_cancellable` helper implemented
- [ ] All blocking job kinds refactored to use helper
- [ ] Subprocess runner supports cancellation with process.kill()
- [ ] wifi_connect use-after-move bug fixed
- [ ] Test: Cancel sleep job → becomes Cancelled quickly (<1s)
- [ ] Test: Cancel update job → child processes killed, no zombies
- [ ] Test: Cancel mount job → blocking syscall returns or times out appropriately

---

## 🟡 STAGE 3: Group-Based Authorization (P1)
**Priority:** MEDIUM - Enables unprivileged UI with admin capabilities
**Status:** NOT IMPLEMENTED (currently uid==0 => Admin, else Operator)

### Problem
Current authorization is binary: root is Admin, everyone else is Operator. This forces the UI to run as root for SystemUpdate, which violates least privilege.

### Implementation Plan

#### 1. Parse Supplementary Groups
**File:** `rustyjack-daemon/src/auth.rs`

```rust
use std::fs;

/// Read supplementary group names for a process.
fn read_supplementary_groups(pid: u32) -> Result<Vec<String>> {
    let status = fs::read_to_string(format!("/proc/{}/status", pid))?;
    
    for line in status.lines() {
        if let Some(gids_str) = line.strip_prefix("Groups:") {
            let gids: Vec<u32> = gids_str
                .split_whitespace()
                .filter_map(|s| s.parse().ok())
                .collect();
            
            // Convert GIDs to group names
            let mut names = Vec::new();
            for gid in gids {
                if let Ok(Some(group)) = get_group_by_gid(gid) {
                    names.push(group.name);
                }
            }
            return Ok(names);
        }
    }
    
    Ok(Vec::new())
}

fn get_group_by_gid(gid: u32) -> Result<Option<GroupInfo>> {
    // Parse /etc/group or use libc getgrgid
    // Implementation details...
}
```

#### 2. Update Authorization Logic
**File:** `rustyjack-daemon/src/auth.rs`

```rust
pub fn authorization_for_peer(peer: &PeerCred, config: &DaemonConfig) -> AuthorizationTier {
    // Root is always admin
    if peer.uid == 0 {
        return AuthorizationTier::Admin;
    }
    
    // Check supplementary groups
    match read_supplementary_groups(peer.pid) {
        Ok(groups) => {
            if groups.contains(&config.admin_group) {
                return AuthorizationTier::Admin;
            }
            if groups.contains(&config.operator_group) {
                return AuthorizationTier::Operator;
            }
            // Not in any special group
            AuthorizationTier::ReadOnly
        }
        Err(e) => {
            log::warn!("Failed to read groups for pid {}: {}", peer.pid, e);
            // Fallback to basic uid check
            AuthorizationTier::ReadOnly
        }
    }
}
```

#### 3. Add Config Fields
**File:** `rustyjack-daemon/src/config.rs`

```rust
pub struct DaemonConfig {
    // ... existing fields ...
    pub admin_group: String,     // default: "rustyjack-admin"
    pub operator_group: String,  // default: "rustyjack-operator"
}
```

**Environment Variables:**
- `RUSTYJACKD_ADMIN_GROUP` (default: "rustyjack-admin")
- `RUSTYJACKD_OPERATOR_GROUP` (default: "rustyjack-operator")

#### 4. Update systemd Unit
**File:** `rustyjack-ui.service`

```ini
[Service]
User=rustyjack-ui
Group=rustyjack-admin
SupplementaryGroups=rustyjack-admin
# ... rest of unit ...
```

#### 5. Update Installer
**File:** `install_rustyjack.sh`

```bash
# Create groups
sudo groupadd -f rustyjack-admin
sudo groupadd -f rustyjack-operator

# Create UI user and add to admin group
sudo useradd -r -s /bin/false -G rustyjack-admin rustyjack-ui || true
sudo usermod -aG rustyjack-admin rustyjack-ui

# Socket accessible by operator group
sudo chgrp rustyjack-operator /run/rustyjackd.sock
sudo chmod 0660 /run/rustyjackd.sock
```

### Acceptance Criteria
- [ ] Config accepts admin_group and operator_group settings
- [ ] authorization_for_peer reads /proc/<pid>/status Groups:
- [ ] Root remains Admin regardless of groups
- [ ] Non-root in admin_group gets Admin tier
- [ ] Non-root in operator_group gets Operator tier
- [ ] Non-root in neither group gets ReadOnly tier
- [ ] Installer creates groups and assigns UI user
- [ ] Test: Non-root operator can mount but cannot SystemUpdate
- [ ] Test: Non-root admin can SystemUpdate (when dangerous_ops_enabled)

---

## 🟡 STAGE 4: Observability & Tests (P2)
**Priority:** MEDIUM - Enables confidence and debugging
**Status:** MINIMAL (few tests, basic logging)

### Implementation Plan

#### 1. Add Guardrail Tests
**File:** `rustyjack-daemon/tests/validation_tests.rs` (new)

```rust
#[cfg(test)]
mod tests {
    use rustyjack_daemon::validation::*;
    
    #[test]
    fn test_mount_rejects_mmcblk() {
        let result = validate_mount_device("/dev/mmcblk0p1");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("mmcblk"));
    }
    
    #[test]
    fn test_mount_rejects_loop() {
        let result = validate_mount_device("/dev/loop0");
        assert!(result.is_err());
    }
    
    #[test]
    fn test_mount_requires_dev_prefix() {
        let result = validate_mount_device("sda1");
        assert!(result.is_err());
    }
    
    // More tests...
}
```

**Additional test files:**
- `tests/retention_tests.rs` - Verify only terminal jobs evicted
- `tests/auth_tests.rs` - Verify tier mapping for job kinds
- `tests/timeout_tests.rs` - Verify frame timeouts work

#### 2. Upgrade to Tracing
**File:** `rustyjack-daemon/src/server.rs`

Replace `log::` macros with `tracing::` spans:

```rust
#[tracing::instrument(
    skip(stream, state),
    fields(
        peer_pid = %peer.pid,
        peer_uid = %peer.uid,
        peer_gid = %peer.gid,
        tier = tracing::field::Empty,
        request_id = tracing::field::Empty,
        endpoint = tracing::field::Empty,
        duration_ms = tracing::field::Empty,
    )
)]
async fn handle_connection(stream: UnixStream, state: Arc<ServerState>) {
    let span = tracing::Span::current();
    
    let authz = authorization_for_peer(&peer, &state.config);
    span.record("tier", &format!("{:?}", authz));
    
    // ... request loop ...
    span.record("request_id", &req.request_id);
    span.record("endpoint", endpoint_str);
    
    let start = Instant::now();
    // ... handle request ...
    span.record("duration_ms", start.elapsed().as_millis());
}
```

#### 3. Feature Negotiation
**File:** `rustyjack-daemon/src/server.rs`

```rust
fn build_hello_ack(config: &DaemonConfig) -> HelloAck {
    let mut features = Vec::new();
    
    if config.dangerous_ops_enabled {
        features.push(FeatureFlag::DangerousOpsEnabled);
    }
    
    features.push(FeatureFlag::JobProgress);
    // Future: ChunkedLogs, Compression, etc.
    
    HelloAck {
        protocol_version: PROTOCOL_VERSION,
        daemon_version: env!("CARGO_PKG_VERSION").to_string(),
        features,
    }
}
```

### Acceptance Criteria
- [ ] Validation tests cover edge cases (mmcblk, loop, /dev requirement)
- [ ] Retention tests verify only terminal jobs evicted
- [ ] Auth tests verify tier mapping
- [ ] Timeout integration tests pass
- [ ] Tracing spans include peer_uid, tier, request_id, endpoint, duration_ms
- [ ] HelloAck.features populated based on config
- [ ] Client can discover DangerousOpsEnabled, JobProgress features

---

## 🟡 STAGE 5: Attack Surface Reduction (P2)
**Priority:** LOW - Long-term hardening
**Status:** DEFERRED (architectural refactor required)

### Goals
1. **WiFi Migration:** Move real wireless operations into daemon-callable services (currently outside boundary)
2. **Portal Isolation:** Run captive portal in separate unprivileged process
3. **Systemd Hardening:** Add CapabilityBoundingSet, ProtectSystem, ProtectKernel*, etc.
4. **CLI Preference:** Make CLI thin daemon client (aligned with security model)

### Rationale for Deferral
These changes require significant architectural refactoring and don't address immediate security vulnerabilities. Complete Stages 1-4 first to establish a solid security foundation.

---

## 📊 Binary Build Status

### Required Binaries
Rustyjack consists of **4 binaries**:

1. **rustyjackd** - Main privileged daemon (rustyjack-daemon crate)
2. **rustyjack-ui** - Display UI for Waveshare HAT (rustyjack-ui crate)
3. **rustyjack-portal** - Captive portal web server (rustyjack-portal crate)
4. **rustyjack** - CLI tool for status/validation (rustyjack-core crate with `cli` feature)

### Build Scripts Status

✅ **FIXED:** All build scripts now build all 4 binaries

**Docker Build Scripts:**
- `tests/compile/build_arm32.ps1` - ✅ Updated to build all 4 binaries
- `tests/compile/build_arm32.sh` - ✅ Already correct
- `tests/compile/build_arm64.ps1` - ✅ Updated to build all 4 binaries
- `tests/compile/build_arm64.sh` - ✅ Already correct

**Install Scripts:**
- `install_rustyjack.sh` - ✅ Builds and installs all 4 binaries (lines 444-480)
- `install_rustyjack_dev.sh` - Should match production installer
- `install_rustyjack_prebuilt.sh` - Copies prebuilt binaries (verify includes all 4)

---

## 🎯 Recommended Implementation Order

1. **NEXT:** Stage 1 (UDS Timeouts) - Critical security fix, ~1-2 days
2. **THEN:** Stage 2 (Real Cancellation) - High priority, includes bug fix, ~2-3 days
3. **THEN:** Stage 3 (Group Auth) - Enables better deployment model, ~1-2 days
4. **THEN:** Stage 4 (Tests & Observability) - Confidence & debugging, ~2-3 days
5. **LATER:** Stage 5 (Attack Surface) - Long-term hardening, TBD

**Total estimated time for Stages 1-4:** ~6-10 days of focused development

---

## 📝 Documentation Updates Needed

After each stage, update:
1. `ROADMAP_COMPARISON.md` - Reflect actual completion status
2. `README.md` - Document new config options
3. `docs/SECURITY.md` - Document authorization model, timeouts
4. `CHANGELOG.md` - Record security improvements

---

## ✅ Acceptance Testing Checklist

### Stage 1 (UDS Timeouts)
- [ ] Client connects and stalls → daemon drops within timeout
- [ ] Client sends request then stops reading → daemon times out on write
- [ ] Timeout errors logged with peer credentials

### Stage 2 (Real Cancellation)
- [ ] Cancel sleep job → becomes Cancelled quickly (<1s)
- [ ] Cancel update job → child processes killed, no zombies
- [ ] Cancel mount job → returns promptly
- [ ] wifi_connect compiles without use-after-move error

### Stage 3 (Group Auth)
- [ ] Non-root operator can mount but not SystemUpdate
- [ ] Non-root admin can SystemUpdate (when dangerous_ops_enabled)
- [ ] Root remains Admin regardless of groups

### Stage 4 (Tests & Observability)
- [ ] All validation tests pass
- [ ] Retention tests pass
- [ ] Auth tier mapping tests pass
- [ ] Structured logs include request_id, peer, endpoint, duration
- [ ] Client can read HelloAck.features

---

## 🔒 Security Review Sign-Off Criteria

Before marking this roadmap COMPLETE:
1. ✅ All Stage 1 acceptance tests pass (UDS robustness)
2. ✅ All Stage 2 acceptance tests pass (real cancellation)
3. ✅ All Stage 3 acceptance tests pass (group-based auth)
4. ✅ Test coverage >50% for auth/validation/retention modules
5. ✅ No clippy warnings with `-D warnings`
6. ✅ Documentation updated to reflect new security model
7. ✅ Manual penetration test: Cannot DoS daemon via socket stall
8. ✅ Manual test: Cancellation works for all long-running job types

---

## 📚 References

- `rustyjack_daemon_verification_report.txt` - Full security analysis
- `rustyjack_security_review.txt` - Original security review
- `rustyjack_security_fix_implementation_roadmap.txt` - Original roadmap
- `ROADMAP_COMPARISON.md` - Current vs planned comparison

---

**Last Updated:** 2026-01-03  
**Next Review:** After Stage 1 completion
