use std::{
    borrow::Cow,
    sync::{Arc, Weak},
    time::SystemTime,
};

use chrono::{DateTime, Utc};

use crate::{
    container::Container, contracts::container::Container as _, support::facades::current_container,
};

use super::{
    Level, LogContext, LogError, LogResult,
    sink::{self, Handler, Handlers},
};

pub type Formatter = dyn Fn(&LogRecord<'_>) -> LogResult<String> + Send + Sync;
pub type Processor = dyn Fn(&mut LogRecord<'_>) -> LogResult<()> + Send + Sync;

pub(super) struct Emergency {
    pub handlers: Handlers,
}

impl Emergency {
    pub fn write(&self, level: Level, message: &str, context: &LogContext) -> LogResult<()> {
        self.write_record(&LogRecord::new(
            "openrust",
            level,
            message,
            context,
            &LogContext::new(),
            false,
        ))
    }

    fn write_record(&self, record: &LogRecord<'_>) -> LogResult<()> {
        if sink::write(&self.handlers, record).is_err() {
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
    pub(super) processors: Vec<Arc<Processor>>,
    pub(super) emergency: Arc<Emergency>,
    pub(super) context_owner: Weak<Container>,
}

impl Logger {
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns a logger with the same handlers and context under a different name.
    #[allow(non_snake_case, reason = "Matches Laravel's logger naming API")]
    #[must_use]
    pub fn withName(&self, name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            handlers: self.handlers.clone(),
            replace_placeholders: self.replace_placeholders,
            processors: self.processors.clone(),
            emergency: self.emergency.clone(),
            context_owner: self.context_owner.clone(),
        }
    }

    /// Assigns the formatter for the channel's byte-oriented handlers.
    #[allow(non_snake_case, reason = "Matches Laravel's handler customization API")]
    pub fn setFormatter(&mut self, formatter: Arc<Formatter>) {
        if let Some(handlers) = &mut self.handlers {
            *handlers = handlers
                .iter()
                .map(|handler| {
                    let handler = match handler.as_ref() {
                        Handler::Formatted { handler, .. } => handler,
                        _ => handler,
                    };
                    Arc::new(Handler::Formatted {
                        handler: handler.clone(),
                        formatter: formatter.clone(),
                    })
                })
                .collect();
        }
    }

    /// Prepends a processor, as in Monolog's pushProcessor.
    #[allow(non_snake_case, reason = "Matches Laravel's logger customization API")]
    pub fn pushProcessor(&mut self, processor: Arc<Processor>) {
        self.processors.insert(0, processor);
    }

    pub fn log(&self, level: Level, message: &str, context: &LogContext) -> LogResult<()> {
        let extra = self
            .context_owner
            .upgrade()
            .and_then(|container| {
                if !container.has_active_scope()
                    || current_container().is_some_and(|active| !Arc::ptr_eq(&container, &active))
                {
                    return None;
                }
                container
                    .make::<super::context::Repository>()
                    .ok()
                    .map(|repository| repository.all())
            })
            .unwrap_or_default();
        self.write(level, message, context, &extra)
    }

    fn write(
        &self,
        level: Level,
        message: &str,
        context: &LogContext,
        extra: &LogContext,
    ) -> LogResult<()> {
        let fallback = || {
            self.emergency.write_record(&LogRecord::new(
                "openrust", level, message, context, extra, false,
            ))
        };
        let Some(handlers) = &self.handlers else {
            return fallback();
        };
        if !self.processors.is_empty() && !sink::accepts(handlers, level) {
            return Ok(());
        }
        let mut record = LogRecord::new(
            &self.name,
            level,
            message,
            context,
            extra,
            self.replace_placeholders,
        );
        if self
            .processors
            .iter()
            .try_for_each(|process| process(&mut record))
            .is_err()
            || sink::write(handlers, &record).is_err()
        {
            self.emergency.write(
                Level::Emergency,
                "Unable to write configured logger. Using emergency logger.",
                &LogContext::new(),
            )?;
            fallback()?;
        }
        Ok(())
    }

    log_methods!(self);
}

impl crate::contracts::log::Logger for Logger {
    fn log(&self, level: Level, message: &str, context: &LogContext) -> LogResult<()> {
        Self::log(self, level, message, context)
    }
    #[allow(non_snake_case)]
    fn logWithContext(
        &self,
        level: Level,
        message: &str,
        context: &LogContext,
        extra: &LogContext,
    ) -> LogResult<()> {
        self.write(level, message, context, extra)
    }
}

/// A channel record. Context fields borrow until a processor changes them.
pub struct LogRecord<'a> {
    pub at: DateTime<Utc>,
    pub level: Level,
    pub context: Cow<'a, LogContext>,
    pub extra: Cow<'a, LogContext>,
    pub channel: Cow<'a, str>,
    pub message: Cow<'a, str>,
    replace: bool,
}

impl<'a> LogRecord<'a> {
    fn new(
        channel: &'a str,
        level: Level,
        message: &'a str,
        context: &'a LogContext,
        extra: &'a LogContext,
        replace: bool,
    ) -> Self {
        Self {
            at: DateTime::from(SystemTime::now()),
            level,
            context: Cow::Borrowed(context),
            extra: Cow::Borrowed(extra),
            channel: Cow::Borrowed(channel),
            message: Cow::Borrowed(message),
            replace,
        }
    }

    pub fn message(&self) -> Cow<'_, str> {
        if self.replace && self.message.contains('{') && !self.context.is_empty() {
            Cow::Owned(interpolate(&self.message, &self.context))
        } else {
            Cow::Borrowed(&self.message)
        }
    }

    pub fn format_line(&self, message: &str) -> String {
        let mut line = format!("[{}] ", self.at.format("%Y-%m-%d %H:%M:%S"));
        append_single_line(&mut line, &self.channel);
        line.push('.');
        line.push_str(self.level.label());
        line.push_str(": ");
        append_single_line(&mut line, message);
        if !self.context.is_empty() || !self.extra.is_empty() {
            append_json(&mut line, &self.context);
        }
        if !self.extra.is_empty() {
            append_json(&mut line, &self.extra);
        }
        line.push('\n');
        line
    }
}

fn append_json(line: &mut String, context: &LogContext) {
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
