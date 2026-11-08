//! Eloquent 原生加密属性的读取、存储和 dirty 比较。
use super::{
    AttributeValue, Json, ModelObject,
    attribute_cast::{
        collection_items, decode_default_text, default_json_equivalent, encode_array_json,
        scalar_string,
    },
    attribute_store::strict_equal,
};
use crate::{Error, Result, encryption::ApplicationEncrypter, support::Collection};
use serde_json::Value;
use std::{fmt, sync::Arc};
use zeroize::{Zeroize, Zeroizing};

/// Laravel 原生加密属性的明文表示。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncryptedCastKind {
    /// `encrypted`，采用 PHP 标量字符串。
    String,
    /// `encrypted:array`，采用 PHP 关联数组解码。
    Array,
    /// `encrypted:json`，与 `encrypted:array` 同义。
    Json,
    /// `encrypted:object`，保留 JSON 对象形状。
    Object,
    /// `encrypted:collection`，每次读取构造独立集合。
    Collection,
}

/// 由应用 Encrypter 支持的 Eloquent 原生加密转换。
#[derive(Clone)]
pub struct EncryptedCast {
    kind: EncryptedCastKind,
    encrypter: Arc<dyn ApplicationEncrypter>,
}

impl EncryptedCast {
    /// 绑定转换类型与应用容器解析的加密器。
    pub fn new(kind: EncryptedCastKind, encrypter: Arc<dyn ApplicationEncrypter>) -> Self {
        Self { kind, encrypter }
    }

    pub(super) fn encrypter(&self) -> &dyn ApplicationEncrypter {
        &*self.encrypter
    }

    pub(super) fn encodes_json(&self) -> bool {
        self.kind != EncryptedCastKind::String
    }

    pub(super) fn read(&self, raw: AttributeValue) -> Result<AttributeValue> {
        if raw.is_null() {
            return Ok(raw);
        }
        let plain = decrypt(&*self.encrypter, &raw)?;
        let text = std::str::from_utf8(&plain);
        match self.kind {
            EncryptedCastKind::String => text
                .map(|value| Value::String(value.to_owned()).into())
                .map_err(|_| {
                    Error::custom(
                        "model_encrypted_attribute_utf8",
                        "加密属性明文不是有效 UTF-8",
                    )
                }),
            EncryptedCastKind::Object => {
                decode_plain_json(&plain, false, true).map(AttributeValue::from)
            }
            EncryptedCastKind::Array | EncryptedCastKind::Json => {
                decode_plain_json(&plain, true, true).map(AttributeValue::from)
            }
            EncryptedCastKind::Collection => {
                let array = collection_items(decode_plain_json(&plain, true, true)?);
                Collection::from_stored_array(array)
                    .map(ModelObject::new)
                    .map(AttributeValue::from)
                    .ok_or_else(|| {
                        Error::custom("model_encrypted_collection_shape", "加密集合的数组形状无效")
                    })
            }
        }
    }

    pub(super) fn write(&self, value: AttributeValue) -> Result<AttributeValue> {
        if value.is_null() {
            return Ok(value);
        }
        let plain = if self.kind == EncryptedCastKind::String {
            Zeroizing::new(scalar_string(value.as_json()?)?)
        } else {
            let encoded = encode_array_json(&value.into_array_json()?)?;
            if encoded.is_null() {
                return Ok(encoded.into());
            }
            encoded_plaintext(encoded)?
        };
        self.encrypter
            .encrypt_string(plain.as_bytes())
            .map(|value| Value::String(value).into())
    }

    pub(super) fn equivalent(
        &self,
        current: &AttributeValue,
        original: &AttributeValue,
    ) -> Result<bool> {
        if !self.encrypter.previous_keys().is_empty() {
            return Ok(false);
        }
        let current = decrypt(&*self.encrypter, current)?;
        if original.is_null() {
            if self.kind != EncryptedCastKind::String && Json::decoder().is_some() {
                let value =
                    decode_plain_json(&current, self.kind != EncryptedCastKind::Object, true)?;
                return Ok(self.kind != EncryptedCastKind::Collection && value.is_null());
            }
            return match self.kind {
                EncryptedCastKind::String | EncryptedCastKind::Collection => Ok(false),
                _ => default_json_equivalent(json_text(&current), ""),
            };
        }
        let original = decrypt(&*self.encrypter, original)?;
        match self.kind {
            EncryptedCastKind::String => Ok(current.as_slice() == original.as_slice()),
            _ if Json::decoder().is_some() => {
                let current = decode_plain_json(&current, true, true)?;
                let original = decode_plain_json(&original, true, true)?;
                Ok(strict_equal(&current, &original))
            }
            _ => default_json_equivalent(json_text(&current), json_text(&original)),
        }
    }
}

impl fmt::Debug for EncryptedCast {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EncryptedCast")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

impl PartialEq for EncryptedCast {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind && Arc::ptr_eq(&self.encrypter, &other.encrypter)
    }
}

impl Eq for EncryptedCast {}

pub(super) fn decrypt(
    encrypter: &dyn ApplicationEncrypter,
    raw: &AttributeValue,
) -> Result<Zeroizing<Vec<u8>>> {
    let ciphertext = raw.as_json()?.as_str().ok_or_else(|| {
        Error::custom(
            "model_encrypted_attribute_payload",
            "加密属性原值必须为密文字符串",
        )
    })?;
    encrypter.decrypt_string(ciphertext).map(Zeroizing::new)
}

fn json_text(plaintext: &[u8]) -> &str {
    // PHP json_decode 对无效 UTF-8 返回 null。
    std::str::from_utf8(plaintext).unwrap_or_default()
}

struct DecoderPlaintext(Value);

impl Drop for DecoderPlaintext {
    fn drop(&mut self) {
        if let Value::String(text) = &mut self.0 {
            text.zeroize();
        }
    }
}

pub(super) fn decode_plain_json(
    plaintext: &[u8],
    associative: bool,
    skip_empty: bool,
) -> Result<Value> {
    let Ok(text) = std::str::from_utf8(plaintext) else {
        return Ok(Value::Null);
    };
    if skip_empty && text.is_empty() {
        return Ok(Value::Null);
    }
    if let Some(decoder) = Json::decoder() {
        let input = DecoderPlaintext(Value::String(text.to_owned()));
        decoder(&input.0, Some(associative))
    } else {
        decode_default_text(text, associative)
    }
}

pub(super) fn encoded_plaintext(value: Value) -> Result<Zeroizing<String>> {
    if value.is_null() {
        return Err(Error::custom(
            "model_json_encryption_value",
            "加密 JSON 编码结果必须可转换为文本",
        ));
    }
    match value {
        Value::String(text) => Ok(Zeroizing::new(text)),
        value => scalar_string(&value).map(Zeroizing::new),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::AppConfig,
        database::eloquent::{AttributeCast, ModelAttributes, ModelMetadata},
        encryption::LaravelEncrypter,
    };
    use serde_json::{Map, json};

    const OLD_KEY: &str = "0123456789abcdef0123456789abcdef";
    const NEW_KEY: &str = "abcdef0123456789abcdef0123456789";

    fn key(value: &str, previous_keys: Vec<String>) -> Result<Arc<dyn ApplicationEncrypter>> {
        Ok(Arc::new(LaravelEncrypter::from_config(AppConfig {
            key: Some(value.into()),
            previous_keys,
            ..AppConfig::default()
        })?))
    }

    fn metadata(kind: EncryptedCastKind, key: &Arc<dyn ApplicationEncrypter>) -> ModelMetadata {
        let mut metadata = ModelMetadata::new("records", "id");
        metadata.casts.insert(
            "data".into(),
            AttributeCast::Encrypted(EncryptedCast::new(kind, Arc::clone(key))),
        );
        metadata
    }

    fn missing_attribute() -> Error {
        Error::custom("test_attribute_missing", "测试属性缺失")
    }

    fn store(raw: Value) -> ModelAttributes {
        ModelAttributes::new(Map::from_iter([("data".into(), raw)]))
    }

    #[tokio::test]
    async fn native_scalar_cast_uses_application_key_rotation_and_php_scalar_text() -> Result<()> {
        let old = key(OLD_KEY, vec![])?;
        let current = key(NEW_KEY, vec![])?;
        let rotated = key(NEW_KEY, vec![OLD_KEY.into()])?;
        let model = metadata(EncryptedCastKind::String, &rotated);
        let attrs = store(json!(old.encrypt_string(b"old")?));
        assert_eq!(attrs.get_attribute(&model, "data")?, Some(json!("old")));
        assert!(attrs.original_is_equivalent(&model, "data")?);
        attrs.set_attribute(&model, "data", json!("old")).await?;
        assert!(attrs.is_dirty(&model, &["data"])?);
        let wire = attrs.raw_attribute("data")?.ok_or_else(missing_attribute)?;
        assert_eq!(
            current.decrypt_string(wire.as_str().unwrap_or_default())?,
            b"old"
        );
        attrs.set_raw_attribute("data", json!("bad ciphertext"))?;
        assert!(attrs.is_dirty(&model, &["data"])?);
        assert!(attrs.get_attribute(&model, "data").is_err());

        for (input, expected) in [
            (json!(true), "1"),
            (json!(false), ""),
            (json!(24), "24"),
            (json!(1.0), "1"),
        ] {
            attrs.set_attribute(&model, "data", input).await?;
            assert_eq!(attrs.get_attribute(&model, "data")?, Some(json!(expected)));
        }
        let raw = attrs.raw_attribute("data")?;
        assert!(
            attrs
                .set_attribute(&model, "data", json!([1]))
                .await
                .is_err()
        );
        assert_eq!(attrs.raw_attribute("data")?, raw);
        attrs.set_attribute(&model, "data", Value::Null).await?;
        assert_eq!(attrs.raw_attribute("data")?, Some(Value::Null));
        assert_eq!(attrs.get_attribute(&model, "data")?, Some(Value::Null));
        Ok(())
    }

    #[tokio::test]
    async fn native_json_variants_share_associative_dirty_comparison_and_keep_read_shape()
    -> Result<()> {
        let key = key(NEW_KEY, vec![])?;
        for kind in [
            EncryptedCastKind::Array,
            EncryptedCastKind::Json,
            EncryptedCastKind::Object,
            EncryptedCastKind::Collection,
        ] {
            let model = metadata(kind, &key);
            let attrs = store(json!(key.encrypt_string(br#"{"0":1}"#)?));
            let expected = if kind == EncryptedCastKind::Object {
                json!({"0":1})
            } else {
                json!([1])
            };
            assert_eq!(
                attrs.attributes_to_json(&model)?.get("data"),
                Some(&expected)
            );
            attrs.set_raw_attribute("data", json!(key.encrypt_string(b"[ 1 ]")?))?;
            assert!(attrs.original_is_equivalent(&model, "data")?);
            attrs.set_raw_attribute("data", json!(key.encrypt_string(b"[1.0]")?))?;
            assert!(!attrs.original_is_equivalent(&model, "data")?);
            attrs
                .set_attribute(&model, "data", json!({"字":"a/b"}))
                .await?;
            let wire = attrs.raw_attribute("data")?.ok_or_else(missing_attribute)?;
            assert_eq!(
                key.decrypt_string(wire.as_str().unwrap_or_default())?,
                br#"{"\u5b57":"a\/b"}"#
            );
            attrs.set_attribute(&model, "data", Value::Null).await?;
            assert_eq!(attrs.raw_attribute("data")?, Some(Value::Null));
            attrs.sync_original()?;
            attrs.set_raw_attribute("data", json!(key.encrypt_string(b"bad json")?))?;
            let (expected, equivalent) = if kind == EncryptedCastKind::Collection {
                (json!([]), false)
            } else {
                (Value::Null, true)
            };
            assert_eq!(
                attrs.attributes_to_json(&model)?.get("data"),
                Some(&expected)
            );
            assert_eq!(attrs.original_is_equivalent(&model, "data")?, equivalent);
        }
        Ok(())
    }

    #[test]
    fn encrypted_object_rejects_nul_property_names_without_changing_array_comparison() -> Result<()>
    {
        let key = key(NEW_KEY, vec![])?;
        let payload = br#"{"nested":{"\u0000name":1}}"#;
        let attrs = store(json!(key.encrypt_string(payload)?));
        let object = metadata(EncryptedCastKind::Object, &key);
        let array = metadata(EncryptedCastKind::Array, &key);
        assert_eq!(attrs.get_attribute(&object, "data")?, Some(Value::Null));
        assert_eq!(
            attrs.get_attribute(&array, "data")?,
            Some(json!({"nested":{"\0name":1}}))
        );
        attrs.set_raw_attribute("data", json!(key.encrypt_string(payload)?))?;
        assert!(attrs.original_is_equivalent(&object, "data")?);
        Ok(())
    }

    #[test]
    fn native_collection_reads_fresh_typed_objects_and_serialize_through_model_projection()
    -> Result<()> {
        let key = key(NEW_KEY, vec![])?;
        let model = metadata(EncryptedCastKind::Collection, &key);
        let attrs = store(json!(key.encrypt_string(b"7")?));
        let read = || {
            attrs
                .get_attribute_value(&model, "data")?
                .and_then(|value| value.as_object().cloned())
                .ok_or_else(missing_attribute)
        };
        let first = read()?;
        first.modify(|collection: &Collection| {
            let mut changed = collection.clone();
            changed.try_put(0, json!(8))?;
            Ok(changed)
        })?;
        assert_eq!(first.project_json()?, json!([8]));
        assert_eq!(read()?.project_json()?, json!([7]));
        assert_eq!(
            attrs.attributes_to_json(&model)?.get("data"),
            Some(&json!([7]))
        );
        assert!(attrs.is_clean(&model, &["data"])?);
        Ok(())
    }
}
