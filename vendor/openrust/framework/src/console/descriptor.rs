use std::fmt::Write as _;

use clap::{Arg, ArgAction, Command};
use quick_xml::escape::escape;
use serde_json::{Map, Value, json};

use super::localization::{description, headings, text};

pub(super) fn usage(command: &Command) -> String {
    command.clone().render_usage().to_string()
}

pub(super) fn help(command: &Command, long: bool) -> String {
    let title = description(command);
    let name = command.get_name();
    let mut order: Vec<_> = command
        .get_arguments()
        .map(|argument| argument.get_id().as_str())
        .collect();
    order.sort_unstable();
    let mut command = command.clone().about(title).mut_args(|argument| {
        let help = argument_description(name, &argument);
        let index = order
            .binary_search(&argument.get_id().as_str())
            .expect("known argument");
        let argument = argument.display_order(index);
        if help.is_empty() {
            argument
        } else {
            argument.help(help)
        }
    });
    let rendered = if long {
        command.render_long_help()
    } else {
        command.render_help()
    };
    headings(&rendered.to_string())
}

pub(super) fn json_command(command: &Command, name: &str, implemented: bool, short: bool) -> Value {
    let mut result = json!({
        "name": name,
        "description": description(command),
        "aliases": command.get_visible_aliases().collect::<Vec<_>>(),
        "implemented": implemented,
        "hidden": command.is_hide_set(),
    });
    if !short {
        result["usage"] = json!(usage(command));
        result["help"] = json!(help(command, true));
        let (mut arguments, mut options) = (Map::new(), Map::new());
        for argument in command
            .get_arguments()
            .filter(|argument| !argument.is_hide_set())
        {
            let key = argument.get_id().as_str();
            let description = argument_description(command.get_name(), argument);
            let values: Vec<_> = argument
                .get_default_values()
                .iter()
                .map(|value| value.to_string_lossy())
                .collect();
            let range = argument.get_num_args();
            let multiple = range.is_some_and(|range| range.max_values() > 1)
                || matches!(argument.get_action(), ArgAction::Append);
            let default = match (values.as_slice(), multiple, argument.get_action()) {
                ([], _, ArgAction::SetTrue) => json!(false),
                ([], _, ArgAction::SetFalse) => json!(true),
                ([], _, ArgAction::Count) => json!(0),
                (_, true, _) => json!(values),
                ([value], _, _) => json!(value),
                _ => Value::Null,
            };
            if argument.get_index().is_some() {
                arguments.insert(key.into(), json!({
                    "name": key, "is_required": argument.is_required_set(), "is_array": multiple,
                    "description": description, "default": default,
                }));
            } else {
                options.insert(key.into(), json!({
                    "name": option_name(argument), "shortcut": argument.get_short().map(|short| format!("-{short}")).unwrap_or_default(),
                    "accept_value": argument.get_action().takes_values(),
                    "is_value_required": argument.get_action().takes_values() && range.is_none_or(|range| range.min_values() > 0),
                    "is_multiple": multiple, "description": description, "default": default,
                }));
            }
        }
        result["definition"] = json!({"arguments": arguments, "options": options});
    }
    result
}

fn argument_description(command: &str, argument: &Arg) -> String {
    let fallback = argument
        .get_help()
        .map(ToString::to_string)
        .unwrap_or_default();
    let key = argument.get_id().as_str();
    let (simplified, traditional) = match key {
        "help" => ("显示帮助", "顯示說明"),
        "version" => ("显示版本", "顯示版本"),
        "format" if matches!(command, "help" | "list") => ("选择输出格式", "選擇輸出格式"),
        "command_name" if command == "help" => ("要查看的命令", "要查看的命令"),
        "namespace" if command == "list" => ("只显示指定命令组", "只顯示指定命令群組"),
        _ => (fallback.as_str(), fallback.as_str()),
    };
    text(
        &format!("commands.{command}.arguments.{key}"),
        &fallback,
        simplified,
        traditional,
    )
}

fn option_name(argument: &Arg) -> String {
    if let Some(long) = argument.get_long() {
        format!("--{long}")
    } else {
        argument
            .get_short()
            .map(|short| format!("-{short}"))
            .unwrap_or_default()
    }
}

pub(super) fn write_xml(
    output: &mut String,
    command: &Command,
    name: &str,
    implemented: bool,
    short: bool,
) {
    let metadata = json_command(command, name, implemented, short);
    writeln!(
        output,
        "<command name=\"{}\" implemented=\"{implemented}\">",
        escape(name)
    )
    .unwrap();
    writeln!(
        output,
        "  <description>{}</description>",
        escape(description(command))
    )
    .unwrap();
    for alias in command.get_visible_aliases() {
        writeln!(output, "  <alias>{}</alias>", escape(alias)).unwrap();
    }
    if !short {
        writeln!(output, "  <usage>{}</usage>", escape(usage(command))).unwrap();
        for (group, node) in [("arguments", "argument"), ("options", "option")] {
            writeln!(output, "  <{group}>").unwrap();
            for (id, value) in metadata["definition"][group]
                .as_object()
                .expect("input definition")
            {
                writeln!(
                    output,
                    "    <{node} id=\"{}\" name=\"{}\">",
                    escape(id),
                    escape(value["name"].as_str().unwrap_or_default())
                )
                .unwrap();
                writeln!(
                    output,
                    "      <description>{}</description>",
                    escape(value["description"].as_str().unwrap_or_default())
                )
                .unwrap();
                writeln!(
                    output,
                    "      <default>{}</default>",
                    escape(value["default"].to_string())
                )
                .unwrap();
                writeln!(output, "    </{node}>").unwrap();
            }
            writeln!(output, "  </{group}>").unwrap();
        }
    }
    writeln!(output, "</command>").unwrap();
}

pub(super) fn write_markdown(output: &mut String, command: &Command) {
    writeln!(
        output,
        "# {}\n\n{}",
        command.get_name(),
        description(command)
    )
    .unwrap();
    writeln!(
        output,
        "\n## {}\n\n```text\n{}\n```",
        text("usage_title", "Usage", "用法", "用法"),
        help(command, true).trim_end(),
    )
    .unwrap();
}
