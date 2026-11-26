use std::{env, fmt::Write as _};

use ::console::{Term, measure_text_width};
use serde_json::{Value, json};

use crate::console::Output;

use super::Event;

const GRAY: &str = "\x1b[38;2;108;114;128m";
const YELLOW: &str = "\x1b[33m";
const RESET_COLOR: &str = "\x1b[39m";

pub(super) struct ListedEvent<'a> {
    pub(super) event: &'a Event,
    pub(super) expressions: Vec<String>,
    pub(super) command: String,
    pub(super) arguments_start: Option<usize>,
    pub(super) next_due_date: String,
    pub(super) next_due_date_human: String,
    pub(super) timezone: &'a str,
    pub(super) has_mutex: bool,
}

impl ListedEvent<'_> {
    fn repeat_seconds(&self) -> Option<u64> {
        self.event.interval.map(|duration| duration.as_secs())
    }
}

pub(super) fn display_json(events: &[ListedEvent<'_>], output: &mut Output) {
    let rows = events
        .iter()
        .flat_map(|row| {
            row.expressions.iter().map(|expression| {
                json!({
                    "expression": expression,
                    "command": row.command,
                    "description": row.event.description,
                    "next_due_date": row.next_due_date,
                    "next_due_date_human": row.next_due_date_human,
                    "timezone": row.timezone,
                    "has_mutex": row.has_mutex,
                    "repeat_seconds": row.repeat_seconds(),
                    "environments": row.event.environments,
                    "on_one_server": row.event.one_server,
                })
            })
        })
        .collect();
    writeln!(output.stdout, "{}", Value::Array(rows)).expect("writing to a String");
}

pub(super) fn display_text(events: &[ListedEvent<'_>], verbose: bool, output: &mut Output) {
    let width = env::var("COLUMNS")
        .ok()
        .and_then(|width| width.parse::<u16>().ok())
        .filter(|width| *width > 0)
        .or_else(|| Term::stdout().size_checked().map(|(_, width)| width))
        .map_or(80, usize::from);
    let mut columns = [0; 5];
    for expression in events.iter().flat_map(|row| &row.expressions) {
        for (column, field) in columns.iter_mut().zip(expression.split_whitespace()) {
            *column = (*column).max(measure_text_width(field));
        }
    }
    let repeat_width = events
        .iter()
        .filter_map(ListedEvent::repeat_seconds)
        .map(|seconds| format!("{seconds}s ").len())
        .max()
        .unwrap_or(0);
    output.stdout.push('\n');
    for row in events {
        let repeat = row
            .repeat_seconds()
            .map_or_else(String::new, |seconds| format!("{seconds}s "));
        let date = if verbose {
            &row.next_due_date
        } else {
            &row.next_due_date_human
        };
        let mutex = if row.has_mutex { "Has Mutex › " } else { "" };
        let tail = format!("{mutex}Next Due: {date}");
        for expression in &row.expressions {
            let aligned = expression
                .split_whitespace()
                .zip(columns)
                .map(|(field, width)| format!("{field:<width$}"))
                .collect::<Vec<_>>()
                .join(" ");
            let dots = width.saturating_sub(
                measure_text_width(&aligned)
                    + repeat_width
                    + measure_text_width(&row.command)
                    + measure_text_width(&tail)
                    + 8,
            );

            output.stdout.push_str("  ");
            output.write_style(&aligned, YELLOW, RESET_COLOR);
            output.stdout.push(' ');
            output.write_style(&format!("{repeat:<repeat_width$}"), GRAY, RESET_COLOR);
            output.stdout.push(' ');
            if let Some(start) = row.arguments_start {
                let (command, arguments) = row.command.split_at(start);
                output.stdout.push_str(command);
                output.write_style(arguments, "\x1b[33;1m", "\x1b[39;22m");
            } else {
                output.stdout.push_str(&row.command);
            }
            output.stdout.push(' ');
            output.write_style(&".".repeat(dots), GRAY, RESET_COLOR);
            output.stdout.push(' ');
            output.write_style(&tail, GRAY, RESET_COLOR);
            output.stdout.push('\n');

            if verbose
                && let Some(description) = &row.event.description
                && description.chars().count() > 1
            {
                let indent = " ".repeat(measure_text_width(&aligned) + 4);
                output.write_style(&format!("{indent}⇁ {description}"), GRAY, RESET_COLOR);
                output.stdout.push('\n');
            }
        }
    }
    output.stdout.push('\n');
}
