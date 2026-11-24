//! Artisan command registration, input, output, and generation.

mod application;
mod command;
mod command_input;
mod descriptor;
pub mod events;
pub mod generator_command;
mod help_command;
mod input;
mod isolation;
mod list_command;
pub(crate) mod localization;
mod planned;
pub mod scheduling;
mod signature;
pub(crate) mod task_command;

use std::{
    env,
    error::Error,
    fmt,
    future::Future,
    io::{self, IsTerminal as _},
    pin::Pin,
    sync::{Arc, LazyLock},
};

use clap::ArgMatches;
use regex::Regex;

pub use application::Application;
pub use command::{
    ClosureCommand, Command, CommandCallback, CommandDefinition, IntoCommandCallback,
};
pub use command_input::CommandInput;
pub use generator_command::GeneratorCommand;
pub(crate) use isolation::IsolatedCommand;
pub(crate) use localization::text as localized_text;

/// A future for a console command or lifecycle hook.
pub type ConsoleFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Parsed arguments and options for a command.
#[derive(Clone)]
pub struct Input {
    syntax: Arc<clap::Command>,
    parsing_syntax: Arc<clap::Command>,
    invoked_name: Option<String>,
    matches: ArgMatches,
    shutdown: Option<tokio_util::sync::CancellationToken>,
}

impl Input {
    pub fn matches(&self) -> &ArgMatches {
        &self.matches
    }

    /// Preserve parsed arguments while supplying a child command's shutdown scope.
    pub fn with_shutdown(&self, shutdown: tokio_util::sync::CancellationToken) -> Self {
        Self {
            syntax: self.syntax.clone(),
            parsing_syntax: self.parsing_syntax.clone(),
            invoked_name: self.invoked_name.clone(),
            matches: self.matches.clone(),
            shutdown: Some(shutdown),
        }
    }

    /// Cooperative shutdown requested by the application runner.
    pub fn shutdown(&self) -> Option<&tokio_util::sync::CancellationToken> {
        self.shutdown.as_ref()
    }
}

/// The command's output streams.
#[derive(Default)]
pub struct Output {
    pub stdout: String,
    pub stderr: String,
    decorated: bool,
    verbosity: u8,
    suppressed: bool,
    terminal: bool,
}

impl Output {
    pub(crate) fn for_terminal() -> Self {
        let disabled = env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty());
        let forced = env::var_os("FORCE_COLOR").is_some_and(|value| !value.is_empty());
        let terminal =
            io::stdout().is_terminal() && env::var_os("TERM").as_deref() != Some("dumb".as_ref());
        Self {
            decorated: !disabled && (forced || terminal),
            terminal: true,
            ..Self::default()
        }
    }

    pub fn is_decorated(&self) -> bool {
        self.decorated
    }

    pub fn is_verbose(&self) -> bool {
        !self.suppressed && self.verbosity >= 1
    }

    pub fn is_very_verbose(&self) -> bool {
        !self.suppressed && self.verbosity >= 2
    }

    /// Writes already-rendered text without interpreting formatter tags.
    pub fn write_raw(&mut self, message: &str) -> io::Result<()> {
        use std::io::Write as _;
        if self.suppressed {
            return Ok(());
        }
        if self.terminal {
            let mut stdout = io::stdout().lock();
            stdout.write_all(message.as_bytes())?;
            stdout.flush()
        } else {
            self.stdout.push_str(message);
            Ok(())
        }
    }

    /// Whether raw output is streamed to the process rather than captured.
    pub fn streams_to_terminal(&self) -> bool {
        self.terminal
    }

    /// Writes a comment line, rendering the supported inline bold and gray styles.
    pub fn comment(&mut self, message: &str) {
        static INLINE_STYLE: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"(?s)<(options=bold|fg=gray)>(.*?)</>").expect("valid style pattern")
        });

        let mut start = 0;
        for span in INLINE_STYLE.captures_iter(message) {
            let tag = span.get(0).expect("style span");
            self.write_style(&message[start..tag.start()], "\x1b[33m", "\x1b[39m");
            let (open, close) = if &span[1] == "options=bold" {
                ("\x1b[1m", "\x1b[22m")
            } else {
                ("\x1b[90m", "\x1b[39m")
            };
            self.write_style(&span[2], open, close);
            start = tag.end();
        }
        self.write_style(&message[start..], "\x1b[33m", "\x1b[39m");
        self.stdout.push('\n');
    }

    /// Writes a console information component.
    pub fn info(&mut self, message: &str) {
        self.stdout.push_str("\n  ");
        self.write_style(" INFO ", "\x1b[37;44m", "\x1b[39;49m");
        self.stdout.push(' ');
        self.stdout.push_str(message);
        self.stdout.push_str("\n\n");
    }

    /// 输出警告组件，保持信息与错误组件的布局。
    pub fn warning(&mut self, message: &str) {
        self.stdout.push_str("\n  ");
        self.write_style(" WARN ", "\x1b[30;43m", "\x1b[39;49m");
        self.stdout.push(' ');
        self.stdout.push_str(message);
        self.stdout.push_str("\n\n");
    }

    /// Writes a console error component, matching the information component layout.
    pub fn error(&mut self, message: &str) {
        self.stdout.push_str("\n  ");
        self.write_style(" ERROR ", "\x1b[37;41m", "\x1b[39;49m");
        self.stdout.push(' ');
        self.stdout.push_str(message);
        self.stdout.push_str("\n\n");
    }

    fn write_style(&mut self, text: &str, open: &str, close: &str) {
        if self.decorated && !text.is_empty() {
            self.stdout.push_str(open);
            self.stdout.push_str(text);
            self.stdout.push_str(close);
        } else {
            self.stdout.push_str(text);
        }
    }
}

/// A failure in command registration, dispatch, or execution.
#[derive(Clone, Debug)]
pub struct ConsoleError(ConsoleFailure);

#[derive(Clone, Debug)]
enum ConsoleFailure {
    Message(String),
    Dump(String),
    Source(Arc<dyn Error + Send + Sync>),
}

impl ConsoleError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(ConsoleFailure::Message(message.into()))
    }

    /// 保留下游错误类型与来源链，同时沿用控制台渲染流程。
    pub fn from_source(error: impl Error + Send + Sync + 'static) -> Self {
        Self(ConsoleFailure::Source(Arc::new(error)))
    }

    pub(crate) fn dump_output(&self) -> Option<&str> {
        match &self.0 {
            ConsoleFailure::Message(_) | ConsoleFailure::Source(_) => None,
            ConsoleFailure::Dump(output) => Some(output),
        }
    }
}

impl From<crate::support::CollectionError> for ConsoleError {
    fn from(error: crate::support::CollectionError) -> Self {
        match error {
            crate::support::CollectionError::DumpAndDie(output) => {
                Self(ConsoleFailure::Dump(output))
            }
            error => Self::new(error.to_string()),
        }
    }
}

impl fmt::Display for ConsoleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            ConsoleFailure::Message(message) => f.write_str(message),
            ConsoleFailure::Source(error) => error.fmt(f),
            ConsoleFailure::Dump(_) => f.write_str("collection debugging stopped execution"),
        }
    }
}

impl Error for ConsoleError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.0 {
            ConsoleFailure::Source(error) => Some(error.as_ref()),
            _ => None,
        }
    }
}

pub(crate) use confirm_to_proceed as confirm_production;

/// Confirms a command before proceeding in production using the shared localized prompt.
///
/// The caller supplies its production condition (confirmation callback).
/// An absent `--force` option never bypasses confirmation. Non-interactive input refuses.
pub fn confirm_to_proceed(
    production: bool,
    matches: &clap::ArgMatches,
) -> Result<bool, ConsoleError> {
    use std::io::Write as _;
    if !production
        || matches
            .try_get_one::<bool>("force")
            .ok()
            .flatten()
            .copied()
            .unwrap_or(false)
    {
        return Ok(true);
    }
    if matches
        .try_get_one::<bool>("no-interaction")
        .ok()
        .flatten()
        .copied()
        .unwrap_or(false)
        || !io::stdin().is_terminal()
    {
        return Ok(false);
    }
    let prompt = localization::text(
        "confirm.production",
        "APPLICATION IN PRODUCTION. Do you really wish to run this command?",
    );
    let mut output = io::stderr().lock();
    write!(output, "{prompt} [y/N] ").map_err(|error| ConsoleError::new(error.to_string()))?;
    output
        .flush()
        .map_err(|error| ConsoleError::new(error.to_string()))?;
    let mut response = String::new();
    io::stdin()
        .read_line(&mut response)
        .map_err(|error| ConsoleError::new(error.to_string()))?;
    Ok(matches!(
        response.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

pub(crate) fn command_cancelled() -> String {
    localization::text("confirm.cancelled", "Command cancelled.")
}

pub(crate) fn monitor_signals(
    shutdown: tokio_util::sync::CancellationToken,
) -> std::io::Result<tokio::task::JoinHandle<()>> {
    #[cfg(unix)]
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    #[cfg(windows)]
    let mut terminate = tokio::signal::windows::ctrl_break()?;
    Ok(tokio::spawn(async move {
        #[cfg(any(unix, windows))]
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = terminate.recv() => {},
        }
        #[cfg(not(any(unix, windows)))]
        let _ = tokio::signal::ctrl_c().await;
        shutdown.cancel();
    }))
}
