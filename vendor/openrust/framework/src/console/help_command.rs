use clap::{Arg, ArgAction, ArgMatches, Command};

use super::{Application, ConsoleError, Output, descriptor};

pub(super) fn command() -> Command {
    Command::new("help")
        .about("Print help for a command")
        .arg(
            Arg::new("command_name")
                .help("The command to describe")
                .default_value("help"),
        )
        .arg(
            Arg::new("format")
                .long("format")
                .value_parser(["txt", "json", "xml", "md"])
                .default_value("txt")
                .help("Choose an output format"),
        )
        .arg(
            Arg::new("raw")
                .long("raw")
                .action(ArgAction::SetTrue)
                .help("Print plain command help"),
        )
}

pub(super) fn run(
    registry: &Application,
    console: &Command,
    options: &ArgMatches,
    output: &mut Output,
) -> Result<i32, ConsoleError> {
    let name = options
        .get_one::<String>("command_name")
        .expect("help target has a default");
    let canonical = registry.resolve(name)?;
    let target = console
        .find_subcommand(canonical)
        .expect("resolved command is registered");
    let format = options
        .get_one::<String>("format")
        .expect("default output format");
    let target = target.clone().bin_name(format!("artisan {canonical}"));
    match format.as_str() {
        "json" => {
            let value = descriptor::json_command(
                &target,
                canonical,
                registry.is_implemented(canonical),
                false,
            );
            output.stdout.push_str(&value.to_string());
            output.stdout.push('\n');
        }
        "xml" => descriptor::write_xml(
            &mut output.stdout,
            &target,
            canonical,
            registry.is_implemented(canonical),
            false,
        ),
        "md" => descriptor::write_markdown(&mut output.stdout, &target),
        _ => output
            .stdout
            .push_str(&descriptor::help(&target, !options.get_flag("raw"))),
    }
    Ok(0)
}
