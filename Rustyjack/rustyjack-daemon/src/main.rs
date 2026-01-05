use anyhow::Result;
use std::sync::Arc;
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::Notify;
use tracing::{info, warn};

mod auth;
mod config;
mod dispatch;
mod jobs;
mod locks;
mod netlink_watcher;
mod server;
mod state;
mod systemd;
mod telemetry;
mod validation;

use config::DaemonConfig;
use state::DaemonState;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    init_tracing();

    let config = DaemonConfig::from_env();
    let state = Arc::new(DaemonState::new(config.clone()));
    let listener = systemd::listener_or_bind(&config)?;

    state.reconcile_on_startup().await;
    systemd::notify_ready();
    systemd::spawn_watchdog_task();

    let shutdown = Arc::new(Notify::new());
    
    let watcher_state = Arc::clone(&state);
    let watcher_shutdown = Arc::clone(&shutdown);
    tokio::spawn(async move {
        tokio::select! {
            result = netlink_watcher::run_netlink_watcher(watcher_state) => {
                if let Err(e) = result {
                    warn!("Netlink watcher stopped with error: {}", e);
                }
            }
            _ = watcher_shutdown.notified() => {
                info!("Netlink watcher stopped by shutdown signal");
            }
        }
    });

    let shutdown_signal = Arc::clone(&shutdown);
    tokio::spawn(async move {
        let mut sigterm = match signal(SignalKind::terminate()) {
            Ok(signal) => signal,
            Err(err) => {
                warn!("Failed to register SIGTERM handler: {}", err);
                return;
            }
        };
        let mut sigint = match signal(SignalKind::interrupt()) {
            Ok(signal) => signal,
            Err(err) => {
                warn!("Failed to register SIGINT handler: {}", err);
                return;
            }
        };

        tokio::select! {
            _ = sigterm.recv() => {},
            _ = sigint.recv() => {},
        }

        shutdown_signal.notify_waiters();
    });

    info!("rustyjackd ready");
    server::run(listener, Arc::clone(&state), Arc::clone(&shutdown)).await;

    state.jobs.cancel_all().await;
    info!("rustyjackd stopped");
    Ok(())
}

fn init_tracing() {
    use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
    
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("info"))
        .unwrap();
    
    let fmt_layer = fmt::layer()
        .with_target(true)
        .with_level(true)
        .with_thread_ids(true)
        .with_line_number(true)
        .compact();
    
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .init();
}
