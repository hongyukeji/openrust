//! Laravel 的 Stringable 类转换。

use super::{
    AttributeCast, AttributeMap, AttributeValue, ClassCast, ClassCastWrite, ClassCaster,
    ModelObject, attribute_cast::scalar_string,
};
use crate::{Error, Result, support::Stringable};
use serde_json::Value;

/// 将持久属性读取为具有缓存身份的 `Stringable` 对象。
pub struct AsStringable;

impl AsStringable {
    /// 创建模型类转换。赋值 null 保存为 SQL NULL。
    pub fn cast() -> AttributeCast {
        AttributeCast::Class(ClassCast::new(|| Ok(StringableCast)))
    }
}

struct StringableCast;

impl ClassCaster for StringableCast {
    fn get_value(
        &self,
        _: &str,
        value: Option<&AttributeValue>,
        _: &AttributeMap,
    ) -> Result<AttributeValue> {
        let Some(value) = value.filter(|value| !value.is_null()) else {
            return Ok(Value::Null.into());
        };
        Ok(ModelObject::new(Stringable::from(cast_string(value)?)).into())
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

fn cast_string(value: &AttributeValue) -> Result<String> {
    match value {
        AttributeValue::Json(value) => return scalar_string(value),
        AttributeValue::Object(object) => object.snapshot()?.string_projection(),
        AttributeValue::Date(_) | AttributeValue::Enum(_) => None,
    }
    .ok_or_else(|| Error::custom("model_stringable_conversion", "模型对象未提供字符串转换"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::eloquent::{ModelAttributes, ModelMetadata};
    use serde_json::{Map, json};

    fn metadata() -> ModelMetadata {
        let mut metadata = ModelMetadata::new("records", "StringableRecord");
        metadata.casts.insert("text".into(), AsStringable::cast());
        metadata
    }

    #[tokio::test]
    async fn stringable_preserves_array_identity_json_text_and_cached_writeback() -> Result<()> {
        let metadata = metadata();
        let attrs = ModelAttributes::new(Map::from_iter([("text".into(), json!("original"))]));
        let assigned = ModelObject::new(Stringable::from("original"));
        attrs
            .set_attribute_value(&metadata, "text", assigned.clone().into())
            .await?;
        let array = attrs.attributes_to_array(&metadata)?;
        assert!(
            array["text"]
                .as_object()
                .is_some_and(|object| object.same_object(&assigned))
        );
        assert!(attrs.is_clean(&metadata, &["text"])?);
        assert_eq!(
            attrs.attributes_to_json(&metadata)?["text"],
            json!("original")
        );
        assigned.modify::<Stringable>(|_| Ok(Stringable::from("改写")))?;
        assert_eq!(attrs.raw_attribute("text")?, Some(json!("改写")));
        assert!(attrs.is_dirty(&metadata, &["text"])?);
        attrs.set_attribute(&metadata, "text", Value::Null).await?;
        assert_eq!(attrs.get_attribute(&metadata, "text")?, Some(Value::Null));
        Ok(())
    }

    #[tokio::test]
    async fn stringable_applies_php_scalars_and_rejects_unconvertible_values_atomically()
    -> Result<()> {
        let metadata = metadata();
        let attrs = ModelAttributes::default();
        for (raw, expected) in [
            (json!(false), ""),
            (json!(true), "1"),
            (json!(17), "17"),
            (json!(1.5), "1.5"),
        ] {
            attrs.set_raw_attribute("text", raw.clone())?;
            assert_eq!(attrs.attributes_to_json(&metadata)?["text"], expected);
            attrs.set_attribute(&metadata, "text", raw).await?;
            assert_eq!(attrs.raw_attribute("text")?, Some(json!(expected)));
        }
        assert!(
            attrs
                .set_attribute(&metadata, "text", json!([]))
                .await
                .is_err()
        );
        assert_eq!(attrs.raw_attribute("text")?, Some(json!("1.5")));
        attrs.set_raw_attribute("text", json!("01"))?;
        attrs.sync_original()?;
        attrs.set_raw_attribute("text", json!("1"))?;
        assert!(attrs.is_dirty(&metadata, &["text"])?);
        Ok(())
    }
}
