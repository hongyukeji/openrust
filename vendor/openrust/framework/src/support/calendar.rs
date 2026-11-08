//! 请求日期的显式时钟、时区与保留扩展年份的表示。
mod date;
mod interval;
mod unit;
pub(crate) use date::model_numeric_date;
pub use date::{
    CalendarClock, CalendarContext, CalendarInstant, DateConstruction, DateOptions,
    DateStringFormat, FixedClock, InputDate, ResolvedTimezone, SystemClock, TimezoneInput,
    format_php_numeric_timestamp, parse_date, parse_php_numeric_timestamp,
};

pub use interval::{InputInterval, IntervalError};
pub use unit::IntervalUnit;

pub(crate) fn parse_interval(
    value: Option<&serde_json::Value>,
    unit: Option<&str>,
) -> crate::Result<Option<InputInterval>> {
    InputInterval::parse(value, unit).map_err(|error| {
        let (status, code) = match error {
            IntervalError::Limit => (http::StatusCode::PAYLOAD_TOO_LARGE, "input_limits_exceeded"),
            IntervalError::InvalidUnit => {
                (http::StatusCode::BAD_REQUEST, "input_interval_unit_invalid")
            }
            IntervalError::OutOfRange => {
                (http::StatusCode::BAD_REQUEST, "input_calendar_out_of_range")
            }
            IntervalError::InvalidSyntax => {
                (http::StatusCode::BAD_REQUEST, "input_interval_invalid")
            }
            IntervalError::UnsupportedRelative => {
                (http::StatusCode::BAD_REQUEST, "input_interval_unsupported")
            }
        };
        crate::Error::custom_with_status(status, code, error.to_string())
    })
}
