use std::{borrow::Cow, fmt};

use jiff::{Error as ParseError, Zoned, fmt::temporal::DateTimeParser, tz::Offset};
use openrust::log::LogContext;
use serde::Deserialize;
use serde_json::Value;

mod json;
pub mod origin;
pub use origin::Origin;

/// A parsed capture record, separate from the framework's live log event.
#[derive(Clone, Debug, Deserialize)]
#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
pub struct MessageLogged {
    message: String,
    datetime: String,
    #[serde(rename = "level_name")]
    levelName: String,
    context: LogContext,
}

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl MessageLogged {
    pub fn fromJson(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
    pub fn message(&self) -> &str {
        &self.message
    }
    pub fn date(&self) -> Result<String, ParseError> {
        Ok(self
            .parsed_datetime()?
            .strftime("%Y-%m-%d %H:%M:%S")
            .to_string())
    }
    pub fn time(&self) -> Result<String, ParseError> {
        Ok(self.parsed_datetime()?.strftime("%H:%M:%S").to_string())
    }
    fn parsed_datetime(&self) -> Result<Zoned, ParseError> {
        let timestamp = self.datetime.parse::<jiff::Timestamp>()?;
        let pieces = DateTimeParser::new().parse_pieces(&self.datetime)?;
        let offset = pieces.to_numeric_offset().unwrap_or(Offset::UTC);
        Ok(timestamp.to_zoned(offset.to_time_zone()))
    }
    pub fn classOrType(&self) -> Cow<'_, str> {
        self.exception("class")
            .map(Cow::Borrowed)
            .unwrap_or_else(|| Cow::Owned(self.levelName.to_ascii_uppercase()))
    }
    pub fn color(&self) -> &'static str {
        match self.levelName.as_str() {
            "INFO" => "blue",
            "NOTICE" | "WARNING" => "yellow",
            "ERROR" | "CRITICAL" | "ALERT" | "EMERGENCY" => "red",
            _ => "gray",
        }
    }
    pub fn level(&self) -> &str {
        &self.levelName
    }
    pub fn file(&self) -> Option<&str> {
        self.exception("file")
    }
    pub fn authId(&self) -> Option<String> {
        let value = self.raw_origin().get("auth_id")?;
        match value {
            Value::String(value) => Some(value.clone()),
            Value::Number(value) => Some(value.to_string()),
            Value::Bool(value) => Some(if *value { "1" } else { "" }.into()),
            _ => None,
        }
    }
    pub fn origin(&self) -> Result<Origin, serde_json::Error> {
        Origin::fromArray(self.raw_origin())
    }
    pub fn trace(&self) -> Option<&[Value]> {
        self.raw_origin()
            .get("trace")?
            .as_array()
            .map(Vec::as_slice)
    }
    pub fn context(&self) -> LogContext {
        self.context
            .iter()
            .filter(|(key, _)| !matches!(key.as_str(), "__pail" | "exception" | "userId"))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()
    }
    fn raw_origin(&self) -> &Value {
        self.context
            .get("__pail")
            .and_then(|value| value.get("origin"))
            .unwrap_or(&Value::Null)
    }
    fn exception(&self, name: &str) -> Option<&str> {
        self.context.get("exception")?.get(name)?.as_str()
    }
}

impl fmt::Display for MessageLogged {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("{\"message\":")?;
        json::string(formatter, &self.message)?;
        formatter.write_str(",\"datetime\":")?;
        json::string(formatter, &self.datetime)?;
        formatter.write_str(",\"level_name\":")?;
        json::string(formatter, &self.levelName)?;
        formatter.write_str(",\"context\":")?;
        json::object(formatter, &self.context)?;
        formatter.write_str("}")
    }
}
