use std::{collections::BTreeMap, fmt::Write as _};

use clap::{Arg, ArgAction, ArgMatches, Command};
use quick_xml::escape::escape;
use serde_json::json;

use super::{Application, Output};

pub(super) fn command() -> Command {
    Command::new("list")
        .about("List available commands")
        .arg(Arg::new("namespace").help("Show commands in a specific namespace"))
        .arg(
            Arg::new("raw")
                .long("raw")
                .help("Print a flat command list")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("format")
                .long("format")
                .value_parser(["txt", "json", "xml", "md"])
                .default_value("txt")
                .help("Choose an output format"),
        )
        .arg(
            Arg::new("short")
                .long("short")
                .help("Omit command usage from structured output")
                .action(ArgAction::SetTrue),
        )
}

pub(super) fn run(
    registry: &Application,
    console: &Command,
    options: Option<&ArgMatches>,
    output: &mut Output,
) -> i32 {
    let filter = options.and_then(|args| args.get_one::<String>("namespace").map(String::as_str));
    let raw = options.is_some_and(|args| args.get_flag("raw"));
    let short = options.is_some_and(|args| args.get_flag("short"));
    let format = options
        .and_then(|args| args.get_one::<String>("format").map(String::as_str))
        .unwrap_or("txt");

    let mut groups = BTreeMap::<&str, BTreeMap<&str, &Command>>::new();
    for command in console.get_subcommands().filter(|cmd| !cmd.is_hide_set()) {
        let name = command.get_name();
        let group = namespace(name);
        match filter {
            None => {
                groups.entry(group).or_default().insert(name, command);
            }
            Some(selected) if selected == group => {
                groups.entry(group).or_default().insert(name, command);
            }
            Some(selected) => {
                for alias in command
                    .get_visible_aliases()
                    .filter(|alias| namespace(alias) == selected)
                {
                    groups.entry(selected).or_default().insert(alias, command);
                }
            }
        }
    }
    if groups.is_empty()
        && let Some(name) = filter
    {
        writeln!(output.stderr, "Namespace '{name}' is not defined.").expect("writing to String");
        return 1;
    }

    match format {
        "json" => {
            let commands: Vec<_> = groups
                .values()
                .flatten()
                .map(|(&name, &cmd)| {
                    let mut entry = json!({
                        "name": name,
                        "description": description(cmd),
                        "aliases": cmd.get_visible_aliases().collect::<Vec<_>>(),
                        "implemented": registry.is_implemented(cmd.get_name()),
                    });
                    if !short {
                        entry["usage"] = json!(usage(cmd));
                    }
                    entry
                })
                .collect();
            let namespaces: Vec<_> = groups.keys().filter(|name| !name.is_empty()).collect();
            writeln!(
                output.stdout,
                "{}",
                json!({ "commands": commands, "namespaces": namespaces })
            )
            .expect("writing to String");
        }
        "xml" => {
            writeln!(output.stdout, "<commands>").expect("writing to String");
            for (&name, &cmd) in groups.values().flatten() {
                writeln!(
                    output.stdout,
                    "  <command name=\"{}\" implemented=\"{}\">",
                    escape(name),
                    registry.is_implemented(cmd.get_name())
                )
                .expect("writing to String");
                writeln!(
                    output.stdout,
                    "    <description>{}</description>",
                    escape(description(cmd))
                )
                .expect("writing to String");
                for alias in cmd.get_visible_aliases() {
                    writeln!(output.stdout, "    <alias>{}</alias>", escape(alias))
                        .expect("writing to String");
                }
                if !short {
                    writeln!(output.stdout, "    <usage>{}</usage>", escape(usage(cmd)))
                        .expect("writing to String");
                }
                writeln!(output.stdout, "  </command>").expect("writing to String");
            }
            writeln!(output.stdout, "</commands>").expect("writing to String");
        }
        "md" => {
            writeln!(output.stdout, "# Available commands").expect("writing to String");
            for (group, entries) in groups {
                writeln!(
                    output.stdout,
                    "\n## {}",
                    if group.is_empty() { "General" } else { group }
                )
                .expect("writing to String");
                for (name, cmd) in entries {
                    writeln!(
                        output.stdout,
                        "- `{name}`{} — {}",
                        status(registry, cmd),
                        description(cmd)
                    )
                    .expect("writing to String");
                    if !short {
                        writeln!(output.stdout, "  `{}`", usage(cmd)).expect("writing to String");
                    }
                }
            }
        }
        _ => {
            if !raw {
                if let Some(name) = filter {
                    writeln!(
                        output.stdout,
                        "Available commands for the '{name}' namespace:"
                    )
                    .expect("writing to String");
                } else {
                    writeln!(
                        output.stdout,
                        "Usage:\n  command [options] [arguments]\n\nAvailable commands:"
                    )
                    .expect("writing to String");
                }
            }
            let width = groups
                .values()
                .flat_map(|entries| entries.keys())
                .map(|name| name.len())
                .max()
                .unwrap_or(0);
            for (group, entries) in groups {
                if !raw && filter.is_none() && !group.is_empty() {
                    writeln!(output.stdout, "\n {group}").expect("writing to String");
                }
                for (name, cmd) in entries {
                    let indent = if raw { "" } else { "  " };
                    let aliases = cmd
                        .get_visible_aliases()
                        .filter(|&alias| alias != name)
                        .collect::<Vec<_>>();
                    let alias_label = if raw || aliases.is_empty() {
                        String::new()
                    } else {
                        format!("[{}] ", aliases.join(", "))
                    };
                    writeln!(
                        output.stdout,
                        "{indent}{name:<width$}  {alias_label}{}{status}",
                        description(cmd),
                        status = status(registry, cmd)
                    )
                    .expect("writing to String");
                }
            }
        }
    }
    0
}

fn namespace(name: &str) -> &str {
    name.split_once(':').map_or("", |(namespace, _)| namespace)
}

fn description(command: &Command) -> String {
    command
        .get_about()
        .map(ToString::to_string)
        .unwrap_or_default()
}

fn status(registry: &Application, command: &Command) -> &'static str {
    if registry.is_implemented(command.get_name()) {
        ""
    } else {
        " [Planned]"
    }
}

fn usage(command: &Command) -> String {
    command.clone().render_usage().to_string()
}
