mod generic;

use super::{
    basename, color_cell, job_color, limit, more, single_line, status_color, text, unit,
    write_rendered_row, write_row,
};
use crate::{EntryResult, EntryType};
use openrust::{console::Output, support::php_value_truthy};
use serde_json::Value;

pub(super) fn heading(entry: &EntryResult) -> String {
    let c = &entry.content;
    match entry.r#type {
        EntryType::Request => format!(
            "Request: {} {} -> {}",
            text(c, "method"),
            text(c, "uri"),
            text(c, "response_status")
        ),
        EntryType::Exception => format!("Exception: {}", basename(&text(c, "class"))),
        EntryType::Job => format!(
            "Job: {} [{}]",
            basename(&text(c, "name")),
            text_or(c, "status", "pending")
        ),
        _ => generic::heading(entry),
    }
}

pub(super) fn render(output: &mut Output, entry: &EntryResult, full: bool) {
    let c = &entry.content;
    details(output, [("Hostname", text(c, "hostname"))]);
    match entry.r#type {
        EntryType::Request => request(output, c, full),
        EntryType::Exception => exception(output, c),
        EntryType::Job => job(output, c, full),
        _ => generic::render(output, entry, full),
    }
}

fn request(output: &mut Output, c: &Value, full: bool) {
    let middleware = match &c["middleware"] {
        Value::Array(entries) => entries
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(", "),
        Value::String(value) => value.clone(),
        _ => String::new(),
    };
    let user = &c["user"];
    let mut identity = Vec::new();
    if php_value_truthy(&user["name"]) {
        identity.push(text(user, "name"));
    }
    if !user["email"].is_null() {
        identity.push(format!("({})", text(user, "email")));
    }
    if !user["id"].is_null() {
        identity.push(format!("#{}", text(user, "id")));
    }
    details(
        output,
        [
            (
                "Controller",
                c["controller_action"]
                    .as_str()
                    .unwrap_or("Closure")
                    .to_owned(),
            ),
            ("Duration", unit(c, "duration", "ms")),
            ("Memory", unit(c, "memory", "MB")),
            ("IP", text(c, "ip_address")),
            ("Middleware", middleware),
            ("User", identity.join(" ")),
        ],
    );
    block(output, "Payload", &c["payload"], 500, full);
    block(output, "Response", &c["response"], 1000, full);
}

fn exception(output: &mut Output, c: &Value) {
    details(
        output,
        [
            ("Class", text(c, "class")),
            ("File", format!("{}:{}", text(c, "file"), text(c, "line"))),
            ("Occurrences", text_or(c, "occurrences", "1")),
        ],
    );
    let resolved = if c["resolved_at"].is_null() {
        color_cell("No", "yellow", output.is_decorated())
    } else {
        single_line(&text(c, "resolved_at"))
    };
    detail(output, "Resolved", &resolved);
    output.error(&single_line(&text(c, "message")));
    if let Some(preview) = c["line_preview"]
        .as_object()
        .filter(|lines| !lines.is_empty())
    {
        output.info("Code Context");
        for (number, line) in preview {
            code_line(output, number, line.as_str().unwrap_or_default(), c);
        }
    } else if let Some(preview) = c["line_preview"]
        .as_array()
        .filter(|lines| !lines.is_empty())
    {
        output.info("Code Context");
        for (number, line) in preview.iter().enumerate() {
            code_line(
                output,
                &number.to_string(),
                line.as_str().unwrap_or_default(),
                c,
            );
        }
    }
    trace(output, &c["trace"], 15);
}

fn code_line(output: &mut Output, number: &str, line: &str, content: &Value) {
    let number = if number == text(content, "line") {
        color_cell(
            &format!("{} >", single_line(number)),
            "red",
            output.is_decorated(),
        )
    } else {
        single_line(number)
    };
    write_rendered_row(output, [&number, &single_line(line)]);
}

fn job(output: &mut Output, c: &Value, full: bool) {
    let status = single_line(&text_or(c, "status", "pending"));
    let status = color_cell(&status, job_color(&status), output.is_decorated());
    detail(output, "Status", &status);
    details(
        output,
        [
            ("Queue", text(c, "queue")),
            ("Connection", text(c, "connection")),
            ("Tries", text(c, "tries")),
            ("Timeout", unit(c, "timeout", "s")),
        ],
    );
    block(output, "Data", &c["data"], 500, full);
    let failure = &c["exception"];
    if php_value_truthy(failure) {
        output.error(&single_line(&text(failure, "message")));
        trace(output, &failure["trace"], 10);
    }
}

fn text_or(c: &Value, key: &str, default: &str) -> String {
    if c[key].is_null() {
        default.to_owned()
    } else {
        text(c, key)
    }
}

fn trace(output: &mut Output, value: &Value, max: usize) {
    let Some(frames) = value.as_array().filter(|frames| !frames.is_empty()) else {
        return;
    };
    output.info("Stack Trace");
    for frame in frames.iter().take(max) {
        let file = text_or(frame, "file", "?");
        let line = text_or(frame, "line", "?");
        write_row(output, [&format!("{file}:{line}")]);
    }
    more(output, frames.len(), max, "frames");
}

fn details<const N: usize>(output: &mut Output, fields: [(&str, String); N]) {
    for (label, value) in fields {
        detail(output, label, &single_line(&value));
    }
}

fn detail(output: &mut Output, label: &str, rendered: &str) {
    if !rendered.is_empty() {
        write_rendered_row(output, [label, rendered]);
    }
}

fn block(output: &mut Output, label: &str, value: &Value, max: usize, full: bool) {
    if !php_value_truthy(value) {
        return;
    }
    output.info(label);
    let value = value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| serde_json::to_string_pretty(value).expect("JSON value"));
    let value = limit(&value, max, full);
    for line in value.split('\n') {
        write_row(output, [line]);
    }
}
