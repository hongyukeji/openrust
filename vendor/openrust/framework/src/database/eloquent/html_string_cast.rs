//! Laravel 的 HTML 字符串类转换。

use super::{
    AttributeCast, AttributeMap, AttributeValue, ClassCast, ClassCastWrite, ClassCaster,
    ModelObject, stringable_cast::cast_string,
};
use crate::{Result, support::HtmlString};
use serde_json::Value;

/// 读取为 `HtmlString`，在模型数组和 JSON 中输出原始标记字符串。
pub struct AsHtmlString;

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
        Ok(ModelObject::new(HtmlString::new(cast_string(value)?)).into())
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
        cast_string(value).map(Value::String)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        database::eloquent::{ModelAttributes, ModelMetadata},
        support::{Htmlable, Stringable},
    };
    use serde_json::{Map, json};

    fn metadata() -> ModelMetadata {
        let mut metadata = ModelMetadata::new("records", "HtmlRecord");
        metadata.casts.insert("body".into(), AsHtmlString::cast());
        metadata
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
                .and_then(|value| value.as_any().downcast_ref::<HtmlString>())
                .map(Htmlable::to_html),
            Some(original)
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
        assert!(HtmlString::default().is_empty());
        assert!(HtmlString::new(" ").is_not_empty());
        Ok(())
    }
}
