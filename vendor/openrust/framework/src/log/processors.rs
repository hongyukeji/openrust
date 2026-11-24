use super::{LogError, LogRecord, LogResult, Processor};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;

/// 记录原生进程的驻留内存，不替换应用的全局分配器。
pub struct MemoryUsageProcessor {
    use_formatting: bool,
}
impl MemoryUsageProcessor {
    pub fn new(real_usage: bool, use_formatting: bool) -> LogResult<Self> {
        if !real_usage {
            return Err(LogError::InvalidConfiguration);
        }
        Ok(Self { use_formatting })
    }
    pub fn process(&self, record: &mut LogRecord<'_>) -> LogResult<()> {
        let bytes = memory_stats::memory_stats()
            .ok_or_else(|| std::io::Error::other("无法读取进程内存。"))?
            .physical_mem as u64;
        let value = if self.use_formatting {
            let (divisor, unit) = if bytes > 1024 * 1024 {
                (1024.0 * 1024.0, "MB")
            } else if bytes > 1024 {
                (1024.0, "KB")
            } else {
                (1.0, "B")
            };
            format!(
                "{} {unit}",
                ((bytes as f64 / divisor) * 100.0).round() / 100.0
            )
            .into()
        } else {
            bytes.into()
        };
        record.extra.to_mut().insert("memory_usage".into(), value);
        Ok(())
    }
}
/// 使用既有占位符实现，并可移除实际参与替换的上下文字段。
pub struct PsrLogMessageProcessor {
    remove_used_context_fields: bool,
}
impl PsrLogMessageProcessor {
    pub fn new(remove_used_context_fields: bool) -> Self {
        Self {
            remove_used_context_fields,
        }
    }
    pub fn process(&self, record: &mut LogRecord<'_>) -> LogResult<()> {
        if !record.message.contains('{') {
            return Ok(());
        }
        let used = if self.remove_used_context_fields {
            record
                .context
                .keys()
                .filter(|key| record.message.contains(&format!("{{{key}}}")))
                .cloned()
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        record.message = super::logger::interpolate(&record.message, &record.context).into();
        for key in used {
            record.context.to_mut().shift_remove(&key);
        }
        Ok(())
    }
}
#[derive(Deserialize, Default)]
#[serde(default)]
struct Options {
    #[serde(rename = "realUsage")]
    real_usage: Option<bool>,
    #[serde(rename = "useFormatting")]
    use_formatting: Option<bool>,
    #[serde(rename = "removeUsedContextFields")]
    remove_used_context_fields: bool,
}
pub(super) fn builtin(name: &str, arguments: &Value) -> LogResult<Option<Arc<Processor>>> {
    if !matches!(name, "memory_usage" | "psr_log_message") {
        return Ok(None);
    }
    let options = if arguments.is_null() {
        Options::default()
    } else {
        serde_json::from_value(arguments.clone()).map_err(|_| LogError::InvalidConfiguration)?
    };
    Ok(Some(match name {
        "memory_usage" => {
            let processor = MemoryUsageProcessor::new(
                options.real_usage.unwrap_or(true),
                options.use_formatting.unwrap_or(true),
            )?;
            Arc::new(move |record| processor.process(record))
        }
        _ => {
            let processor = PsrLogMessageProcessor::new(options.remove_used_context_fields);
            Arc::new(move |record| processor.process(record))
        }
    }))
}
