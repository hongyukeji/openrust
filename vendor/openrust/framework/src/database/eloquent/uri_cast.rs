//! Laravel 的 URI 类转换。

use super::{
    AttributeCast, AttributeMap, AttributeValue, ClassCast, ClassCastWrite, ClassCaster,
    ModelObject, stringable_cast::cast_string,
};
use crate::{Result, support::Uri};
use serde_json::Value;

/// 将属性读取为 Support `Uri` 对象，字符串赋值保留原始 URI 文本。
pub struct AsUri;

impl AsUri {
    /// 创建具有对象缓存的模型 URI 转换。
    pub fn cast() -> AttributeCast {
        AttributeCast::Class(ClassCast::new(|| Ok(UriCast)))
    }
}

struct UriCast;

impl ClassCaster for UriCast {
    fn get_value(
        &self,
        _: &str,
        value: Option<&AttributeValue>,
        _: &AttributeMap,
    ) -> Result<AttributeValue> {
        let Some(value) = value.filter(|value| !value.is_null()) else {
            return Ok(Value::Null.into());
        };
        Ok(ModelObject::new(Uri::of(&cast_string(value)?)?).into())
    }

    fn set(&self, _: &str, value: AttributeValue, _: &AttributeMap) -> Result<ClassCastWrite> {
        if value.is_null() {
            return Ok(ClassCastWrite::Value(value));
        }
        Ok(ClassCastWrite::Value(
            Value::String(cast_string(&value)?).into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::eloquent::{ModelAttributes, ModelMetadata};
    use serde_json::{Map, json};

    #[tokio::test]
    async fn uri_cast_keeps_read_and_assigned_object_identity_and_projects_json() -> Result<()> {
        let mut metadata = ModelMetadata::new("records", "UriRecord");
        metadata.casts.insert("link".into(), AsUri::cast());
        let attrs = ModelAttributes::new(Map::from_iter([(
            "link".into(),
            json!("HTTPS://Example.com:443/a?"),
        )]));
        let value = attrs.get_attribute_value(&metadata, "link")?;
        let value = value.as_ref().and_then(AttributeValue::as_object);
        let array = attrs.attributes_to_array(&metadata)?;
        assert!(
            value
                .zip(array["link"].as_object())
                .is_some_and(|(first, second)| first.same_object(second))
        );
        assert_eq!(
            attrs.attributes_to_json(&metadata)?["link"],
            "https://example.com/a?"
        );
        assert_eq!(
            attrs.raw_attribute("link")?,
            Some(json!("https://example.com/a?"))
        );
        assert!(attrs.is_dirty(&metadata, &["link"])?);

        let uri = ModelObject::new(Uri::of("../x#")?);
        attrs
            .set_attribute_value(&metadata, "link", uri.clone().into())
            .await?;
        assert!(
            attrs.attributes_to_array(&metadata)?["link"]
                .as_object()
                .is_some_and(|value| value.same_object(&uri))
        );
        uri.modify::<Uri>(|_| Uri::of("../new?x#"))?;
        assert_eq!(attrs.raw_attribute("link")?, Some(json!("../new?x#")));
        Ok(())
    }

    #[tokio::test]
    async fn uri_cast_defers_invalid_text_until_read_and_preserves_null() -> Result<()> {
        let mut metadata = ModelMetadata::new("records", "UriRecord");
        metadata.casts.insert("link".into(), AsUri::cast());
        let attrs = ModelAttributes::default();
        assert_eq!(
            attrs
                .get_attribute_value(&metadata, "link")?
                .and_then(|value| value.try_into_json().ok()),
            Some(Value::Null)
        );
        for (value, expected) in [
            (json!(true), "1"),
            (json!(13), "13"),
            (json!(1.5), "1.5"),
            (json!(""), ""),
        ] {
            attrs.set_attribute(&metadata, "link", value).await?;
            assert_eq!(attrs.attributes_to_json(&metadata)?["link"], expected);
        }
        attrs
            .set_attribute(&metadata, "link", json!("http://"))
            .await?;
        assert_eq!(attrs.raw_attribute("link")?, Some(json!("http://")));
        assert!(
            matches!(attrs.get_attribute_value(&metadata, "link"), Err(error) if error.code() == "support_uri_invalid")
        );
        assert!(
            attrs
                .set_attribute(&metadata, "link", json!([]))
                .await
                .is_err()
        );
        assert_eq!(attrs.raw_attribute("link")?, Some(json!("http://")));
        attrs.set_attribute(&metadata, "link", Value::Null).await?;
        assert_eq!(attrs.attributes_to_json(&metadata)?["link"], Value::Null);
        Ok(())
    }
}
