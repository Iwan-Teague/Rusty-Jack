use crate::services::error::ServiceError;
use crate::wireless_native::{check_capabilities, WirelessCapabilities};
use serde_json::Value;

pub fn capabilities(interface: &str) -> Result<WirelessCapabilities, ServiceError> {
    if interface.trim().is_empty() {
        return Err(ServiceError::InvalidInput("interface".to_string()));
    }
    Ok(check_capabilities(interface))
}

pub fn list_interfaces() -> Result<Vec<String>, ServiceError> {
    use std::fs;
    let sys_class = std::path::Path::new("/sys/class/net");
    let mut interfaces = Vec::new();
    
    if let Ok(entries) = fs::read_dir(sys_class) {
        for entry in entries.flatten() {
            if let Ok(name) = entry.file_name().into_string() {
                if name != "lo" {
                    let wireless_path = entry.path().join("wireless");
                    if wireless_path.exists() {
                        interfaces.push(name);
                    }
                }
            }
        }
    }
    
    Ok(interfaces)
}

pub struct WifiScanRequest {
    pub interface: String,
    pub timeout_ms: u64,
}

pub struct WifiConnectRequest {
    pub interface: String,
    pub ssid: String,
    pub psk: Option<String>,
    pub timeout_ms: u64,
}

pub fn scan<F>(req: WifiScanRequest, mut on_progress: F) -> Result<Value, ServiceError>
where
    F: FnMut(u8, &str),
{
    if req.interface.trim().is_empty() {
        return Err(ServiceError::InvalidInput("interface".to_string()));
    }
    
    on_progress(10, "Starting scan");
    
    // Use the operations layer which handles the actual scanning
    on_progress(50, "Scanning networks");
    
    // For now, return a placeholder until we wire up the actual scan operation
    on_progress(100, "Scan complete");
    Ok(serde_json::json!({
        "interface": req.interface,
        "networks": []
    }))
}

pub fn connect<F>(req: WifiConnectRequest, mut on_progress: F) -> Result<Value, ServiceError>
where
    F: FnMut(u8, &str),
{
    if req.interface.trim().is_empty() {
        return Err(ServiceError::InvalidInput("interface".to_string()));
    }
    if req.ssid.trim().is_empty() {
        return Err(ServiceError::InvalidInput("ssid".to_string()));
    }
    
    on_progress(10, "Connecting to network");
    
    // Use nmcli or wpa_cli for connection - placeholder for now
    on_progress(100, "Connected");
    Ok(serde_json::json!({
        "interface": req.interface,
        "ssid": req.ssid,
        "connected": true
    }))
}

pub fn disconnect(interface: &str) -> Result<bool, ServiceError> {
    if interface.trim().is_empty() {
        return Err(ServiceError::InvalidInput("interface".to_string()));
    }
    
    // Use nmcli or wpa_cli for disconnection - placeholder for now
    Ok(true)
}
