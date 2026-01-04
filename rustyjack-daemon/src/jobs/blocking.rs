use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use rustyjack_ipc::{DaemonError, ErrorCode};

/// Run a blocking operation with cancellation support.
///
/// When the cancellation token fires:
/// 1. Aborts the spawn_blocking task
/// 2. Returns Cancelled error immediately
///
/// Note: This does NOT kill child processes spawned by the blocking work.
/// For operations that spawn subprocesses, use cancellable subprocess helpers instead.
pub async fn run_blocking_cancellable<F, T>(
    cancel: &CancellationToken,
    f: F,
) -> Result<T, DaemonError>
where
    F: FnOnce() -> Result<T, DaemonError> + Send + 'static,
    T: Send + 'static,
{
    let mut handle: JoinHandle<Result<T, DaemonError>> = tokio::task::spawn_blocking(f);

    tokio::select! {
        _ = cancel.cancelled() => {
            handle.abort();
            Err(DaemonError::new(
                ErrorCode::Cancelled,
                "operation cancelled",
                false,
            ))
        }
        result = &mut handle => {
            match result {
                Ok(inner) => inner,
                Err(err) => Err(
                    DaemonError::new(
                        ErrorCode::Internal,
                        "blocking task panicked",
                        false,
                    )
                    .with_detail(err.to_string())
                ),
            }
        }
    }
}

/// Run a blocking operation with progress reporting and cancellation support.
///
/// Progress messages are sent via the provided channel, allowing the caller
/// to update job progress while the blocking work executes.
pub async fn run_blocking_cancellable_with_progress<F, T>(
    cancel: &CancellationToken,
    f: F,
    mut on_progress: impl FnMut(u8, String) + Send,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<(u8, String)>,
) -> Result<T, DaemonError>
where
    F: FnOnce() -> Result<T, DaemonError> + Send + 'static,
    T: Send + 'static,
{
    let mut handle: JoinHandle<Result<T, DaemonError>> = tokio::task::spawn_blocking(f);

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                handle.abort();
                return Err(DaemonError::new(
                    ErrorCode::Cancelled,
                    "operation cancelled",
                    false,
                ));
            }
            result = &mut handle => {
                return match result {
                    Ok(inner) => inner,
                    Err(err) => Err(
                        DaemonError::new(
                            ErrorCode::Internal,
                            "blocking task panicked",
                            false,
                        )
                        .with_detail(err.to_string())
                    ),
                };
            }
            Some((percent, message)) = rx.recv() => {
                on_progress(percent, message);
            }
        }
    }
}
