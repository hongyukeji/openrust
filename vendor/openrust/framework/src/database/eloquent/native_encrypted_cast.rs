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

/// 原生加密属性的明文表示。
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
#[path = "../../../tests/unit/database/eloquent/native_encrypted_cast.rs"]
mod tests;
