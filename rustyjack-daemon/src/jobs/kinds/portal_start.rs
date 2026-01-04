use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use rustyjack_ipc::{DaemonError, ErrorCode, PortalStartRequestIpc};

pub async fn run<F, Fut>(
    req: PortalStartRequestIpc,
    cancel: &CancellationToken,
    progress: &mut F,
) -> Result<serde_json::Value, DaemonError>
where
    F: FnMut(&str, u8, &str) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    if cancel.is_cancelled() {
        return Err(DaemonError::new(ErrorCode::Cancelled, "Job cancelled", false));
    }

    // Check if we should use external portal process
    if std::env::var("RUSTYJACK_PORTAL_MODE").as_deref() == Ok("external") {
        run_external_portal(req, cancel, progress).await
    } else {
        // Fallback to embedded portal
        run_embedded_portal(req, cancel, progress).await
    }
}

async fn run_external_portal<F, Fut>(
    req: PortalStartRequestIpc,
    cancel: &CancellationToken,
    _progress: &mut F,
) -> Result<serde_json::Value, DaemonError>
where
    F: FnMut(&str, u8, &str) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let portal_bin = std::env::var("RUSTYJACK_PORTAL_BIN")
        .unwrap_or_else(|_| "/usr/local/bin/rustyjack-portal".to_string());

    // Check if portal binary exists
    if !std::path::Path::new(&portal_bin).exists() {
        return Err(DaemonError::new(
            ErrorCode::NotFound,
            "Portal binary not found",
            false,
        )
        .with_detail(format!("Expected at: {}", portal_bin))
        .with_source("daemon.jobs.portal_start"));
    }

    let bind_ip = "192.168.4.1"; // TODO: Derive from interface or config
    
    let child_handle: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));
    let child_handle_clone = child_handle.clone();

    // Spawn kill task for cancellation
    let cancel_clone = cancel.clone();
    let kill_task = tokio::spawn(async move {
        cancel_clone.cancelled().await;
        if let Some(mut child) = child_handle_clone.lock().unwrap().take() {
            log::info!("Killing portal process due to cancellation");
            let _ = child.kill();
        }
    });

    // Spawn portal process
    let result = tokio::task::spawn_blocking(move || {
        let mut cmd = Command::new(&portal_bin);
        cmd.env("RUSTYJACK_PORTAL_INTERFACE", &req.interface)
            .env("RUSTYJACK_PORTAL_BIND", bind_ip)
            .env("RUSTYJACK_PORTAL_PORT", req.port.to_string())
            .env("RUSTYJACK_PORTAL_SITE_DIR", "/var/lib/rustyjack/portal/site")
            .env("RUSTYJACK_PORTAL_CAPTURE_DIR", "/var/lib/rustyjack/loot/Portal")
            .env("RUST_LOG", "info")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        log::info!("Spawning portal process: {}", portal_bin);
        let mut child = cmd.spawn().map_err(|e| {
            rustyjack_core::services::error::ServiceError::External(format!(
                "Failed to spawn portal process: {}",
                e
            ))
        })?;

        // Store child handle
        *child_handle.lock().unwrap() = Some(child);

        // Wait a moment for portal to start
        std::thread::sleep(std::time::Duration::from_millis(500));

        // Check if still running
        if let Some(child_ref) = child_handle.lock().unwrap().as_mut() {
            match child_ref.try_wait() {
                Ok(Some(status)) => {
                    return Err(rustyjack_core::services::error::ServiceError::External(
                        format!("Portal process exited early with status: {}", status),
                    ));
                }
                Ok(None) => {
                    // Still running, good
                    log::info!("Portal process started successfully (PID: {})", child_ref.id());
                }
                Err(e) => {
                    return Err(rustyjack_core::services::error::ServiceError::External(
                        format!("Failed to check portal status: {}", e),
                    ));
                }
            }
        }

        // Return success with process info
        Ok(serde_json::json!({
            "status": "started",
            "mode": "external",
            "interface": req.interface,
            "port": req.port,
            "bind_ip": bind_ip,
        }))
    })
    .await;

    kill_task.abort();

    match result {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(err)) => Err(err.to_daemon_error_with_source("daemon.jobs.portal_start")),
        Err(err) => Err(DaemonError::new(
            ErrorCode::Internal,
            "portal start task panicked",
            false,
        )
        .with_detail(err.to_string())
        .with_source("daemon.jobs.portal_start")),
    }
}

async fn run_embedded_portal<F, Fut>(
    req: PortalStartRequestIpc,
    cancel: &CancellationToken,
    progress: &mut F,
) -> Result<serde_json::Value, DaemonError>
where
    F: FnMut(&str, u8, &str) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let request = rustyjack_core::services::portal::PortalStartRequest {
        interface: req.interface,
        port: req.port,
    };

    let (tx, mut rx) = mpsc::unbounded_channel::<(u8, String)>();
    let mut handle = tokio::task::spawn_blocking(move || {
        rustyjack_core::services::portal::start(request, |percent, message| {
            let _ = tx.send((percent, message.to_string()));
        })
    });

    let result = loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                handle.abort();
                let _ = tokio::task::spawn_blocking(|| {
                    let _ = rustyjack_core::services::portal::stop();
                }).await;
                return Err(DaemonError::new(
                    ErrorCode::Cancelled,
                    "Job cancelled",
                    false
                ).with_source("daemon.jobs.portal_start"));
            }
            res = &mut handle => {
                break res;
            }
            Some((percent, message)) = rx.recv() => {
                progress("portal_start", percent, &message).await;
            }
        }
    };

    match result {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(err)) => Err(err.to_daemon_error_with_source("daemon.jobs.portal_start")),
        Err(err) => Err(
            DaemonError::new(ErrorCode::Internal, "portal start job panicked", false)
                .with_detail(err.to_string())
                .with_source("daemon.jobs.portal_start"),
        ),
    }
}
