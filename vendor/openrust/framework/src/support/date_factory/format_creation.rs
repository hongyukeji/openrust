//! 按格式创建共用日期解析器；严格模式归所属工厂，不泄漏到其他应用。
use super::{Date, DateError, DateFacade, DateFactory, DateInput, DateTimezone};
use std::sync::atomic::{AtomicBool, Ordering};
pub(super) struct CreationSettings {
    strict: AtomicBool,
}
impl Default for CreationSettings {
    fn default() -> Self {
        Self {
            strict: AtomicBool::new(true),
        }
    }
}
impl DateFactory {
    pub fn can_be_created_from_format<'a>(
        &self,
        input: impl Into<Option<&'a str>>,
        format: &str,
    ) -> Result<bool, DateError> {
        let Some(input) = input.into() else {
            return Ok(false);
        };
        if input.contains('\0') {
            return Err(DateError::InvalidExpression);
        }
        match self.raw_create_from_format(format, input, None) {
            Ok(Some(_)) => Ok(self.has_format_with_modifiers(input, format)),
            Ok(None) | Err(DateError::InvalidExpression | DateError::OutOfRange) => Ok(false),
            Err(error) => Err(error),
        }
    }

    pub fn use_strict_mode(&self, enabled: impl Into<Option<bool>>) {
        self.creation
            .strict
            .store(enabled.into().unwrap_or(true), Ordering::Release);
    }
    pub fn is_strict_mode_enabled(&self) -> bool {
        self.creation.strict.load(Ordering::Acquire)
    }
    pub fn create_from_format(
        &self,
        format: &str,
        input: impl Into<DateInput>,
        timezone: Option<DateTimezone>,
    ) -> Result<Option<Date>, DateError> {
        self.raw_create_from_format(format, input, timezone)
    }
    pub fn raw_create_from_format(
        &self,
        format: &str,
        input: impl Into<DateInput>,
        timezone: Option<DateTimezone>,
    ) -> Result<Option<Date>, DateError> {
        use crate::support::calendar::parse_factory_format;
        let text = crate::support::input_scalars::string(input.into().scalar()?);
        // 上游的 NUL 输入错误不受严格模式开关控制。
        if text.contains('\0') {
            return Err(DateError::InvalidExpression);
        }
        let explicit = timezone.is_some();
        let zone = self.zone(timezone)?;
        let result = parse_factory_format(&text, format, zone, explicit, self.clock.date());
        self.format_result(result)?
            .map(Date::from_input)
            .transpose()
    }
    pub(crate) fn input_from_format(
        &self,
        format: &str,
        text: &str,
        timezone: &str,
        explicit: bool,
    ) -> Result<Option<crate::support::InputDate>, DateError> {
        if text.contains('\0') {
            return Err(DateError::InvalidExpression);
        }
        crate::support::calendar::validate_timezone_name(timezone)?;
        self.format_result(crate::support::calendar::parse_factory_format_named(
            text,
            format,
            timezone,
            explicit,
            self.clock.date(),
        ))
    }
    fn format_result(
        &self,
        result: Result<crate::support::InputDate, crate::support::calendar::CalendarError>,
    ) -> Result<Option<crate::support::InputDate>, DateError> {
        use crate::support::calendar::CalendarError;
        match result {
            Ok(parsed) => Ok(Some(parsed)),
            Err(CalendarError::InvalidExpression) if !self.is_strict_mode_enabled() => Ok(None),
            Err(CalendarError::InvalidExpression) => Err(DateError::InvalidExpression),
            Err(CalendarError::OutOfRange) => Err(DateError::OutOfRange),
            Err(CalendarError::LimitExceeded) => Err(DateError::InputTooLarge),
        }
    }
}
impl DateFacade {
    pub fn can_be_created_from_format<'a>(
        input: impl Into<Option<&'a str>>,
        format: &str,
    ) -> Result<bool, DateError> {
        Self::factory()?.can_be_created_from_format(input, format)
    }

    pub(crate) fn input_from_format(
        format: &str,
        text: &str,
        timezone: &str,
        explicit: bool,
    ) -> Result<Option<crate::support::InputDate>, DateError> {
        Self::factory()?.input_from_format(format, text, timezone, explicit)
    }

    pub fn use_strict_mode(enabled: impl Into<Option<bool>>) -> Result<(), DateError> {
        Self::factory()?.use_strict_mode(enabled);
        Ok(())
    }
    pub fn is_strict_mode_enabled() -> Result<bool, DateError> {
        Ok(Self::factory()?.is_strict_mode_enabled())
    }
    pub fn create_from_format(
        format: &str,
        input: impl Into<DateInput>,
        timezone: Option<DateTimezone>,
    ) -> Result<Option<Date>, DateError> {
        Self::factory()?.create_from_format(format, input, timezone)
    }
    pub fn raw_create_from_format(
        format: &str,
        input: impl Into<DateInput>,
        timezone: Option<DateTimezone>,
    ) -> Result<Option<Date>, DateError> {
        Self::factory()?.raw_create_from_format(format, input, timezone)
    }
}
