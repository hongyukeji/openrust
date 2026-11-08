//! Carbon 日期实例与工厂共享的字符串转换设置。

use super::InputDate;
use crate::{Error, Result};
use std::{
    fmt,
    sync::{Arc, RwLock},
};

pub(super) const DEFAULT_STRING_FORMAT: &str = "Y-m-d H:i:s";

type Render = dyn Fn(&InputDate) -> Result<String> + Send + Sync;

/// 日期转换为字符串时使用的 PHP 格式或应用回调。
#[derive(Clone)]
pub struct DateStringFormat(Format);

#[derive(Clone)]
enum Format {
    Pattern(Arc<str>),
    Custom(Arc<Render>),
}

impl DateStringFormat {
    /// 使用 PHP 日期格式。空字符串与 "0" 使用内置默认格式。
    pub fn pattern(pattern: impl Into<Arc<str>>) -> Self {
        Self(Format::Pattern(pattern.into()))
    }

    /// 在实际字符串转换时调用应用提供的格式函数。
    pub fn using(format: impl Fn(&InputDate) -> Result<String> + Send + Sync + 'static) -> Self {
        Self(Format::Custom(Arc::new(format)))
    }

    pub(super) fn render(&self, date: &InputDate) -> Result<String> {
        match &self.0 {
            Format::Custom(format) => format(date),
            Format::Pattern(pattern) => date.format(match &**pattern {
                "" | "0" => DEFAULT_STRING_FORMAT,
                pattern => pattern,
            }),
        }
    }
}

impl fmt::Debug for DateStringFormat {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DateStringFormat(..)")
    }
}

#[derive(Clone, Debug, Default)]
pub(super) enum FactoryStringFormat {
    #[default]
    Unconfigured,
    Configured(Option<DateStringFormat>),
}

#[derive(Clone, Debug, Default)]
pub(super) struct StringFormatSettings(Arc<RwLock<FactoryStringFormat>>);

impl StringFormatSettings {
    pub(super) fn snapshot(&self) -> Result<FactoryStringFormat> {
        self.0
            .read()
            .map(|settings| settings.clone())
            .map_err(|_| unavailable())
    }

    pub(super) fn set(&self, format: Option<DateStringFormat>) -> Result<()> {
        let retired = {
            let mut settings = self.0.write().map_err(|_| unavailable())?;
            std::mem::replace(&mut *settings, FactoryStringFormat::Configured(format))
        };
        drop(retired);
        Ok(())
    }
}

fn unavailable() -> Error {
    Error::custom(
        "calendar_string_format_unavailable",
        "无法读取日期字符串格式",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::support::calendar::{CalendarContext, DateConstruction, DateOptions, parse_date};
    use serde_json::json;

    fn scoped_date(context: &CalendarContext) -> Result<InputDate> {
        context
            .make_date(DateConstruction::Parse(&json!(
                "2026-11-08T13:00:00.123456+08:00"
            )))?
            .ok_or_else(unavailable)
    }

    #[test]
    fn date_string_factory_applies_initial_settings_and_live_fallback() -> Result<()> {
        let factory = CalendarContext::default();
        let untouched = scoped_date(&CalendarContext::default())?;
        let before = scoped_date(&factory)?;
        assert_eq!(before.to_string_value()?, "2026-11-08 13:00:00");

        factory.set_date_string_format(Some(DateStringFormat::pattern("Y/m/d H:i")))?;
        let copied = scoped_date(&factory)?;
        assert_eq!(before.to_string_value()?, "2026/11/08 13:00");
        factory
            .clone()
            .set_date_string_format(Some(DateStringFormat::pattern("d-m-Y H:i")))?;
        assert_eq!(before.to_string_value()?, "08-11-2026 13:00");
        assert_eq!(copied.to_string_value()?, "2026/11/08 13:00");
        assert_eq!(untouched.to_string_value()?, "2026-11-08 13:00:00");
        assert_eq!(
            copied.clone().with_string_format(None).to_string_value()?,
            "08-11-2026 13:00"
        );

        let other = CalendarContext::default();
        other.set_date_string_format(Some(DateStringFormat::pattern("d/m/Y")))?;
        let rebound = other
            .make_date(DateConstruction::Instance(&copied))?
            .ok_or_else(unavailable)?;
        assert_eq!(rebound.to_string_value()?, "08/11/2026");
        other.set_date_string_format(None)?;
        let reset = other
            .make_date(DateConstruction::Instance(&copied))?
            .ok_or_else(unavailable)?;
        assert_eq!(reset.to_string_value()?, "08-11-2026 13:00");

        let unconfigured = CalendarContext::default();
        let from_other = unconfigured
            .make_date(DateConstruction::Instance(&before))?
            .ok_or_else(unavailable)?;
        unconfigured.set_date_string_format(Some(DateStringFormat::pattern("d/m/Y")))?;
        assert_eq!(from_other.to_string_value()?, "08-11-2026 13:00");
        let parsed = parse_date(
            &json!("2026-11-08T13:00:00+08:00"),
            DateOptions::default(),
            &factory,
        )?
        .ok_or_else(unavailable)?;
        assert_eq!(parsed.to_string_value()?, "08-11-2026 13:00");
        Ok(())
    }

    #[test]
    fn date_string_local_policy_uses_php_fallback_and_leaves_json_unchanged() -> Result<()> {
        let factory = CalendarContext::default();
        factory.set_date_string_format(Some(DateStringFormat::pattern("d-m-Y H:i")))?;
        let date = scoped_date(&factory)?;
        for pattern in ["", "0"] {
            let local = date
                .clone()
                .with_string_format(Some(DateStringFormat::pattern(pattern)));
            assert_eq!(local.to_string_value()?, "2026-11-08 13:00:00");
        }
        let local = date
            .clone()
            .with_string_format(Some(DateStringFormat::pattern("Y-m-d H:i")));
        assert_eq!(local.start_of_day()?.to_string_value()?, "2026-11-08 00:00");
        let custom = date
            .clone()
            .with_string_format(Some(DateStringFormat::using(|date| {
                Ok(format!("custom {}", date.format("H:i P")?))
            })));
        assert_eq!(custom.to_string_value()?, "custom 13:00 +08:00");
        assert_eq!(
            serde_json::to_value(&custom).map_err(Error::internal)?,
            json!("2026-11-08T05:00:00.123456Z")
        );
        assert_eq!(custom.format("u")?, "123456");
        assert_eq!(
            custom
                .with_string_format(Some(DateStringFormat::using(|_| Ok(String::new()))))
                .to_string_value()?,
            ""
        );
        let failing = date.with_string_format(Some(DateStringFormat::using(|_| {
            Err(Error::custom(
                "custom_date_string_failed",
                "测试日期字符串转换失败",
            ))
        })));
        assert!(
            matches!(failing.to_string_value(), Err(error) if error.code() == "custom_date_string_failed")
        );
        Ok(())
    }
}
