use std::{collections::HashSet, fmt::Write as _};

use clap::{Arg, ArgMatches, Command};

use crate::foundation::Application as FoundationApplication;

use super::{Application, ConsoleError, Output};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskCommand {
    Reload,
    Optimize,
    OptimizeClear,
}

impl TaskCommand {
    pub(super) fn command(self) -> Command {
        let (name, description) = match self {
            Self::Reload => ("reload", "Reload running services"),
            Self::Optimize => ("optimize", "Cache application resources for deployment"),
            Self::OptimizeClear => ("optimize:clear", "Clear cached application resources"),
        };
        Command::new(name).about(description).arg(
            Arg::new("except")
                .short('e')
                .long("except")
                .help("Comma-separated task names or commands to skip"),
        )
    }

    pub(crate) fn defaults(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Reload => &[
                ("queue", "queue:restart"),
                ("schedule", "schedule:interrupt"),
            ],
            Self::Optimize => &[
                ("config", "config:cache"),
                ("events", "event:cache"),
                ("views", "view:cache"),
            ],
            Self::OptimizeClear => &[
                ("config", "config:clear"),
                ("cache", "cache:clear"),
                ("compiled", "clear-compiled"),
                ("events", "event:clear"),
                ("views", "view:clear"),
            ],
        }
    }

    fn heading(self) -> &'static str {
        match self {
            Self::Reload => "Reloading services.",
            Self::Optimize => "Caching framework bootstrap, configuration, and metadata.",
            Self::OptimizeClear => "Clearing cached bootstrap files.",
        }
    }
}

pub(super) async fn run(
    console: &Application,
    app: &FoundationApplication,
    syntax: &Command,
    operation: TaskCommand,
    matches: &ArgMatches,
    output: &mut Output,
) -> Result<i32, ConsoleError> {
    let except: HashSet<_> = matches
        .get_one::<String>("except")
        .into_iter()
        .flat_map(|value| value.split(',').map(str::trim))
        .filter(|name| !name.is_empty())
        .collect();

    let environment = matches
        .try_get_many::<String>("env")
        .ok()
        .flatten()
        .and_then(|mut values| values.next());
    output.info(operation.heading());
    for (name, command) in app.console_tasks(operation) {
        if except.contains(name.as_str()) || except.contains(command.as_str()) {
            continue;
        }
        let status = if console
            .call_task(app, syntax, &command, environment)
            .await?
            == 0
        {
            "DONE"
        } else {
            "FAIL"
        };
        writeln!(output.stdout, "  {name} ... {status}").expect("writing to String");
    }
    output.stdout.push('\n');
    Ok(0)
}
