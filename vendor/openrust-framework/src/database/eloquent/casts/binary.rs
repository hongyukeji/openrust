use crate::database::{DatabaseError, DatabaseResult, Value};

use super::AttributeCast;

/// Converts identifier strings to bytes for database storage.
pub struct AsBinary;

impl AsBinary {
    pub const fn uuid() -> AttributeCast {
        Self::of(BinaryFormat::Uuid)
    }

    pub const fn ulid() -> AttributeCast {
        Self::of(BinaryFormat::Ulid)
    }

    pub const fn of(format: BinaryFormat) -> AttributeCast {
        AttributeCast::Binary(format)
    }
}

/// A binary format selected by an attribute declaration.
#[derive(Clone, Copy)]
pub enum BinaryFormat {
    Uuid,
    Ulid,
    Custom(BinaryCodec),
}

type Encode = fn(&str) -> DatabaseResult<Vec<u8>>;
type Decode = fn(&[u8]) -> DatabaseResult<String>;

/// Encodes and decodes a custom binary identifier format.
#[derive(Clone, Copy)]
pub struct BinaryCodec {
    encode: Encode,
    decode: Decode,
}

impl BinaryCodec {
    pub const fn new(encode: Encode, decode: Decode) -> Self {
        Self { encode, decode }
    }
}

impl BinaryFormat {
    pub(super) fn read(self, value: &Value) -> DatabaseResult<Value> {
        match value {
            Value::Null => Ok(Value::Null),
            Value::Bytes(bytes) if bytes.is_empty() => Ok(Value::Null),
            Value::Bytes(bytes) => self.decode(bytes).map(Value::Text),
            Value::Text(text) if text.trim().is_empty() => Ok(Value::Null),
            Value::Text(text) => match self {
                Self::Custom(codec) => (codec.decode)(text.as_bytes()).map(Value::Text),
                _ => self.decode(&self.encode(text)?).map(Value::Text),
            },
            _ => Err(invalid()),
        }
    }

    pub(super) fn write(self, value: Value) -> DatabaseResult<Value> {
        match value {
            Value::Null => Ok(Value::Null),
            Value::Text(text) if text.trim().is_empty() => Ok(Value::Null),
            Value::Bytes(bytes) if bytes.is_empty() => Ok(Value::Null),
            Value::Bytes(bytes) if bytes.len() == 16 || matches!(self, Self::Custom(_)) => {
                Ok(Value::Bytes(bytes))
            }
            Value::Text(text) => self.encode(&text).map(Value::Bytes),
            _ => Err(invalid()),
        }
    }

    fn encode(self, value: &str) -> DatabaseResult<Vec<u8>> {
        match self {
            Self::Uuid => uuid_bytes(value).map(Vec::from),
            Self::Ulid => ulid_bytes(value).map(Vec::from),
            Self::Custom(codec) => (codec.encode)(value),
        }
    }

    fn decode(self, bytes: &[u8]) -> DatabaseResult<String> {
        match self {
            Self::Custom(codec) => (codec.decode)(bytes),
            Self::Uuid => uuid::Uuid::from_slice(bytes)
                .map(|uuid| uuid.to_string())
                .map_err(|_| invalid()),
            Self::Ulid => {
                let bytes = bytes.try_into().map_err(|_| invalid())?;
                Ok(ulid::Ulid::from_bytes(bytes).to_string())
            }
        }
    }
}

fn uuid_bytes(value: &str) -> DatabaseResult<[u8; 16]> {
    uuid::Uuid::parse_str(value)
        .map(|uuid| *uuid.as_bytes())
        .map_err(|_| invalid())
}

fn ulid_bytes(value: &str) -> DatabaseResult<[u8; 16]> {
    match value.len() {
        36 => uuid_bytes(value),
        26 if value.as_bytes()[0].is_ascii_digit() && value.as_bytes()[0] <= b'7' => {
            ulid::Ulid::from_string(value)
                .map(|ulid| ulid.to_bytes())
                .map_err(|_| invalid())
        }
        22 => {
            const BASE58: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
            let value = value
                .bytes()
                .try_fold(0_u128, |value, byte| {
                    let digit = BASE58.iter().position(|candidate| *candidate == byte)?;
                    value.checked_mul(58)?.checked_add(digit as u128)
                })
                .ok_or_else(invalid)?;
            Ok(value.to_be_bytes())
        }
        _ => Err(invalid()),
    }
}

fn invalid() -> DatabaseError {
    DatabaseError::InvalidQuery("binary identifier attribute")
}
