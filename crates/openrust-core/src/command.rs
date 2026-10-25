//! Runtime commands exposed by the application binary.

use std::fmt::Write as _;

use crate::app::App;
use crate::error::Result;
use crate::plugin::BoxFuture;

/// A named command runnable as `<app-binary> <name> [args…]`.
///
/// Plugins build commands during [`Plugin::build`](crate::Plugin::build);
/// the HTTP crate contributes `serve`, the database crate `migrate`, and so on. Names use space-separated words for nested commands (`queue work`, `db seed`).
pub trait Command: Send + Sync + 'static {
    /// Command name as typed on the command line.
    fn name(&self) -> &'static str;

    /// One-line description shown by `help`.
    fn about(&self) -> &'static str;

    /// Whether this command accepts arguments after its name. Defaults to no arguments.
    fn accepts_args(&self) -> bool {
        false
    }

    /// Executes the command with the remaining arguments.
    fn run<'a>(&'a self, app: &'a App, args: Vec<String>) -> BoxFuture<'a, Result<()>>;
}

/// Lists builded commands.
pub(crate) struct HelpCommand;

impl Command for HelpCommand {
    fn name(&self) -> &'static str {
        "help"
    }

    fn about(&self) -> &'static str {
        "List available commands"
    }

    fn run<'a>(&'a self, app: &'a App, _args: Vec<String>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let mut commands: Vec<&dyn Command> = app.commands().collect();
            commands.sort_by_key(|c| c.name());
            let width = commands.iter().map(|c| c.name().len()).max().unwrap_or(0);
            let mut out = format!("{} commands:\n", app.config().app.name);
            for command in commands {
                // Writing into a String cannot fail.
                let _ = writeln!(out, "  {:<width$}  {}", command.name(), command.about());
            }
            print(&out);
            Ok(())
        })
    }
}

/// Prints the merged configuration with secrets redacted.
pub(crate) struct ConfigShowCommand;

impl Command for ConfigShowCommand {
    fn name(&self) -> &'static str {
        "config show"
    }

    fn about(&self) -> &'static str {
        "Print the effective configuration with secrets redacted"
    }

    fn run<'a>(&'a self, app: &'a App, _args: Vec<String>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let mut value: serde_json::Value = app
                .figment()
                .extract()
                .map_err(crate::config::ConfigError::from)?;
            crate::config::redact_secrets(&mut value);
            let rendered = toml::to_string_pretty(&value)
                .unwrap_or_else(|_| serde_json::to_string_pretty(&value).unwrap_or_default());
            print(&rendered);
            Ok(())
        })
    }
}

// Commands are the one place where writing to stdout is the intended behaviour.
#[allow(clippy::print_stdout)]
fn print(text: &str) {
    println!("{text}");
}
