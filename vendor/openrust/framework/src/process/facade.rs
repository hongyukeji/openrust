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

    fn get_facade_accessor() -> ServiceKey {
        ServiceKey::typed::<Factory>()
    }
}

impl Process {
    pub fn command(command: impl Into<Command>) -> Result<PendingProcess> {
        Ok(Self::get_facade_root()?.command(command))
    }

    pub async fn run(command: impl Into<Command>) -> Result<ProcessResult> {
        Self::command(command)?.run().await
    }

    pub fn start(command: impl Into<Command>) -> Result<InvokedProcess> {
        Self::command(command)?.start()
    }

    pub fn pool(configure: impl FnOnce(&mut Pool)) -> Result<Pool> {
        Ok(Self::get_facade_root()?.pool(configure))
    }

    pub async fn concurrently(configure: impl FnOnce(&mut Pool)) -> Result<ProcessPoolResults> {
        Self::pool(configure)?.run().await
    }

    pub async fn pipe<C: Into<Command>>(
        commands: impl IntoIterator<Item = C>,
    ) -> Result<ProcessResult> {
        Self::get_facade_root()?.pipe(commands).await
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
        Self::get_facade_root()?.fake(callback);
        Ok(())
    }

    pub fn prevent_stray_processes(prevent: bool) -> Result<()> {
        Self::get_facade_root()?.prevent_stray_processes(prevent);
        Ok(())
    }

    #[track_caller]
    pub fn assert_ran(
        predicate: impl Fn(&RecordedProcess, Option<&ProcessResult>) -> bool,
    ) -> Result<()> {
        Self::get_facade_root()?.assert_ran(predicate);
        Ok(())
    }

    #[track_caller]
    pub fn assert_ran_times(
        predicate: impl Fn(&RecordedProcess, Option<&ProcessResult>) -> bool,
        expected: usize,
    ) -> Result<()> {
        Self::get_facade_root()?.assert_ran_times(predicate, expected);
        Ok(())
    }

    #[track_caller]
    pub fn assert_not_ran(
        predicate: impl Fn(&RecordedProcess, Option<&ProcessResult>) -> bool,
    ) -> Result<()> {
        Self::get_facade_root()?.assert_not_ran(predicate);
        Ok(())
    }

    #[track_caller]
    pub fn assert_nothing_ran() -> Result<()> {
        Self::get_facade_root()?.assert_nothing_ran();
        Ok(())
    }

    #[track_caller]
    pub fn assert_ran_in_order(callbacks: &[&super::ProcessAssertion<'_>]) -> Result<()> {
        Self::get_facade_root()?.assert_ran_in_order(callbacks);
        Ok(())
    }

    #[track_caller]
    pub fn assert_didnt_run(
        predicate: impl Fn(&RecordedProcess, Option<&ProcessResult>) -> bool,
    ) -> Result<()> {
        Self::get_facade_root()?.assert_didnt_run(predicate);
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
