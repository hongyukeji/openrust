//! Fluent 属性转换。

use super::{
    AttributeCast, AttributeMap, AttributeValue, ClassCast, ClassCastWrite, ClassCaster, Json,
    ModelObject, attribute_cast::encode_array_json,
};
use crate::{Error, Result, support::Fluent};
use serde_json::Value;

/// 将已存 JSON 属性读取为可缓存和回写的 `Fluent` 对象。
pub struct AsFluent;

impl AsFluent {
    /// 创建模型类转换。赋值 null 保存为 SQL NULL。
    pub fn cast() -> AttributeCast {
        AttributeCast::Class(ClassCast::new(|| Ok(FluentCast)).with_arrayable_projection())
    }
}

struct FluentCast;

impl ClassCaster for FluentCast {
    fn get_value(
        &self,
        _: &str,
        value: Option<&AttributeValue>,
        _: &AttributeMap,
    ) -> Result<AttributeValue> {
        let Some(value) = value.filter(|value| !value.is_null()) else {
            return Ok(Value::Null.into());
        };
        let decoded = Json::decode(value.as_json()?, Some(true))?;
        let fluent = Fluent::from_stored_attributes(decoded).ok_or_else(|| {
            Error::custom("model_fluent_shape", "模型 Fluent 属性必须解码为数组或对象")
        })?;
        Ok(ModelObject::new(fluent).into())
    }

    fn set(&self, _: &str, value: AttributeValue, _: &AttributeMap) -> Result<ClassCastWrite> {
        if value.is_null() {
            return Ok(ClassCastWrite::Value(value));
        }
        let value = value.into_array_json()?;
        Ok(ClassCastWrite::Value(encode_array_json(&value)?.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::eloquent::{ModelAttributes, ModelMetadata};
    use serde_json::{Map, json};

    fn metadata() -> ModelMetadata {
        let mut metadata = ModelMetadata::new("records", "FluentRecord");
        metadata.casts.insert("data".into(), AsFluent::cast());
        metadata
    }

    #[tokio::test]
    async fn fluent_cache_writes_mutations_and_refreshes_after_json_path_assignment() -> Result<()>
    {
        let metadata = metadata();
        let attrs = ModelAttributes::new(Map::from_iter([("data".into(), json!(r#"{"x":1}"#))]));
        let read = || {
            attrs
                .get_attribute_value(&metadata, "data")?
                .and_then(|value| value.as_object().cloned())
                .ok_or_else(|| Error::custom("test_fluent_missing", "缺少模型 Fluent 对象"))
        };
        let first = read()?;
        assert!(first.same_object(&read()?));
        assert!(attrs.is_clean(&metadata, &["data"])?);
        first.modify::<Fluent>(|value| {
            let mut value = value.clone();
            value.set("x", json!(2))?;
            Ok(value)
        })?;
        assert_eq!(attrs.raw_attribute("data")?, Some(json!(r#"{"x":2}"#)));
        assert!(attrs.is_dirty(&metadata, &["data"])?);
        attrs
            .set_attribute(&metadata, "data->y", json!("next"))
            .await?;
        assert!(!first.same_object(&read()?));
        assert_eq!(
            attrs.attributes_to_json(&metadata)?["data"],
            json!({"x":2,"y":"next"})
        );
        attrs.set_attribute(&metadata, "data", Value::Null).await?;
        assert_eq!(attrs.raw_attribute("data")?, Some(Value::Null));
        Ok(())
    }

    #[test]
    fn fluent_retains_stored_shapes_and_default_raw_comparison() -> Result<()> {
        let metadata = metadata();
        let attrs = ModelAttributes::default();
        assert_eq!(attrs.get_attribute(&metadata, "data")?, Some(Value::Null));
        for raw in ["", "null", "bad", "1", "true", r#""scalar""#] {
            attrs.set_raw_attribute("data", json!(raw))?;
            assert!(matches!(attrs.get_attribute_value(&metadata, "data"),
                Err(error) if error.code()=="model_fluent_shape"));
        }
        attrs.set_raw_attribute("data", json!(r#"{"a":{},"b":{"0":"x"},"2":"last"}"#))?;
        assert_eq!(
            attrs.attributes_to_json(&metadata)?["data"],
            json!({"a":[],"b":["x"],"2":"last"})
        );
        let attrs = ModelAttributes::new(Map::from_iter([("data".into(), json!("{ \"x\": 1 }"))]));
        attrs.set_raw_attribute("data", json!(r#"{"x":1}"#))?;
        assert!(attrs.is_dirty(&metadata, &["data"])?);

        let wide = json!(vec![0; 1025]);
        attrs.set_raw_attribute("data", Value::String(wide.to_string()))?;
        assert_eq!(attrs.attributes_to_json(&metadata)?["data"], wide);
        Ok(())
    }
}
