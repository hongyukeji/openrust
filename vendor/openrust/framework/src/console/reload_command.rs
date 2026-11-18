use std::{collections::HashSet, fmt::Write as _};

use clap::{Arg, ArgMatches, Command};

use crate::foundation::Application as FoundationApplication;

use super::{Application, ConsoleError, Output};

pub(super) fn command() -> Command {
    Command::new("reload").about("Reload running services").arg(
        Arg::new("except")
            .short('e')
            .long("except")
            .help("Comma-separated reload task names or commands to skip"),
    )
}

pub(super) async fn run(
    console: &Application,
    app: &FoundationApplication,
    matches: &ArgMatches,
    output: &mut Output,
) -> Result<i32, ConsoleError> {
    let except: HashSet<_> = matches
        .get_one::<String>("except")
        .into_iter()
        .flat_map(|value| value.split(',').map(str::trim))
        .filter(|name| !name.is_empty())
        .collect();

    output.info("Reloading services.");
    for (name, command) in app.reload_commands() {
        if except.contains(name.as_str()) || except.contains(command.as_str()) {
            continue;
        }
        let status = if console.call_reload_target(app, &command).await? == 0 {
            "DONE"
        } else {
            "FAIL"
        };
        writeln!(output.stdout, "  {name} ... {status}").expect("writing to String");
    }
    output.stdout.push('\n');
    Ok(0)
}
