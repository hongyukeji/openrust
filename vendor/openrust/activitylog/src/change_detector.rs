use crate::{Error, Result};
use serde_json::{Map, Value};

pub struct ChangeDetector;
impl ChangeDetector {
    /// Keeps changed keys and the matching old values. Null remains distinct from missing/zero.
    pub fn filterDirty(
        newAttributes: &Map<String, Value>,
        oldAttributes: &Map<String, Value>,
    ) -> Value {
        let changed: Map<_, _> = newAttributes
            .iter()
            .filter(|(key, new)| {
                oldAttributes
                    .get(*key)
                    .is_none_or(|old| !equivalent(new, old))
            })
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        let old: Map<_, _> = oldAttributes
            .iter()
            .filter(|(key, _)| changed.contains_key(*key))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        serde_json::json!({"attributes":changed,"old":old})
    }
    pub fn resolveJsonAttribute(attributes: &Map<String, Value>, attribute: &str) -> Value {
        let mut path = attribute.split("->");
        let Some(mut value) = path.next().and_then(|key| attributes.get(key)) else {
            return Value::Null;
        };
        for key in path {
            let next = match value {
                Value::Array(values) => key.parse::<usize>().ok().and_then(|key| values.get(key)),
                _ => value.get(key),
            };
            let Some(next) = next else {
                return Value::Null;
            };
            value = next;
        }
        value.clone()
    }
    pub(crate) fn json(value: &openrust::database::Value) -> Result<Value> {
        use openrust::database::Value as Sql;
        Ok(match value {
            Sql::Null => Value::Null,
            Sql::Bool(value) => (*value).into(),
            Sql::Integer(value) => (*value).into(),
            Sql::Unsigned(value) => (*value).into(),
            Sql::Real(value) => serde_json::Number::from_f64(*value)
                .ok_or(Error::InvalidActivity)?
                .into(),
            Sql::Text(value) => value.clone().into(),
            Sql::Json(value) => value.clone(),
            Sql::Bytes(_) => return Err(Error::InvalidActivity),
            _ => return Err(Error::InvalidActivity),
        })
    }
}
fn equivalent(new: &Value, old: &Value) -> bool {
    if old.is_null() || new.is_null() {
        return new.is_null() && old.is_null();
    }
    phpEqual(new, old)
}
fn phpEqual(new: &Value, old: &Value) -> bool {
    if new.is_boolean() || old.is_boolean() {
        return truthy(new) == truthy(old);
    }
    if new.is_null() || old.is_null() {
        let other = if new.is_null() { old } else { new };
        return match other {
            Value::String(value) => value.is_empty(),
            _ => !truthy(other),
        };
    }
    if (new.is_array() || new.is_object()) && (old.is_array() || old.is_object()) {
        fn entries(value: &Value) -> std::collections::BTreeMap<String, &Value> {
            match value {
                Value::Array(values) => values
                    .iter()
                    .enumerate()
                    .map(|(key, value)| (key.to_string(), value))
                    .collect(),
                Value::Object(values) => values
                    .iter()
                    .map(|(key, value)| (key.clone(), value))
                    .collect(),
                _ => unreachable!(),
            }
        }
        let a = entries(new);
        let b = entries(old);
        return a.len() == b.len()
            && a.iter()
                .all(|(key, a)| b.get(key).is_some_and(|b| phpEqual(a, b)));
    }
    match (new, old) {
        (Value::Number(a), Value::Number(b)) => numeric(&a.to_string()) == numeric(&b.to_string()),
        (Value::Number(a), Value::String(b)) | (Value::String(b), Value::Number(a)) => numeric(b)
            .map_or_else(
                || a.to_string() == *b,
                |b| numeric(&a.to_string()) == Some(b),
            ),
        (Value::String(a), Value::String(b)) => match (numeric(a), numeric(b)) {
            (Some(a), Some(b)) => a == b,
            _ => a == b,
        },
        _ => new == old,
    }
}
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(v) => *v,
        Value::Number(v) => v.as_f64().is_some_and(|v| v != 0.0),
        Value::String(v) => !v.is_empty() && v != "0",
        Value::Array(v) => !v.is_empty(),
        Value::Object(v) => !v.is_empty(),
    }
}
// Decimal normalization avoids collapsing distinct u64 values through f64 conversion.
fn numeric(value: &str) -> Option<(bool, String, i64)> {
    let value = value.trim_matches(|c: char| c.is_ascii_whitespace());
    let (negative, value) = value
        .strip_prefix('-')
        .map_or((false, value.strip_prefix('+').unwrap_or(value)), |value| {
            (true, value)
        });
    let (coefficient, exponent) = value
        .split_once(['e', 'E'])
        .map_or(Some((value, 0)), |(coefficient, exponent)| {
            exponent.parse::<i64>().ok().map(|e| (coefficient, e))
        })?;
    let mut fraction = 0i64;
    let mut dot = false;
    let mut digits = String::new();
    for byte in coefficient.bytes() {
        if byte == b'.' && !dot {
            dot = true;
        } else if byte.is_ascii_digit() {
            digits.push(byte as char);
            if dot {
                fraction += 1;
            }
        } else {
            return None;
        }
    }
    if digits.is_empty() {
        return None;
    }
    let digits = digits.trim_start_matches('0');
    if digits.is_empty() {
        return Some((false, "0".into(), 0));
    }
    let significant = digits.trim_end_matches('0');
    let exponent = exponent
        .checked_sub(fraction)?
        .checked_add(i64::try_from(digits.len() - significant.len()).ok()?)?;
    Some((negative, significant.into(), exponent))
}
