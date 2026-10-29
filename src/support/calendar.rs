//! 请求日期的显式时钟、时区与保留扩展年份的表示。
mod date;
mod interval;
mod unit;
pub use date::{
    CalendarClock, CalendarContext, CalendarInstant, DateOptions, FixedClock, InputDate,
    ResolvedTimezone, SystemClock, TimezoneInput, parse_date,
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
