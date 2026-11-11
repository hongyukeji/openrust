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

pub(super) fn write(target: &mut Value, parts: &[&str], value: &Value) {
    let Some((&key, rest)) = parts.split_first() else {
        target.clone_from(value);
        return;
    };
    if key == "*" {
        match target {
            Value::Array(items) => {
                for child in items {
                    write(child, rest, value);
                }
            }
            Value::Object(items) => {
                for child in items.values_mut() {
                    write(child, rest, value);
                }
            }
            _ => *target = Value::Array(Vec::new()),
        }
        return;
    }
    let mut items = entries(std::mem::take(target));
    write(items.entry(key).or_insert(Value::Null), rest, value);
    *target = packed(items);
}
