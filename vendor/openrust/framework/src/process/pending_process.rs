use std::{ffi::OsString, io, path::PathBuf, process::Stdio, time::Duration};

use indexmap::IndexMap;

use super::{
    Error, Factory, InvokedProcess, OutputStream, ProcessResult, RecordedProcess, Result,
    child::ChildGroup,
    invoked_process::{Options, OutputCallback},
};

/// A shell script or an executable followed by its literal arguments.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Command {
    Shell(OsString),
    Arguments(Vec<OsString>),
}

impl Command {
    pub fn arguments(arguments: impl IntoIterator<Item = impl Into<OsString>>) -> Self {
        Self::Arguments(arguments.into_iter().map(Into::into).collect())
    }

    fn native(&self) -> Result<tokio::process::Command> {
        match self {
            Self::Shell(script) if !script.is_empty() => {
                #[cfg(unix)]
                let (shell, option) = ("/bin/sh", "-c");
                #[cfg(windows)]
                let (shell, option) = ("cmd.exe", "/C");
                let mut command = tokio::process::Command::new(shell);
                command.arg(option);
                #[cfg(windows)]
                command.raw_arg(script);
                #[cfg(unix)]
                command.arg(script);
                Ok(command)
            }
            Self::Arguments(arguments) => {
                let (program, args) = arguments.split_first().ok_or(Error::InvalidCommand)?;
                if program.is_empty() {
                    return Err(Error::InvalidCommand);
                }
                let mut command = tokio::process::Command::new(program);
                command.args(args);
                Ok(command)
            }
            _ => Err(Error::InvalidCommand),
        }
    }
}

impl From<&str> for Command {
    fn from(script: &str) -> Self {
        Self::Shell(script.into())
    }
}

impl From<String> for Command {
    fn from(script: String) -> Self {
        Self::Shell(script.into())
    }
}

impl<S: Into<OsString>, const N: usize> From<[S; N]> for Command {
    fn from(arguments: [S; N]) -> Self {
        Self::arguments(arguments)
    }
}

impl<S: Into<OsString>> From<Vec<S>> for Command {
    fn from(arguments: Vec<S>) -> Self {
        Self::arguments(arguments)
    }
}

/// Configuration for one process invocation.
pub struct PendingProcess {
    factory: Factory,
    command: Command,
    path: Option<PathBuf>,
    environment: IndexMap<OsString, Option<OsString>>,
    input: Vec<u8>,
    timeout: Option<Duration>,
    idle_timeout: Option<Duration>,
    quiet: bool,
}

#[allow(non_snake_case, reason = "Matches Laravel's pending process API")]
impl PendingProcess {
    pub(super) fn new(factory: Factory, command: Command) -> Self {
        Self {
            factory,
            command,
            path: None,
            environment: IndexMap::new(),
            input: Vec::new(),
            timeout: Some(Duration::from_secs(60)),
            idle_timeout: None,
            quiet: false,
        }
    }

    pub fn path(mut self, path: impl Into<PathBuf>) -> Self {
        self.path = Some(path.into());
        self
    }

    pub fn env<K, V>(mut self, environment: impl IntoIterator<Item = (K, V)>) -> Self
    where
        K: Into<OsString>,
        V: Into<OsString>,
    {
        self.environment.extend(
            environment
                .into_iter()
                .map(|(key, value)| (key.into(), Some(value.into()))),
        );
        self
    }

    pub fn removeEnv(mut self, key: impl Into<OsString>) -> Self {
        self.environment.insert(key.into(), None);
        self
    }

    pub fn input(mut self, bytes: impl Into<Vec<u8>>) -> Self {
        self.input = bytes.into();
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = (!timeout.is_zero()).then_some(timeout);
        self
    }

    pub fn idleTimeout(mut self, timeout: Duration) -> Self {
        self.idle_timeout = (!timeout.is_zero()).then_some(timeout);
        self
    }

    pub fn forever(mut self) -> Self {
        self.timeout = None;
        self
    }

    pub fn quietly(mut self) -> Self {
        self.quiet = true;
        self
    }

    pub async fn run(self) -> Result<ProcessResult> {
        self.start()?.wait().await
    }

    pub async fn runWithOutput(
        self,
        callback: impl FnMut(OutputStream, &[u8]) + Send + 'static,
    ) -> Result<ProcessResult> {
        self.startWithOutput(callback)?.wait().await
    }

    pub fn start(self) -> Result<InvokedProcess> {
        self.spawn(None)
    }

    pub fn startWithOutput(
        self,
        callback: impl FnMut(OutputStream, &[u8]) + Send + 'static,
    ) -> Result<InvokedProcess> {
        self.spawn(Some(Box::new(callback)))
    }

    fn spawn(self, callback: Option<OutputCallback>) -> Result<InvokedProcess> {
        tokio::runtime::Handle::try_current().map_err(io::Error::other)?;
        if self.quiet && self.idle_timeout.is_some() {
            return Err(Error::InvalidTimeout);
        }
        let mut native = self.command.native()?;
        if let Some(result) = self.factory.resolveFake(|| RecordedProcess {
            command: self.command.clone(),
            path: self.path.clone(),
            environment: self.environment.clone(),
            input: self.input.clone(),
            timeout: self.timeout,
            idle_timeout: self.idle_timeout,
            quiet: self.quiet,
        })? {
            return Ok(InvokedProcess::fake(self.command, result?, callback));
        }
        if let Some(path) = &self.path {
            native.current_dir(path);
        }
        for (key, value) in &self.environment {
            if let Some(value) = value {
                native.env(key, value);
            } else {
                native.env_remove(key);
            }
        }
        native.stdin(if self.input.is_empty() {
            Stdio::null()
        } else {
            Stdio::piped()
        });
        if self.quiet {
            native.stdout(Stdio::null()).stderr(Stdio::null());
        } else {
            native.stdout(Stdio::piped()).stderr(Stdio::piped());
        }
        let child = ChildGroup::spawn(&mut native)?;
        Ok(InvokedProcess::spawn(
            self.command,
            child,
            Options {
                input: self.input,
                timeout: self.timeout,
                idle_timeout: self.idle_timeout,
                callback,
            },
        ))
    }
}

impl Command {
    pub(super) fn matches(&self, pattern: &str) -> bool {
        match self {
            Self::Shell(script) => {
                crate::support::Str::is(&[pattern], &script.to_string_lossy(), false)
            }
            Self::Arguments(arguments) => {
                let command = arguments
                    .iter()
                    .map(|argument| argument.to_string_lossy())
                    .collect::<Vec<_>>()
                    .join(" ");
                crate::support::Str::is(&[pattern], &command, false)
            }
        }
    }
}
