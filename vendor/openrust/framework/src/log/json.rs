use std::{error::Error, sync::Arc};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::{contracts::debug::Exception, http::HttpError};

use super::{Formatter, Level, LogContext, LogError, LogRecord, LogResult};

const MAX_SOURCE_DEPTH: usize = 32;

pub(crate) fn snapshot(
    exception: &Exception<'_>,
    name: Option<&'static str>,
    source_context: impl Fn(&(dyn Error + 'static)) -> (Option<&'static str>, LogContext),
    root_context: LogContext,
) -> Value {
    let root = exception.root_error();
    let mut errors = vec![root];
    while let Some(mut source) = errors.last().copied().and_then(|error| error.source()) {
        while source.is::<crate::contracts::debug::SharedException>() {
            source = source.source().expect("共享异常保留原始错误");
        }
        if errors.len() == MAX_SOURCE_DEPTH
            || errors.iter().any(|error| std::ptr::eq(*error, source))
        {
            break;
        }
        errors.push(source);
    }
    let mut previous = None;
    for error in errors.iter().skip(1).rev() {
        let (kind, context) = source_context(*error);
        previous = Some(node(*error, kind, context, previous));
    }
    node(root, name, root_context, previous)
}

fn node(
    error: &(dyn Error + 'static),
    name: Option<&'static str>,
    mut context: LogContext,
    previous: Option<Value>,
) -> Value {
    let kind = name.unwrap_or_else(|| {
        if error.is::<HttpError>() {
            std::any::type_name::<HttpError>()
        } else if error.is::<std::io::Error>() {
            std::any::type_name::<std::io::Error>()
        } else {
            "Error"
        }
    });
    context.remove("previous");
    context.insert("class".into(), kind.into());
    context.insert("message".into(), error.to_string().into());
    if let Some(previous) = previous {
        context.insert("previous".into(), previous);
    }
    Value::Object(context)
}

#[derive(Deserialize)]
#[serde(default)]
struct Options {
    append_newline: bool,
    ignore_empty_context_and_extra: bool,
    max_normalize_depth: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            append_newline: true,
            ignore_empty_context_and_extra: false,
            max_normalize_depth: 9,
        }
    }
}

pub(super) fn formatter(arguments: &Value) -> LogResult<Arc<Formatter>> {
    let options = if arguments.is_null() {
        Options::default()
    } else {
        Options::deserialize(arguments).map_err(|_| LogError::InvalidConfiguration)?
    };
    Ok(Arc::new(move |record| Ok(options.format(record))))
}

/// 复用现有的标准深度与异常归一化，网络附件不另建一套错误序列化。
pub(super) fn normalized_fields(record: &LogRecord<'_>) -> (LogContext, LogContext) {
    let options = Options::default();
    let mut context = options.fields(&record.context, 2);
    if let Some(exception) = &record.exception {
        context.insert("exception".into(), options.exception(exception, 2));
    }
    (context, options.fields(&record.extra, 2))
}

impl Options {
    fn format(&self, record: &LogRecord<'_>) -> String {
        let level = match record.level {
            Level::Debug => 100,
            Level::Info => 200,
            Level::Notice => 250,
            Level::Warning => 300,
            Level::Error => 400,
            Level::Critical => 500,
            Level::Alert => 550,
            Level::Emergency => 600,
        };
        let mut value = json!({
            "message": record.message(),
            "level": level,
            "level_name": record.level.label(),
            "channel": record.channel,
            "datetime": record.at.strftime("%Y-%m-%dT%H:%M:%S%.6f%:z").to_string(),
        });
        let mut context = self.fields(&record.context, 2);
        if let Some(exception) = &record.exception {
            context.insert("exception".into(), self.exception(exception, 2));
        }
        if !context.is_empty() || !self.ignore_empty_context_and_extra {
            value["context"] = Value::Object(context);
        }
        if !record.extra.is_empty() || !self.ignore_empty_context_and_extra {
            value["extra"] = Value::Object(self.fields(&record.extra, 2));
        }
        let mut line = value.to_string();
        if self.append_newline {
            line.push('\n');
        }
        line
    }

    fn exception(&self, value: &Value, depth: usize) -> Value {
        let Some(fields) = value.as_object() else {
            return self.normalize(value, depth);
        };
        if depth > self.max_normalize_depth {
            return Value::Array(vec![self.limit()]);
        }
        let mut normalized = LogContext::new();
        for (key, value) in fields {
            match key.as_str() {
                "class" | "message" => {
                    normalized.insert(key.clone(), value.clone());
                }
                "previous" => {
                    normalized.insert(key.clone(), self.exception(value, depth + 1));
                }
                _ if depth < self.max_normalize_depth => {
                    normalized.insert(key.clone(), self.normalize(value, depth + 1));
                }
                _ => {}
            }
        }
        Value::Object(normalized)
    }

    fn fields(&self, fields: &LogContext, depth: usize) -> LogContext {
        fields
            .iter()
            .map(|(key, value)| (key.clone(), self.normalize(value, depth)))
            .collect()
    }

    fn normalize(&self, value: &Value, depth: usize) -> Value {
        if depth > self.max_normalize_depth {
            return self.limit();
        }
        match value {
            Value::Object(fields) => Value::Object(self.fields(fields, depth + 1)),
            Value::Array(values) => Value::Array(
                values
                    .iter()
                    .map(|value| self.normalize(value, depth + 1))
                    .collect(),
            ),
            _ => value.clone(),
        }
    }

    fn limit(&self) -> Value {
        format!(
            "Over {} levels deep, aborting normalization",
            self.max_normalize_depth
        )
        .into()
    }
}
