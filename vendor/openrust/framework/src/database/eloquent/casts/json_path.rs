use serde_json::{Map, Value as Json};

use crate::database::{Connection, DatabaseError, DatabaseResult, Value};

use super::{AttributeCast, EncryptedCast, json_value};

const MAX_DEPTH: usize = 127;

pub(super) fn write(
    path: &str,
    raw: Option<&Value>,
    value: Value,
    cast: Option<AttributeCast>,
    connection: Option<&Connection>,
) -> DatabaseResult<Value> {
    let keys: Vec<_> = path.split("->").flat_map(|key| key.split('.')).collect();
    if keys.len() > MAX_DEPTH || keys.iter().any(|key| key.is_empty()) {
        return Err(invalid());
    }
    let encrypted = matches!(cast, Some(AttributeCast::Encrypted(_)));
    let root = match raw {
        None | Some(Value::Null) => Json::Null,
        Some(raw) if encrypted => super::encrypted::read_json(raw, connection)?,
        Some(Value::Json(json)) => json.clone(),
        Some(Value::Text(text)) => serde_json::from_str(text).map_err(|_| invalid())?,
        _ => return Err(invalid()),
    };
    let updated = match root {
        Json::String(text) if keys.len() == 1 => {
            write_string_offset(text, keys[0], &super::primitive::scalar_string(&value)?)?
        }
        Json::Null | Json::Bool(false) | Json::Array(_) | Json::Object(_) => {
            update(root, &keys, json_value(value)?)
        }
        _ => return Err(invalid()),
    };
    check_depth(&updated)?;
    let updated = Value::Json(updated);
    if encrypted {
        return EncryptedCast::Json.write(updated, connection);
    }
    Ok(updated)
}

fn write_string_offset(text: String, offset: &str, replacement: &str) -> DatabaseResult<Json> {
    const MAX_GROWTH: usize = 1024 * 1024;
    let offset = offset.trim_ascii().parse::<i64>().map_err(|_| invalid())?;
    let mut bytes = text.into_bytes();
    let index = if offset.is_negative() {
        usize::try_from(offset.unsigned_abs())
            .ok()
            .and_then(|offset| bytes.len().checked_sub(offset))
    } else {
        usize::try_from(offset).ok()
    }
    .ok_or_else(invalid)?;
    let length = index.checked_add(1).ok_or_else(invalid)?;
    if length.saturating_sub(bytes.len()) > MAX_GROWTH {
        return Err(invalid());
    }
    let [byte] = replacement.as_bytes() else {
        return Err(invalid());
    };
    if length > bytes.len() {
        bytes.resize(length, b' ');
    }
    bytes[index] = *byte;
    String::from_utf8(bytes)
        .map(Json::String)
        .map_err(|_| invalid())
}

fn update(mut current: Json, keys: &[&str], mut value: Json) -> Json {
    let mut parents = Vec::with_capacity(keys.len());
    for &key in keys {
        let mut parent = match current {
            Json::Object(values) => values,
            Json::Array(values) => values
                .into_iter()
                .enumerate()
                .map(|(index, value)| (index.to_string(), value))
                .collect(),
            _ => Map::new(),
        };
        current = parent.remove(key).unwrap_or(Json::Null);
        parents.push((parent, key));
    }
    for (mut parent, key) in parents.into_iter().rev() {
        parent.insert(key.into(), value);
        value = if (0..parent.len()).all(|index| parent.contains_key(&index.to_string())) {
            Json::Array(
                (0..parent.len())
                    .map(|index| parent.remove(&index.to_string()).expect("checked index"))
                    .collect(),
            )
        } else {
            Json::Object(parent)
        };
    }
    value
}

fn check_depth(value: &Json) -> DatabaseResult<()> {
    let mut pending = vec![(value, 0)];
    while let Some((value, depth)) = pending.pop() {
        if depth >= MAX_DEPTH && matches!(value, Json::Array(_) | Json::Object(_)) {
            return Err(invalid());
        }
        match value {
            Json::Array(values) => pending.extend(values.iter().map(|value| (value, depth + 1))),
            Json::Object(values) => pending.extend(values.values().map(|value| (value, depth + 1))),
            _ => {}
        }
    }
    Ok(())
}

pub(super) fn invalid() -> DatabaseError {
    DatabaseError::InvalidQuery("model JSON path")
}
