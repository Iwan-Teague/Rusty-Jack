use std::env;
use std::future::Future;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rustyjack_client::{ClientConfig, DaemonClient};
use rustyjack_commands::Commands;
use rustyjack_ipc::{
    BlockDeviceInfo, HotspotClient, HotspotDiagnosticsResponse, HotspotWarningsResponse,
    WifiCapabilitiesResponse,
};
use serde_json::Value;
use tokio::runtime::Handle;

pub type HandlerResult = (String, Value);

#[derive(Clone)]
pub struct CoreBridge {
    root: PathBuf,
}

impl CoreBridge {
    pub fn with_root(root: Option<PathBuf>) -> Result<Self> {
        let resolved = resolve_root(root)?;
        Ok(Self { root: resolved })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    async fn create_client(&self) -> Result<DaemonClient> {
        let config = ClientConfig {
            socket_path: daemon_socket_path(),
            client_name: "rustyjack-ui".to_string(),
            client_version: env!("CARGO_PKG_VERSION").to_string(),
            ..Default::default()
        };
        DaemonClient::connect_with_config(config).await
    }

    pub fn dispatch(&self, _command: Commands) -> Result<HandlerResult> {
        self.block_on(async move {
            let _client = self.create_client().await?;
            
            // For now, return a NotImplemented error since CoreDispatch is deprecated
            // UI should be updated to use explicit endpoints instead
            Err(anyhow::anyhow!("CoreDispatch is deprecated - use explicit endpoints instead"))
        })
    }

    pub fn disk_usage(&self, path: &str) -> Result<(u64, u64)> {
        let path = path.to_string();
        self.block_on(async move {
            let mut client = self.create_client().await?;
            let response = client.disk_usage(&path).await?;
            Ok((response.used_bytes, response.total_bytes))
        })
    }

    pub fn block_devices(&self) -> Result<Vec<BlockDeviceInfo>> {
        self.block_on(async move {
            let mut client = self.create_client().await?;
            let response = client.block_devices().await?;
            Ok(response.devices)
        })
    }

    pub fn system_logs(&self) -> Result<String> {
        self.block_on(async move {
            let mut client = self.create_client().await?;
            let response = client.system_logs().await?;
            Ok(response.content)
        })
    }

    pub fn wifi_capabilities(&self, interface: &str) -> Result<WifiCapabilitiesResponse> {
        let interface = interface.to_string();
        self.block_on(async move {
            let mut client = self.create_client().await?;
            client.wifi_capabilities(&interface).await
        })
    }

    pub fn hotspot_warnings(&self) -> Result<HotspotWarningsResponse> {
        self.block_on(async move {
            let mut client = self.create_client().await?;
            client.hotspot_warnings().await
        })
    }

    pub fn hotspot_diagnostics(&self, ap_interface: &str) -> Result<HotspotDiagnosticsResponse> {
        let ap_interface = ap_interface.to_string();
        self.block_on(async move {
            let mut client = self.create_client().await?;
            client.hotspot_diagnostics(&ap_interface).await
        })
    }

    pub fn hotspot_clients(&self) -> Result<Vec<HotspotClient>> {
        self.block_on(async move {
            let mut client = self.create_client().await?;
            let response = client.hotspot_clients().await?;
            Ok(response.clients)
        })
    }

    #[allow(dead_code)]
    pub fn gpio_diagnostics(&self) -> Result<String> {
        self.block_on(async move {
            let mut client = self.create_client().await?;
            let response = client.gpio_diagnostics().await?;
            Ok(response.content)
        })
    }

    fn block_on<F, T>(&self, fut: F) -> Result<T>
    where
        F: Future<Output = Result<T>>,
    {
        match Handle::try_current() {
            Ok(handle) => handle.block_on(fut),
            Err(_) => {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .context("building tokio runtime for daemon client")?;
                rt.block_on(fut)
            }
        }
    }
}

fn daemon_socket_path() -> PathBuf {
    env::var("RUSTYJACKD_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/run/rustyjack/rustyjackd.sock"))
}

fn resolve_root(input: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = input {
        return Ok(path);
    }

    if let Ok(env_path) = env::var("RUSTYJACK_ROOT") {
        return Ok(PathBuf::from(env_path));
    }

    let default = PathBuf::from("/var/lib/rustyjack");
    if default.exists() {
        return Ok(default);
    }

    let legacy = PathBuf::from("/root/Rustyjack");
    if legacy.exists() {
        return Ok(legacy);
    }

    env::current_dir().context("determining current directory")
}
