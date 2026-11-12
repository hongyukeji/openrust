use std::{
    collections::{BTreeMap, HashSet},
    error::Error,
    fmt, io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use futures_util::future::join_all;
use tokio::signal;
use tokio_util::task::AbortOnDropHandle;

use crate::http::HttpFuture;

use super::{Application, runner_pattern::RunnerFilter};

pub use tokio_util::sync::CancellationToken as RunnerContext;

pub type RuntimeResult<T> = Result<T, Box<dyn Error + Send + Sync>>;
pub type RunningService = HttpFuture<'static, RuntimeResult<()>>;

/// A long-running application service. A successful return from run can also
/// mean that the runner started its own background work.
#[allow(non_snake_case, reason = "Matches the Goravel Runner contract")]
pub trait Runner: Send + Sync + 'static {
    fn signature(&self) -> &str;

    fn shouldRun(&self) -> HttpFuture<'_, RuntimeResult<bool>> {
        Box::pin(async { Ok(true) })
    }

    fn run(&self, context: RunnerContext) -> HttpFuture<'_, RuntimeResult<()>>;

    fn shutdown(&self, _context: RunnerContext) -> HttpFuture<'_, RuntimeResult<()>> {
        Box::pin(async { Ok(()) })
    }

    fn shutdownPriority(&self) -> i32 {
        0
    }

    /// Allows transports to reserve resources before any runner reports readiness.
    #[doc(hidden)]
    fn prepare(
        &self,
        _context: RunnerContext,
    ) -> HttpFuture<'_, RuntimeResult<Option<RunningService>>> {
        Box::pin(async { Ok(None) })
    }
}

pub(super) const HTTP: &str = "http";

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

#[derive(Debug)]
struct RunnerErrors(Vec<(String, Box<dyn Error + Send + Sync>)>);

impl fmt::Display for RunnerErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, (signature, error)) in self.0.iter().enumerate() {
            if index != 0 {
                f.write_str("\n")?;
            }
            write!(f, "Runner '{signature}' failed: {error}")?;
        }
        Ok(())
    }
}
impl Error for RunnerErrors {}

pub(super) fn exit_status(error: &(dyn Error + 'static)) -> Option<u8> {
    if let Some(exit) = error.downcast_ref::<ServiceExit>() {
        return u8::try_from(exit.status).ok();
    }
    if let Some(errors) = error.downcast_ref::<RunnerErrors>() {
        return errors
            .0
            .iter()
            .find_map(|(_, error)| exit_status(error.as_ref()));
    }
    None
}

pub(super) async fn configured(
    app: &Application,
    http: Option<Arc<dyn Runner>>,
    filter: &RunnerFilter,
) -> RuntimeResult<Vec<Arc<dyn Runner>>> {
    let mut seen = HashSet::new();
    let mut eligible = Vec::new();
    if http.is_none() {
        seen.insert(HTTP.to_owned());
    }
    let providers = app.provider_runners().await;
    let custom = app
        .take_custom_runners()
        .into_iter()
        .flat_map(|register| register());
    for runner in http.into_iter().chain(providers).chain(custom) {
        let signature = runner.signature();
        if seen.insert(signature.to_owned())
            && !filter.disables(signature)
            && runner
                .shouldRun()
                .await
                .map_err(|error| RunnerErrors(vec![(signature.to_owned(), error)]))?
        {
            eligible.push(runner);
        }
    }
    Ok(eligible)
}

pub(super) async fn run_until(
    runners: Vec<Arc<dyn Runner>>,
    signal: impl Future<Output = io::Result<()>>,
) -> RuntimeResult<()> {
    let mut active = RunnerSet::default();
    active.start(runners).await?;
    active.wait(signal).await
}

#[derive(Default)]
pub(super) struct RunnerSet {
    failure: RunnerContext,
    groups: BTreeMap<i32, Vec<ActiveRunner>>,
}

impl RunnerSet {
    pub(super) fn failure_signal(&self) -> RunnerContext {
        self.failure.clone()
    }

    pub(super) async fn start(&mut self, runners: Vec<Arc<dyn Runner>>) -> RuntimeResult<()> {
        let mut prepared = Vec::new();
        for runner in runners {
            let context = RunnerContext::new();
            let service = runner
                .prepare(context.clone())
                .await
                .map_err(|error| RunnerErrors(vec![(runner.signature().to_owned(), error)]))?;
            prepared.push((runner, context, service));
        }
        for (runner, context, service) in prepared {
            let active = ActiveRunner::start(runner, context, service, &self.failure);
            self.groups
                .entry(active.runner.shutdownPriority())
                .or_default()
                .push(active);
        }
        Ok(())
    }

    pub(super) async fn wait(
        self,
        signal: impl Future<Output = io::Result<()>>,
    ) -> RuntimeResult<()> {
        if self.groups.is_empty() {
            return Ok(());
        }
        let result = tokio::select! {
            biased;
            result = signal => result,
            () = self.failure.cancelled() => Ok(()),
        };
        self.stop().await?;
        result.map_err(Into::into)
    }

    pub(super) async fn stop(self) -> RuntimeResult<()> {
        let mut errors = Vec::new();
        for group in self.groups.into_values() {
            errors.extend(
                join_all(group.into_iter().map(ActiveRunner::stop))
                    .await
                    .into_iter()
                    .flatten(),
            );
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(RunnerErrors(errors).into())
        }
    }
}

struct ActiveRunner {
    runner: Arc<dyn Runner>,
    context: RunnerContext,
    failed: Arc<AtomicBool>,
    task: AbortOnDropHandle<RuntimeResult<()>>,
}

impl ActiveRunner {
    fn start(
        runner: Arc<dyn Runner>,
        context: RunnerContext,
        prepared: Option<RunningService>,
        failure_signal: &RunnerContext,
    ) -> Self {
        let failed = Arc::new(AtomicBool::new(false));
        let mut notice = FailureNotice {
            failed: failed.clone(),
            signal: failure_signal.clone(),
            armed: true,
        };
        let task_runner = runner.clone();
        let task_context = context.clone();
        let task = AbortOnDropHandle::new(tokio::spawn(async move {
            let result = match prepared {
                Some(service) => service.await,
                None => task_runner.run(task_context).await,
            };
            if result.is_ok() {
                notice.armed = false;
            }
            drop(notice);
            result
        }));
        Self {
            runner,
            context,
            failed,
            task,
        }
    }

    async fn stop(self) -> Option<(String, Box<dyn Error + Send + Sync>)> {
        let signature = self.runner.signature();
        if !self.failed.load(Ordering::Acquire) {
            self.context.cancel();
            if let Err(error) = self.runner.shutdown(self.context).await {
                eprintln!("Runner '{signature}' shutdown failed: {error}");
            }
        }
        match self.task.await {
            Ok(Ok(())) => None,
            Ok(Err(error)) => Some((signature.to_owned(), error)),
            Err(error) => Some((signature.to_owned(), error.into())),
        }
    }
}

struct FailureNotice {
    failed: Arc<AtomicBool>,
    signal: RunnerContext,
    armed: bool,
}

impl Drop for FailureNotice {
    fn drop(&mut self) {
        if self.armed {
            self.failed.store(true, Ordering::Release);
            self.signal.cancel();
        }
    }
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
