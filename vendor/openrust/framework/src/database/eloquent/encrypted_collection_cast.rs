//! 使用应用加密器的对象和集合转换。
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

/// 加密数组对象转换。
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

/// 加密集合转换。
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
#[path = "../../../tests/unit/database/eloquent/encrypted_collection_cast.rs"]
mod tests;
