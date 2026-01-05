use anyhow::{anyhow, Context, Result};
use tracing::debug;

#[cfg(target_os = "linux")]
use zbus::blocking::Connection;

#[cfg(target_os = "linux")]
pub struct NetworkManagerClient {
    enabled: bool,
}

#[cfg(not(target_os = "linux"))]
pub struct NetworkManagerClient {
    enabled: bool,
}

impl NetworkManagerClient {
    pub fn new(enabled: bool) -> Self {
        Self { enabled }
    }
    
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
    
    #[cfg(target_os = "linux")]
    pub fn set_device_managed(&self, interface: &str, managed: bool) -> Result<()> {
        if !self.enabled {
            debug!("NetworkManager integration disabled, skipping set_device_managed for {}", interface);
            return Ok(());
        }
        
        let device_path = self.find_device_path(interface)?;
        let connection = Connection::system()
            .context("failed to connect to system D-Bus")?;
        
        let proxy = zbus::blocking::Proxy::new(
            &connection,
            "org.freedesktop.NetworkManager",
            device_path.as_str(),
            "org.freedesktop.DBus.Properties",
        ).context("failed to create D-Bus proxy")?;
        
        let _: () = proxy.call(
            "Set",
            &("org.freedesktop.NetworkManager.Device", "Managed", zbus::zvariant::Value::new(managed))
        ).context(format!("failed to set managed={} for {}", managed, interface))?;
        
        debug!("NetworkManager: set {} managed={}", interface, managed);
        Ok(())
    }
    
    #[cfg(not(target_os = "linux"))]
    pub fn set_device_managed(&self, _interface: &str, _managed: bool) -> Result<()> {
        debug!("NetworkManager integration not available on non-Linux platform");
        Ok(())
    }
    
    #[cfg(target_os = "linux")]
    fn find_device_path(&self, interface: &str) -> Result<String> {
        let connection = Connection::system()
            .context("failed to connect to system D-Bus")?;
        
        let nm_proxy = zbus::blocking::Proxy::new(
            &connection,
            "org.freedesktop.NetworkManager",
            "/org/freedesktop/NetworkManager",
            "org.freedesktop.NetworkManager",
        ).context("failed to create NetworkManager proxy")?;
        
        let devices: Vec<zbus::zvariant::OwnedObjectPath> = nm_proxy
            .call("GetDevices", &())
            .context("failed to get device list from NetworkManager")?;
        
        for device_path in devices {
            let device_proxy = zbus::blocking::Proxy::new(
                &connection,
                "org.freedesktop.NetworkManager",
                device_path.as_str(),
                "org.freedesktop.DBus.Properties",
            )?;
            
            let iface_result: std::result::Result<zbus::zvariant::OwnedValue, zbus::Error> = device_proxy
                .call("Get", &("org.freedesktop.NetworkManager.Device", "Interface"));
            
            if let Ok(iface_value) = iface_result {
                // Try to extract the interface name from the D-Bus variant
                if let Ok(iface) = iface_value.downcast_ref::<String>() {
                    if iface == interface {
                        return Ok(device_path.to_string());
                    }
                } else if let Ok(iface) = iface_value.downcast_ref::<&str>() {
                    if iface == interface {
                        return Ok(device_path.to_string());
                    }
                }
            }
        }
        
        Err(anyhow!("NetworkManager device not found for interface: {}", interface))
    }
    
    #[cfg(target_os = "linux")]
    pub fn get_device_managed(&self, interface: &str) -> Result<bool> {
        if !self.enabled {
            return Ok(false);
        }
        
        let device_path = self.find_device_path(interface)?;
        let connection = Connection::system()
            .context("failed to connect to system D-Bus")?;
        
        let proxy = zbus::blocking::Proxy::new(
            &connection,
            "org.freedesktop.NetworkManager",
            device_path.as_str(),
            "org.freedesktop.DBus.Properties",
        ).context("failed to create D-Bus proxy")?;
        
        let managed_result: std::result::Result<zbus::zvariant::OwnedValue, zbus::Error> = proxy
            .call("Get", &("org.freedesktop.NetworkManager.Device", "Managed"));
        
        let managed_value = managed_result
            .context(format!("failed to get managed state for {}", interface))?;
        
        let managed = managed_value
            .downcast_ref::<bool>()
            .map_err(|_| anyhow!("failed to parse managed state as boolean"))?;
        
        Ok(managed)
    }
    
    #[cfg(not(target_os = "linux"))]
    pub fn get_device_managed(&self, _interface: &str) -> Result<bool> {
        Ok(false)
    }
    
    pub fn is_available() -> bool {
        #[cfg(target_os = "linux")]
        {
            match Connection::system() {
                Ok(conn) => {
                    zbus::blocking::Proxy::new(
                        &conn,
                        "org.freedesktop.NetworkManager",
                        "/org/freedesktop/NetworkManager",
                        "org.freedesktop.NetworkManager",
                    ).is_ok()
                }
                Err(_) => false,
            }
        }
        
        #[cfg(not(target_os = "linux"))]
        {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_client_disabled_by_default() {
        let client = NetworkManagerClient::new(false);
        assert!(!client.is_enabled());
    }
    
    #[test]
    fn test_client_can_be_enabled() {
        let client = NetworkManagerClient::new(true);
        assert!(client.is_enabled());
    }
    
    #[test]
    fn test_set_device_managed_when_disabled_succeeds() {
        let client = NetworkManagerClient::new(false);
        let result = client.set_device_managed("eth0", false);
        assert!(result.is_ok());
    }
}
