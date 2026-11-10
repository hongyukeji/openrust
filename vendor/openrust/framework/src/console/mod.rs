//! Artisan command registration, input, output, and generation.

mod application;
mod command;
mod generator_command;
mod help_command;
mod list_command;
mod planned;
pub mod scheduling;

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
pub use command::{ClosureCommand, Command, CommandDefinition};
pub use generator_command::GeneratorCommand;

/// A future for a console command or lifecycle hook.
pub type ConsoleFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Parsed arguments and options for a command.
pub struct Input {
    syntax: Arc<clap::Command>,
    matches: ArgMatches,
}

impl Input {
    pub fn matches(&self) -> &ArgMatches {
        &self.matches
    }
}

/// The command's output streams.
#[derive(Default)]
pub struct Output {
    pub stdout: String,
    pub stderr: String,
    decorated: bool,
}

impl Output {
    pub(crate) fn for_terminal() -> Self {
        let disabled = env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty());
        let forced = env::var_os("FORCE_COLOR").is_some_and(|value| !value.is_empty());
        let terminal =
            io::stdout().is_terminal() && env::var_os("TERM").as_deref() != Some("dumb".as_ref());
        Self {
            decorated: !disabled && (forced || terminal),
            ..Self::default()
        }
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
pub struct ConsoleError(String);

impl ConsoleError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for ConsoleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Error for ConsoleError {}
