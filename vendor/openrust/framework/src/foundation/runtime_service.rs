use std::{error::Error, io, sync::Arc};

use futures_util::stream::{FuturesUnordered, StreamExt as _};
use tokio::signal;
use tokio_util::sync::CancellationToken;

use crate::http::HttpFuture;

pub type RuntimeResult<T> = Result<T, Box<dyn Error + Send + Sync>>;
pub type RunningService = HttpFuture<'static, RuntimeResult<()>>;

/// An optional listener managed by the application's HTTP process.
pub trait RuntimeService: Send + Sync {
    /// Bind resources before any listener reports readiness.
    fn prepare(&self, shutdown: CancellationToken)
    -> HttpFuture<'_, RuntimeResult<RunningService>>;
}

pub(super) async fn run(
    services: Vec<Arc<dyn RuntimeService>>,
    http: impl FnOnce(CancellationToken) -> RunningService,
) -> RuntimeResult<()> {
    let shutdown = CancellationToken::new();
    let mut running = FuturesUnordered::new();
    for service in services {
        running.push(service.prepare(shutdown.clone()).await?);
    }

    running.push(http(shutdown.clone()));
    let first = tokio::select! {
        biased;
        result = wait_for_shutdown() => result.map_err(Into::into),
        result = running.next() => result.unwrap_or_else(|| Err(io::Error::other("No application services are running.").into())),
    };
    shutdown.cancel();

    let mut result = first;
    while let Some(service) = running.next().await {
        if result.is_ok() {
            result = service;
        }
    }
    result
}

/// Waits for the process termination signals used by framework listeners.
pub async fn wait_for_shutdown() -> io::Result<()> {
    #[cfg(unix)]
    let mut terminate = signal::unix::signal(signal::unix::SignalKind::terminate())?;
    #[cfg(unix)]
    let result = tokio::select! {
        result = signal::ctrl_c() => result,
        _ = terminate.recv() => Ok(()),
    };
    #[cfg(not(unix))]
    let result = signal::ctrl_c().await;
    result
}
