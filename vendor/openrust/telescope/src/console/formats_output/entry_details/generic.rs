use super::{
    basename, block, color_cell, detail, details, single_line, status_color, text, unit, write_row,
};
use crate::{EntryResult, EntryType};
use openrust::{
    console::Output,
    support::{php_integer_cast, php_value_truthy},
};
use serde_json::Value;

pub(super) fn heading(entry: &EntryResult) -> String {
    let c = &entry.content;
    let (label, subtitle) = match entry.r#type {
        EntryType::Query => ("Query", String::new()),
        EntryType::Cache => ("Cache", text(c, "type")),
        EntryType::Log => ("Log", text(c, "level")),
        EntryType::Mail => ("Mail", text(c, "subject")),
        EntryType::Event => ("Event", text(c, "name")),
        EntryType::Command => ("Command", text(c, "command")),
        EntryType::ScheduledTask => ("Scheduled Task", text(c, "command")),
        EntryType::ClientRequest => (
            "Client Request",
            format!("{} {}", text(c, "method"), text(c, "uri")),
        ),
        EntryType::Gate => (
            "Gate",
            format!("{}: {}", text(c, "ability"), text(c, "result")),
        ),
        EntryType::Model => (
            "Model",
            format!("{}: {}", text(c, "model"), text(c, "action")),
        ),
        EntryType::Notification => (
            "Notification",
            basename(&text(c, "notification")).to_owned(),
        ),
        EntryType::View => ("View", text(c, "name")),
        EntryType::Redis => ("Redis", String::new()),
        EntryType::Dump => ("Dump", String::new()),
        EntryType::Batch => ("Batch", String::new()),
        _ => unreachable!(),
    };
    if subtitle.is_empty() {
        label.to_owned()
    } else {
        format!("{label}: {subtitle}")
    }
}

pub(super) fn render(output: &mut Output, entry: &EntryResult, full: bool) {
    let c = &entry.content;
    let t = |key| text(c, key);
    match entry.r#type {
        EntryType::Query => {
            let mut duration = single_line(&unit(c, "time", "ms"));
            if php_value_truthy(&c["slow"]) {
                duration.push_str("  ");
                duration.push_str(&color_cell("SLOW", "red", output.is_decorated()));
            }
            details(output, [("Connection", t("connection"))]);
            detail(output, "Duration", &duration);
            details(output, [("Source", source(c)), ("SQL", t("sql"))]);
            block(output, "Bindings", &c["bindings"], 500, full);
        }
        EntryType::Cache => {
            details(
                output,
                [
                    ("Key", t("key")),
                    ("Expiration", unit(c, "expiration", "s")),
                ],
            );
            block(output, "Value", &c["value"], 500, full);
        }
        EntryType::Log => {
            details(output, [("Message", t("message"))]);
            block(output, "Context", &c["context"], 500, full);
        }
        EntryType::Mail => {
            details(
                output,
                [
                    ("Mailable", t("mailable")),
                    ("Subject", t("subject")),
                    ("Queued", yes(&c["queued"])),
                    ("To", addresses(&c["to"])),
                    ("From", addresses(&c["from"])),
                ],
            );
        }
        EntryType::Event => {
            details(output, [("Broadcast", yes(&c["broadcast"]))]);
            listing(output, "Listeners", &c["listeners"], |listener| {
                let name = text(listener, "name");
                if php_value_truthy(&listener["queued"]) {
                    format!("{name} (queued)")
                } else {
                    name
                }
            });
            block(output, "Payload", &c["payload"], 500, full);
        }
        EntryType::Command => {
            details(output, [("Exit Code", t("exit_code"))]);
            block(output, "Arguments", &c["arguments"], 500, full);
            block(output, "Options", &c["options"], 500, full);
        }
        EntryType::ScheduledTask => {
            details(
                output,
                [
                    ("Expression", t("expression")),
                    ("Timezone", t("timezone")),
                    ("Description", t("description")),
                ],
            );
            block(output, "Output", &c["output"], 500, full);
        }
        EntryType::ClientRequest => {
            let status = if c["response_status"].is_null() {
                "N/A".to_owned()
            } else {
                let status = php_integer_cast(&c["response_status"]);
                color_cell(
                    &status.to_string(),
                    status_color(status),
                    output.is_decorated(),
                )
            };
            detail(output, "Status", &status);
            details(output, [("Duration", unit(c, "duration", "ms"))]);
            block(output, "Payload", &c["payload"], 500, full);
            block(output, "Response", &c["response"], 500, full);
        }
        EntryType::Gate => {
            details(output, [("Source", source(c))]);
            block(output, "Arguments", &c["arguments"], 500, full);
        }
        EntryType::Model => {
            details(output, [("Count", t("count"))]);
            block(output, "Changes", &c["changes"], 500, full);
        }
        EntryType::Notification => {
            details(
                output,
                [
                    ("Channel", t("channel")),
                    ("Notifiable", t("notifiable")),
                    ("Queued", yes(&c["queued"])),
                ],
            );
        }
        EntryType::View => {
            details(output, [("Path", t("path"))]);
            listing(output, "Composers", &c["composers"], |composer| {
                let name = text(composer, "name");
                if composer["type"].is_null() {
                    name
                } else {
                    format!("{name} ({})", text(composer, "type"))
                }
            });
            block(output, "Data", &c["data"], 500, full);
        }
        EntryType::Redis => details(
            output,
            [
                ("Connection", t("connection")),
                ("Duration", unit(c, "time", "ms")),
                ("Command", t("command")),
            ],
        ),
        EntryType::Dump | EntryType::Batch => block(output, "Content", c, 1000, full),
        _ => unreachable!(),
    }
}

fn source(c: &Value) -> String {
    if c["file"].is_null() {
        String::new()
    } else {
        format!("{}:{}", text(c, "file"), text(c, "line"))
    }
}

fn yes(value: &Value) -> String {
    if php_value_truthy(value) {
        "Yes".into()
    } else {
        String::new()
    }
}

fn listing(output: &mut Output, label: &str, value: &Value, describe: impl Fn(&Value) -> String) {
    let Some(items) = value.as_array().filter(|items| !items.is_empty()) else {
        return;
    };
    output.info(label);
    for item in items {
        write_row(output, [&describe(item)]);
    }
}

fn addresses(value: &Value) -> String {
    value
        .as_object()
        .into_iter()
        .flatten()
        .map(|(email, name)| {
            if php_value_truthy(name) {
                format!("{} <{email}>", name.as_str().unwrap_or_default())
            } else {
                email.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}
