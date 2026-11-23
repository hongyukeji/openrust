use std::sync::Arc;

use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value as Json;
use zeroize::Zeroizing;

use crate::{
    contracts::{
        container::{BindingResolutionError, Container as _},
        encryption::Encrypter,
    },
    database::{Connection, DatabaseError, DatabaseResult, Value},
    encryption::{DecryptException, EncryptException},
    support::Collection,
};

use super::{
    AttributeCast, BuildJsonCollection, MapJsonItem, collection_cast,
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

    /// Projects each item through its application type after decryption.
    pub const fn of<T: DeserializeOwned + Serialize>() -> AttributeCast {
        Self::map(collection_cast::project_item::<T>)
    }

    pub const fn map(mapper: MapJsonItem) -> AttributeCast {
        AttributeCast::Encrypted(EncryptedCast::CollectionWith {
            factory: None,
            mapper: Some(mapper),
        })
    }

    /// Builds the decrypted collection before applying its optional mapper.
    pub const fn using(factory: BuildJsonCollection, mapper: Option<MapJsonItem>) -> AttributeCast {
        AttributeCast::Encrypted(EncryptedCast::CollectionWith {
            factory: Some(factory),
            mapper,
        })
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
    CollectionWith {
        factory: Option<BuildJsonCollection>,
        mapper: Option<MapJsonItem>,
    },
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
        let json = super::Json::for_model(connection)?
            .decode_bytes_lossy(&plain)?
            .unwrap_or(Json::Null);
        if let Self::CollectionWith { factory, mapper } = self {
            return collection_cast::project_collection(
                Collection::from_value(json),
                factory,
                mapper,
            )?
            .to_array()
            .map(Value::Json)
            .map_err(|_| collection_cast::invalid());
        }
        Ok(Value::Json(match (self, json) {
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
            super::Json::for_model(connection)?
                .encode(&json_value(value)?)?
                .into_bytes()
        });
        encrypter
            .encrypt_string(&plain)
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
        if !encrypter.get_previous_keys().is_empty() {
            return Ok(false);
        }
        let current = decrypt(current, encrypter.as_ref())?;
        let original = decrypt(original, encrypter.as_ref())?;
        Ok(match self {
            Self::String
            | Self::ArrayObject
            | Self::CollectionObject
            | Self::CollectionWith { .. } => current.as_slice() == original.as_slice(),
            _ => {
                let codec = super::Json::for_model(connection)?;
                codec.decode_bytes_lossy(&current)?.unwrap_or(Json::Null)
                    == codec.decode_bytes_lossy(&original)?.unwrap_or(Json::Null)
            }
        })
    }
}

pub(super) fn read_json(raw: &Value, connection: Option<&Connection>) -> DatabaseResult<Json> {
    let encrypter = encrypter(connection).map_err(decryption_unavailable)?;
    let plain = decrypt(raw, encrypter.as_ref())?;
    let text = std::str::from_utf8(&plain).map_err(|_| super::json_path::invalid())?;
    super::Json::for_model(connection)?.decode(text)
}

fn decrypt(raw: &Value, encrypter: &dyn Encrypter) -> DatabaseResult<Zeroizing<Vec<u8>>> {
    let Value::Text(raw) = raw else {
        return Err(DatabaseError::AttributeDecryption(
            DecryptException::InvalidPayload,
        ));
    };
    encrypter
        .decrypt_string(raw)
        .map(Zeroizing::new)
        .map_err(DatabaseError::AttributeDecryption)
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
