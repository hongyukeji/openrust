use std::sync::Arc;

use crate::{
    container::ServiceKey,
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider, facades::Facade},
};

use super::{
    Command, Error, Factory, InvokedProcess, PendingProcess, Pool, ProcessPoolResults,
    ProcessResult, RecordedProcess, Result,
};

/// Resolves the process factory in the current application scope.
pub struct Process;

impl Facade for Process {
    type Service = Factory;

    fn getFacadeAccessor() -> ServiceKey {
        ServiceKey::typed::<Factory>()
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's Process facade API")]
impl Process {
    pub fn command(command: impl Into<Command>) -> Result<PendingProcess> {
        Ok(Self::getFacadeRoot()
            .map_err(Error::Unavailable)?
            .command(command))
    }

    pub async fn run(command: impl Into<Command>) -> Result<ProcessResult> {
        Self::command(command)?.run().await
    }

    pub fn start(command: impl Into<Command>) -> Result<InvokedProcess> {
        Self::command(command)?.start()
    }

    pub fn pool(configure: impl FnOnce(&mut Pool)) -> Result<Pool> {
        Ok(Self::getFacadeRoot()
            .map_err(Error::Unavailable)?
            .pool(configure))
    }

    pub async fn concurrently(configure: impl FnOnce(&mut Pool)) -> Result<ProcessPoolResults> {
        Self::pool(configure)?.run().await
    }

    pub async fn pipe<C: Into<Command>>(
        commands: impl IntoIterator<Item = C>,
    ) -> Result<ProcessResult> {
        Self::getFacadeRoot()
            .map_err(Error::Unavailable)?
            .pipe(commands)
            .await
    }

    pub fn result(
        output: impl Into<Vec<u8>>,
        error_output: impl Into<Vec<u8>>,
        exit_code: i32,
    ) -> ProcessResult {
        Factory::result(output, error_output, exit_code)
    }

    pub fn fake(
        callback: impl Fn(&RecordedProcess) -> Option<Result<ProcessResult>> + Send + Sync + 'static,
    ) -> Result<()> {
        Self::getFacadeRoot()
            .map_err(Error::Unavailable)?
            .fake(callback);
        Ok(())
    }

    pub fn preventStrayProcesses(prevent: bool) -> Result<()> {
        Self::getFacadeRoot()
            .map_err(Error::Unavailable)?
            .preventStrayProcesses(prevent);
        Ok(())
    }

    #[track_caller]
    pub fn assertRan(predicate: impl Fn(&RecordedProcess) -> bool) -> Result<()> {
        Self::getFacadeRoot()
            .map_err(Error::Unavailable)?
            .assertRan(predicate);
        Ok(())
    }

    #[track_caller]
    pub fn assertDidntRun(predicate: impl Fn(&RecordedProcess) -> bool) -> Result<()> {
        Self::getFacadeRoot()
            .map_err(Error::Unavailable)?
            .assertDidntRun(predicate);
        Ok(())
    }
}

pub(crate) struct ProcessServiceProvider;

impl ServiceProvider for ProcessServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container()
                .singleton::<Factory, _>(|_| Ok(Arc::new(Factory::new())))?;
            Ok(())
        })
    }
}
