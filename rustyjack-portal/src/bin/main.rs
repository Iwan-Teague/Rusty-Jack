use std::env;
use std::path::PathBuf;

use anyhow::{Context, Result};
use tokio::signal;

use rustyjack_portal::{build_router, run_server, PortalConfig, PortalLogger, PortalState};

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    log::info!("Rustyjack Portal starting");
    log::info!("Version: {}", env!("CARGO_PKG_VERSION"));

    let config = load_config()?;
    log::info!("Portal configuration loaded");
    log::info!("  Interface: {}", config.interface);
    log::info!("  Bind: {}:{}", config.listen_ip, config.listen_port);
    log::info!("  Site dir: {}", config.site_dir.display());
    log::info!("  Capture dir: {}", config.capture_dir.display());

    let index_path = config.site_dir.join("index.html");
    let index_html = std::fs::read_to_string(&index_path)
        .with_context(|| format!("reading portal HTML from {}", index_path.display()))?;

    let logger = PortalLogger::new(&config.capture_dir)?;
    let state = PortalState::new(logger, index_html);
    
    let router = build_router(&config, state);
    
    let addr = std::net::SocketAddr::new(config.listen_ip.into(), config.listen_port);
    let listener = std::net::TcpListener::bind(addr)
        .with_context(|| format!("binding portal listener to {}", addr))?;
    
    log::info!("Portal server listening on {}", addr);
    
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    
    let server_task = tokio::spawn(async move {
        if let Err(e) = run_server(listener, router, shutdown_rx).await {
            log::error!("Portal server error: {}", e);
        }
    });
    
    // Wait for shutdown signal
    tokio::select! {
        _ = signal::ctrl_c() => {
            log::info!("Received SIGINT, shutting down...");
        }
        _ = async {
            #[cfg(unix)]
            {
                use tokio::signal::unix::{signal, SignalKind};
                let mut term = signal(SignalKind::terminate()).expect("failed to setup SIGTERM handler");
                term.recv().await;
            }
            #[cfg(not(unix))]
            {
                futures::future::pending::<()>().await;
            }
        } => {
            log::info!("Received SIGTERM, shutting down...");
        }
    }
    
    let _ = shutdown_tx.send(());
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), server_task).await;
    
    log::info!("Portal shutdown complete");
    Ok(())
}

fn load_config() -> Result<PortalConfig> {
    let interface = env::var("RUSTYJACK_PORTAL_INTERFACE")
        .unwrap_or_else(|_| "wlan0".to_string());
    
    let listen_ip = env::var("RUSTYJACK_PORTAL_BIND")
        .unwrap_or_else(|_| "192.168.4.1".to_string())
        .parse()
        .context("invalid RUSTYJACK_PORTAL_BIND")?;
    
    let listen_port = env::var("RUSTYJACK_PORTAL_PORT")
        .unwrap_or_else(|_| "3000".to_string())
        .parse()
        .context("invalid RUSTYJACK_PORTAL_PORT")?;
    
    let site_dir = env::var("RUSTYJACK_PORTAL_SITE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/var/lib/rustyjack/portal/site"));
    
    let capture_dir = env::var("RUSTYJACK_PORTAL_CAPTURE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/var/lib/rustyjack/loot/Portal"));
    
    let dnat_mode = false;
    let bind_to_device = true;
    let request_timeout = std::time::Duration::from_secs(30);
    let max_body_bytes = 4096;
    let max_concurrency = 32;

    Ok(PortalConfig {
        interface,
        listen_ip,
        listen_port,
        site_dir,
        capture_dir,
        dnat_mode,
        bind_to_device,
        request_timeout,
        max_body_bytes,
        max_concurrency,
    })
}
