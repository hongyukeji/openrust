use std::sync::Arc;

use crate::{
    container::ServiceKey,
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider, facades::Facade},
};

use super::{
    Command, Factory, FakeProcessDescription, InvokedProcess, PendingProcess, Pool,
    ProcessPoolResults, ProcessResult, RecordedProcess, Result,
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
        Ok(Self::getFacadeRoot()?.command(command))
    }

    pub async fn run(command: impl Into<Command>) -> Result<ProcessResult> {
        Self::command(command)?.run().await
    }

    pub fn start(command: impl Into<Command>) -> Result<InvokedProcess> {
        Self::command(command)?.start()
    }

    pub fn pool(configure: impl FnOnce(&mut Pool)) -> Result<Pool> {
        Ok(Self::getFacadeRoot()?.pool(configure))
    }

    pub async fn concurrently(configure: impl FnOnce(&mut Pool)) -> Result<ProcessPoolResults> {
        Self::pool(configure)?.run().await
    }

    pub async fn pipe<C: Into<Command>>(
        commands: impl IntoIterator<Item = C>,
    ) -> Result<ProcessResult> {
        Self::getFacadeRoot()?.pipe(commands).await
    }

    pub fn describe() -> FakeProcessDescription {
        Factory::describe()
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
        Self::getFacadeRoot()?.fake(callback);
        Ok(())
    }

    pub fn preventStrayProcesses(prevent: bool) -> Result<()> {
        Self::getFacadeRoot()?.preventStrayProcesses(prevent);
        Ok(())
    }

    #[track_caller]
    pub fn assertRan(
        predicate: impl Fn(&RecordedProcess, Option<&ProcessResult>) -> bool,
    ) -> Result<()> {
        Self::getFacadeRoot()?.assertRan(predicate);
        Ok(())
    }

    #[track_caller]
    pub fn assertRanTimes(
        predicate: impl Fn(&RecordedProcess, Option<&ProcessResult>) -> bool,
        expected: usize,
    ) -> Result<()> {
        Self::getFacadeRoot()?.assertRanTimes(predicate, expected);
        Ok(())
    }

    #[track_caller]
    pub fn assertNotRan(
        predicate: impl Fn(&RecordedProcess, Option<&ProcessResult>) -> bool,
    ) -> Result<()> {
        Self::getFacadeRoot()?.assertNotRan(predicate);
        Ok(())
    }

    #[track_caller]
    pub fn assertNothingRan() -> Result<()> {
        Self::getFacadeRoot()?.assertNothingRan();
        Ok(())
    }

    #[track_caller]
    pub fn assertRanInOrder(callbacks: &[&super::ProcessAssertion<'_>]) -> Result<()> {
        Self::getFacadeRoot()?.assertRanInOrder(callbacks);
        Ok(())
    }

    #[track_caller]
    pub fn assertDidntRun(
        predicate: impl Fn(&RecordedProcess, Option<&ProcessResult>) -> bool,
    ) -> Result<()> {
        Self::getFacadeRoot()?.assertDidntRun(predicate);
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
