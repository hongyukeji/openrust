use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::{Map, Value as Json};

use crate::database::Value;

use super::{PaginationError, PaginationResult};

const DIRECTION: &str = "_pointsToNextItems";
const MAX_ENCODED: usize = 8192;

/// Ordered key values and traversal direction, encoded as URL-safe JSON.
#[derive(Clone, Debug, PartialEq)]
pub struct Cursor {
    values: Map<String, Json>,
    next: bool,
}

#[allow(non_snake_case, reason = "Matches Laravel's Cursor API")]
impl Cursor {
    pub fn fromEncoded(encoded: &str) -> PaginationResult<Self> {
        if encoded.len() > MAX_ENCODED {
            return Err(PaginationError::InvalidCursor);
        }
        let raw = URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| PaginationError::InvalidCursor)?;
        let mut values: Map<String, Json> =
            serde_json::from_slice(&raw).map_err(|_| PaginationError::InvalidCursor)?;
        let next = match values.remove(DIRECTION) {
            Some(Json::Bool(next)) => next,
            _ => return Err(PaginationError::InvalidCursor),
        };
        if values.is_empty() || values.values().any(|value| scalar(value).is_none()) {
            return Err(PaginationError::InvalidCursor);
        }
        Ok(Self { values, next })
    }

    pub fn encode(&self) -> PaginationResult<String> {
        let mut values = self.values.clone();
        values.insert(DIRECTION.into(), Json::Bool(self.next));
        let bytes = serde_json::to_vec(&values).map_err(|_| PaginationError::InvalidCursor)?;
        let encoded = URL_SAFE_NO_PAD.encode(bytes);
        if encoded.len() > MAX_ENCODED {
            return Err(PaginationError::InvalidCursor);
        }
        Ok(encoded)
    }

    pub const fn pointsToNextItems(&self) -> bool {
        self.next
    }

    pub const fn pointsToPreviousItems(&self) -> bool {
        !self.next
    }

    pub(crate) fn keys(&self, columns: &[String]) -> PaginationResult<Vec<Value>> {
        if self.values.len() != columns.len() {
            return Err(PaginationError::InvalidCursor);
        }
        columns
            .iter()
            .map(|column| {
                self.values
                    .get(column)
                    .and_then(scalar)
                    .ok_or(PaginationError::InvalidCursor)
            })
            .collect()
    }

    pub(crate) fn from_keys<'a>(
        keys: impl IntoIterator<Item = (&'a str, &'a Value)>,
        next: bool,
    ) -> PaginationResult<Self> {
        let mut values = Map::new();
        for (name, key) in keys {
            let value = match key {
                Value::Bool(value) => Json::from(*value),
                Value::Integer(value) => Json::from(*value),
                Value::Unsigned(value) => Json::from(*value),
                Value::Real(value) if value.is_finite() => Json::from(*value),
                Value::Text(value) => Json::from(value.as_str()),
                _ => return Err(PaginationError::InvalidCursor),
            };
            if name == DIRECTION || values.insert(name.into(), value).is_some() {
                return Err(PaginationError::InvalidCursor);
            }
        }
        Ok(Self { values, next })
    }
}

fn scalar(value: &Json) -> Option<Value> {
    match value {
        Json::Bool(value) => Some(Value::Bool(*value)),
        Json::Number(value) => value
            .as_i64()
            .map(Value::Integer)
            .or_else(|| value.as_u64().map(Value::Unsigned))
            .or_else(|| value.as_f64().map(Value::Real)),
        Json::String(value) => Some(Value::Text(value.clone())),
        _ => None,
    }
}
