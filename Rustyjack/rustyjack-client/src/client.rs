use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use rustyjack_ipc::{
    endpoint_for_body, BlockDevicesResponse, ClientHello, CoreDispatchRequest, CoreDispatchResponse,
    DaemonError, DiskUsageRequest, DiskUsageResponse, ErrorCode, FeatureFlag,
    GpioDiagnosticsResponse, HealthResponse, HelloAck, HostnameResponse, HotspotClientsResponse,
    HotspotDiagnosticsRequest, HotspotDiagnosticsResponse, HotspotWarningsResponse, JobCancelRequest,
    JobCancelResponse, JobKind, JobSpec, JobStartRequest, JobStarted, JobStatusRequest,
    JobStatusResponse, RequestBody, RequestEnvelope, ResponseBody, ResponseEnvelope, ResponseOk,
    StatusResponse, SystemActionResponse, SystemLogsResponse, SystemStatusResponse, VersionResponse,
    WifiCapabilitiesRequest, WifiCapabilitiesResponse, MAX_FRAME, PROTOCOL_VERSION,
};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
#[cfg(unix)]
use tokio::net::UnixStream;
use tokio::time::{sleep, timeout};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(2);
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const LONG_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_RETRY_ATTEMPTS: u32 = 3;
const INITIAL_RETRY_DELAY: Duration = Duration::from_millis(100);

#[derive(Debug, Clone)]
pub struct DaemonClientInfo {
    pub daemon_version: String,
    pub protocol_version: u32,
    pub features: Vec<FeatureFlag>,
    pub authz: rustyjack_ipc::AuthzSummary,
    pub max_frame: u32,
}

#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub socket_path: PathBuf,
    pub client_name: String,
    pub client_version: String,
    pub request_timeout: Duration,
    pub long_request_timeout: Duration,
    pub max_retries: u32,
    pub retry_delay_ms: u64,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            socket_path: PathBuf::from("/run/rustyjack/rustyjackd.sock"),
            client_name: "rustyjack-client".to_string(),
            client_version: env!("CARGO_PKG_VERSION").to_string(),
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            long_request_timeout: LONG_REQUEST_TIMEOUT,
            max_retries: MAX_RETRY_ATTEMPTS,
            retry_delay_ms: INITIAL_RETRY_DELAY.as_millis() as u64,
        }
    }
}

pub struct DaemonClient {
    #[cfg(unix)]
    stream: Option<UnixStream>,
    #[cfg(not(unix))]
    stream: Option<()>,
    next_request_id: AtomicU64,
    info: Option<DaemonClientInfo>,
    config: ClientConfig,
}

impl DaemonClient {
    #[cfg(unix)]
    pub async fn connect<P: AsRef<Path>>(
        path: P,
        client_name: &str,
        client_version: &str,
    ) -> Result<Self> {
        let config = ClientConfig {
            socket_path: path.as_ref().to_path_buf(),
            client_name: client_name.to_string(),
            client_version: client_version.to_string(),
            ..Default::default()
        };
        Self::connect_with_config(config).await
    }

    #[cfg(not(unix))]
    pub async fn connect<P: AsRef<Path>>(
        _path: P,
        _client_name: &str,
        _client_version: &str,
    ) -> Result<Self> {
        bail!("Unix domain sockets not supported on this platform")
    }

    #[cfg(unix)]
    pub async fn connect_with_config(config: ClientConfig) -> Result<Self> {
        let mut client = Self {
            stream: None,
            next_request_id: AtomicU64::new(1),
            info: None,
            config,
        };
        client.reconnect().await?;
        Ok(client)
    }

    #[cfg(not(unix))]
    pub async fn connect_with_config(_config: ClientConfig) -> Result<Self> {
        bail!("Unix domain sockets not supported on this platform")
    }

    pub fn new_disconnected(config: ClientConfig) -> Self {
        Self {
            stream: None,
            next_request_id: AtomicU64::new(1),
            info: None,
            config,
        }
    }

    #[cfg(unix)]
    async fn reconnect(&mut self) -> Result<()> {
        let mut stream = UnixStream::connect(&self.config.socket_path)
            .await
            .with_context(|| format!("connecting to {}", self.config.socket_path.display()))?;
        
        let hello = ClientHello {
            protocol_version: PROTOCOL_VERSION,
            client_name: self.config.client_name.clone(),
            client_version: self.config.client_version.clone(),
            supports: Vec::new(),
        };
        let hello_bytes = serde_json::to_vec(&hello)?;
        write_frame(&mut stream, &hello_bytes, MAX_FRAME).await?;

        let ack_bytes = timeout(HANDSHAKE_TIMEOUT, read_frame(&mut stream, MAX_FRAME))
            .await
            .context("handshake timed out")??;
        let ack: HelloAck = serde_json::from_slice(&ack_bytes)?;
        if ack.protocol_version != PROTOCOL_VERSION {
            bail!(
                "protocol mismatch: client={} daemon={}",
                PROTOCOL_VERSION,
                ack.protocol_version
            );
        }

        let info = DaemonClientInfo {
            daemon_version: ack.daemon_version,
            protocol_version: ack.protocol_version,
            features: ack.features,
            authz: ack.authz,
            max_frame: ack.max_frame,
        };

        self.stream = Some(stream);
        self.info = Some(info);
        Ok(())
    }

    #[cfg(not(unix))]
    async fn reconnect(&mut self) -> Result<()> {
        bail!("Unix domain sockets not supported on this platform")
    }

    pub fn is_connected(&self) -> bool {
        self.stream.is_some()
    }

    pub fn info(&self) -> Option<&DaemonClientInfo> {
        self.info.as_ref()
    }

    pub async fn ensure_connected(&mut self) -> Result<()> {
        if !self.is_connected() {
            self.reconnect().await?;
        }
        Ok(())
    }

    pub async fn request(&mut self, body: RequestBody) -> Result<ResponseBody> {
        self.request_with_timeout(body, self.config.request_timeout)
            .await
    }

    pub async fn request_long(&mut self, body: RequestBody) -> Result<ResponseBody> {
        self.request_with_timeout(body, self.config.long_request_timeout)
            .await
    }

    pub async fn request_with_timeout(
        &mut self,
        body: RequestBody,
        req_timeout: Duration,
    ) -> Result<ResponseBody> {
        let mut attempts = 0;
        let mut last_error = None;

        while attempts < self.config.max_retries {
            if attempts > 0 {
                let delay = Duration::from_millis(
                    self.config.retry_delay_ms * (1u64 << (attempts - 1).min(4)),
                );
                sleep(delay).await;
            }

            match self.try_request(&body, req_timeout).await {
                Ok(response) => return Ok(response),
                Err(err) => {
                    let should_retry = is_retryable_error(&err);
                    last_error = Some(err);
                    
                    if !should_retry {
                        break;
                    }
                    
                    attempts += 1;
                    
                    if attempts < self.config.max_retries {
                        self.stream = None;
                        if let Err(e) = self.reconnect().await {
                            last_error = Some(e);
                        }
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow!("request failed with no error")))
    }

    #[cfg(unix)]
    async fn try_request(
        &mut self,
        body: &RequestBody,
        req_timeout: Duration,
    ) -> Result<ResponseBody> {
        self.ensure_connected().await?;
        
        let stream = self.stream.as_mut().ok_or_else(|| anyhow!("not connected"))?;
        let info = self.info.as_ref().ok_or_else(|| anyhow!("no info"))?;

        let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let envelope = RequestEnvelope {
            v: info.protocol_version,
            request_id,
            endpoint: endpoint_for_body(body),
            body: body.clone(),
        };
        let payload = serde_json::to_vec(&envelope)?;
        write_frame(stream, &payload, info.max_frame).await?;

        let response_bytes = timeout(req_timeout, read_frame(stream, info.max_frame))
            .await
            .context("response timed out")??;
        let response: ResponseEnvelope = serde_json::from_slice(&response_bytes)?;
        if response.request_id != request_id {
            bail!(
                "response request_id mismatch: expected {} got {}",
                request_id,
                response.request_id
            );
        }
        if response.v != info.protocol_version {
            bail!(
                "protocol version mismatch: expected {} got {}",
                info.protocol_version,
                response.v
            );
        }
        Ok(response.body)
    }

    #[cfg(not(unix))]
    async fn try_request(
        &mut self,
        _body: &RequestBody,
        _req_timeout: Duration,
    ) -> Result<ResponseBody> {
        bail!("Unix domain sockets not supported on this platform")
    }

    pub async fn health(&mut self) -> Result<HealthResponse> {
        match self.request(RequestBody::Health).await? {
            ResponseBody::Ok(ResponseOk::Health(resp)) => Ok(resp),
            ResponseBody::Err(err) => Err(daemon_error(err)),
            _ => Err(anyhow!("unexpected response body")),
        }
    }

    pub async fn version(&mut self) -> Result<VersionResponse> {
        match self.request(RequestBody::Version).await? {
            ResponseBody::Ok(ResponseOk::Version(resp)) => Ok(resp),
            ResponseBody::Err(err) => Err(daemon_error(err)),
            _ => Err(anyhow!("unexpected response body")),
        }
    }

    pub async fn status(&mut self) -> Result<StatusResponse> {
        match self.request(RequestBody::Status).await? {
            ResponseBody::Ok(ResponseOk::Status(resp)) => Ok(resp),
            ResponseBody::Err(err) => Err(daemon_error(err)),
            _ => Err(anyhow!("unexpected response body")),
        }
    }

    pub async fn system_status(&mut self) -> Result<SystemStatusResponse> {
        match self.request(RequestBody::SystemStatusGet).await? {
            ResponseBody::Ok(ResponseOk::SystemStatus(resp)) => Ok(resp),
            ResponseBody::Err(err) => Err(daemon_error(err)),
            _ => Err(anyhow!("unexpected response body")),
        }
    }

    pub async fn disk_usage(&mut self, path: &str) -> Result<DiskUsageResponse> {
        let body = RequestBody::DiskUsageGet(DiskUsageRequest {
            path: path.to_string(),
        });
        match self.request(body).await? {
            ResponseBody::Ok(ResponseOk::DiskUsage(resp)) => Ok(resp),
            ResponseBody::Err(err) => Err(daemon_error(err)),
            _ => Err(anyhow!("unexpected response body")),
        }
    }

    pub async fn system_reboot(&mut self) -> Result<SystemActionResponse> {
        match self.request(RequestBody::SystemReboot).await? {
            ResponseBody::Ok(ResponseOk::SystemAction(resp)) => Ok(resp),
            ResponseBody::Err(err) => Err(daemon_error(err)),
            _ => Err(anyhow!("unexpected response body")),
        }
    }

    pub async fn system_shutdown(&mut self) -> Result<SystemActionResponse> {
        match self.request(RequestBody::SystemShutdown).await? {
            ResponseBody::Ok(ResponseOk::SystemAction(resp)) => Ok(resp),
            ResponseBody::Err(err) => Err(daemon_error(err)),
            _ => Err(anyhow!("unexpected response body")),
        }
    }

    pub async fn system_sync(&mut self) -> Result<SystemActionResponse> {
        match self.request(RequestBody::SystemSync).await? {
            ResponseBody::Ok(ResponseOk::SystemAction(resp)) => Ok(resp),
            ResponseBody::Err(err) => Err(daemon_error(err)),
            _ => Err(anyhow!("unexpected response body")),
        }
    }

    pub async fn hostname_randomize_now(&mut self) -> Result<HostnameResponse> {
        match self.request(RequestBody::HostnameRandomizeNow).await? {
            ResponseBody::Ok(ResponseOk::Hostname(resp)) => Ok(resp),
            ResponseBody::Err(err) => Err(daemon_error(err)),
            _ => Err(anyhow!("unexpected response body")),
        }
    }

    pub async fn block_devices(&mut self) -> Result<BlockDevicesResponse> {
        match self.request(RequestBody::BlockDevicesList).await? {
            ResponseBody::Ok(ResponseOk::BlockDevices(resp)) => Ok(resp),
            ResponseBody::Err(err) => Err(daemon_error(err)),
            _ => Err(anyhow!("unexpected response body")),
        }
    }

    pub async fn system_logs(&mut self) -> Result<SystemLogsResponse> {
        match self.request(RequestBody::SystemLogsGet).await? {
            ResponseBody::Ok(ResponseOk::SystemLogs(resp)) => Ok(resp),
            ResponseBody::Err(err) => Err(daemon_error(err)),
            _ => Err(anyhow!("unexpected response body")),
        }
    }

    pub async fn wifi_capabilities(
        &mut self,
        interface: &str,
    ) -> Result<WifiCapabilitiesResponse> {
        let body = RequestBody::WifiCapabilitiesGet(WifiCapabilitiesRequest {
            interface: interface.to_string(),
        });
        match self.request(body).await? {
            ResponseBody::Ok(ResponseOk::WifiCapabilities(resp)) => Ok(resp),
            ResponseBody::Err(err) => Err(daemon_error(err)),
            _ => Err(anyhow!("unexpected response body")),
        }
    }

    pub async fn hotspot_warnings(&mut self) -> Result<HotspotWarningsResponse> {
        match self.request(RequestBody::HotspotWarningsGet).await? {
            ResponseBody::Ok(ResponseOk::HotspotWarnings(resp)) => Ok(resp),
            ResponseBody::Err(err) => Err(daemon_error(err)),
            _ => Err(anyhow!("unexpected response body")),
        }
    }

    pub async fn hotspot_diagnostics(
        &mut self,
        ap_interface: &str,
    ) -> Result<HotspotDiagnosticsResponse> {
        let body = RequestBody::HotspotDiagnosticsGet(HotspotDiagnosticsRequest {
            ap_interface: ap_interface.to_string(),
        });
        match self.request(body).await? {
            ResponseBody::Ok(ResponseOk::HotspotDiagnostics(resp)) => Ok(resp),
            ResponseBody::Err(err) => Err(daemon_error(err)),
            _ => Err(anyhow!("unexpected response body")),
        }
    }

    pub async fn hotspot_clients(&mut self) -> Result<HotspotClientsResponse> {
        match self.request(RequestBody::HotspotClientsList).await? {
            ResponseBody::Ok(ResponseOk::HotspotClients(resp)) => Ok(resp),
            ResponseBody::Err(err) => Err(daemon_error(err)),
            _ => Err(anyhow!("unexpected response body")),
        }
    }

    pub async fn gpio_diagnostics(&mut self) -> Result<GpioDiagnosticsResponse> {
        match self.request(RequestBody::GpioDiagnosticsGet).await? {
            ResponseBody::Ok(ResponseOk::GpioDiagnostics(resp)) => Ok(resp),
            ResponseBody::Err(err) => Err(daemon_error(err)),
            _ => Err(anyhow!("unexpected response body")),
        }
    }

    pub async fn job_start(&mut self, kind: JobKind) -> Result<JobStarted> {
        let body = RequestBody::JobStart(JobStartRequest {
            job: JobSpec {
                kind,
                requested_by: None,
            },
        });
        match self.request(body).await? {
            ResponseBody::Ok(ResponseOk::JobStarted(resp)) => Ok(resp),
            ResponseBody::Err(err) => Err(daemon_error(err)),
            _ => Err(anyhow!("unexpected response body")),
        }
    }

    pub async fn job_status(&mut self, job_id: u64) -> Result<JobStatusResponse> {
        let body = RequestBody::JobStatus(JobStatusRequest { job_id });
        match self.request(body).await? {
            ResponseBody::Ok(ResponseOk::JobStatus(resp)) => Ok(resp),
            ResponseBody::Err(err) => Err(daemon_error(err)),
            _ => Err(anyhow!("unexpected response body")),
        }
    }

    pub async fn job_cancel(&mut self, job_id: u64) -> Result<JobCancelResponse> {
        let body = RequestBody::JobCancel(JobCancelRequest { job_id });
        match self.request(body).await? {
            ResponseBody::Ok(ResponseOk::JobCancelled(resp)) => Ok(resp),
            ResponseBody::Err(err) => Err(daemon_error(err)),
            _ => Err(anyhow!("unexpected response body")),
        }
    }

    pub async fn wifi_interfaces(&mut self) -> Result<rustyjack_ipc::WifiInterfacesResponse> {
        match self.request(RequestBody::WifiInterfacesList).await? {
            ResponseBody::Ok(ResponseOk::WifiInterfaces(resp)) => Ok(resp),
            ResponseBody::Err(err) => Err(daemon_error(err)),
            _ => Err(anyhow!("unexpected response body")),
        }
    }

    pub async fn wifi_disconnect(&mut self, interface: &str) -> Result<rustyjack_ipc::WifiDisconnectResponse> {
        let body = RequestBody::WifiDisconnect(rustyjack_ipc::WifiDisconnectRequest {
            interface: interface.to_string(),
        });
        match self.request(body).await? {
            ResponseBody::Ok(ResponseOk::WifiDisconnect(resp)) => Ok(resp),
            ResponseBody::Err(err) => Err(daemon_error(err)),
            _ => Err(anyhow!("unexpected response body")),
        }
    }

    pub async fn wifi_scan_start(&mut self, interface: &str, timeout_ms: u64) -> Result<JobStarted> {
        let body = RequestBody::WifiScanStart(rustyjack_ipc::WifiScanStartRequest {
            interface: interface.to_string(),
            timeout_ms,
        });
        match self.request(body).await? {
            ResponseBody::Ok(ResponseOk::JobStarted(resp)) => Ok(resp),
            ResponseBody::Err(err) => Err(daemon_error(err)),
            _ => Err(anyhow!("unexpected response body")),
        }
    }

    pub async fn wifi_connect_start(&mut self, interface: &str, ssid: &str, psk: Option<String>, timeout_ms: u64) -> Result<JobStarted> {
        let body = RequestBody::WifiConnectStart(rustyjack_ipc::WifiConnectStartRequest {
            interface: interface.to_string(),
            ssid: ssid.to_string(),
            psk,
            timeout_ms,
        });
        match self.request(body).await? {
            ResponseBody::Ok(ResponseOk::JobStarted(resp)) => Ok(resp),
            ResponseBody::Err(err) => Err(daemon_error(err)),
            _ => Err(anyhow!("unexpected response body")),
        }
    }

    pub async fn hotspot_start(&mut self, interface: &str, ssid: &str, passphrase: Option<String>, channel: Option<u8>) -> Result<JobStarted> {
        let body = RequestBody::HotspotStart(rustyjack_ipc::HotspotStartRequest {
            interface: interface.to_string(),
            ssid: ssid.to_string(),
            passphrase,
            channel,
        });
        match self.request(body).await? {
            ResponseBody::Ok(ResponseOk::JobStarted(resp)) => Ok(resp),
            ResponseBody::Err(err) => Err(daemon_error(err)),
            _ => Err(anyhow!("unexpected response body")),
        }
    }

    pub async fn hotspot_stop(&mut self) -> Result<rustyjack_ipc::HotspotActionResponse> {
        match self.request(RequestBody::HotspotStop).await? {
            ResponseBody::Ok(ResponseOk::HotspotAction(resp)) => Ok(resp),
            ResponseBody::Err(err) => Err(daemon_error(err)),
            _ => Err(anyhow!("unexpected response body")),
        }
    }

    pub async fn portal_start(&mut self, interface: &str, port: u16) -> Result<JobStarted> {
        let body = RequestBody::PortalStart(rustyjack_ipc::PortalStartRequest {
            interface: interface.to_string(),
            port,
        });
        match self.request(body).await? {
            ResponseBody::Ok(ResponseOk::JobStarted(resp)) => Ok(resp),
            ResponseBody::Err(err) => Err(daemon_error(err)),
            _ => Err(anyhow!("unexpected response body")),
        }
    }

    pub async fn portal_stop(&mut self) -> Result<rustyjack_ipc::PortalActionResponse> {
        match self.request(RequestBody::PortalStop).await? {
            ResponseBody::Ok(ResponseOk::PortalAction(resp)) => Ok(resp),
            ResponseBody::Err(err) => Err(daemon_error(err)),
            _ => Err(anyhow!("unexpected response body")),
        }
    }

    pub async fn portal_status(&mut self) -> Result<rustyjack_ipc::PortalStatusResponse> {
        match self.request(RequestBody::PortalStatus).await? {
            ResponseBody::Ok(ResponseOk::PortalStatus(resp)) => Ok(resp),
            ResponseBody::Err(err) => Err(daemon_error(err)),
            _ => Err(anyhow!("unexpected response body")),
        }
    }

    pub async fn mount_list(&mut self) -> Result<rustyjack_ipc::MountListResponse> {
        match self.request(RequestBody::MountList).await? {
            ResponseBody::Ok(ResponseOk::MountList(resp)) => Ok(resp),
            ResponseBody::Err(err) => Err(daemon_error(err)),
            _ => Err(anyhow!("unexpected response body")),
        }
    }

    pub async fn mount_start(&mut self, device: &str, filesystem: Option<String>) -> Result<JobStarted> {
        let body = RequestBody::MountStart(rustyjack_ipc::MountStartRequest {
            device: device.to_string(),
            filesystem,
        });
        match self.request(body).await? {
            ResponseBody::Ok(ResponseOk::JobStarted(resp)) => Ok(resp),
            ResponseBody::Err(err) => Err(daemon_error(err)),
            _ => Err(anyhow!("unexpected response body")),
        }
    }

    pub async fn unmount_start(&mut self, device: &str) -> Result<JobStarted> {
        let body = RequestBody::UnmountStart(rustyjack_ipc::UnmountStartRequest {
            device: device.to_string(),
        });
        match self.request(body).await? {
            ResponseBody::Ok(ResponseOk::JobStarted(resp)) => Ok(resp),
            ResponseBody::Err(err) => Err(daemon_error(err)),
            _ => Err(anyhow!("unexpected response body")),
        }
    }

    pub async fn core_dispatch(&mut self, legacy: rustyjack_ipc::LegacyCommand, args: Value) -> Result<CoreDispatchResponse> {
        let body = RequestBody::CoreDispatch(CoreDispatchRequest { legacy, args });
        match self.request(body).await? {
            ResponseBody::Ok(ResponseOk::CoreDispatch(resp)) => Ok(resp),
            ResponseBody::Err(err) => Err(daemon_error(err)),
            _ => Err(anyhow!("unexpected response body")),
        }
    }
}

fn daemon_error(err: DaemonError) -> anyhow::Error {
    let mut message = format!("{}", err.message);
    if let Some(detail) = err.detail {
        message.push_str(": ");
        message.push_str(&detail);
    }
    if err.retryable {
        message.push_str(" (retryable)");
    }
    anyhow!(message).context(match err.code {
        ErrorCode::Unauthorized => "unauthorized",
        ErrorCode::Forbidden => "forbidden",
        ErrorCode::NotFound => "not found",
        ErrorCode::Busy => "busy",
        ErrorCode::Timeout => "timeout",
        ErrorCode::Cancelled => "cancelled",
        ErrorCode::BadRequest => "bad request",
        ErrorCode::IncompatibleProtocol => "protocol",
        ErrorCode::Io => "io",
        ErrorCode::Netlink => "netlink",
        ErrorCode::MountFailed => "mount",
        ErrorCode::WifiFailed => "wifi",
        ErrorCode::UpdateFailed => "update",
        ErrorCode::CleanupFailed => "cleanup",
        ErrorCode::NotImplemented => "not implemented",
        ErrorCode::Internal => "internal",
    })
}

fn is_retryable_error(err: &anyhow::Error) -> bool {
    if let Some(io_err) = err.downcast_ref::<std::io::Error>() {
        matches!(
            io_err.kind(),
            std::io::ErrorKind::ConnectionRefused
                | std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::ConnectionAborted
                | std::io::ErrorKind::BrokenPipe
                | std::io::ErrorKind::TimedOut
                | std::io::ErrorKind::Interrupted
        )
    } else {
        err.to_string().contains("retryable")
            || err.to_string().contains("timed out")
            || err.to_string().contains("connection")
    }
}

#[cfg(unix)]
async fn read_frame(stream: &mut UnixStream, max_frame: u32) -> Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = rustyjack_ipc::decode_frame_length(len_buf, max_frame)
        .map_err(|err| anyhow!("invalid frame length: {:?}", err))?;
    let mut buf = vec![0u8; len as usize];
    stream.read_exact(&mut buf).await?;
    Ok(buf)
}

#[cfg(unix)]
async fn write_frame(stream: &mut UnixStream, payload: &[u8], max_frame: u32) -> Result<()> {
    if payload.is_empty() {
        bail!("empty payload");
    }
    if payload.len() as u32 > max_frame {
        bail!("payload exceeds max_frame");
    }
    let frame = rustyjack_ipc::encode_frame(payload);
    stream.write_all(&frame).await?;
    Ok(())
}
