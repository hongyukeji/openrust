use super::{ValidationError, ValidationResult, Value};
use crate::support::{calendar, facades};

/// 与请求日期输入共享解析器、应用时区及测试时钟。
pub(super) struct DateContext {
    timezone: String,
    reference: jiff::Timestamp,
    frozen: bool,
}

impl DateContext {
    pub(super) fn new() -> ValidationResult<Self> {
        let configured = if facades::current_container().is_some_and(|container| {
            crate::contracts::container::Container::has(container.as_ref(), "config")
        }) {
            facades::Config::get("app.timezone").map_err(|source| ValidationError::Execution {
                source: source.into(),
            })?
        } else {
            None
        };
        let timezone = configured
            .as_ref()
            .map_or(Ok("UTC"), |value| {
                value.as_str().ok_or(ValidationError::InvalidRule)
            })?
            .to_owned();
        crate::support::Date::timezone(&timezone).map_err(|source| ValidationError::Execution {
            source: source.into(),
        })?;
        let fixed = crate::support::frozen_utc_now();
        Ok(Self {
            timezone,
            reference: fixed.unwrap_or_else(jiff::Timestamp::now),
            frozen: fixed.is_some(),
        })
    }

    pub(super) fn parse(&self, value: &Value, format: Option<&str>) -> Option<calendar::InputDate> {
        if !(value.is_string() || value.is_number()) {
            return None;
        }
        let parse = |format: Option<&str>| {
            let reference = if self.frozen {
                calendar::DateReference::Frozen(self.reference)
            } else {
                calendar::DateReference::Wall(self.reference)
            };
            calendar::parse_input_date(value.clone(), format, &self.timezone, reference).ok()
        };
        format
            .and_then(|format| parse(Some(&format!("!{format}"))))
            .or_else(|| parse(None))
    }
}
