use std::{borrow::Cow, sync::Arc, time::SystemTime};

use chrono::{DateTime, Utc};

use super::{
    Level, LogContext, LogError, LogResult,
    sink::{self, Handlers},
};

pub(super) struct Emergency {
    pub handlers: Handlers,
}

impl Emergency {
    pub fn write(&self, level: Level, message: &str, context: &LogContext) -> LogResult<()> {
        let record = Record::new("openrust", level, message, context, false);
        if sink::write(&self.handlers, &record).is_err() {
            use std::io::Write as _;
            std::io::stderr()
                .lock()
                .write_all(record.format_line(&record.message()).as_bytes())
                .map_err(LogError::write)?;
        }
        Ok(())
    }
}

/// Writes structured records through a resolved channel.
pub struct Logger {
    pub(super) name: String,
    pub(super) handlers: Option<Handlers>,
    pub(super) replace_placeholders: bool,
    pub(super) emergency: Arc<Emergency>,
}

impl Logger {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn log(&self, level: Level, message: &str, context: &LogContext) -> LogResult<()> {
        let Some(handlers) = &self.handlers else {
            return self.emergency.write(level, message, context);
        };
        let record = Record::new(
            &self.name,
            level,
            message,
            context,
            self.replace_placeholders,
        );
        if sink::write(handlers, &record).is_err() {
            self.emergency.write(
                Level::Emergency,
                "Unable to write configured logger. Using emergency logger.",
                &LogContext::new(),
            )?;
            self.emergency.write(level, message, context)?;
        }
        Ok(())
    }

    log_methods!(self);
}

impl crate::contracts::log::Logger for Logger {
    fn log(&self, level: Level, message: &str, context: &LogContext) -> LogResult<()> {
        Self::log(self, level, message, context)
    }
}

pub(super) struct Record<'a> {
    pub at: DateTime<Utc>,
    pub level: Level,
    pub context: &'a LogContext,
    channel: &'a str,
    message: &'a str,
    replace: bool,
}

impl<'a> Record<'a> {
    fn new(
        channel: &'a str,
        level: Level,
        message: &'a str,
        context: &'a LogContext,
        replace: bool,
    ) -> Self {
        Self {
            at: DateTime::from(SystemTime::now()),
            level,
            context,
            channel,
            message,
            replace,
        }
    }

    pub fn message(&self) -> Cow<'a, str> {
        if self.replace && self.message.contains('{') && !self.context.is_empty() {
            Cow::Owned(interpolate(self.message, self.context))
        } else {
            Cow::Borrowed(self.message)
        }
    }

    pub fn format_line(&self, message: &str) -> String {
        let mut line = format!("[{}] ", self.at.format("%Y-%m-%d %H:%M:%S"));
        append_single_line(&mut line, self.channel);
        line.push('.');
        line.push_str(self.level.label());
        line.push_str(": ");
        append_single_line(&mut line, message);
        let context = self.context;
        if !context.is_empty() {
            line.push(' ');
            for character in serde_json::to_string(context)
                .expect("JSON values serialize")
                .chars()
            {
                match character {
                    '\u{0085}' => line.push_str(r"\u0085"),
                    '\u{2028}' => line.push_str(r"\u2028"),
                    '\u{2029}' => line.push_str(r"\u2029"),
                    _ => line.push(character),
                }
            }
        }
        line.push('\n');
        line
    }
}

fn append_single_line(line: &mut String, text: &str) {
    for character in text.chars() {
        if character.is_control() || matches!(character, '\u{2028}' | '\u{2029}') {
            line.extend(character.escape_default());
        } else {
            line.push(character);
        }
    }
}

fn interpolate(message: &str, context: &LogContext) -> String {
    use serde_json::Value;
    let mut result = String::with_capacity(message.len());
    let mut remaining = message;
    while let Some(open) = remaining.find('{') {
        result.push_str(&remaining[..open]);
        remaining = &remaining[open + 1..];
        let Some(end) = remaining.find(['{', '}']) else {
            result.push('{');
            break;
        };
        let key = &remaining[..end];
        if remaining.as_bytes()[end] == b'{' {
            result.push('{');
            result.push_str(key);
            remaining = &remaining[end..];
            continue;
        }
        match context.get(key) {
            Some(Value::Null | Value::Bool(false)) => {}
            Some(Value::Bool(true)) => result.push('1'),
            Some(Value::String(value)) => result.push_str(value),
            Some(value @ (Value::Array(_) | Value::Object(_))) => {
                result.push_str("array");
                result.push_str(&value.to_string());
            }
            Some(Value::Number(value)) => result.push_str(&value.to_string()),
            None => {
                result.push('{');
                result.push_str(key);
                result.push('}');
            }
        }
        remaining = &remaining[end + 1..];
    }
    result.push_str(remaining);
    result
}
