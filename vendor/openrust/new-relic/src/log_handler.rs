//! 通过已有日志渠道发送结构化 OTLP 记录。

use std::sync::Arc;

use openrust::log::{FormattableHandler, Formatter, Level, LogError, LogRecord, LogResult};
use opentelemetry::{
    Context,
    logs::{AnyValue, LogRecord as _, Logger as _, Severity},
    propagation::TextMapPropagator as _,
    trace::TraceContextExt as _,
};
use opentelemetry_http::HeaderExtractor;
use opentelemetry_sdk::{logs::SdkLogger, propagation::TraceContextPropagator};
use serde_json::Value;

/// 由 New Relic Provider 注册的日志处理器。
pub struct NewRelicHandler {
    recorder: Arc<crate::recorder::Recorder>,
    report_errors: bool,
    logger: SdkLogger,
    fields: Vec<String>,
}

impl NewRelicHandler {
    pub(crate) fn from_arguments(
        recorder: Arc<crate::recorder::Recorder>,
        logger: SdkLogger,
        arguments: &Value,
    ) -> LogResult<Self> {
        let report_errors = match arguments.get("report_errors") {
            None => true,
            Some(value) => value.as_bool().ok_or(LogError::InvalidConfiguration)?,
        };
        let fields = match arguments.get("fields") {
            None => Vec::new(),
            Some(fields) => fields
                .as_array()
                .filter(|fields| fields.len() <= 32)
                .ok_or(LogError::InvalidConfiguration)?
                .iter()
                .map(|field| {
                    field
                        .as_str()
                        .filter(|field| {
                            field.len() <= 128
                                && (field.starts_with("context.") || field.starts_with("extra."))
                                && field
                                    .split_once('.')
                                    .is_some_and(|(_, name)| !name.is_empty())
                        })
                        .map(str::to_owned)
                        .ok_or(LogError::InvalidConfiguration)
                })
                .collect::<LogResult<Vec<_>>>()?,
        };
        Ok(Self {
            recorder,
            report_errors,
            logger,
            fields,
        })
    }
}

impl FormattableHandler for NewRelicHandler {
    fn get_default_formatter(&self) -> LogResult<Arc<Formatter>> {
        Ok(Arc::new(|record| Ok(record.message.to_string())))
    }

    fn write(&self, record: &LogRecord<'_>, formatted: &str) -> LogResult<()> {
        let mut log = self.logger.create_log_record();
        log.set_timestamp(record.at.into());
        let (label, severity) = match record.level {
            Level::Debug => ("debug", Severity::Debug),
            Level::Info => ("info", Severity::Info),
            Level::Notice => ("notice", Severity::Info2),
            Level::Warning => ("warning", Severity::Warn),
            Level::Error => ("error", Severity::Error),
            Level::Critical => ("critical", Severity::Fatal),
            Level::Alert => ("alert", Severity::Fatal2),
            Level::Emergency => ("emergency", Severity::Fatal3),
        };
        log.set_severity_text(label);
        log.set_severity_number(severity);
        let body = formatted.chars().take(1024).collect::<String>();
        log.set_body(body.into());
        log.add_attribute(
            "log.channel",
            record.channel.chars().take(64).collect::<String>(),
        );
        if let Some(trace) = record.trace_context() {
            let context = TraceContextPropagator::new()
                .extract_with_context(&Context::new(), &HeaderExtractor(&trace.headers()));
            let span = context.span();
            let trace = span.span_context();
            if trace.is_valid() {
                log.set_trace_context(trace.trace_id(), trace.span_id(), Some(trace.trace_flags()));
            }
        }
        for field in &self.fields {
            let Some((group, key)) = field.split_once('.') else {
                continue;
            };
            let fields = if group == "context" {
                &record.context
            } else {
                &record.extra
            };
            if let Some(value) = fields.get(key).and_then(attribute) {
                log.add_attribute(field.clone(), value);
            }
        }
        if let Some(kind) = record
            .exception
            .as_ref()
            .and_then(|exception| exception.get("class"))
            .and_then(Value::as_str)
        {
            log.add_attribute("exception.type", kind.chars().take(128).collect::<String>());
        }
        if self.report_errors && record.level >= Level::Error && !record.is_exception_report() {
            let kind = record
                .exception
                .as_ref()
                .and_then(|exception| exception.get("class"))
                .and_then(Value::as_str)
                .unwrap_or("log.error");
            self.recorder.report_log(kind, record.trace_context());
        }
        self.logger.emit(log);
        Ok(())
    }
}

fn attribute(value: &Value) -> Option<AnyValue> {
    match value {
        Value::Bool(value) => Some((*value).into()),
        Value::String(value) => Some(value.chars().take(512).collect::<String>().into()),
        Value::Number(value) => value
            .as_i64()
            .map(AnyValue::from)
            .or_else(|| value.as_f64().map(AnyValue::from)),
        Value::Array(values) if values.len() <= 16 => {
            let scalars = values
                .iter()
                .filter(|value| !value.is_object() && !value.is_array())
                .filter_map(attribute)
                .collect::<Vec<_>>();
            Some(AnyValue::ListAny(Box::new(scalars)))
        }
        _ => None,
    }
}
