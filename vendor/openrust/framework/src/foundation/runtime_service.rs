use std::{error::Error, fmt, io, sync::Arc};

use futures_util::stream::{FuturesUnordered, StreamExt as _};
use tokio::signal;
use tokio_util::sync::CancellationToken;

use crate::{
    config::{ConfigError, Value},
    contracts::{config::Repository, console::Kernel as _},
    http::HttpFuture,
    queue::QueueManager,
};

use super::{Application, console};

pub type RuntimeResult<T> = Result<T, Box<dyn Error + Send + Sync>>;
pub type RunningService = HttpFuture<'static, RuntimeResult<()>>;

/// A service managed by the application's runtime.
pub trait RuntimeService: Send + Sync {
    /// Bind resources before any listener reports readiness.
    fn prepare(&self, shutdown: CancellationToken)
    -> HttpFuture<'_, RuntimeResult<RunningService>>;
}

#[derive(Debug)]
pub(super) struct ServiceExit {
    pub(super) name: &'static str,
    pub(super) status: i32,
}

impl fmt::Display for ServiceExit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} exited with status {}", self.name, self.status)
    }
}

impl Error for ServiceExit {}

pub(super) fn serves_http(config: &dyn Repository) -> Result<bool, ConfigError> {
    Ok(!enabled(config, "runtime.enabled", false)? || enabled(config, "runtime.http", true)?)
}

fn enabled(config: &dyn Repository, key: &str, default: bool) -> Result<bool, ConfigError> {
    match config.get(key) {
        None => Ok(default),
        Some(Value::Bool(value)) => Ok(value),
        _ => Err(ConfigError::InvalidType {
            key: key.into(),
            expected: "a boolean",
        }),
    }
}

pub(super) async fn configured_services(
    app: &Application,
) -> RuntimeResult<Vec<Arc<dyn RuntimeService>>> {
    let config = app.configuration_repository()?;
    let mut managed = Vec::new();
    if enabled(config.as_ref(), "runtime.enabled", false)? {
        let queue = enabled(config.as_ref(), "runtime.queue", true)?
            && !app.make::<QueueManager>().await?.default_is_synchronous()?;
        let schedule = enabled(config.as_ref(), "runtime.schedule", true)?;
        if queue || schedule {
            let mut kernel = console::Kernel::for_artisan(app.share());
            kernel.bootstrap().await?;
            managed = kernel.managed_services(queue, schedule);
        }
    }
    let mut services = app.take_runtime_services();
    services.append(&mut managed);
    Ok(services)
}

pub(super) async fn prepare(
    services: Vec<Arc<dyn RuntimeService>>,
    shutdown: &CancellationToken,
) -> RuntimeResult<FuturesUnordered<RunningService>> {
    let running = FuturesUnordered::new();
    for service in services {
        let run = service.prepare(shutdown.clone()).await?;
        running.push(run);
    }
    Ok(running)
}

pub(super) async fn run(
    services: Vec<Arc<dyn RuntimeService>>,
    http: impl FnOnce(CancellationToken) -> Option<RunningService>,
) -> RuntimeResult<()> {
    let shutdown = CancellationToken::new();
    let mut running = prepare(services, &shutdown).await?;
    if let Some(http) = http(shutdown.clone()) {
        running.push(http);
    }
    if running.is_empty() {
        return Err(io::Error::other("No application services are enabled.").into());
    }
    supervise(&shutdown, &mut running, wait_for_shutdown()).await
}

pub(super) async fn supervise(
    shutdown: &CancellationToken,
    running: &mut FuturesUnordered<RunningService>,
    stop: impl std::future::Future<Output = io::Result<()>>,
) -> RuntimeResult<()> {
    let mut result = tokio::select! {
        biased;
        result = stop => result.map_err(Into::into),
        result = running.next() => result.unwrap_or(Ok(())),
    };
    shutdown.cancel();
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
