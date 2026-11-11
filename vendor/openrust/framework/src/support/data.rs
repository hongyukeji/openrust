//! Helpers for ordered JSON arrays and objects, using Laravel dot paths.

use serde_json::{Map, Value};

pub(super) fn read(value: &Value, parts: &[&str]) -> Option<Value> {
    let Some((&part, rest)) = parts.split_first() else {
        return Some(value.clone());
    };
    if part == "*" {
        let read_child = |item: &Value| read(item, rest).unwrap_or(Value::Null);
        let result: Vec<_> = match value {
            Value::Array(values) => values.iter().map(read_child).collect(),
            Value::Object(values) => values.values().map(read_child).collect(),
            _ => return None,
        };
        return Some(if rest.contains(&"*") {
            collapse(result)
        } else {
            Value::Array(result)
        });
    }
    let child = match part {
        "\\*" => direct(value, "*"),
        "\\{first}" => direct(value, "{first}"),
        "\\{last}" => direct(value, "{last}"),
        "{first}" => match value {
            Value::Array(values) => values.first(),
            Value::Object(values) => values.values().next(),
            _ => None,
        },
        "{last}" => match value {
            Value::Array(values) => values.last(),
            Value::Object(values) => values.values().next_back(),
            _ => None,
        },
        _ => direct(value, part),
    }?;
    read(child, rest)
}

// Laravel collapses nested wildcard results with array_merge semantics.
fn collapse(values: Vec<Value>) -> Value {
    let mut result = Map::new();
    let mut next = 0_usize;
    for value in values {
        for (key, child) in entries(value) {
            if integer_key(&key) {
                result.insert(next.to_string(), child);
                next += 1;
            } else {
                result.insert(key, child);
            }
        }
    }
    packed(result)
}

pub(super) fn direct<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    match value {
        Value::Object(values) => values.get(key),
        Value::Array(values) => key
            .parse::<usize>()
            .ok()
            .filter(|index| index.to_string() == key)
            .and_then(|index| values.get(index)),
        _ => None,
    }
}

pub(super) fn normalize(value: Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.into_iter().map(normalize).collect()),
        Value::Object(values) => packed(
            values
                .into_iter()
                .map(|(key, value)| (key, normalize(value)))
                .collect(),
        ),
        scalar => scalar,
    }
}

fn entries(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(values) => values,
        Value::Array(values) => values
            .into_iter()
            .enumerate()
            .map(|(index, value)| (index.to_string(), value))
            .collect(),
        _ => Map::new(),
    }
}

pub(super) fn packed(values: Map<String, Value>) -> Value {
    if values
        .keys()
        .enumerate()
        .all(|(index, key)| key == &index.to_string())
    {
        Value::Array(values.into_values().collect())
    } else {
        Value::Object(values)
    }
}

fn integer_key(key: &str) -> bool {
    key.parse::<i64>()
        .is_ok_and(|number| number.to_string() == key)
}

pub(super) fn write(
    target: &mut Value,
    parts: &[&str],
    value: &Value,
    overwrite: bool,
    expand_wildcards: bool,
) {
    let Some((&key, rest)) = parts.split_first() else {
        if overwrite {
            target.clone_from(value);
        }
        return;
    };
    if expand_wildcards && key == "*" {
        match target {
            Value::Array(items) => {
                for child in items {
                    write(child, rest, value, overwrite, expand_wildcards);
                }
            }
            Value::Object(items) => {
                for child in items.values_mut() {
                    write(child, rest, value, overwrite, expand_wildcards);
                }
            }
            _ => *target = Value::Array(Vec::new()),
        }
        return;
    }
    let container = target.is_array() || target.is_object();
    let mut items = entries(std::mem::take(target));
    if !rest.is_empty() {
        write(
            items.entry(key).or_insert_with(|| Value::Array(Vec::new())),
            rest,
            value,
            overwrite,
            expand_wildcards,
        );
    } else if overwrite || (container && !items.contains_key(key)) {
        items.insert(key.to_owned(), value.clone());
    }
    *target = packed(items);
}

fn forget(target: &mut Value, parts: &[&str]) {
    let Some((&key, rest)) = parts.split_first() else {
        return;
    };
    if key == "*" {
        match target {
            Value::Array(items) => items.iter_mut().for_each(|child| forget(child, rest)),
            Value::Object(items) => items.values_mut().for_each(|child| forget(child, rest)),
            _ => {}
        }
        return;
    }
    if direct(target, key).is_none() {
        return;
    }
    if let Value::Object(items) = target {
        if rest.is_empty() {
            items.shift_remove(key);
        } else if let Some(child) = items.get_mut(key) {
            forget(child, rest);
        }
        return;
    }
    if rest.is_empty() {
        let mut items = entries(std::mem::take(target));
        items.shift_remove(key);
        *target = packed(items);
    } else if let Value::Array(items) = target
        && let Some(child) = key
            .parse::<usize>()
            .ok()
            .and_then(|index| items.get_mut(index))
    {
        forget(child, rest);
    }
}

pub(super) fn segments(key: &str) -> Result<Vec<&str>, DataPathError> {
    let parts: Vec<_> = key.split('.').take(MAX_DEPTH + 1).collect();
    if parts.len() > MAX_DEPTH {
        return Err(DataPathError);
    }
    Ok(parts)
}

pub(super) fn check_depth(value: &Value, depth: usize) -> Result<(), DataPathError> {
    if !matches!(value, Value::Array(_) | Value::Object(_)) {
        return Ok(());
    }
    if depth >= MAX_DEPTH {
        return Err(DataPathError);
    }
    match value {
        Value::Array(items) => items
            .iter()
            .try_for_each(|value| check_depth(value, depth + 1)),
        Value::Object(items) => items
            .values()
            .try_for_each(|value| check_depth(value, depth + 1)),
        _ => Ok(()),
    }
}

const MAX_DEPTH: usize = 127;

/// The path or resulting nested value exceeds the JSON nesting limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataPathError;

impl std::fmt::Display for DataPathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("data path or value exceeds maximum nesting depth")
    }
}

impl std::error::Error for DataPathError {}

/// Reads a dot path. A missing member yields None, while JSON null remains a value.
pub fn data_get(target: &Value, key: Option<&str>) -> Option<Value> {
    match key {
        Some(key) => read(target, &segments(key).ok()?),
        None => Some(target.clone()),
    }
}

/// Tests an exact sequence of path components. Wildcards are literal for existence checks.
pub fn data_has(target: &Value, key: &str) -> bool {
    segments(key)
        .ok()
        .and_then(|parts| parts.into_iter().try_fold(target, direct))
        .is_some()
}

/// Sets a path, expanding wildcards over existing children.
pub fn data_set(target: &mut Value, key: &str, value: Value) -> Result<(), DataPathError> {
    set(target, key, value, true, true)
}

/// Fills missing leaves, preserving existing values including explicit null.
pub fn data_fill(target: &mut Value, key: &str, value: Value) -> Result<(), DataPathError> {
    set(target, key, value, false, true)
}

pub(super) fn set(
    target: &mut Value,
    key: &str,
    value: Value,
    overwrite: bool,
    expand_wildcards: bool,
) -> Result<(), DataPathError> {
    let parts = segments(key)?;
    check_depth(target, 0)?;
    check_depth(&value, 0)?;
    let mut next = target.clone();
    write(&mut next, &parts, &value, overwrite, expand_wildcards);
    check_depth(&next, 0)?;
    *target = next;
    Ok(())
}

/// Removes a dot path. When an array gains an index gap, it becomes a keyed JSON object.
pub fn data_forget(target: &mut Value, key: &str) -> Result<(), DataPathError> {
    let parts = segments(key)?;
    forget(target, &parts);
    Ok(())
}
