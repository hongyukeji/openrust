//! 转换模型日期，并分离存储格式与 cast 的输出格式。

use super::{AttributeCast, AttributeValue, ModelDate, ModelMetadata};
use crate::{
    Db, Error, Result,
    support::{
        CalendarContext, DateConstruction, DateOptions, InputDate,
        calendar::{model_numeric_date, parse_date},
    },
};
use serde_json::Value;
use std::sync::Arc;

type DateSerializer = dyn Fn(&InputDate) -> Result<String> + Send + Sync;

/// 模型日期解析使用的应用时钟、时区与已选连接的日期格式。
#[derive(Clone)]
pub struct ModelDateContext {
    calendar: CalendarContext,
    grammar_format: String,
    serializer: Option<Arc<DateSerializer>>,
}

impl std::fmt::Debug for ModelDateContext {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ModelDateContext")
            .field("calendar", &self.calendar)
            .field("grammar_format", &self.grammar_format)
            .finish_non_exhaustive()
    }
}

impl Default for ModelDateContext {
    fn default() -> Self {
        Self::new(CalendarContext::default(), "Y-m-d H:i:s")
    }
}

impl ModelDateContext {
    /// 以应用 Calendar 和已选连接 grammar 的 PHP 格式构造上下文。
    pub fn new(calendar: CalendarContext, grammar_format: impl Into<String>) -> Self {
        Self {
            calendar,
            grammar_format: grammar_format.into(),
            serializer: None,
        }
    }

    /// 使用应用时钟、日期工厂和已选连接的查询语法，不获取物理连接。
    pub fn for_connection(calendar: CalendarContext, database: &Db) -> Result<Self> {
        Ok(Self::new(calendar, database.query_date_format()?))
    }

    /// 覆写当前模型普通日期及有类型访问器日期的序列化。
    /// 带格式的 cast 继续使用其自身格式，数据库绑定不使用此回调。
    #[must_use]
    pub fn with_serializer(
        mut self,
        serializer: impl Fn(&InputDate) -> Result<String> + Send + Sync + 'static,
    ) -> Self {
        self.serializer = Some(Arc::new(serializer));
        self
    }

    pub(super) fn serialize(&self, date: &InputDate) -> Result<Value> {
        match &self.serializer {
            Some(serializer) => serializer(date).map(Value::String),
            None => serde_json::to_value(date).map_err(Error::internal),
        }
    }

    pub(super) fn serialize_date_attribute(
        &self,
        metadata: &ModelMetadata,
        value: &AttributeValue,
    ) -> Result<AttributeValue> {
        self.as_date_time(value, self.storage_format(metadata))
            .and_then(|date| self.serialize(&date))
            .map(AttributeValue::from)
    }

    fn storage_format<'a>(&'a self, metadata: &'a ModelMetadata) -> &'a str {
        metadata
            .date_format
            .as_deref()
            .filter(|format| !matches!(*format, "" | "0"))
            .unwrap_or(&self.grammar_format)
    }

    fn as_date_time(&self, value: &AttributeValue, format: &str) -> Result<InputDate> {
        if let Some(date) = value.as_date() {
            return self.construct(DateConstruction::Instance(&date.snapshot()?));
        }
        let value = value.as_json()?;
        if let Some(instant) = model_numeric_date(value)? {
            return self.construct(DateConstruction::FromTimestamp {
                source: value,
                instant,
            });
        }
        if let Value::String(text) = value
            && is_standard_date(text)
        {
            let date = parse_date(
                value,
                DateOptions {
                    format: Some("!Y-m-d"),
                    timezone: None,
                },
                &self.calendar,
            )?
            .ok_or_else(invalid_date)?;
            return self.construct(DateConstruction::Instance(&date));
        }
        match self
            .calendar
            .make_date(DateConstruction::FromFormat { value, format })
        {
            Ok(Some(date)) => Ok(date),
            Ok(None) => self.construct(DateConstruction::Parse(value)),
            Err(error)
                if error.code() == "input_date_invalid"
                    || error.code() == "input_date_format_invalid" =>
            {
                self.construct(DateConstruction::Parse(value))
            }
            Err(error) => Err(error),
        }
    }

    fn construct(&self, operation: DateConstruction<'_>) -> Result<InputDate> {
        self.calendar.make_date(operation)?.ok_or_else(invalid_date)
    }

    pub(super) fn read(
        &self,
        metadata: &ModelMetadata,
        cast: &AttributeCast,
        value: AttributeValue,
    ) -> Result<AttributeValue> {
        if value.is_null() {
            return Ok(value);
        }
        let mut date = self.as_date_time(&value, self.storage_format(metadata))?;
        if matches!(cast, AttributeCast::Timestamp) {
            return Ok(Value::from(date.timestamp()).into());
        }
        if matches!(cast, AttributeCast::Date | AttributeCast::ImmutableDate) {
            date = date.start_of_day()?;
        }
        let immutable = matches!(
            cast,
            AttributeCast::ImmutableDate
                | AttributeCast::ImmutableDateTime
                | AttributeCast::ImmutableCustomDateTime(_)
        );
        Ok(ModelDate::new(date, immutable).into())
    }

    pub(super) fn store(
        &self,
        metadata: &ModelMetadata,
        value: AttributeValue,
    ) -> Result<AttributeValue> {
        if value.as_json().is_ok_and(is_empty) {
            return Ok(value);
        }
        let format = self.storage_format(metadata);
        let date = self.as_date_time(&value, format)?;
        Ok(Value::String(date.format(format)?).into())
    }
}

fn invalid_date() -> Error {
    Error::custom_with_status(
        http::StatusCode::BAD_REQUEST,
        "model_attribute_date_invalid",
        "模型日期属性无法解析",
    )
}

fn is_standard_date(text: &str) -> bool {
    let mut parts = text.split('-');
    let digits = |text: &str, range: std::ops::RangeInclusive<usize>| {
        range.contains(&text.len()) && text.bytes().all(|byte| byte.is_ascii_digit())
    };
    parts.next().is_some_and(|year| digits(year, 4..=4))
        && parts.next().is_some_and(|month| digits(month, 1..=2))
        && parts.next().is_some_and(|day| digits(day, 1..=2))
        && parts.next().is_none()
}

fn is_empty(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Bool(value) => !value,
        Value::String(value) => matches!(value.as_str(), "" | "0"),
        Value::Number(value) => value.as_f64() == Some(0.0),
        Value::Array(value) => value.is_empty(),
        Value::Object(value) => value.is_empty(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        database::eloquent::ModelAttributes,
        support::{CalendarInstant, FixedClock, ResolvedTimezone},
    };
    use serde_json::{Map, json};
    use std::sync::Arc;

    fn context(format: &str) -> Result<ModelDateContext> {
        Ok(ModelDateContext::new(
            CalendarContext::new(
                ResolvedTimezone::parse("America/New_York")?,
                Arc::new(FixedClock::new(CalendarInstant::new(946_782_245, 123_456)?)),
            ),
            format,
        ))
    }

    #[test]
    fn date_factory_receives_eloquent_operations_and_can_replace_results() -> Result<()> {
        let called = Arc::new(std::sync::Mutex::new(Vec::new()));
        let dates = ModelDateContext::new(
            context("Y-m-d H:i:s")?.calendar.with_date_factory({
                let called = called.clone();
                move |operation, calendar| {
                    let label = match operation {
                        DateConstruction::Now => "now",
                        DateConstruction::Instance(_) => "instance",
                        DateConstruction::FromTimestamp { source, .. } => {
                            assert_eq!(source, "12");
                            "timestamp"
                        }
                        DateConstruction::FromFormat { .. } => "format",
                        DateConstruction::Parse(_) => "parse",
                    };
                    called
                        .lock()
                        .map_err(|_| super::super::attribute_store::state_error())?
                        .push(label);
                    operation
                        .default_date(calendar)?
                        .map(|date| {
                            Ok(InputDate::new(
                                CalendarInstant::new(date.timestamp() + 1, date.microseconds())?,
                                date.timezone().clone(),
                            ))
                        })
                        .transpose()
                }
            }),
            "Y-m-d H:i:s",
        );
        let metadata = ModelMetadata::new("events", "Event");
        let seconds = |value| -> Result<i64> {
            let result = dates.read(&metadata, &AttributeCast::DateTime, value)?;
            Ok(result
                .as_date()
                .ok_or_else(invalid_date)?
                .snapshot()?
                .timestamp())
        };
        assert_eq!(seconds(json!("12").into())?, 13);
        assert_eq!(
            seconds(json!("2024-03-10").into())?,
            seconds(json!("2024-03-10 00:00:00").into())?
        );
        assert_eq!(
            seconds(json!("2024-03-10T05:00:00Z").into())?,
            1_710_046_801
        );
        assert_eq!(
            *called
                .lock()
                .map_err(|_| super::super::attribute_store::state_error())?,
            ["timestamp", "instance", "format", "format", "parse"]
        );
        Ok(())
    }

    #[test]
    fn model_date_serializer_preserves_cast_and_accessor_precedence() -> Result<()> {
        use crate::database::eloquent::ModelAttribute;

        let mut metadata = ModelMetadata::new("events", "Event");
        metadata
            .casts
            .insert("ordinary".into(), AttributeCast::DateTime);
        for key in ["custom", "accessor"] {
            metadata
                .casts
                .insert(key.into(), AttributeCast::CustomDateTime("U.u".into()));
        }
        let model = ModelAttributes::new(Map::from_iter([
            ("ordinary".into(), json!("10")),
            ("custom".into(), json!("10")),
        ]))
        .with_date_context(
            context("Y-m-d H:i:s")?
                .with_serializer(|date| Ok(format!("model:{}", date.timestamp()))),
        );
        let getter_date = ModelDate::new(
            InputDate::new(
                CalendarInstant::new(20, 0)?,
                crate::support::ResolvedTimezone::utc(),
            ),
            false,
        );
        model.register_attribute(
            "accessor",
            ModelAttribute::new().get(move |_, _| Ok(getter_date.clone().into())),
        )?;
        let projection = |name| {
            let value = model
                .get_attribute_value(&metadata, name)?
                .ok_or_else(invalid_date)?;
            model.project_attribute_json(&metadata, name, value)
        };
        assert_eq!(projection("ordinary")?, json!("model:10"));
        assert_eq!(projection("custom")?, json!("10.000000"));
        assert_eq!(projection("accessor")?, json!("model:20"));
        assert_eq!(model.raw_attribute("ordinary")?, Some(json!("10")));
        Ok(())
    }

    #[test]
    fn attribute_array_serializes_date_list_before_getters_and_casts() -> Result<()> {
        use crate::database::eloquent::ModelAttribute;

        let mut metadata = ModelMetadata::new("events", "Event");
        metadata.hidden = vec!["hidden".into()];
        metadata.date_attributes = vec!["date_list".into(), "hidden".into()];
        metadata
            .casts
            .insert("date_list".into(), AttributeCast::Integer);
        metadata
            .casts
            .insert("custom".into(), AttributeCast::CustomDateTime("U.u".into()));
        let model = ModelAttributes::new(Map::from_iter([
            ("date_list".into(), json!("10")),
            ("custom".into(), json!("10")),
            ("hidden".into(), json!("invalid date")),
            ("nullable".into(), Value::Null),
        ]))
        .with_date_context(
            ModelDateContext::default()
                .with_serializer(|date| Ok(format!("formatted:{}", date.timestamp()))),
        );
        model.register_attribute(
            "date_list",
            ModelAttribute::new().get(|value, original| {
                assert_eq!(
                    value.ok_or_else(invalid_date)?.as_json()?,
                    &json!("formatted:10")
                );
                assert_eq!(original["date_list"].as_json()?, &json!("10"));
                assert!(original.contains_key("hidden"));
                Ok(json!("getter").into())
            }),
        )?;
        let uncast = ModelDate::new(
            InputDate::new(
                CalendarInstant::new(20, 0)?,
                crate::support::ResolvedTimezone::utc(),
            ),
            false,
        );
        model.set_raw_attribute_value("raw", uncast.clone().into())?;
        let values = model.attributes_to_array(&metadata)?;
        assert_eq!(values["date_list"].as_json()?, &json!("getter"));
        assert_eq!(values["custom"].as_json()?, &json!("10.000000"));
        assert!(!values.contains_key("hidden"));
        assert!(values["nullable"].is_null());
        assert!(
            values["raw"]
                .as_date()
                .ok_or_else(invalid_date)?
                .same_object(&uncast)
        );
        assert_eq!(
            model.attributes_to_json(&metadata)?["raw"],
            json!("1970-01-01T00:00:20.000000Z")
        );
        assert_eq!(model.raw_attribute("date_list")?, Some(json!("10")));
        Ok(())
    }

    #[tokio::test]
    async fn date_casts_use_distinct_read_store_and_output_rules() -> Result<()> {
        let dates = context("Y/m/d H:i:s.u")?;
        let mut metadata = ModelMetadata::new("events", "Event");
        metadata.casts.insert("day".into(), AttributeCast::Date);
        metadata.casts.insert(
            "custom".into(),
            AttributeCast::ImmutableCustomDateTime("Y-m-d H:i P".into()),
        );
        metadata
            .casts
            .insert("timestamp".into(), AttributeCast::Timestamp);
        let raw = Map::from_iter([
            ("day".into(), json!("2024-03-10 23:30:00")),
            ("custom".into(), json!("2024-03-10 23:30:00")),
            ("timestamp".into(), json!("0")),
        ]);
        let model = ModelAttributes::new(raw).with_date_context(dates);
        let read = |name| {
            model
                .get_attribute_value(&metadata, name)?
                .ok_or_else(super::super::attribute_store::state_error)
        };
        let first = read("day")?;
        let second = read("day")?;
        let first = first
            .as_date()
            .ok_or_else(super::super::attribute_store::state_error)?;
        let second = second
            .as_date()
            .ok_or_else(super::super::attribute_store::state_error)?;
        assert!(!first.same_object(second));
        assert_eq!(
            first.snapshot()?.format("Y-m-d H:i P")?,
            "2024-03-10 00:00 -05:00"
        );
        assert_eq!(
            metadata.project_attribute_json("day", first.clone().into())?,
            json!("2024-03-10T05:00:00.000000Z")
        );
        let custom = read("custom")?;
        assert!(custom.as_date().is_some_and(ModelDate::is_immutable));
        assert_eq!(
            metadata.project_attribute_json("custom", custom)?,
            json!("2024-03-10 23:30 -04:00")
        );
        assert_eq!(read("timestamp")?.try_into_json()?, json!(0));
        assert!(model.get_attribute(&metadata, "day").is_err());

        model
            .set_attribute(&metadata, "day", json!("2024-01-02 08:09:10.123456"))
            .await?;
        assert_eq!(
            model.raw_attribute("day")?,
            Some(json!("2024/01/02 08:09:10.123456"))
        );
        model
            .set_attribute(&metadata, "custom", json!("2024/03/10 23:30:00.000000"))
            .await?;
        assert_eq!(
            model.raw_attribute("custom")?,
            Some(json!("2024/03/10 23:30:00.000000"))
        );
        assert!(!model.is_dirty(&metadata, &["custom"])?);

        model
            .set_attribute_value(&metadata, "updated_at", first.clone().into())
            .await?;
        assert_eq!(
            model.raw_attribute("updated_at")?,
            Some(json!("2024/03/10 00:00:00.000000"))
        );
        assert!(read("updated_at")?.as_date().is_some());
        Ok(())
    }

    #[test]
    fn model_dates_follow_numeric_empty_and_dst_branches() -> Result<()> {
        let dates = context("Y-m-d H:i:s")?;
        let metadata = ModelMetadata::new("events", "Event");
        let cast = AttributeCast::DateTime;
        for (raw, expected) in [
            (
                json!("2024-03-10 02:30:00"),
                "2024-03-10 03:30:00.000000 -04:00",
            ),
            (
                json!("2024-11-03 01:30:00"),
                "2024-11-03 01:30:00.000000 -04:00",
            ),
            (
                json!("2024-11-03T01:30:00-05:00"),
                "2024-11-03 01:30:00.000000 -05:00",
            ),
            (
                json!("1710055800.123456"),
                "2024-03-10 03:30:00.123456 -04:00",
            ),
            (json!("2024-3-9"), "2024-03-09 00:00:00.000000 -05:00"),
            (json!(""), "2000-01-01 22:04:05.123456 -05:00"),
        ] {
            let value = dates.read(&metadata, &cast, raw.into())?;
            let date = value
                .as_date()
                .ok_or_else(super::super::attribute_store::state_error)?;
            assert_eq!(date.snapshot()?.format("Y-m-d H:i:s.u P")?, expected);
        }
        assert!(dates.read(&metadata, &cast, Value::Null.into())?.is_null());
        assert_eq!(
            dates.store(&metadata, json!("0").into())?.try_into_json()?,
            json!("0")
        );
        Ok(())
    }
}
