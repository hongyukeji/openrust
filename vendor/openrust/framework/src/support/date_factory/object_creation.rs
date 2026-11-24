//! 原生日历对象与带时间ID创建；ID解析复用既有UUID/ULID库。
use super::{Date, DateError, DateFacade, DateFactory, DateInput, DateTimezone};
use serde_json::Value;

pub enum DateId {
    Text(String),
    Uuid(uuid::Uuid),
    Ulid(ulid::Ulid),
}
impl From<&str> for DateId {
    fn from(value: &str) -> Self {
        Self::Text(value.into())
    }
}
impl From<&String> for DateId {
    fn from(value: &String) -> Self {
        Self::Text(value.clone())
    }
}
impl From<String> for DateId {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}
impl From<uuid::Uuid> for DateId {
    fn from(value: uuid::Uuid) -> Self {
        Self::Uuid(value)
    }
}
impl From<&uuid::Uuid> for DateId {
    fn from(value: &uuid::Uuid) -> Self {
        Self::Uuid(*value)
    }
}
impl From<ulid::Ulid> for DateId {
    fn from(value: ulid::Ulid) -> Self {
        Self::Ulid(value)
    }
}
impl From<&ulid::Ulid> for DateId {
    fn from(value: &ulid::Ulid) -> Self {
        Self::Ulid(*value)
    }
}
impl DateFactory {
    pub fn make(
        &self,
        input: impl Into<DateInput>,
        timezone: Option<DateTimezone>,
    ) -> Result<Option<Date>, DateError> {
        match input.into() {
            DateInput::Date(date) => Ok(Some(date)),
            DateInput::Text(text) | DateInput::Json(Value::String(text)) => {
                let text = text.trim_matches([' ', '\t', '\n', '\r', '\0', '\x0b']);
                let bytes = text.as_bytes();
                if !bytes.iter().any(u8::is_ascii_alphanumeric)
                    || (bytes.first() == Some(&b'P')
                        && bytes
                            .get(1)
                            .is_some_and(|c| c.is_ascii_digit() || *c == b'T'))
                    || (bytes.first() == Some(&b'R')
                        && bytes.get(1).is_some_and(u8::is_ascii_digit))
                {
                    return Ok(None);
                }
                self.parse(text, timezone).map(Some)
            }
            _ => Ok(None),
        }
    }
    pub fn create_from_interface(&self, input: impl Into<DateInput>) -> Result<Date, DateError> {
        match input.into() {
            DateInput::Date(date) => Ok(date),
            _ => Err(DateError::InvalidExpression),
        }
    }
    pub fn create_from_immutable(&self, input: impl Into<DateInput>) -> Result<Date, DateError> {
        self.create_from_interface(input)
    }
    pub fn create_from_id(&self, id: impl Into<DateId>) -> Result<Date, DateError> {
        match id.into() {
            DateId::Text(text) => {
                if text.len() == 26
                    && text.as_bytes()[0] <= b'7'
                    && let Ok(id) = ulid::Ulid::from_string(&text)
                {
                    return self.create_from_id(id);
                }
                let text = if text
                    .get(..9)
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case("urn:uuid:"))
                {
                    format!("urn:uuid:{}", &text[9..])
                } else {
                    text
                };
                self.create_from_id(
                    uuid::Uuid::parse_str(&text).map_err(|_| DateError::InvalidExpression)?,
                )
            }
            DateId::Ulid(id) => self.create_from_timestamp_ms(id.timestamp_ms(), None),
            DateId::Uuid(mut id) => {
                if id.get_variant() != uuid::Variant::RFC4122 {
                    return Err(DateError::InvalidExpression);
                }
                // DCE v2丢弃local identifier的32位，将其作为零时刻分量读取。
                if id.get_version_num() == 2 {
                    let mut bytes = *id.as_bytes();
                    bytes[..4].fill(0);
                    bytes[6] = (bytes[6] & 15) | 16;
                    id = uuid::Uuid::from_bytes(bytes);
                }
                let timestamp = id.get_timestamp().ok_or(DateError::InvalidExpression)?;
                if id.get_version_num() == 7 {
                    let (seconds, nanos) = timestamp.to_unix();
                    return self
                        .create_from_timestamp(format!("{seconds}.{:06}", nanos / 1000), None);
                }
                let ticks = i128::from(timestamp.to_gregorian().0)
                    - i128::from(uuid::timestamp::UUID_TICKS_BETWEEN_EPOCHS);
                self.create_from_timestamp(uuid_time(ticks)?, None)
            }
        }
    }
}
// 固定比较器的默认PHP时间转换先尝试14位double文本，精度不足时改用整数half-up。
#[allow(
    clippy::cast_precision_loss,
    reason = "此路径显式复现固定比较器的PHP double转换，精度不足时使用整数后备"
)]
fn uuid_time(ticks: i128) -> Result<String, DateError> {
    let text = crate::support::input_scalars::double_string(ticks as f64 / 10_000_000.0);
    if text.contains(['e', 'E']) {
        return Err(DateError::InvalidExpression);
    }
    if let Some((seconds, fraction)) = text.split_once('.')
        && !(fraction.len() < 6 && text.len() >= 14)
    {
        let microseconds = if fraction.len() > 6 {
            let first = fraction[..6]
                .parse::<u32>()
                .map_err(|_| DateError::InvalidExpression)?;
            (first + u32::from(fraction.as_bytes()[6] >= b'5')).to_string()
        } else {
            fraction.into()
        };
        if microseconds.len() > 6 {
            return Err(DateError::InvalidExpression);
        }
        return Ok(format!("{seconds}.{microseconds:0<6}"));
    }
    let micros = (ticks.abs() + 5) / 10;
    Ok(format!(
        "{}{}.{:06}",
        if ticks < 0 { "-" } else { "" },
        micros / 1_000_000,
        micros % 1_000_000
    ))
}
impl DateFacade {
    pub fn make(
        input: impl Into<DateInput>,
        timezone: Option<DateTimezone>,
    ) -> Result<Option<Date>, DateError> {
        Self::factory()?.make(input, timezone)
    }
    pub fn create_from_interface(input: impl Into<DateInput>) -> Result<Date, DateError> {
        Self::factory()?.create_from_interface(input)
    }
    pub fn create_from_immutable(input: impl Into<DateInput>) -> Result<Date, DateError> {
        Self::factory()?.create_from_immutable(input)
    }
    pub fn create_from_id(id: impl Into<DateId>) -> Result<Date, DateError> {
        Self::factory()?.create_from_id(id)
    }
}
