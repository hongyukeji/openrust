//! Artisan command registration, input, output, and generation.

mod ansi_color_mode;
mod application;
mod color;
mod command;
mod command_input;
mod command_io;
mod descriptor;
pub mod events;
mod formatter;
mod formatter_style;
mod formatting;
pub mod generator_command;
mod help_command;
mod input;
mod isolation;
mod list_command;
pub(crate) mod localization;
mod output_style;
mod planned;
mod progress_bar;
mod prompting;
pub mod question;
pub mod scheduling;
mod signature;
mod table;
pub(crate) mod task_command;
mod terminal;

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

pub use ansi_color_mode::AnsiColorMode;
pub use application::Application;
pub use color::Color;
pub use command::{
    AsyncCommandCallback, ClosureCommand, Command, CommandCallback, CommandDefinition,
    IntoCommandCallback,
};
pub use command_input::CommandInput;
pub use command_io::CommandIo;
pub use formatter::{
    FormatterHandle, OutputFormatter, OutputFormatterInterface, WrappableOutputFormatterInterface,
};
pub use formatter_style::{
    FormatterStyle, OutputFormatterStyle, OutputFormatterStyleInterface, OutputFormatterStyleStack,
    OutputFormatterStyleStackHandle,
};
pub use formatting::{ConsoleFormatError, ConsoleTemplateError};
pub use generator_command::GeneratorCommand;
pub(crate) use isolation::IsolatedCommand;
pub(crate) use localization::text as localized_text;
pub use output_style::{OutputFormat, OutputMessages, OutputOptions, OutputStyle, OutputVerbosity};
pub(crate) use progress_bar::ProgressDefinitions;
pub use progress_bar::{
    ConsoleProgressError, ProgressBar, ProgressFormatter, ProgressIterable, ProgressIterableMut,
    ProgressIterator, ProgressOutput, ProgressStyleIterator,
};
pub use prompting::{CommandRegistration, MissingInputPrompt};
pub use question::{
    ChoiceKey, ChoiceLabel, ChoiceQuestion, CompletionCallback, ConfirmationQuestion,
    ConsoleInputStream, ConsoleQuestionError, ConsoleReader, Question, QuestionCallback,
};
pub(crate) use table::TableDefinitions;
pub use table::{
    ConsoleTableError, Table, TableCell, TableCellStyle, TablePadding, TableRow, TableSeparator,
    TableStyle,
};
pub use terminal::Terminal;

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
    interaction: question::Interaction,
    stream: Option<ConsoleInputStream>,
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
            interaction: self.interaction.clone(),
            stream: self.stream.clone(),
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
    formatter_generation: u64,
    error_formatter_generation: u64,
    error_decorated: bool,
    error_verbosity: Option<OutputVerbosity>,
    verbosity: u8,
    suppressed: bool,
    silent: bool,
    accepted_ranges: Vec<std::ops::Range<usize>>,
    terminal: bool,
    capture_generation: u64,
    formatter: formatting::Formatter,
    error_formatter: Option<formatting::Formatter>,
}

impl Output {
    pub(crate) fn for_terminal() -> Self {
        let disabled = env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty());
        let forced = env::var_os("FORCE_COLOR").is_some_and(|value| !value.is_empty());
        let terminal =
            io::stdout().is_terminal() && env::var_os("TERM").as_deref() != Some("dumb".as_ref());
        let formatter = formatting::Formatter::default();
        formatter.set_decorated(!disabled && (forced || terminal));
        Self {
            formatter,
            error_decorated: !disabled
                && (forced
                    || (io::stderr().is_terminal()
                        && env::var_os("TERM").as_deref() != Some("dumb".as_ref()))),
            terminal: true,
            ..Self::default()
        }
    }

    pub fn is_decorated(&self) -> bool {
        self.formatter.is_decorated()
    }

    /// 获取当前对象；通过返回引用修改配置会立即影响后续写入。
    pub fn get_formatter(&self) -> FormatterHandle {
        self.formatter.handle()
    }
    /// 真实控制台的两条流使用同一个显式替换对象。
    pub fn set_formatter(&mut self, formatter: FormatterHandle) {
        self.formatter = formatting::Formatter::from_handle(formatter.clone());
        self.formatter_generation = self.formatter_generation.wrapping_add(1);
        if self.terminal {
            self.error_formatter = Some(formatting::Formatter::from_handle(formatter));
            self.error_formatter_generation = self.error_formatter_generation.wrapping_add(1);
        }
    }
    fn set_progress_decorated(&mut self, decorated: bool) {
        if self.terminal {
            self.error_decorated = decorated;
            if let Some(formatter) = &self.error_formatter {
                formatter.set_decorated(decorated);
            }
        } else {
            self.formatter.set_decorated(decorated);
        }
    }
    pub fn is_verbose(&self) -> bool {
        !self.suppressed && self.verbosity >= 1
    }

    pub fn is_very_verbose(&self) -> bool {
        !self.suppressed && self.verbosity >= 2
    }

    /// 设置普通输出；真实控制台同时更新错误流的默认级别。
    pub fn set_verbosity(&mut self, level: OutputVerbosity) {
        self.verbosity = match level {
            OutputVerbosity::Verbose => 1,
            OutputVerbosity::VeryVerbose => 2,
            OutputVerbosity::Debug => 3,
            _ => 0,
        };
        self.suppressed = matches!(level, OutputVerbosity::Quiet | OutputVerbosity::Silent);
        self.silent = level == OutputVerbosity::Silent;
        self.error_verbosity = None;
    }
    pub fn set_decorated(&mut self, decorated: bool) {
        self.formatter.set_decorated(decorated);
        self.set_progress_decorated(decorated);
    }

    pub fn verbosity(&self) -> OutputVerbosity {
        if self.silent {
            OutputVerbosity::Silent
        } else if self.suppressed {
            OutputVerbosity::Quiet
        } else {
            match self.verbosity {
                0 => OutputVerbosity::Normal,
                1 => OutputVerbosity::Verbose,
                2 => OutputVerbosity::VeryVerbose,
                _ => OutputVerbosity::Debug,
            }
        }
    }
    pub fn is_quiet(&self) -> bool {
        self.verbosity() == OutputVerbosity::Quiet
    }
    pub fn is_silent(&self) -> bool {
        self.silent
    }
    pub fn is_debug(&self) -> bool {
        self.verbosity() == OutputVerbosity::Debug
    }

    /// 保留经输出管线按写入时级别接受的消息；原始缓冲仍受调用静默选项控制。
    pub(super) fn suppress_stdout_since(&mut self, start: usize) {
        let start = start.min(self.stdout.len());
        let mut accepted = String::new();
        let mut ranges = Vec::new();
        for range in &self.accepted_ranges {
            if range.end <= start {
                ranges.push(range.clone());
                continue;
            }
            if range.start < start {
                ranges.push(range.start..start);
            }
            let from = range.start.max(start);
            if let Some(text) = self.stdout.get(from..range.end) {
                let offset = start + accepted.len();
                accepted.push_str(text);
                ranges.push(offset..start + accepted.len());
            }
        }
        self.stdout.truncate(start);
        self.stdout.push_str(&accepted);
        self.accepted_ranges = ranges;
    }

    pub(super) fn emit_rendered(
        &mut self,
        message: &str,
        level: OutputVerbosity,
    ) -> Result<(), ConsoleError> {
        if self.verbosity() < level || self.silent {
            return Ok(());
        }
        if self.terminal {
            if !self.stdout.is_empty() && !self.suppressed {
                self.capture_generation = self.capture_generation.wrapping_add(1);
                let pending = std::mem::take(&mut self.stdout);
                if let Err(source) = self.write_raw(&pending) {
                    self.stdout = pending;
                    return Err(ConsoleError::from_source(source));
                }
                self.accepted_ranges.clear();
            }
            use std::io::Write as _;
            let mut stdout = io::stdout().lock();
            stdout
                .write_all(message.as_bytes())
                .and_then(|()| stdout.flush())
                .map_err(ConsoleError::from_source)
        } else {
            let start = self.stdout.len();
            // 调用方可以清空捕获缓冲，旧的可见区间不能再次命中新内容。
            self.accepted_ranges.retain(|range| range.end <= start);
            self.stdout.push_str(message);
            if !message.is_empty() {
                if let Some(last) = self
                    .accepted_ranges
                    .last_mut()
                    .filter(|last| last.end == start)
                {
                    last.end = self.stdout.len();
                } else {
                    self.accepted_ranges.push(start..self.stdout.len());
                }
            }
            Ok(())
        }
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
        if self.is_decorated() && !text.is_empty() {
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
    Runtime(Arc<dyn Error + Send + Sync>),
}

impl ConsoleError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(ConsoleFailure::Message(message.into()))
    }

    /// 保留下游错误类型与来源链，同时沿用控制台渲染流程。
    pub fn from_source(error: impl Error + Send + Sync + 'static) -> Self {
        Self(ConsoleFailure::Source(Arc::new(error)))
    }

    /// 标记应立即终止问答的运行错误，同时保留具体错误与来源链。
    pub fn from_runtime_source(error: impl Error + Send + Sync + 'static) -> Self {
        Self(ConsoleFailure::Runtime(Arc::new(error)))
    }
    pub fn is_runtime(&self) -> bool {
        matches!(self.0, ConsoleFailure::Runtime(_))
    }

    pub(crate) fn command_exit_code(&self) -> i32 {
        let mut error: Option<&(dyn std::error::Error + 'static)> = Some(self);
        while let Some(source) = error {
            if source.downcast_ref::<ConsoleQuestionError>()
                == Some(&ConsoleQuestionError::Interrupted)
            {
                return 130;
            }
            error = source.source();
        }
        1
    }

    pub(crate) fn dump_output(&self) -> Option<&str> {
        match &self.0 {
            ConsoleFailure::Message(_) | ConsoleFailure::Source(_) | ConsoleFailure::Runtime(_) => {
                None
            }
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
            ConsoleFailure::Source(error) | ConsoleFailure::Runtime(error) => error.fmt(f),
            ConsoleFailure::Dump(_) => f.write_str("collection debugging stopped execution"),
        }
    }
}

impl Error for ConsoleError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.0 {
            ConsoleFailure::Source(error) | ConsoleFailure::Runtime(error) => Some(error.as_ref()),
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
