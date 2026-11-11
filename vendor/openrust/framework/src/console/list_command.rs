use std::{collections::BTreeMap, fmt::Write as _};

use clap::{Arg, ArgAction, ArgMatches, Command};
use serde_json::json;

use super::{
    Application, Output, descriptor,
    localization::{description, text},
};

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
        let message = text(
            "namespace_missing",
            "Namespace ':name' is not defined.",
            "命令组「:name」不存在。",
            "命令群組「:name」不存在。",
        )
        .replace(":name", name);
        writeln!(output.stderr, "{message}").expect("writing to String");
        return 1;
    }

    match format {
        "json" => {
            let commands: Vec<_> = groups
                .values()
                .flatten()
                .map(|(&name, &cmd)| {
                    descriptor::json_command(
                        cmd,
                        name,
                        registry.is_implemented(cmd.get_name()),
                        short,
                    )
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
                descriptor::write_xml(
                    &mut output.stdout,
                    cmd,
                    name,
                    registry.is_implemented(cmd.get_name()),
                    short,
                );
            }
            writeln!(output.stdout, "</commands>").expect("writing to String");
        }
        "md" => {
            let title = text(
                "available_commands",
                "Available commands",
                "可用命令",
                "可用命令",
            );
            writeln!(output.stdout, "# {title}").expect("writing to String");
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
                        writeln!(output.stdout, "  `{}`", descriptor::usage(cmd))
                            .expect("writing to String");
                    }
                }
            }
        }
        _ => {
            if !raw {
                if let Some(name) = filter {
                    let title = text(
                        "namespace_commands",
                        "Available commands for the ':name' namespace:",
                        "「:name」命令组的可用命令：",
                        "「:name」命令群組的可用命令：",
                    )
                    .replace(":name", name);
                    writeln!(output.stdout, "{title}").expect("writing to String");
                } else {
                    let usage = text("usage", "Usage:", "用法：", "用法：");
                    let available = text(
                        "available_commands_heading",
                        "Available commands:",
                        "可用命令：",
                        "可用命令：",
                    );
                    writeln!(
                        output.stdout,
                        "{usage}\n  command [options] [arguments]\n\n{available}"
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

fn status(registry: &Application, command: &Command) -> &'static str {
    if registry.is_implemented(command.get_name()) {
        ""
    } else {
        " [Planned]"
    }
}
