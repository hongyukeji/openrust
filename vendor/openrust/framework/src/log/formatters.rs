use super::{Formatter, Level, LogError, LogRecord, LogResult};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;

/// 记录HTML表格；日期格式复用框架已有的日期机制。
pub struct HtmlFormatter {
    date_format: String,
}
impl HtmlFormatter {
    pub fn new(date_format: Option<&str>) -> Self {
        Self {
            date_format: date_format.unwrap_or("Y-m-d\\TH:i:sP").to_owned(),
        }
    }
    pub fn format(&self, record: &LogRecord<'_>) -> LogResult<String> {
        let color = match record.level {
            Level::Debug => "#CCCCCC",
            Level::Info => "#28A745",
            Level::Notice => "#17A2B8",
            Level::Warning => "#FFC107",
            Level::Error => "#FD7E14",
            Level::Critical => "#DC3545",
            Level::Alert => "#821722",
            Level::Emergency => "#000000",
        };
        let date = crate::support::Date::from_utc(record.at, jiff::tz::TimeZone::UTC)
            .format(&self.date_format)
            .map_err(|error| LogError::WriteFailed(Box::new(error)))?;
        let mut text = format!(
            "<h1 style=\"background: {color};color: #ffffff;padding: 5px;\" class=\"monolog-output\">{}</h1><table cellspacing=\"1\" width=\"100%\" class=\"monolog-output\">",
            record.level.label()
        );
        text.push_str(&row("Message", &record.message(), true));
        text.push_str(&row("Time", &date, true));
        text.push_str(&row("Channel", &record.channel, true));
        let (context, extra) = super::json::normalized_fields(record);
        for (key, values) in [("Context", context), ("Extra", extra)] {
            if values.is_empty() {
                continue;
            }
            let mut table = "<table cellspacing=\"1\" width=\"100%\">".to_owned();
            for (key, value) in values {
                let value = match value {
                    Value::Null => String::new(),
                    Value::Bool(false) => String::new(),
                    Value::Bool(true) => "1".into(),
                    Value::String(value) => value,
                    Value::Number(value) if value.is_f64() => {
                        crate::support::php_float_string(value.as_f64().expect("浮点数值"))
                    }
                    Value::Number(value) => value.to_string(),
                    value => super::json::pretty(&value),
                };
                table.push_str(&row(&key, &value, true));
            }
            table.push_str("</table>");
            text.push_str(&row(key, &table, false));
        }
        text.push_str("</table>");
        Ok(text)
    }
    pub fn format_batch(&self, records: &[LogRecord<'_>]) -> LogResult<String> {
        records
            .iter()
            .try_fold(String::new(), |mut result, record| {
                result.push_str(&self.format(record)?);
                Ok(result)
            })
    }
}
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
fn row(title: &str, value: &str, escape_value: bool) -> String {
    let value = if escape_value {
        format!("<pre>{}</pre>", escape(value))
    } else {
        value.to_owned()
    };
    format!(
        "<tr style=\"padding: 4px;text-align: left;\">\n<th style=\"vertical-align: top;background: #ccc;color: #000\" width=\"100\">{}:</th>\n<td style=\"padding: 4px;text-align: left;vertical-align: top;background: #eee;color: #000\">{value}</td>\n</tr>",
        escape(title)
    )
}
#[derive(Deserialize)]
struct Options {
    #[serde(rename = "dateFormat")]
    date_format: Option<String>,
}
pub(super) fn builtin(name: &str, arguments: &Value) -> LogResult<Option<Arc<Formatter>>> {
    if name != "html" {
        return Ok(None);
    }
    let options = if arguments.is_null() {
        Options { date_format: None }
    } else {
        serde_json::from_value(arguments.clone()).map_err(|_| LogError::InvalidConfiguration)?
    };
    let formatter = HtmlFormatter::new(options.date_format.as_deref());
    Ok(Some(Arc::new(move |record| formatter.format(record))))
}
