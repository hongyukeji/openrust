use std::{
    borrow::Cow,
    sync::{Arc, Weak},
};

use jiff::Timestamp;
use serde_json::Value;

use crate::{
    container::Container,
    contracts::{
        container::Container as _,
        debug::{Exception, ExceptionHandler},
    },
    support::facades::current_container,
};

use super::{
    Level, LogContext, LogResult,
    sink::{self, Handler, Handlers},
};

pub type Formatter = dyn Fn(&LogRecord<'_>) -> LogResult<String> + Send + Sync;
pub type Processor = dyn Fn(&mut LogRecord<'_>) -> LogResult<()> + Send + Sync;

pub(super) struct Emergency {
    pub handlers: Handlers,
    pub listeners: Arc<super::events::Listeners>,
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
        ))?;
        self.listeners.fire(level, message, context, None)
    }

    fn write_record(&self, record: &LogRecord<'_>) -> LogResult<()> {
        if sink::write(&self.handlers, record).is_err() {
            use std::io::Write as _;
            std::io::stderr()
                .lock()
                .write_all(record.format_line(&record.message()).as_bytes())?;
        }
        Ok(())
    }
}

/// 原生日志执行器，拥有通道处理器、格式器和真实输出目标。
#[derive(Clone)]
pub struct NativeLogger {
    pub(super) name: String,
    pub(super) handlers: Option<Handlers>,
    pub(super) replace_placeholders: bool,
    pub(super) processors: Vec<Arc<Processor>>,
    pub(super) emergency: Arc<Emergency>,
}

/// 为原生日志器提供应用上下文和日志事件。
pub struct Logger {
    pub(super) native: Arc<NativeLogger>,
    pub(super) context_owner: Weak<Container>,
    pub(super) context_fallback: Arc<super::channel_context::ChannelContext>,
    pub(super) context_identity: Arc<()>,
    pub(super) listeners: Arc<super::events::Listeners>,
}

impl NativeLogger {
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 为通道的字节输出目标设置格式器。
    #[allow(non_snake_case, reason = "保留日志公开方法名")]
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

    /// 将处理器插入执行顺序的最前面。
    #[allow(non_snake_case, reason = "保留日志公开方法名")]
    pub fn pushProcessor(&mut self, processor: Arc<Processor>) {
        self.processors.insert(0, processor);
    }

    /// 直接写入原生通道，不附加框架上下文、元数据或事件。
    pub fn log(&self, level: Level, message: &str, context: &LogContext) -> LogResult<()> {
        let extra = LogContext::new();
        let mut record = LogRecord::new(
            &self.name,
            level,
            message,
            context,
            &extra,
            self.replace_placeholders,
        );
        self.emit(&mut record)
    }

    fn emit(&self, record: &mut LogRecord<'_>) -> LogResult<()> {
        let Some(handlers) = &self.handlers else {
            return self.emergency.write_record(record);
        };
        if !sink::accepts(handlers, record.level) {
            return Ok(());
        }
        self.processors
            .iter()
            .try_for_each(|process| process(record))?;
        sink::write(handlers, record).map_err(Into::into)
    }

    log_methods!(self);
}

impl crate::contracts::log::Logger for NativeLogger {
    fn log(&self, level: Level, message: &str, context: &LogContext) -> LogResult<()> {
        Self::log(self, level, message, context)
    }
}

impl Logger {
    fn channel_context(&self) -> Arc<super::channel_context::ChannelContext> {
        self.context_owner
            .upgrade()
            .and_then(|owner| owner.make().ok())
            .unwrap_or_else(|| self.context_fallback.clone())
    }
    fn context_key(&self) -> usize {
        Arc::as_ptr(&self.context_identity).addr()
    }
    fn merged_context(&self, context: &LogContext) -> LogContext {
        let state = self.channel_context();
        let mut fields = state.0.lock().expect("日志上下文锁中毒");
        let shared = fields.shared.clone();
        let mut merged = fields
            .channels
            .entry(self.context_key())
            .or_insert_with(|| (self.context_identity.clone(), shared))
            .1
            .clone();
        merged.extend(context.clone());
        merged
    }
    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    pub fn withContext(&self, context: LogContext) -> &Self {
        let state = self.channel_context();
        let mut fields = state.0.lock().expect("日志上下文锁中毒");
        let shared = fields.shared.clone();
        fields
            .channels
            .entry(self.context_key())
            .or_insert_with(|| (self.context_identity.clone(), shared))
            .1
            .extend(context);
        self
    }
    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    pub fn withoutContext(&self, keys: Option<&[&str]>) -> &Self {
        let state = self.channel_context();
        let mut fields = state.0.lock().expect("日志上下文锁中毒");
        let shared = fields.shared.clone();
        let context = &mut fields
            .channels
            .entry(self.context_key())
            .or_insert_with(|| (self.context_identity.clone(), shared))
            .1;
        if let Some(keys) = keys {
            for key in keys {
                context.remove(*key);
            }
        } else {
            context.clear();
        }
        self
    }

    pub fn name(&self) -> &str {
        &self.native.name
    }

    /// Returns a logger with the same handlers and context under a different name.
    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    #[must_use]
    pub fn withName(&self, name: impl Into<String>) -> Self {
        let mut native = (*self.native).clone();
        native.name = name.into();
        Self {
            native: Arc::new(native),
            context_owner: self.context_owner.clone(),
            context_fallback: self.context_fallback.clone(),
            context_identity: self.context_identity.clone(),
            listeners: self.listeners.clone(),
        }
    }

    #[allow(non_snake_case, reason = "保留日志公开方法名")]
    pub fn setFormatter(&mut self, formatter: Arc<Formatter>) {
        Arc::make_mut(&mut self.native).setFormatter(formatter);
    }

    #[allow(non_snake_case, reason = "保留日志公开方法名")]
    pub fn pushProcessor(&mut self, processor: Arc<Processor>) {
        Arc::make_mut(&mut self.native).pushProcessor(processor);
    }

    /// 返回本通道实际使用的原生日志器；直接写入不合并应用上下文或派发框架事件。
    #[allow(non_snake_case, reason = "保留日志公开方法名")]
    pub fn getLogger(&self) -> Arc<NativeLogger> {
        self.native.clone()
    }

    pub fn log(&self, level: Level, message: &str, context: &LogContext) -> LogResult<()> {
        self.write(level, message, context, &self.extra(), None)
    }

    /// Writes a record, then awaits observers that require the current request.
    #[allow(non_snake_case, reason = "OpenRust asynchronous request log adapter")]
    pub async fn logWithRequest(
        &self,
        request: &mut crate::http::Request,
        level: Level,
        message: &str,
        context: &LogContext,
    ) -> LogResult<()> {
        self.write_for_request(request, level, message, context, None)
            .await
    }

    /// 在已接收日志完成通道写入后同步派发事件。
    pub fn listen(
        &self,
        listener: impl Fn(&super::events::MessageLogged) -> LogResult<()> + Send + Sync + 'static,
    ) {
        self.listeners.listen(listener);
    }

    /// Logs an error and its source chain as structured exception data.
    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    pub fn logException<E: std::error::Error + Send + Sync + 'static>(
        &self,
        level: Level,
        message: &str,
        error: &E,
        context: &LogContext,
    ) -> LogResult<()> {
        let exception = Exception::Error(error);
        let name = Some(std::any::type_name::<E>());
        let handler = self
            .context_owner
            .upgrade()
            .and_then(|owner| owner.make::<dyn ExceptionHandler>().ok());
        let snapshot = match handler {
            Some(handler) => handler.buildLogException(&exception, name, false),
            None => super::json::snapshot(
                &exception,
                name,
                |_| (None, LogContext::new()),
                LogContext::new(),
            ),
        };
        self.write(level, message, context, &self.extra(), Some(&snapshot))
    }

    fn extra(&self) -> LogContext {
        self.context_owner
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
            .unwrap_or_default()
    }

    fn write(
        &self,
        level: Level,
        message: &str,
        context: &LogContext,
        extra: &LogContext,
        exception: Option<&Value>,
    ) -> LogResult<()> {
        let merged = self.merged_context(context);
        let context = &merged;
        let fallback = || {
            self.native.emergency.write_record(&LogRecord::new(
                "openrust", level, message, context, extra, false,
            ))
        };
        let Some(handlers) = &self.native.handlers else {
            fallback()?;
            return self.listeners.fire(level, message, context, exception);
        };
        if !sink::accepts(handlers, level) {
            return Ok(());
        }
        let mut record = LogRecord::new(
            &self.native.name,
            level,
            message,
            context,
            extra,
            self.native.replace_placeholders,
        );
        record.exception = exception.map(Cow::Borrowed);
        if self.native.emit(&mut record).is_err() {
            self.native.emergency.write(
                Level::Emergency,
                "Unable to write configured logger. Using emergency logger.",
                &LogContext::new(),
            )?;
            fallback()?;
        }
        self.listeners.fire(level, message, context, exception)
    }

    async fn write_for_request(
        &self,
        request: &mut crate::http::Request,
        level: Level,
        message: &str,
        context: &LogContext,
        exception: Option<&Value>,
    ) -> LogResult<()> {
        let merged = self.merged_context(context);
        let context = &merged;
        let extra = self.extra();
        let fallback = || {
            self.native.emergency.write_record(&LogRecord::new(
                "openrust", level, message, context, &extra, false,
            ))
        };
        let Some(handlers) = &self.native.handlers else {
            fallback()?;
            return self
                .listeners
                .fire_for_request(request, level, message, context, exception)
                .await;
        };
        if !sink::accepts(handlers, level) {
            return Ok(());
        }
        let mut record = LogRecord::new(
            &self.native.name,
            level,
            message,
            context,
            &extra,
            self.native.replace_placeholders,
        );
        record.exception = exception.map(Cow::Borrowed);
        if self.native.emit(&mut record).is_err() {
            let message = "Unable to write configured logger. Using emergency logger.";
            let empty = LogContext::new();
            self.native.emergency.write_record(&LogRecord::new(
                "openrust",
                Level::Emergency,
                message,
                &empty,
                &empty,
                false,
            ))?;
            self.native
                .emergency
                .listeners
                .fire_for_request(request, Level::Emergency, message, &empty, None)
                .await?;
            fallback()?;
        }
        self.listeners
            .fire_for_request(request, level, message, context, exception)
            .await
    }

    log_methods!(self);
}

impl crate::contracts::log::Logger for Logger {
    fn log(&self, level: Level, message: &str, context: &LogContext) -> LogResult<()> {
        Self::log(self, level, message, context)
    }
    #[allow(non_snake_case)]
    fn logWithExceptionForRequest<'a>(
        &'a self,
        request: &'a mut crate::http::Request,
        level: Level,
        message: &'a str,
        context: &'a LogContext,
        exception: &'a Value,
    ) -> crate::http::HttpFuture<'a, LogResult<()>> {
        Box::pin(self.write_for_request(request, level, message, context, Some(exception)))
    }
    #[allow(non_snake_case)]
    fn logWithContext(
        &self,
        level: Level,
        message: &str,
        context: &LogContext,
        extra: &LogContext,
    ) -> LogResult<()> {
        self.write(level, message, context, extra, None)
    }

    #[allow(non_snake_case)]
    fn logWithException(
        &self,
        level: Level,
        message: &str,
        context: &LogContext,
        extra: Option<&LogContext>,
        exception: &Value,
    ) -> LogResult<()> {
        let existing;
        let extra = match extra {
            Some(extra) => extra,
            None => {
                existing = self.extra();
                &existing
            }
        };
        self.write(level, message, context, extra, Some(exception))
    }
}

/// A channel record. Context fields borrow until a processor changes them.
pub struct LogRecord<'a> {
    pub at: Timestamp,
    pub level: Level,
    pub context: Cow<'a, LogContext>,
    pub extra: Cow<'a, LogContext>,
    pub channel: Cow<'a, str>,
    pub message: Cow<'a, str>,
    pub exception: Option<Cow<'a, Value>>,
    replace: bool,
}

impl<'a> LogRecord<'a> {
    pub(super) fn owned(&self) -> LogRecord<'static> {
        LogRecord {
            at: self.at,
            level: self.level,
            context: Cow::Owned(self.context.as_ref().clone()),
            extra: Cow::Owned(self.extra.as_ref().clone()),
            channel: Cow::Owned(self.channel.to_string()),
            message: Cow::Owned(self.message.to_string()),
            exception: self
                .exception
                .as_ref()
                .map(|value| Cow::Owned(value.as_ref().clone())),
            replace: self.replace,
        }
    }

    fn new(
        channel: &'a str,
        level: Level,
        message: &'a str,
        context: &'a LogContext,
        extra: &'a LogContext,
        replace: bool,
    ) -> Self {
        Self {
            at: Timestamp::now(),
            level,
            context: Cow::Borrowed(context),
            extra: Cow::Borrowed(extra),
            channel: Cow::Borrowed(channel),
            message: Cow::Borrowed(message),
            exception: None,
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
        let mut line = format!("[{}] ", self.at.strftime("%Y-%m-%d %H:%M:%S"));
        self.append_payload(&mut line, message);
        line.push('\n');
        line
    }

    pub(super) fn format_syslog(&self) -> String {
        let mut line = String::new();
        self.append_payload(&mut line, &self.message());
        line
    }

    fn append_payload(&self, line: &mut String, message: &str) {
        append_single_line(line, &self.channel);
        line.push('.');
        line.push_str(self.level.label());
        line.push_str(": ");
        append_single_line(line, message);
        if !self.context.is_empty() || !self.extra.is_empty() {
            append_json(line, &self.context);
        }
        if !self.extra.is_empty() {
            append_json(line, &self.extra);
        }
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
