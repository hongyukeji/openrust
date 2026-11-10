use clap::{Arg, ArgMatches, Command};

use super::{ConsoleError, Output};

pub(super) fn command() -> Command {
    Command::new("help").about("Print help for a command").arg(
        Arg::new("command_name")
            .help("The command to describe")
            .default_value("help"),
    )
}

pub(super) fn run(
    console: &Command,
    options: &ArgMatches,
    output: &mut Output,
) -> Result<i32, ConsoleError> {
    let name = options
        .get_one::<String>("command_name")
        .expect("help target has a default");
    let target = console
        .find_subcommand(name)
        .ok_or_else(|| ConsoleError::new(format!("Command '{name}' is not defined.")))?;
    let help = target
        .clone()
        .bin_name(format!("artisan {}", target.get_name()))
        .render_help();
    output.stdout.push_str(&help.to_string());
    Ok(0)
}
