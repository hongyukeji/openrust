use crate::{File, Options, ProcessFactory, ProcessOutcome};
use clap::{Arg, Command as Syntax};
use openrust::{
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    contracts::foundation::Application as _,
    foundation::Application,
};
use std::{fs, io, path::Path};

/// Tails the application logs through an owned capture session.
pub struct PailCommand {
    definition: CommandDefinition,
}
impl Default for PailCommand {
    fn default() -> Self {
        let mut syntax = Syntax::new("pail").about("Tails the application logs.");
        for (name, help) in [
            ("filter", "Filter the logs by the given value"),
            ("message", "Filter the logs by the given message"),
            ("level", "Filter the logs by the given level"),
            ("auth", "Filter the logs by the given authenticated ID"),
            (
                "user",
                "Filter the logs by the given authenticated ID (alias for --auth)",
            ),
        ] {
            syntax = syntax.arg(Arg::new(name).long(name).help(help));
        }
        syntax = syntax.arg(
            Arg::new("timeout")
                .long("timeout")
                .default_value("3600")
                .allow_hyphen_values(true)
                .help("The maximum execution time in seconds"),
        );
        Self {
            definition: CommandDefinition::new(syntax),
        }
    }
}
impl Command for PailCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.definition
    }
    fn handle<'a>(
        &'a self,
        app: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            banner(output)?;
            let session = Session::create(&app.storage_path("pail")).map_err(error)?;
            let options = Options::fromCommand(input)?;
            let result = ProcessFactory::default()
                .with_shutdown(input.shutdown().cloned())
                .run(&session.0, output, app.base_path(), &options)
                .await;
            session.0.destroy().map_err(error)?;
            match result? {
                ProcessOutcome::Interrupted => output.write_raw("\n").map_err(error)?,
                ProcessOutcome::TimedOut => {
                    let label = label(output);
                    output
                        .write_raw(&format!("\n  {label} Maximum execution time exceeded.\n\n"))
                        .map_err(error)?;
                }
            }
            Ok(0)
        })
    }
}
fn error(error: impl std::fmt::Display) -> ConsoleError {
    ConsoleError::new(error.to_string())
}
fn label(output: &Output) -> &'static str {
    if output.is_decorated() {
        "\x1b[44;37m INFO \x1b[0m"
    } else {
        " INFO "
    }
}
fn banner(output: &mut Output) -> Result<(), ConsoleError> {
    let width = std::env::var("COLUMNS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or_else(|| usize::from(console::Term::stdout().size().1))
        .min(150)
        .saturating_sub(4);
    let hint = "Press Ctrl+C to exit";
    let details = "Use -v|-vv to show more details";
    let color = |value: &str| {
        if output.is_decorated() {
            format!("\x1b[90m{value}\x1b[0m")
        } else {
            value.into()
        }
    };
    let banner = format!(
        "\n  {} Tailing application logs.{}{}\n  {}{}\n",
        label(output),
        " ".repeat(width.saturating_sub(31 + hint.len()).max(1)),
        color(hint),
        " ".repeat(width.saturating_sub(details.len())),
        color(details)
    );
    output.write_raw(&banner).map_err(error)
}
struct Session(File);
impl Session {
    fn create(directory: &Path) -> io::Result<Self> {
        // Reserve exclusively: File::create deliberately tolerates existing files.
        let directory_created = !directory.exists();
        fs::create_dir_all(directory)?;
        if directory_created {
            use std::io::Write;
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(directory.join(".gitignore"))
            {
                Ok(mut file) => file.write_all(b"*\n!.gitignore\n")?,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        loop {
            let path = directory.join(format!("{:032x}.pail", fastrand::u128(..)));
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(_) => return Ok(Self(File::new(path))),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.0.destroy();
    }
}
