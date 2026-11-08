//! 使用应用 Laravel Encrypter 的对象和集合转换。
use super::{ArrayObject, AsCollection, CollectionFactory};
use crate::{
    Error, Result,
    database::eloquent::{
        AttributeCast, AttributeMap, AttributeValue, ClassCast, ClassCastWrite, ClassCaster,
        attribute_cast::encode_array_json,
        native_encrypted_cast::{decode_plain_json, decrypt, encoded_plaintext},
    },
    encryption::ApplicationEncrypter,
    support::Collection,
};
use serde_json::Value;
use std::sync::Arc;

/// Laravel 加密数组对象转换。
pub struct AsEncryptedArrayObject;

impl AsEncryptedArrayObject {
    /// 使用应用容器解析的加密器构建转换。
    pub fn cast(encrypter: Arc<dyn ApplicationEncrypter>) -> AttributeCast {
        let caster = EncryptedCollectionCast {
            encrypter,
            factory: CollectionFactory::new(ArrayObject::new),
            shape: Shape::ArrayObject,
        };
        AttributeCast::Class(caster.declaration().with_serialization())
    }
}

/// Laravel 加密集合转换。
pub struct AsEncryptedCollection;

impl AsEncryptedCollection {
    /// 使用标准集合和应用加密器。
    pub fn cast(encrypter: Arc<dyn ApplicationEncrypter>) -> AttributeCast {
        Self::using(encrypter, AsCollection::default())
    }

    /// 使用应用加密器和已声明的集合工厂或映射。
    /// 加密转换始终将 null 保存为 SQL NULL。
    pub fn using(
        encrypter: Arc<dyn ApplicationEncrypter>,
        collection: AsCollection,
    ) -> AttributeCast {
        AttributeCast::Class(
            EncryptedCollectionCast {
                encrypter,
                factory: collection.factory,
                shape: Shape::Collection,
            }
            .declaration(),
        )
    }
}

#[derive(Clone, Copy)]
enum Shape {
    ArrayObject,
    Collection,
}

#[derive(Clone)]
struct EncryptedCollectionCast {
    encrypter: Arc<dyn ApplicationEncrypter>,
    factory: CollectionFactory,
    shape: Shape,
}

impl EncryptedCollectionCast {
    fn declaration(self) -> ClassCast {
        let encrypter = self.encrypter.clone();
        ClassCast::new(move || Ok(self.clone()))
            .with_builtin_encrypter(encrypter)
            .with_arrayable_projection()
    }
}

impl ClassCaster for EncryptedCollectionCast {
    fn get_value(
        &self,
        name: &str,
        _: Option<&AttributeValue>,
        raw: &AttributeMap,
    ) -> Result<AttributeValue> {
        let Some(raw) = raw.get(name).filter(|value| !value.is_null()) else {
            return Ok(Value::Null.into());
        };
        let plaintext = decrypt(&*self.encrypter, raw)?;
        let value = decode_plain_json(&plaintext, true, false)?;
        let value = match (self.shape, value) {
            (_, value @ (Value::Array(_) | Value::Object(_))) => value,
            (Shape::Collection, Value::Null) => Value::Array(Vec::new()),
            (Shape::Collection, value) => Value::Array(vec![value]),
            (Shape::ArrayObject, _) => {
                return Err(Error::custom(
                    "model_encrypted_array_object_shape",
                    "加密数组对象的明文必须解码为数组",
                ));
            }
        };
        let collection = Collection::from_stored_array(value).ok_or_else(|| {
            Error::custom("model_encrypted_collection_shape", "加密集合的数组形状无效")
        })?;
        (self.factory.0)(collection).map(AttributeValue::from)
    }

    fn set(&self, _: &str, value: AttributeValue, _: &AttributeMap) -> Result<ClassCastWrite> {
        if value.is_null() {
            return Ok(ClassCastWrite::Value(value));
        }
        let value = value.into_array_json()?;
        let json = encoded_plaintext(encode_array_json(&value)?)?;
        let ciphertext = self.encrypter.encrypt_string(json.as_bytes())?;
        Ok(ClassCastWrite::Value(Value::String(ciphertext).into()))
    }

    fn compare(
        &self,
        _: &str,
        original: &AttributeValue,
        current: &AttributeValue,
    ) -> Result<Option<bool>> {
        if original.is_null() || !self.encrypter.previous_keys().is_empty() {
            return Ok(Some(false));
        }
        let current = decrypt(&*self.encrypter, current)?;
        let original = decrypt(&*self.encrypter, original)?;
        Ok(Some(current.as_slice() == original.as_slice()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::AppConfig,
        database::eloquent::{ModelAttributes, ModelMetadata},
        encryption::LaravelEncrypter,
    };
    use serde_json::{Map, json};

    const OLD_KEY: &str = "0123456789abcdef0123456789abcdef";
    const NEW_KEY: &str = "abcdef0123456789abcdef0123456789";

    fn service(key: &str, previous_keys: Vec<String>) -> Result<Arc<dyn ApplicationEncrypter>> {
        Ok(Arc::new(LaravelEncrypter::from_config(AppConfig {
            key: Some(key.into()),
            previous_keys,
            ..AppConfig::default()
        })?))
    }

    fn model(cast: AttributeCast) -> ModelMetadata {
        let mut model = ModelMetadata::new("records", "Record");
        model.casts.insert("data".into(), cast);
        model
    }

    fn store(value: Value) -> ModelAttributes {
        ModelAttributes::new(Map::from_iter([("data".into(), value)]))
    }

    #[tokio::test]
    async fn encrypted_object_reads_history_and_rewrites_with_current_key() -> Result<()> {
        let old = service(OLD_KEY, Vec::new())?;
        let current = service(NEW_KEY, Vec::new())?;
        let rotated = service(NEW_KEY, vec![OLD_KEY.into()])?;
        let previous = old.encrypt_string(br#"{"tag":"old"}"#)?;
        let model = model(AsEncryptedArrayObject::cast(Arc::clone(&rotated)));
        let attributes = store(json!(previous));
        assert!(attributes.original_is_equivalent(&model, "data")?);
        attributes
            .set_raw_attribute("data", json!(current.encrypt_string(br#"{"tag":"old"}"#)?))?;
        assert!(!attributes.original_is_equivalent(&model, "data")?);
        attributes.set_raw_attribute("data", json!("bad ciphertext"))?;
        assert!(!attributes.original_is_equivalent(&model, "data")?);
        attributes.set_raw_attribute("data", json!(old.encrypt_string(br#"{"tag":"old"}"#)?))?;
        let value = attributes.get_attribute_value(&model, "data")?;
        let object = value
            .as_ref()
            .and_then(AttributeValue::as_object)
            .ok_or_else(|| Error::custom("test_encrypted_array_missing", "缺少已解密对象"))?;
        object.modify(|value: &ArrayObject| {
            let mut next = value.clone();
            next.as_collection_mut().try_put("tag", json!("changed"))?;
            Ok(next)
        })?;

        let dirty = attributes.get_dirty(&model)?;
        let payload = dirty["data"]
            .as_str()
            .ok_or_else(|| Error::custom("test_encrypted_payload", "缺少回写密文"))?;
        assert_eq!(current.decrypt_string(payload)?, br#"{"tag":"changed"}"#);
        assert!(old.decrypt_string(payload).is_err());
        assert_eq!(
            attributes.attributes_to_json(&model)?["data"],
            json!({"tag":"changed"})
        );
        attributes
            .set_attribute(&model, "data", Value::Null)
            .await?;
        assert_eq!(attributes.raw_attribute("data")?, Some(Value::Null));
        Ok(())
    }

    #[test]
    fn encrypted_collection_compares_plaintext_only_without_history_and_maps_scalars() -> Result<()>
    {
        let encrypter = service(NEW_KEY, Vec::new())?;
        let model = model(AsEncryptedCollection::using(
            Arc::clone(&encrypter),
            AsCollection::of(|value, key| Ok((key.to_string(), value))),
        ));
        let attributes = store(json!(encrypter.encrypt_string(br#"{"0":1}"#)?));
        attributes.set_raw_attribute("data", json!(encrypter.encrypt_string(br#"{"0":1}"#)?))?;
        assert!(attributes.original_is_equivalent(&model, "data")?);
        attributes.set_raw_attribute("data", json!(encrypter.encrypt_string(b"[1]")?))?;
        assert!(!attributes.original_is_equivalent(&model, "data")?);
        attributes.set_raw_attribute("data", json!(encrypter.encrypt_string(b"7")?))?;
        assert_eq!(
            attributes.attributes_to_array(&model)?["data"].as_json()?,
            &json!([["0", 7]])
        );
        attributes.set_raw_attribute("data", json!(encrypter.encrypt_string(b"bad json")?))?;
        assert_eq!(attributes.attributes_to_json(&model)?["data"], json!([]));
        attributes.set_raw_attribute("data", json!("not ciphertext"))?;
        assert!(attributes.get_attribute_value(&model, "data").is_err());
        assert!(attributes.original_is_equivalent(&model, "data").is_err());
        Ok(())
    }

    #[test]
    fn encrypted_array_object_rejects_non_array_plaintext() -> Result<()> {
        let encrypter = service(NEW_KEY, Vec::new())?;
        let model = model(AsEncryptedArrayObject::cast(Arc::clone(&encrypter)));
        let attributes = store(json!(encrypter.encrypt_string(b"7")?));
        assert!(matches!(
            attributes.get_attribute_value(&model, "data"),
            Err(error) if error.code()=="model_encrypted_array_object_shape"
        ));
        Ok(())
    }
}
