use super::super::{Formatter, LogContext, LogError, LogRecord, LogResult};
use serde::Deserialize;
use serde_json::Value;
use std::sync::{Arc, OnceLock};
/// 可配置的行格式器；日期和错误值复用现有归一化机制。
pub struct LineFormatter {
    format: String,
    date_format: String,
    allow_inline_line_breaks: bool,
    ignore_empty_context_and_extra: bool,
}
impl LineFormatter {
    pub const SIMPLE_FORMAT: &'static str =
        "[%datetime%] %channel%.%level_name%: %message% %context% %extra%\n";
    pub fn new(format: Option<&str>, date_format: Option<&str>) -> Self {
        Self {
            format: format.unwrap_or(Self::SIMPLE_FORMAT).to_owned(),
            date_format: date_format.unwrap_or("Y-m-d\\TH:i:sP").to_owned(),
            allow_inline_line_breaks: false,
            ignore_empty_context_and_extra: false,
        }
    }
    pub fn allow_inline_line_breaks(&mut self, allow: bool) -> &mut Self {
        self.allow_inline_line_breaks = allow;
        self
    }
    pub fn ignore_empty_context_and_extra(&mut self, ignore: bool) -> &mut Self {
        self.ignore_empty_context_and_extra = ignore;
        self
    }
    pub fn stringify(&self, value: &Value) -> String {
        let text = match value {
            Value::Null => "NULL".into(),
            Value::Bool(v) => v.to_string(),
            Value::String(v) => v.to_owned(),
            Value::Number(v) if v.is_f64() => {
                crate::support::php_float_string(v.as_f64().expect("浮点数值"))
            }
            Value::Number(v) => v.to_string(),
            v => v.to_string(),
        };
        if !self.allow_inline_line_breaks {
            return text.replace("\r\n", " ").replace(['\r', '\n'], " ");
        }
        if !text.starts_with(['{', '[']) {
            return text;
        }
        let bytes = text.as_bytes();
        let mut result = String::with_capacity(text.len());
        let mut start = 0;
        let mut at = 0;
        while at + 1 < bytes.len() {
            if bytes[at] == b'\\'
                && matches!(bytes[at + 1], b'r' | b'n')
                && (at == 0 || bytes[at - 1] != b'\\')
            {
                result.push_str(&text[start..at]);
                result.push('\n');
                at += 2;
                start = at;
            } else {
                at += 1;
            }
        }
        result.push_str(&text[start..]);
        result
    }
    pub fn format(&self, record: &LogRecord<'_>) -> LogResult<String> {
        let (mut context, mut extra) = super::super::json::normalized_fields(record);
        let mut output = self.format.clone();
        for (prefix, values) in [("extra", &mut extra), ("context", &mut context)] {
            let keys = values.keys().cloned().collect::<Vec<_>>();
            for key in keys {
                let token = format!("%{prefix}.{key}%");
                if output.contains(&token) {
                    output = output.replace(
                        &token,
                        &self.stringify(&values.shift_remove(&key).expect("已存在字段")),
                    );
                }
            }
        }
        let date = crate::support::Date::from_utc(record.at, jiff::tz::TimeZone::UTC)
            .format(&self.date_format)
            .map_err(|e| LogError::WriteFailed(Box::new(e)))?;
        let fields = [
            ("message", Value::String(record.message().into_owned())),
            ("context", self.fields(context)),
            ("level", record.level.code().into()),
            ("level_name", record.level.label().into()),
            ("channel", record.channel.to_string().into()),
            ("datetime", date.into()),
            ("extra", self.fields(extra)),
        ];
        for (key, value) in fields {
            output = output.replace(&format!("%{key}%"), &self.stringify(&value));
        }
        static REMAINDER: OnceLock<regex::Regex> = OnceLock::new();
        Ok(REMAINDER
            .get_or_init(|| regex::Regex::new(r"%(?:extra|context)\..+?%").expect("固定格式表达式"))
            .replace_all(&output, "")
            .into_owned())
    }
    fn fields(&self, values: LogContext) -> Value {
        if values.is_empty() {
            if self.ignore_empty_context_and_extra {
                Value::String(String::new())
            } else {
                Value::Array(Vec::new())
            }
        } else {
            Value::Object(values)
        }
    }
    pub fn format_batch(&self, records: &[LogRecord<'_>]) -> LogResult<String> {
        records
            .iter()
            .try_fold(String::new(), |mut output, record| {
                output.push_str(&self.format(record)?);
                Ok(output)
            })
    }
}
#[derive(Deserialize, Default)]
#[serde(default)]
struct Options {
    format: Option<String>,
    #[serde(rename = "dateFormat")]
    date_format: Option<String>,
    #[serde(rename = "allowInlineLineBreaks")]
    allow_inline_line_breaks: bool,
    #[serde(rename = "ignoreEmptyContextAndExtra")]
    ignore_empty_context_and_extra: bool,
}
pub(super) fn formatter(arguments: &Value) -> LogResult<Arc<Formatter>> {
    let options = if arguments.is_null() {
        Options::default()
    } else {
        serde_json::from_value(arguments.clone()).map_err(|_| LogError::InvalidConfiguration)?
    };
    let mut formatter =
        LineFormatter::new(options.format.as_deref(), options.date_format.as_deref());
    formatter
        .allow_inline_line_breaks(options.allow_inline_line_breaks)
        .ignore_empty_context_and_extra(options.ignore_empty_context_and_extra);
    Ok(Arc::new(move |record| formatter.format(record)))
}
