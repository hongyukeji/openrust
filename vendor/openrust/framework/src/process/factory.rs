use std::{
    collections::VecDeque,
    ffi::OsString,
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

use indexmap::IndexMap;

use super::{
    Command, Error, InvokedProcess, PendingProcess, Pool, ProcessPoolResults, ProcessResult, Result,
};

type Fake = dyn Fn(&RecordedProcess) -> Option<Result<ProcessResult>> + Send + Sync;

/// The process builder and testing state owned by one application.
#[derive(Clone, Default)]
pub struct Factory {
    testing: Arc<Mutex<Testing>>,
}

#[derive(Default)]
struct Testing {
    fakes: IndexMap<String, Arc<Fake>>,
    prevent_stray: bool,
    recorded: Vec<(RecordedProcess, Option<ProcessResult>)>,
}

#[derive(Clone)]
pub struct RecordedProcess {
    pub command: Command,
    pub path: Option<PathBuf>,
    pub environment: IndexMap<OsString, Option<OsString>>,
    pub input: Vec<u8>,
    pub timeout: Option<Duration>,
    pub idle_timeout: Option<Duration>,
    pub quiet: bool,
}

#[allow(non_snake_case, reason = "Matches Laravel's process factory API")]
impl Factory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn command(&self, command: impl Into<Command>) -> PendingProcess {
        PendingProcess::new(self.clone(), command.into())
    }

    pub async fn run(&self, command: impl Into<Command>) -> Result<ProcessResult> {
        self.command(command).run().await
    }

    pub fn start(&self, command: impl Into<Command>) -> Result<InvokedProcess> {
        self.command(command).start()
    }

    pub fn pool(&self, configure: impl FnOnce(&mut Pool)) -> Pool {
        let mut pool = Pool::new(self.clone());
        configure(&mut pool);
        pool
    }

    pub async fn concurrently(
        &self,
        configure: impl FnOnce(&mut Pool),
    ) -> Result<ProcessPoolResults> {
        self.pool(configure).run().await
    }

    pub async fn pipe<C: Into<Command>>(
        &self,
        commands: impl IntoIterator<Item = C>,
    ) -> Result<ProcessResult> {
        let mut commands = commands.into_iter();
        let command = commands.next().ok_or(Error::InvalidCommand)?;
        let mut result = self.run(command).await?;
        for command in commands {
            if result.failed() {
                break;
            }
            result = self.command(command).input(result.stdout).run().await?;
        }
        Ok(result)
    }

    pub fn result(
        output: impl Into<Vec<u8>>,
        error_output: impl Into<Vec<u8>>,
        exit_code: i32,
    ) -> ProcessResult {
        ProcessResult {
            command: Command::Arguments(Vec::new()),
            exit_code,
            stdout: output.into(),
            stderr: error_output.into(),
        }
    }

    pub fn fake(
        &self,
        callback: impl Fn(&RecordedProcess) -> Option<Result<ProcessResult>> + Send + Sync + 'static,
    ) {
        let mut testing = self.testing();
        testing.fakes.clear();
        testing.fakes.insert("*".into(), Arc::new(callback));
    }

    pub fn fakeCommand(&self, pattern: impl Into<String>, result: ProcessResult) {
        self.testing()
            .fakes
            .insert(pattern.into(), Arc::new(move |_| Some(Ok(result.clone()))));
    }

    pub fn fakeSequence(
        &self,
        pattern: impl Into<String>,
        results: impl IntoIterator<Item = Result<ProcessResult>>,
    ) {
        let queue = Mutex::new(results.into_iter().collect::<VecDeque<_>>());
        self.testing().fakes.insert(
            pattern.into(),
            Arc::new(move |_| {
                Some(
                    queue
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .pop_front()
                        .unwrap_or(Err(Error::SequenceExhausted)),
                )
            }),
        );
    }

    pub fn preventStrayProcesses(&self, prevent: bool) {
        self.testing().prevent_stray = prevent;
    }

    pub fn recorded(&self) -> Vec<(RecordedProcess, Option<ProcessResult>)> {
        self.testing().recorded.clone()
    }

    #[track_caller]
    pub fn assertRan(&self, predicate: impl Fn(&RecordedProcess) -> bool) {
        assert!(
            self.recorded()
                .iter()
                .any(|(process, _)| predicate(process)),
            "no recorded process matched"
        );
    }

    #[track_caller]
    pub fn assertDidntRun(&self, predicate: impl Fn(&RecordedProcess) -> bool) {
        assert!(
            !self
                .recorded()
                .iter()
                .any(|(process, _)| predicate(process)),
            "a recorded process matched"
        );
    }

    #[track_caller]
    pub fn assertRanTimes(&self, predicate: impl Fn(&RecordedProcess) -> bool, expected: usize) {
        assert_eq!(
            self.recorded()
                .iter()
                .filter(|(process, _)| predicate(process))
                .count(),
            expected
        );
    }

    pub(super) fn resolveFake(
        &self,
        process: impl FnOnce() -> RecordedProcess,
    ) -> Result<Option<Result<ProcessResult>>> {
        let (fakes, prevent_stray) = {
            let testing = self.testing();
            (testing.fakes.clone(), testing.prevent_stray)
        };
        if fakes.is_empty() {
            return if prevent_stray {
                Err(Error::StrayProcess)
            } else {
                Ok(None)
            };
        }
        let process = process();
        if let Some(mut result) = fakes
            .iter()
            .find(|(pattern, _)| process.command.matches(pattern))
            .and_then(|(_, fake)| fake(&process))
        {
            if let Ok(result) = &mut result {
                result.command = process.command.clone();
                if process.quiet {
                    result.stdout.clear();
                    result.stderr.clear();
                }
            }
            self.testing()
                .recorded
                .push((process, result.as_ref().ok().cloned()));
            return Ok(Some(result));
        }
        if prevent_stray {
            Err(Error::StrayProcess)
        } else {
            Ok(None)
        }
    }

    fn testing(&self) -> MutexGuard<'_, Testing> {
        self.testing
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}
