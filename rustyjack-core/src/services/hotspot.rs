use crate::services::error::ServiceError;

use rustyjack_ipc::{
    HotspotApSupport, HotspotClient, HotspotClientsResponse, HotspotDiagnosticsResponse,
    HotspotWarningsResponse, RfkillEntry,
};

#[cfg(target_os = "linux")]
use rustyjack_netlink::{
    allowed_ap_channels, peek_last_start_ap_error, take_last_ap_error, RfkillManager,
    WirelessManager,
};
#[cfg(target_os = "linux")]
use rustyjack_wireless::{hotspot_leases, read_regdom_info, take_last_hotspot_warning};

pub fn warnings() -> Result<HotspotWarningsResponse, ServiceError> {
    #[cfg(target_os = "linux")]
    {
        Ok(HotspotWarningsResponse {
            last_warning: take_last_hotspot_warning(),
            last_ap_error: take_last_ap_error(),
            last_start_error: peek_last_start_ap_error(),
        })
    }

    #[cfg(not(target_os = "linux"))]
    {
        Ok(HotspotWarningsResponse {
            last_warning: None,
            last_ap_error: None,
            last_start_error: None,
        })
    }
}

pub fn diagnostics(ap_interface: &str) -> Result<HotspotDiagnosticsResponse, ServiceError> {
    if ap_interface.trim().is_empty() {
        return Err(ServiceError::InvalidInput("interface".to_string()));
    }

    #[cfg(target_os = "linux")]
    {
        let regdom = read_regdom_info();
        let rfkill = match RfkillManager::new().list() {
            Ok(devices) => devices
                .into_iter()
                .map(|dev| RfkillEntry {
                    idx: dev.idx,
                    type_name: dev.type_.name().to_string(),
                    state: dev.state_string().to_string(),
                    name: dev.name.clone(),
                })
                .collect(),
            Err(err) => {
                return Err(ServiceError::Netlink(format!(
                    "rfkill list error: {err}"
                )))
            }
        };

        let ap_support = match WirelessManager::new() {
            Ok(mut mgr) => match mgr.get_phy_capabilities(ap_interface) {
                Ok(caps) => Some(HotspotApSupport {
                    supports_ap: caps.supports_ap,
                    supported_modes: caps
                        .supported_modes
                        .iter()
                        .map(|mode| mode.to_string().to_string())
                        .collect(),
                    supported_bands: caps.supported_bands.clone(),
                }),
                Err(_) => None,
            },
            Err(_) => None,
        };

        let allowed_channels = allowed_ap_channels(ap_interface).unwrap_or_default();

        Ok(HotspotDiagnosticsResponse {
            regdom_raw: regdom.raw,
            regdom_valid: regdom.valid,
            rfkill,
            ap_support,
            allowed_channels,
            last_start_error: peek_last_start_ap_error(),
        })
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = ap_interface;
        Ok(HotspotDiagnosticsResponse {
            regdom_raw: None,
            regdom_valid: false,
            rfkill: Vec::new(),
            ap_support: None,
            allowed_channels: Vec::new(),
            last_start_error: None,
        })
    }
}

pub fn clients() -> Result<HotspotClientsResponse, ServiceError> {
    #[cfg(target_os = "linux")]
    {
        let clients = hotspot_leases()
            .into_iter()
            .map(|lease| HotspotClient {
                mac: format_mac(&lease.mac),
                ip: lease.ip.to_string(),
                hostname: lease.hostname,
                lease_start: lease.lease_start,
            })
            .collect();
        Ok(HotspotClientsResponse { clients })
    }

    #[cfg(not(target_os = "linux"))]
    {
        Ok(HotspotClientsResponse { clients: Vec::new() })
    }
}

#[cfg(target_os = "linux")]
fn format_mac(mac: &[u8; 6]) -> String {
    format!(
        "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
    )
}

pub struct HotspotStartRequest {
    pub interface: String,
    pub ssid: String,
    pub passphrase: Option<String>,
    pub channel: Option<u8>,
}

pub fn start<F>(req: HotspotStartRequest, mut on_progress: F) -> Result<serde_json::Value, ServiceError>
where
    F: FnMut(u8, &str),
{
    if req.interface.trim().is_empty() {
        return Err(ServiceError::InvalidInput("interface".to_string()));
    }
    if req.ssid.trim().is_empty() {
        return Err(ServiceError::InvalidInput("ssid".to_string()));
    }
    
    on_progress(10, "Starting hotspot");
    
    #[cfg(target_os = "linux")]
    {
        use rustyjack_wireless::start_hotspot;
        
        on_progress(50, "Configuring access point");
        
        // Create config for hotspot
        let config = rustyjack_wireless::HotspotConfig {
            ap_interface: req.interface.clone(),
            upstream_interface: "eth0".to_string(), // Default to eth0, should be configurable
            ssid: req.ssid.clone(),
            password: req.passphrase.clone().unwrap_or_default(),
            channel: req.channel.unwrap_or(6),
            restore_nm_on_stop: true,
        };
        
        match start_hotspot(config) {
            Ok(_) => {
                on_progress(100, "Hotspot started");
                Ok(serde_json::json!({
                    "interface": req.interface,
                    "ssid": req.ssid,
                    "started": true
                }))
            }
            Err(e) => Err(ServiceError::OperationFailed(format!("Hotspot start failed: {}", e))),
        }
    }
    
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (req, on_progress);
        Err(ServiceError::External("Hotspot not supported on this platform".to_string()))
    }
}

pub fn stop() -> Result<bool, ServiceError> {
    #[cfg(target_os = "linux")]
    {
        use rustyjack_wireless::stop_hotspot;
        
        match stop_hotspot() {
            Ok(_) => Ok(true),
            Err(e) => Err(ServiceError::OperationFailed(format!("Hotspot stop failed: {}", e))),
        }
    }
    
    #[cfg(not(target_os = "linux"))]
    {
        Err(ServiceError::NotSupported)
    }
}
