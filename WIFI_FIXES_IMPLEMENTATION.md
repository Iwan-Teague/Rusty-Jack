# WiFi Manager Critical Fixes Implementation Plan

**Date**: 2025-11-22  
**Priority**: CRITICAL
**Estimated Time**: 40-60 hours

---

## Phase 1: Security Fixes (IMMEDIATE)

### 1.1 Password Encryption
**File**: `rustyjack-core/src/system.rs`
**Issue**: Passwords stored in plaintext

**Implementation**:
```rust
use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use base64::{Engine as _, engine::general_purpose};

const KEY_FILE: &str = "wifi/.keystore";

fn get_or_create_encryption_key(root: &Path) -> Result<[u8; 32]> {
    let key_path = root.join(KEY_FILE);
    if key_path.exists() {
        let encoded = fs::read_to_string(&key_path)?;
        let decoded = general_purpose::STANDARD.decode(encoded.trim())?;
        let mut key = [0u8; 32];
        key.copy_from_slice(&decoded);
        Ok(key)
    } else {
        let mut key = [0u8; 32];
        OsRng.fill_bytes(&mut key);
        let encoded = general_purpose::STANDARD.encode(&key);
        if let Some(parent) = key_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&key_path, encoded)?;
        // Set restrictive permissions
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&key_path)?.permissions();
            perms.set_mode(0o600);
            fs::set_permissions(&key_path, perms)?;
        }
        Ok(key)
    }
}

pub fn encrypt_password(root: &Path, password: &str) -> Result<String> {
    let key = get_or_create_encryption_key(root)?;
    let cipher = Aes256Gcm::new(&key.into());
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    
    let ciphertext = cipher
        .encrypt(nonce, password.as_bytes())
        .map_err(|e| anyhow!("Encryption failed: {}", e))?;
    
    let mut combined = nonce_bytes.to_vec();
    combined.extend_from_slice(&ciphertext);
    Ok(general_purpose::STANDARD.encode(&combined))
}

pub fn decrypt_password(root: &Path, encrypted: &str) -> Result<String> {
    let key = get_or_create_encryption_key(root)?;
    let cipher = Aes256Gcm::new(&key.into());
    
    let combined = general_purpose::STANDARD.decode(encrypted)?;
    if combined.len() < 12 {
        bail!("Invalid encrypted password format");
    }
    
    let (nonce_bytes, ciphertext) = combined.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);
    
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| anyhow!("Decryption failed: {}", e))?;
    
    String::from_utf8(plaintext).context("Invalid UTF-8 in decrypted password")
}
```

### 1.2 Secure nmcli Password Handling
**File**: `rustyjack-core/src/system.rs:1369-1396`

**Current (INSECURE)**:
```rust
if let Some(pass) = password {
    if !pass.is_empty() {
        cmd.args(["password", pass]);  // ← Visible in ps aux!
    }
}
```

**Fixed (SECURE)**:
```rust
use std::io::Write;
use std::process::{Command, Stdio};

pub fn connect_wifi_network_secure(
    interface: &str,
    ssid: &str,
    password: Option<&str>,
) -> Result<()> {
    // Kill existing connections (with wait)
    let _ = Command::new("pkill")
        .args(["-f", &format!("wpa_supplicant.*{interface}")])
        .status();
    thread::sleep(Duration::from_millis(200));  // ← Wait for cleanup
    
    let _ = Command::new("dhclient").args(["-r", interface]).status();
    let _ = Command::new("ip").args(["link", "set", interface, "up"]).status();
    
    // Build nmcli command without password in args
    let mut cmd = Command::new("nmcli");
    cmd.args(["--terse", "--wait", "15", "device", "wifi", "connect", ssid]);
    
    if !interface.is_empty() {
        cmd.args(["ifname", interface]);
    }
    
    // Handle password via stdin (secure)
    if let Some(pass) = password {
        if !pass.is_empty() {
            cmd.arg("--ask");  // ← Prompts for password on stdin
            cmd.stdin(Stdio::piped());
            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());
            
            let mut child = cmd.spawn().context("spawning nmcli")?;
            
            // Write password to stdin (not visible in process list)
            if let Some(mut stdin) = child.stdin.take() {
                writeln!(stdin, "{}", pass).context("writing password to nmcli")?;
            }
            
            let output = child.wait_with_output().context("waiting for nmcli")?;
            
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                bail!("nmcli connect failed: {}", parse_nmcli_error(&stderr));
            }
        } else {
            // Open network, no password
            cmd.stdout(Stdio::null());
            cmd.stderr(Stdio::piped());
            let output = cmd.output().context("executing nmcli")?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                bail!("nmcli connect failed: {}", parse_nmcli_error(&stderr));
            }
        }
    } else {
        // Open network
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::piped());
        let output = cmd.output().context("executing nmcli")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("nmcli connect failed: {}", parse_nmcli_error(&stderr));
        }
    }
    
    // Request DHCP (with verification)
    let dhcp_status = Command::new("dhclient").arg(interface).status();
    thread::sleep(Duration::from_secs(2));  // ← Wait for DHCP
    
    // Verify IP obtained
    let has_ip = verify_interface_has_ip(interface)?;
    if !has_ip {
        bail!("Connected but failed to obtain IP address via DHCP");
    }
    
    Ok(())
}

fn parse_nmcli_error(stderr: &str) -> String {
    // Parse specific nmcli errors for user-friendly messages
    if stderr.contains("Timeout") || stderr.contains("timeout") {
        "Connection timeout - network may be out of range or overloaded. Try moving closer to the access point.".to_string()
    } else if stderr.contains("AP not found") {
        "Access point not found - check SSID spelling or rescan networks.".to_string()
    } else if stderr.contains("Secrets were required") || stderr.contains("802-1x") {
        "Authentication failed - check password or network security settings.".to_string()
    } else if stderr.contains("No suitable device") {
        "Network interface not available - check interface name and status.".to_string()
    } else if stderr.contains("already active") {
        "Already connected to this network.".to_string()
    } else {
        format!("Connection failed: {}", stderr.lines().next().unwrap_or("Unknown error"))
    }
}

fn verify_interface_has_ip(interface: &str) -> Result<bool> {
    let output = Command::new("ip")
        .args(["-4", "addr", "show", "dev", interface])
        .output()?;
    
    if !output.status.success() {
        return Ok(false);
    }
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if line.trim().starts_with("inet ") && !line.contains("127.0.0.1") {
            return Ok(true);
        }
    }
    Ok(false)
}
```

### 1.3 Input Validation
**File**: `rustyjack-core/src/system.rs`

```rust
use regex::Regex;
use lazy_static::lazy_static;

lazy_static! {
    static ref INTERFACE_REGEX: Regex = Regex::new(r"^[a-zA-Z0-9_-]{1,15}$").unwrap();
    static ref SSID_REGEX: Regex = Regex::new(r"^[\x20-\x7E]{1,32}$").unwrap();
}

pub fn validate_interface_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("Interface name cannot be empty");
    }
    
    if name.len() > 15 {
        bail!("Interface name too long (max 15 characters)");
    }
    
    if !INTERFACE_REGEX.is_match(name) {
        bail!("Interface name contains invalid characters (only alphanumeric, dash, underscore allowed)");
    }
    
    // Check interface actually exists
    let sys_path = PathBuf::from("/sys/class/net").join(name);
    if !sys_path.exists() {
        bail!("Interface '{}' does not exist", name);
    }
    
    Ok(())
}

pub fn validate_ssid(ssid: &str) -> Result<()> {
    if ssid.is_empty() {
        bail!("SSID cannot be empty");
    }
    
    let byte_len = ssid.as_bytes().len();
    if byte_len > 32 {
        bail!("SSID too long ({} bytes, max 32 bytes)", byte_len);
    }
    
    // SSID can contain any bytes except null, but we'll be conservative
    if !SSID_REGEX.is_match(ssid) {
        bail!("SSID contains invalid characters");
    }
    
    Ok(())
}

pub fn validate_wifi_password(password: &str, security_type: &str) -> Result<()> {
    if password.is_empty() {
        return Ok(()); // Open network
    }
    
    let len = password.len();
    
    match security_type {
        "WPA" | "WPA2" | "WPA3" | "WPA-PSK" | "WPA2-PSK" => {
            if len < 8 {
                bail!("WPA/WPA2/WPA3 password must be at least 8 characters");
            }
            if len > 63 {
                bail!("WPA/WPA2/WPA3 password must be at most 63 characters");
            }
        }
        "WEP" => {
            // WEP uses 5 or 13 character ASCII or 10/26 hex digits
            if len != 5 && len != 13 && len != 10 && len != 26 {
                bail!("WEP password must be 5, 10, 13, or 26 characters");
            }
        }
        _ => {
            // Unknown security, be permissive but warn
            log::warn!("Unknown security type '{}', skipping password validation", security_type);
        }
    }
    
    Ok(())
}

// Wrap existing functions with validation
pub fn connect_wifi_network(
    interface: &str,
    ssid: &str,
    password: Option<&str>,
) -> Result<()> {
    // Validate inputs
    validate_interface_name(interface)?;
    validate_ssid(ssid)?;
    
    if let Some(pass) = password {
        // Assume WPA2 if we don't know security type
        validate_wifi_password(pass, "WPA2")?;
    }
    
    // Call secure implementation
    connect_wifi_network_secure(interface, ssid, password)
}
```

---

## Phase 2: Routing Fixes (CRITICAL)

### 2.1 Atomic Route Changes with Rollback
**File**: `rustyjack-core/src/system.rs:815-835`

```rust
pub fn set_default_route_atomic(interface: &str, gateway: Ipv4Addr) -> Result<()> {
    // Validate inputs
    validate_interface_name(interface)?;
    
    // Check interface is up and has IP
    let info = detect_interface(Some(interface.to_string()))?;
    if info.address.is_unspecified() {
        bail!("Interface {} has no IP address", interface);
    }
    
    // Check gateway is reachable
    if !is_gateway_reachable(gateway, Duration::from_secs(3))? {
        log::warn!("Gateway {} may not be reachable", gateway);
    }
    
    // Backup current route for rollback
    let old_route = read_default_route().ok().flatten();
    
    // Use REPLACE instead of DEL+ADD (atomic operation)
    let status = Command::new("ip")
        .args([
            "route",
            "replace",  // ← Atomic!  Not "del" then "add"
            "default",
            "via",
            &gateway.to_string(),
            "dev",
            interface,
        ])
        .status()
        .with_context(|| format!("replacing default route via {interface}"))?;
    
    if !status.success() {
        bail!("Failed to set default route via {} to {}", interface, gateway);
    }
    
    // Verify route was actually applied
    thread::sleep(Duration::from_millis(100));
    let new_route = read_default_route()
        .context("verifying new route")?
        .ok_or_else(|| anyhow!("Route applied but cannot be read back"))?;
    
    if new_route.interface.as_deref() != Some(interface) {
        // Route didn't apply correctly, rollback
        log::error!("Route verification failed, rolling back");
        if let Some(old) = old_route {
            if let (Some(old_iface), Some(old_gw)) = (old.interface, old.gateway) {
                let _ = Command::new("ip")
                    .args([
                        "route",
                        "replace",
                        "default",
                        "via",
                        &old_gw.to_string(),
                        "dev",
                        &old_iface,
                    ])
                    .status();
            }
        }
        bail!("Route verification failed after applying");
    }
    
    log::info!("Default route successfully changed to {} via {}", gateway, interface);
    Ok(())
}

fn is_gateway_reachable(gateway: Ipv4Addr, timeout: Duration) -> Result<bool> {
    // Try ping first
    if ping_host(&gateway.to_string(), timeout).unwrap_or(false) {
        return Ok(true);
    }
    
    // If ping fails, try ARP (works even if ICMP blocked)
    let output = Command::new("arping")
        .args([
            "-c", "1",
            "-W", &timeout.as_secs().to_string(),
            &gateway.to_string(),
        ])
        .output();
    
    if let Ok(output) = output {
        return Ok(output.status.success());
    }
    
    Ok(false)  // Can't verify, but allow anyway
}
```

### 2.2 Versioned Route Backups
**File**: `rustyjack-core/src/system.rs:1051-1080`

```rust
pub fn backup_routing_state(root: &Path) -> Result<PathBuf> {
    let backup_dir = root.join("wifi").join("route_backups");
    fs::create_dir_all(&backup_dir)?;
    
    // Create timestamped backup file
    let timestamp = Local::now().format("%Y%m%d_%H%M%S");
    let filename = format!("routes_{}.json", timestamp);
    let path = backup_dir.join(&filename);
    
    // Collect routing state
    let routes = Command::new("ip")
        .args(["route", "show"])
        .output()
        .context("backing up route table")?;
    
    let default_route = read_default_route().unwrap_or(None);
    
    let mut interfaces = serde_json::Map::new();
    for iface in ["eth0", "wlan0", "wlan1", "wlan2", "br0"] {
        if let Ok(output) = Command::new("ip").args(["addr", "show", iface]).output() {
            if output.status.success() {
                interfaces.insert(
                    iface.to_string(),
                    Value::String(String::from_utf8_lossy(&output.stdout).to_string()),
                );
            }
        }
    }
    
    let json_value = json!({
        "timestamp": Local::now().to_rfc3339(),
        "version": "1.0",
        "default_route": default_route,
        "all_routes": String::from_utf8_lossy(&routes.stdout),
        "interfaces": interfaces,
        "hostname": std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown".to_string()),
    });
    
    fs::write(&path, serde_json::to_string_pretty(&json_value)?)?;
    
    // Keep only last 10 backups
    cleanup_old_backups(&backup_dir, 10)?;
    
    log::info!("Routing state backed up to {}", path.display());
    Ok(path)
}

fn cleanup_old_backups(dir: &Path, keep: usize) -> Result<()> {
    let mut backups: Vec<_> = fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|s| s.to_str())
                == Some("json")
        })
        .collect();
    
    if backups.len() <= keep {
        return Ok(());
    }
    
    // Sort by modification time (newest first)
    backups.sort_by_key(|e| {
        e.metadata()
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH)
    });
    backups.reverse();
    
    // Delete old backups
    for old_backup in backups.iter().skip(keep) {
        let _ = fs::remove_file(old_backup.path());
    }
    
    Ok(())
}

pub fn restore_routing_state(root: &Path) -> Result<()> {
    let backup_dir = root.join("wifi").join("route_backups");
    
    // Find most recent backup
    let mut backups: Vec<_> = fs::read_dir(&backup_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|s| s.to_str())
                == Some("json")
        })
        .collect();
    
    if backups.is_empty() {
        bail!("No routing backups found in {}", backup_dir.display());
    }
    
    // Sort by modification time (newest first)
    backups.sort_by_key(|e| {
        e.metadata()
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH)
    });
    backups.reverse();
    
    let latest = &backups[0];
    let contents = fs::read_to_string(latest.path())?;
    let value: Value = serde_json::from_str(&contents)?;
    
    let route: Option<DefaultRouteInfo> = value
        .get("default_route")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok());
    
    let route = route.ok_or_else(|| anyhow!("Backup file missing default route"))?;
    let interface = route
        .interface
        .ok_or_else(|| anyhow!("Backup missing interface"))?;
    let gateway = route
        .gateway
        .ok_or_else(|| anyhow!("Backup missing gateway"))?;
    
    // VALIDATE before restoring
    validate_interface_name(&interface)?;
    
    let iface_exists = PathBuf::from("/sys/class/net").join(&interface).exists();
    if !iface_exists {
        bail!(
            "Cannot restore: interface '{}' no longer exists. Available interfaces: {}",
            interface,
            list_available_interfaces()?.join(", ")
        );
    }
    
    // Check if interface has IP
    let has_ip = verify_interface_has_ip(&interface)?;
    if !has_ip {
        log::warn!("Interface {} has no IP address, route may not work", interface);
    }
    
    // Apply route atomically
    set_default_route_atomic(&interface, gateway)?;
    
    log::info!("Routing state restored from {}", latest.path().display());
    Ok(())
}

fn list_available_interfaces() -> Result<Vec<String>> {
    let mut interfaces = Vec::new();
    for entry in fs::read_dir("/sys/class/net")? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name != "lo" {
            interfaces.push(name);
        }
    }
    Ok(interfaces)
}
```

---

## Phase 3: WiFi Scanning Improvements

### 3.1 Retry Logic & Better Errors
**File**: `rustyjack-core/src/system.rs:1163-1235`

```rust
pub fn scan_wifi_networks_robust(interface: &str, max_retries: u32) -> Result<Vec<WifiNetwork>> {
    validate_interface_name(interface)?;
    
    // Check interface is up
    let sys_path = PathBuf::from("/sys/class/net").join(interface);
    let operstate_path = sys_path.join("operstate");
    if operstate_path.exists() {
        let state = fs::read_to_string(&operstate_path)?.trim().to_string();
        if state != "up" {
            // Try to bring it up
            let _ = Command::new("ip").args(["link", "set", interface, "up"]).status();
            thread::sleep(Duration::from_secs(1));
        }
    }
    
    let mut last_error = None;
    
    for attempt in 0..max_retries {
        if attempt > 0 {
            log::info!("Retrying WiFi scan (attempt {}/{})", attempt + 1, max_retries);
            thread::sleep(Duration::from_secs(2));
        }
        
        match attempt_wifi_scan(interface) {
            Ok(networks) => {
                log::info!("WiFi scan successful, found {} networks", networks.len());
                return Ok(networks);
            }
            Err(e) => {
                last_error = Some(e);
                
                // Check if error is retryable
                if let Some(err_str) = last_error.as_ref().map(|e| e.to_string()) {
                    if err_str.contains("Device or resource busy") {
                        log::warn!("Interface busy, will retry...");
                        continue;
                    } else if err_str.contains("Operation not permitted") {
                        // Not retryable, need sudo
                        bail!("Permission denied: WiFi scanning requires root privileges. Run with sudo.");
                    } else if err_str.contains("No such device") {
                        bail!("Interface '{}' not found. Check interface name with: ip link show", interface);
                    }
                }
            }
        }
    }
    
    Err(last_error.unwrap_or_else(|| anyhow!("WiFi scan failed after {} attempts", max_retries)))
}

fn attempt_wifi_scan(interface: &str) -> Result<Vec<WifiNetwork>> {
    let output = Command::new("iwlist")
        .arg(interface)
        .arg("scan")
        .output()
        .with_context(|| format!("executing iwlist scan on {interface}"))?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("iwlist scan failed: {}", stderr);
    }
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut networks = parse_iwlist_output(&stdout)?;
    
    // Enhance with security information
    for network in &mut networks {
        if let Some(ssid) = &network.ssid {
            let security = detect_network_security(interface, ssid)?;
            network.encrypted = security != "Open";
        }
    }
    
    // Sort by signal strength (strongest first)
    networks.sort_by(|a, b| {
        b.signal_dbm
            .unwrap_or(-100)
            .cmp(&a.signal_dbm.unwrap_or(-100))
    });
    
    Ok(networks)
}

fn parse_iwlist_output(stdout: &str) -> Result<Vec<WifiNetwork>> {
    let mut networks = Vec::new();
    let mut current: Option<WifiNetwork> = None;
    
    for line in stdout.lines() {
        let line = line.trim();
        
        if line.starts_with("Cell ") && line.contains("Address:") {
            // Save previous network
            if let Some(net) = current.take() {
                networks.push(net);
            }
            
            // Start new network
            let mut network = WifiNetwork {
                ssid: None,
                bssid: None,
                quality: None,
                signal_dbm: None,
                channel: None,
                encrypted: true,
            };
            
            if let Some(addr) = line.split("Address:").nth(1) {
                network.bssid = Some(addr.trim().to_string());
            }
            
            current = Some(network);
            continue;
        }
        
        if let Some(net) = current.as_mut() {
            if let Some(idx) = line.find("ESSID:") {
                let essid = line[idx + 6..].trim().trim_matches('"');
                if !essid.is_empty() && essid != "\\x00" {
                    net.ssid = Some(essid.to_string());
                }
            } else if line.contains("Quality=") {
                // Parse quality (e.g., "Quality=70/70  Signal level=-30 dBm")
                if let Some(value) = line.split("Quality=").nth(1) {
                    net.quality = value.split_whitespace().next().map(|s| s.to_string());
                }
                if let Some(pos) = line.find("Signal level=") {
                    let value = &line[pos + "Signal level=".len()..];
                    if let Some(level) = value.split_whitespace().next() {
                        let cleaned = level.trim_end_matches("dBm").trim();
                        if let Ok(dbm) = cleaned.parse() {
                            net.signal_dbm = Some(dbm);
                        }
                    }
                }
            } else if line.starts_with("Channel") || line.contains("Channel:") {
                let parts: Vec<&str> = line.split(|c: char| c == ' ' || c == ':').collect();
                for part in parts {
                    if let Ok(ch) = part.parse::<u8>() {
                        if ch >= 1 && ch <= 165 {
                            net.channel = Some(ch);
                            break;
                        }
                    }
                }
            } else if line.contains("Encryption key:") {
                net.encrypted = !line.contains(":off");
            }
        }
    }
    
    // Save last network
    if let Some(net) = current {
        networks.push(net);
    }
    
    // Filter out networks without SSID (hidden networks handled separately)
    networks.retain(|n| n.ssid.is_some());
    
    Ok(networks)
}

fn detect_network_security(interface: &str, ssid: &str) -> Result<String> {
    // Use iw to get detailed security info
    let output = Command::new("iw")
        .args(["dev", interface, "scan", "ssid", ssid])
        .output();
    
    if let Ok(output) = output {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            
            if stdout.contains("WPA3") {
                return Ok("WPA3".to_string());
            } else if stdout.contains("WPA2") || stdout.contains("RSN") {
                return Ok("WPA2".to_string());
            } else if stdout.contains("WPA") {
                return Ok("WPA".to_string());
            } else if stdout.contains("WEP") {
                return Ok("WEP".to_string());
            } else if stdout.contains("capability: ESS") && !stdout.contains("Privacy") {
                return Ok("Open".to_string());
            }
        }
    }
    
    // Fallback to basic detection
    Ok("Unknown".to_string())
}
```

---

## Dependencies to Add

**File**: `rustyjack-core/Cargo.toml`

```toml
[dependencies]
# ... existing deps ...

# For password encryption
aes-gcm = "0.10"
base64 = "0.21"

# For validation
regex = "1.10"
lazy_static = "1.4"
```

---

## Testing Plan

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    #[test]
    fn test_encrypt_decrypt_password() {
        let temp_dir = TempDir::new().unwrap();
        let password = "SuperSecret123!";
        
        let encrypted = encrypt_password(temp_dir.path(), password).unwrap();
        assert_ne!(encrypted, password);
        
        let decrypted = decrypt_password(temp_dir.path(), &encrypted).unwrap();
        assert_eq!(decrypted, password);
    }
    
    #[test]
    fn test_validate_interface_name() {
        assert!(validate_interface_name("eth0").is_ok());
        assert!(validate_interface_name("wlan0").is_ok());
        assert!(validate_interface_name("wlp3s0").is_ok());
        
        assert!(validate_interface_name("").is_err());
        assert!(validate_interface_name("../../../etc/passwd").is_err());
        assert!(validate_interface_name("eth0; rm -rf /").is_err());
    }
    
    #[test]
    fn test_validate_ssid() {
        assert!(validate_ssid("MyNetwork").is_ok());
        assert!(validate_ssid("Test-Net_123").is_ok());
        
        assert!(validate_ssid("").is_err());
        assert!(validate_ssid(&"a".repeat(33)).is_err());  // Too long
    }
    
    #[test]
    fn test_validate_wifi_password() {
        assert!(validate_wifi_password("password123", "WPA2").is_ok());
        assert!(validate_wifi_password("12345678", "WPA2").is_ok());
        
        assert!(validate_wifi_password("short", "WPA2").is_err());  // Too short
        assert!(validate_wifi_password(&"a".repeat(64), "WPA2").is_err());  // Too long
    }
}
```

---

## Migration Guide

### For Existing Users

**Script**: `migrate_wifi_profiles.sh`

```bash
#!/bin/bash
# Migrate existing plaintext WiFi profiles to encrypted format

PROFILES_DIR="$HOME/Rustyjack/wifi/profiles"

if [ ! -d "$PROFILES_DIR" ]; then
    echo "No profiles to migrate"
    exit 0
fi

echo "=== WiFi Profile Migration ==="
echo "This will encrypt all saved WiFi passwords."
echo ""

# Backup existing profiles
BACKUP_DIR="$HOME/Rustyjack/wifi/profiles_backup_$(date +%Y%m%d_%H%M%S)"
mkdir -p "$BACKUP_DIR"
cp -r "$PROFILES_DIR"/* "$BACKUP_DIR/" 2>/dev/null

echo "Backed up to: $BACKUP_DIR"
echo ""

# Run Rust migration tool
cargo run --release --bin migrate-wifi-profiles -- "$PROFILES_DIR"

echo ""
echo "=== Migration Complete ==="
echo "Original profiles backed up to: $BACKUP_DIR"
echo "Encrypted profiles in: $PROFILES_DIR"
```

---

**STATUS**: Implementation plan ready. Would you like me to start implementing these fixes?

