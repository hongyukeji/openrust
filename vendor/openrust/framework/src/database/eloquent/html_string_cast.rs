//! HTML 字符串属性转换。

use super::{
    AttributeCast, AttributeMap, AttributeValue, ClassCast, ClassCastWrite, ClassCaster,
    ModelObject, attribute_cast::scalar_string,
};
use crate::{
    Error, Result,
    support::{DataObject, HtmlString, Htmlable},
};
use serde_json::Value;
use std::{any::Any, borrow::Cow};

/// 读取为 `HtmlString`，在模型数组和 JSON 中输出原始标记字符串。
pub struct AsHtmlString;

/// 保留 Eloquent 原始值的 HTML 字符串，包括原始 JSON 形状和对象身份。
pub type ModelHtmlString = HtmlString<AttributeValue>;

impl ModelHtmlString {
    /// 判断原始值是否为空字符串或 null。布尔值和数值保持原类型。
    pub fn is_empty(&self) -> bool {
        matches!(self.to_html(), AttributeValue::Json(value) if value.is_null() || value.as_str() == Some(""))
    }

    /// 判断原始值是否包含内容。
    pub fn is_not_empty(&self) -> bool {
        !self.is_empty()
    }
}

impl Htmlable for ModelHtmlString {
    type Raw = AttributeValue;

    fn to_html(&self) -> &AttributeValue {
        self.raw()
    }

    fn to_html_text(&self) -> Result<Cow<'_, str>> {
        render_html_text(self.to_html(), 0)
    }
}

impl DataObject for ModelHtmlString {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn string_projection(&self) -> Option<String> {
        self.to_html_text().ok().map(Cow::into_owned)
    }
}

impl AsHtmlString {
    /// 创建有对象缓存及显式序列化的模型转换。
    pub fn cast() -> AttributeCast {
        AttributeCast::Class(ClassCast::new(|| Ok(HtmlStringCast)).with_serialization())
    }
}

struct HtmlStringCast;

impl ClassCaster for HtmlStringCast {
    fn get_value(
        &self,
        _: &str,
        value: Option<&AttributeValue>,
        _: &AttributeMap,
    ) -> Result<AttributeValue> {
        let Some(value) = value.filter(|value| !value.is_null()) else {
            return Ok(Value::Null.into());
        };
        Ok(ModelObject::new(ModelHtmlString::from_raw(value.clone())).into())
    }

    fn set(&self, _: &str, value: AttributeValue, _: &AttributeMap) -> Result<ClassCastWrite> {
        Ok(ClassCastWrite::Value(html_text(&value)?.into()))
    }

    fn serialize_value(&self, _: &str, value: AttributeValue, _: &AttributeMap) -> Result<Value> {
        html_text(&value)
    }
}

fn html_text(value: &AttributeValue) -> Result<Value> {
    if value.is_null() {
        Ok(Value::Null)
    } else {
        render_html_text(value, 0).map(|text| Value::String(text.into_owned()))
    }
}

fn render_html_text(value: &AttributeValue, depth: usize) -> Result<Cow<'_, str>> {
    match value {
        AttributeValue::Json(Value::String(text)) => Ok(Cow::Borrowed(text)),
        AttributeValue::Json(value) => scalar_string(value)
            .map(Cow::Owned)
            .map_err(|_| invalid_html_source()),
        AttributeValue::Date(date) => date.snapshot()?.to_string_value().map(Cow::Owned),
        AttributeValue::Enum(_) => Err(invalid_html_source()),
        AttributeValue::Object(object) => {
            // 版本化对象可以在水合后形成自引用。限制内置 HTML 包装的递归转换。
            if depth >= 64 {
                return Err(invalid_html_source());
            }
            let raw = object.snapshot()?;
            if let Some(html) = raw.as_any().downcast_ref::<ModelHtmlString>() {
                return render_html_text(html.to_html(), depth + 1)
                    .map(|text| Cow::Owned(text.into_owned()));
            }
            raw.string_projection()
                .map(Cow::Owned)
                .ok_or_else(invalid_html_source)
        }
    }
}

fn invalid_html_source() -> Error {
    Error::custom(
        "model_html_string_conversion",
        "模型 HTML 内容无法转换为字符串",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        database::eloquent::{EnumCast, ModelAttributes, ModelDate, ModelMetadata},
        support::{CalendarInstant, DateStringFormat, InputDate, ResolvedTimezone, Stringable},
    };
    use serde_json::{Map, json};

    fn metadata() -> ModelMetadata {
        let mut metadata = ModelMetadata::new("records", "HtmlRecord");
        metadata.casts.insert("body".into(), AsHtmlString::cast());
        metadata
    }

    fn read_html(attrs: &ModelAttributes, metadata: &ModelMetadata) -> Result<ModelHtmlString> {
        attrs
            .get_attribute_value(metadata, "body")?
            .as_ref()
            .and_then(AttributeValue::as_object)
            .map(ModelObject::snapshot)
            .transpose()?
            .as_deref()
            .and_then(|value| value.as_any().downcast_ref::<ModelHtmlString>())
            .cloned()
            .ok_or_else(invalid_html_source)
    }

    #[tokio::test]
    async fn html_cast_keeps_object_on_read_and_serializes_as_text() -> Result<()> {
        let metadata = metadata();
        let original = "<p>&</p>";
        let attrs = ModelAttributes::new(Map::from_iter([("body".into(), json!(original))]));
        let read = attrs.get_attribute_value(&metadata, "body")?;
        let object = read.as_ref().and_then(AttributeValue::as_object);
        let snapshot = object.map(ModelObject::snapshot).transpose()?;
        assert_eq!(
            snapshot
                .as_deref()
                .and_then(|value| value.as_any().downcast_ref::<ModelHtmlString>())
                .map(Htmlable::to_html)
                .map(AttributeValue::as_json)
                .transpose()?,
            Some(&json!(original))
        );
        let reread = attrs.get_attribute_value(&metadata, "body")?;
        assert!(
            reread
                .as_ref()
                .and_then(AttributeValue::as_object)
                .zip(object)
                .is_some_and(|(left, right)| left.same_object(right))
        );
        assert_eq!(
            attrs.attributes_to_array(&metadata)?["body"].as_json()?,
            &json!(original)
        );
        assert_eq!(
            attrs.attributes_to_json(&metadata)?["body"],
            json!(original)
        );

        let assigned = ModelObject::new(Stringable::from(original));
        attrs
            .set_attribute_value(&metadata, "body", assigned.clone().into())
            .await?;
        assert!(attrs.is_clean(&metadata, &["body"])?);
        assigned.modify::<Stringable>(|_| Ok(Stringable::from("<strong>更新</strong>")))?;
        assert_eq!(
            attrs.raw_attribute("body")?,
            Some(json!("<strong>更新</strong>"))
        );
        assert_eq!(
            attrs.attributes_to_json(&metadata)?["body"],
            json!("<strong>更新</strong>")
        );
        assert!(attrs.is_dirty(&metadata, &["body"])?);
        Ok(())
    }

    #[tokio::test]
    async fn html_cast_preserves_null_and_rejects_invalid_assignment_atomically() -> Result<()> {
        let metadata = metadata();
        let attrs = ModelAttributes::default();
        assert!(!attrs.attributes_to_array(&metadata)?.contains_key("body"));
        attrs.set_attribute(&metadata, "body", Value::Null).await?;
        assert_eq!(attrs.get_attribute(&metadata, "body")?, Some(Value::Null));
        for (value, text) in [
            (json!(false), ""),
            (json!(true), "1"),
            (json!(17), "17"),
            (json!(1.5), "1.5"),
        ] {
            attrs.set_raw_attribute("body", value.clone())?;
            let html = read_html(&attrs, &metadata)?;
            assert_eq!(html.to_html().as_json()?, &value);
            assert!(html.is_not_empty());
            assert_eq!(html.to_html_text()?, text);
            assert_eq!(attrs.attributes_to_json(&metadata)?["body"], text);
            attrs.set_attribute(&metadata, "body", value).await?;
            assert_eq!(attrs.raw_attribute("body")?, Some(json!(text)));
        }
        assert!(
            attrs
                .set_attribute(&metadata, "body", json!([]))
                .await
                .is_err()
        );
        assert_eq!(attrs.raw_attribute("body")?, Some(json!("1.5")));
        assert!(HtmlString::<String>::default().is_empty());
        assert!(HtmlString::new(" ").is_not_empty());
        Ok(())
    }

    #[test]
    fn html_cast_defers_mixed_value_errors_until_text_is_required() -> Result<()> {
        let metadata = metadata();
        for raw in [json!([]), json!({"a":1})] {
            let attrs = ModelAttributes::new(Map::from_iter([("body".into(), raw.clone())]));
            let html = read_html(&attrs, &metadata)?;
            assert_eq!(html.to_html().as_json()?, &raw);
            assert!(html.is_not_empty());
            assert!(html.to_html_text().is_err());
            assert!(attrs.attributes_to_array(&metadata).is_err());
            assert!(attrs.get_attribute_value(&metadata, "body").is_err());
            assert_eq!(attrs.get_raw_original("body")?, Some(raw));
        }
        let case =
            EnumCast::unit(|name| (name == "Active").then_some(()), |()| "Active").case(())?;
        let attrs = ModelAttributes::from_attribute_values(AttributeMap::from_iter([(
            "body".into(),
            case.into(),
        )]));
        let html = read_html(&attrs, &metadata)?;
        assert!(html.to_html().as_enum().is_some());
        assert!(html.to_html_text().is_err());
        Ok(())
    }

    #[tokio::test]
    async fn html_cast_retains_live_objects_and_projects_dates_in_local_time() -> Result<()> {
        let metadata = metadata();
        let inner = ModelObject::new(Stringable::from("before"));
        let attrs = ModelAttributes::from_attribute_values(AttributeMap::from_iter([(
            "body".into(),
            inner.clone().into(),
        )]));
        let html = read_html(&attrs, &metadata)?;
        assert!(
            html.to_html()
                .as_object()
                .is_some_and(|object| object.same_object(&inner))
        );
        inner.modify::<Stringable>(|_| Ok(Stringable::from("after")))?;
        assert_eq!(attrs.attributes_to_json(&metadata)?["body"], "after");

        let date = ModelDate::new(
            InputDate::new(
                CalendarInstant::new(1_794_114_000, 123_456)?,
                ResolvedTimezone::parse("Asia/Shanghai")?,
            ),
            false,
        );
        attrs.set_raw_attribute_value("body", date.clone().into())?;
        let html = read_html(&attrs, &metadata)?;
        assert!(
            html.to_html()
                .as_date()
                .is_some_and(|value| value.same_object(&date))
        );
        assert_eq!(html.to_html_text()?, "2026-11-08 13:00:00");
        date.modify(|value| {
            Ok(value.with_string_format(Some(DateStringFormat::pattern("Y/m/d H:i"))))
        })?;
        assert_eq!(html.to_html_text()?, "2026/11/08 13:00");
        attrs
            .set_attribute_value(&metadata, "body", date.into())
            .await?;
        assert_eq!(
            attrs.raw_attribute("body")?,
            Some(json!("2026/11/08 13:00"))
        );
        Ok(())
    }

    #[test]
    fn nested_html_sources_have_a_bounded_text_conversion() {
        let mut source = AttributeValue::from(json!("text"));
        for _ in 0..=64 {
            source = ModelObject::new(ModelHtmlString::from_raw(source)).into();
        }
        assert!(render_html_text(&source, 0).is_err());
    }
}
