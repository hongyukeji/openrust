use std::sync::Arc;

use serde_json::Value as Json;
use zeroize::Zeroizing;

use crate::{
    contracts::{
        container::{BindingResolutionError, Container as _},
        encryption::Encrypter,
    },
    database::{Connection, DatabaseError, DatabaseResult, Value},
    encryption::{DecryptException, EncryptException},
};

use super::{
    AttributeCast,
    primitive::{json_value, scalar_string},
};

/// Encrypted JSON backed by an array object.
pub struct AsEncryptedArrayObject;

impl AsEncryptedArrayObject {
    pub const fn cast() -> AttributeCast {
        AttributeCast::Encrypted(EncryptedCast::ArrayObject)
    }
}

/// Encrypted JSON backed by a collection.
pub struct AsEncryptedCollection;

impl AsEncryptedCollection {
    pub const fn cast() -> AttributeCast {
        AttributeCast::Encrypted(EncryptedCast::CollectionObject)
    }
}

/// The plaintext representation of an encrypted model attribute.
#[derive(Clone, Copy)]
pub enum EncryptedCast {
    String,
    Array,
    Json,
    Object,
    Collection,
    ArrayObject,
    CollectionObject,
}

impl EncryptedCast {
    pub(super) fn read(
        self,
        raw: &Value,
        connection: Option<&Connection>,
    ) -> DatabaseResult<Value> {
        if matches!(raw, Value::Null) {
            return Ok(Value::Null);
        }
        let encrypter = encrypter(connection).map_err(decryption_unavailable)?;
        let plain = decrypt(raw, encrypter.as_ref())?;
        if matches!(self, Self::String) {
            return std::str::from_utf8(&plain)
                .map(|value| Value::Text(value.to_owned()))
                .map_err(|_| DatabaseError::InvalidQuery("encrypted attribute UTF-8"));
        }
        Ok(Value::Json(match (self, decode_json(&plain)) {
            (Self::ArrayObject, value) if !value.is_array() && !value.is_object() => {
                return Err(DatabaseError::InvalidQuery("encrypted array object"));
            }
            (Self::Collection | Self::CollectionObject, Json::Null) => Json::Array(Vec::new()),
            (Self::Collection | Self::CollectionObject, scalar)
                if !scalar.is_array() && !scalar.is_object() =>
            {
                Json::Array(vec![scalar])
            }
            (_, json) => json,
        }))
    }

    pub(super) fn write(
        self,
        value: Value,
        connection: Option<&Connection>,
    ) -> DatabaseResult<Value> {
        if matches!(value, Value::Null) {
            return Ok(Value::Null);
        }
        let encrypter = encrypter(connection)
            .map_err(EncryptException::Unavailable)
            .map_err(DatabaseError::AttributeEncryption)?;
        let plain = Zeroizing::new(if matches!(self, Self::String) {
            scalar_string(&value)?.into_bytes()
        } else {
            serde_json::to_vec(&json_value(value)?)
                .map_err(|_| DatabaseError::InvalidQuery("encrypted JSON attribute"))?
        });
        encrypter
            .encryptString(&plain)
            .map(Value::Text)
            .map_err(DatabaseError::AttributeEncryption)
    }

    pub(super) fn equivalent(
        self,
        current: &Value,
        original: &Value,
        connection: Option<&Connection>,
    ) -> DatabaseResult<bool> {
        let encrypter = encrypter(connection).map_err(decryption_unavailable)?;
        if !encrypter.getPreviousKeys().is_empty() {
            return Ok(false);
        }
        let current = decrypt(current, encrypter.as_ref())?;
        let original = decrypt(original, encrypter.as_ref())?;
        Ok(match self {
            Self::String | Self::ArrayObject | Self::CollectionObject => {
                current.as_slice() == original.as_slice()
            }
            _ => decode_json(&current) == decode_json(&original),
        })
    }
}

pub(super) fn read_json(raw: &Value, connection: Option<&Connection>) -> DatabaseResult<Json> {
    let encrypter = encrypter(connection).map_err(decryption_unavailable)?;
    let plain = decrypt(raw, encrypter.as_ref())?;
    serde_json::from_slice(&plain).map_err(|_| super::json_path::invalid())
}

fn decrypt(raw: &Value, encrypter: &dyn Encrypter) -> DatabaseResult<Zeroizing<Vec<u8>>> {
    let Value::Text(raw) = raw else {
        return Err(DatabaseError::AttributeDecryption(
            DecryptException::InvalidPayload,
        ));
    };
    encrypter
        .decryptString(raw)
        .map(Zeroizing::new)
        .map_err(DatabaseError::AttributeDecryption)
}

fn decode_json(plain: &[u8]) -> Json {
    serde_json::from_slice(plain).unwrap_or(Json::Null)
}

fn encrypter(
    connection: Option<&Connection>,
) -> Result<Arc<dyn Encrypter>, BindingResolutionError> {
    Connection::model_container(connection)
        .ok_or_else(|| BindingResolutionError::NotBound {
            service: "encrypter".into(),
        })?
        .make::<dyn Encrypter>()
}

fn decryption_unavailable(error: BindingResolutionError) -> DatabaseError {
    DatabaseError::AttributeDecryption(DecryptException::Unavailable(error))
}
