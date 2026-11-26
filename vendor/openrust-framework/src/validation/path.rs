//! Resolves framework field paths within a bounded input and match budget.
use super::{ValidationError, ValidationResult as Result};
use serde_json::Value;

pub(super) const MAX_FIELDS: usize = 1024;
pub(super) const MAX_PATH: usize = 1024;
pub(super) const MAX_EXPANDED: usize = 4096;
const MAX_VALUE_BYTES: usize = 20 * 1024 * 1024;
const MAX_MATCH_STEPS: usize = 1024 * 1024;

fn limit() -> ValidationError {
    ValidationError::LimitsExceeded
}

fn invalid() -> ValidationError {
    ValidationError::InvalidPath
}

pub(super) fn inspect(input: &Value) -> Result<()> {
    fn visit(value: &Value, depth: usize, fields: &mut usize, bytes: &mut usize) -> Result<()> {
        if depth > 64 {
            return Err(limit());
        }
        match value {
            Value::Object(values) => {
                for (key, value) in values {
                    *fields += 1;
                    *bytes = bytes.saturating_add(key.len());
                    if *fields > MAX_FIELDS || *bytes > MAX_VALUE_BYTES {
                        return Err(limit());
                    }
                    visit(value, depth + 1, fields, bytes)?;
                }
            }
            Value::Array(values) => {
                for value in values {
                    *fields += 1;
                    if *fields > MAX_FIELDS {
                        return Err(limit());
                    }
                    visit(value, depth + 1, fields, bytes)?;
                }
            }
            Value::String(value) => {
                *bytes = bytes.saturating_add(value.len());
            }
            _ => {}
        }
        if *bytes > MAX_VALUE_BYTES {
            return Err(limit());
        }
        Ok(())
    }
    visit(input, 0, &mut 0, &mut 0)
}

#[derive(Clone)]
pub(super) enum Segment {
    Key(String),
    Any,
}

pub(super) fn parse(path: &str) -> Result<Vec<Segment>> {
    if path.len() > MAX_PATH {
        return Err(limit());
    }
    let mut segments = Vec::new();
    let mut key = String::new();
    let mut escaped = false;
    let mut literal_star = false;
    for ch in path.chars() {
        if escaped {
            if !matches!(ch, '.' | '*' | '\\') {
                return Err(invalid());
            }
            literal_star |= ch == '*';
            key.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '.' {
            segments.push(segment(std::mem::take(&mut key), literal_star));
            literal_star = false;
        } else {
            key.push(ch);
        }
    }
    if escaped {
        return Err(invalid());
    }
    segments.push(segment(key, literal_star));
    if segments.len() > 64 {
        return Err(limit());
    }
    Ok(segments)
}
fn segment(key: String, literal_star: bool) -> Segment {
    if key == "*" && !literal_star {
        Segment::Any
    } else {
        Segment::Key(key)
    }
}
pub(super) fn join(prefix: &str, key: &str) -> Result<String> {
    if prefix.len().saturating_add(key.len()) > MAX_PATH {
        return Err(limit());
    }
    let mut out = prefix.to_owned();
    if !prefix.is_empty() {
        out.push('.');
    }
    for ch in key.chars() {
        if matches!(ch, '.' | '*' | '\\') {
            out.push('\\');
        }
        out.push(ch);
        if out.len() > MAX_PATH {
            return Err(limit());
        }
    }
    Ok(out)
}

#[derive(Default)]
pub(super) struct Budget {
    count: usize,
    bytes: usize,
    steps: usize,
}

pub(super) fn expand<'a>(
    input: &'a Value,
    segments: &[Segment],
    budget: &mut Budget,
) -> Result<Vec<(String, Option<&'a Value>)>> {
    fn walk<'a>(
        value: Option<&'a Value>,
        segments: &[Segment],
        prefix: &str,
        output: &mut Vec<(String, Option<&'a Value>)>,
        budget: &mut Budget,
    ) -> Result<()> {
        budget.steps += 1;
        if budget.steps > MAX_MATCH_STEPS {
            return Err(limit());
        }
        let Some((first, rest)) = segments.split_first() else {
            budget.count += 1;
            budget.bytes = budget.bytes.saturating_add(prefix.len());
            if budget.count > MAX_EXPANDED || budget.bytes > MAX_VALUE_BYTES {
                return Err(limit());
            }
            output.push((prefix.to_owned(), value));
            return Ok(());
        };
        match first {
            Segment::Key(key) => {
                let next = match value {
                    Some(Value::Object(map)) => map.get(key),
                    Some(Value::Array(values)) => key
                        .parse::<usize>()
                        .ok()
                        .filter(|index| index.to_string() == *key)
                        .and_then(|index| values.get(index)),
                    _ => None,
                };
                walk(next, rest, &join(prefix, key)?, output, budget)?;
            }
            Segment::Any => match value {
                Some(Value::Object(map)) => {
                    for (key, value) in map {
                        walk(Some(value), rest, &join(prefix, key)?, output, budget)?;
                    }
                }
                Some(Value::Array(values)) => {
                    for (key, value) in values.iter().enumerate() {
                        walk(
                            Some(value),
                            rest,
                            &join(prefix, &key.to_string())?,
                            output,
                            budget,
                        )?;
                    }
                }
                _ => {}
            },
        }
        Ok(())
    }
    let mut output = Vec::new();
    walk(Some(input), segments, "", &mut output, budget)?;
    Ok(output)
}

/// 与规则解析共用转义路径，避免把字面点误作层级或为稀疏索引分配巨量数组。
pub(super) fn set(input: &mut Value, segments: &[Segment], value: Value) -> Result<()> {
    let Some((Segment::Key(key), rest)) = segments.split_first() else {
        return Err(invalid());
    };
    if !input.is_object() && !input.is_array() {
        *input = if key == "0" {
            Value::Array(Vec::new())
        } else {
            Value::Object(serde_json::Map::new())
        };
    }
    if let Value::Array(items) = input {
        let index = key
            .parse::<usize>()
            .ok()
            .filter(|index| index.to_string() == *key);
        if let Some(index) = index.filter(|index| *index <= items.len()) {
            if index == items.len() {
                items.push(Value::Null);
            }
            if rest.is_empty() {
                items[index] = value;
            } else {
                set(&mut items[index], rest, value)?;
            }
            return Ok(());
        }
        let values = std::mem::take(items)
            .into_iter()
            .enumerate()
            .map(|(i, value)| (i.to_string(), value))
            .collect();
        *input = Value::Object(values);
    }
    let values = input.as_object_mut().expect("已转换为对象");
    if rest.is_empty() {
        values.insert(key.clone(), value);
    } else {
        set(values.entry(key).or_insert(Value::Null), rest, value)?;
    }
    Ok(())
}

pub(super) fn from_flat(flat: &serde_json::Map<String, Value>) -> Result<Value> {
    let mut input = Value::Null;
    for (key, value) in flat {
        set(&mut input, &parse(key)?, value.clone())?;
    }
    fn pack(value: Value) -> Value {
        match value {
            Value::Object(mut fields) => {
                for value in fields.values_mut() {
                    *value = pack(std::mem::take(value));
                }
                if !fields.is_empty()
                    && (0..fields.len()).all(|i| fields.contains_key(&i.to_string()))
                {
                    Value::Array(
                        (0..fields.len())
                            .map(|i| fields.remove(&i.to_string()).expect("连续索引"))
                            .collect(),
                    )
                } else {
                    Value::Object(fields)
                }
            }
            Value::Array(fields) => Value::Array(fields.into_iter().map(pack).collect()),
            value => value,
        }
    }
    let input = if input.is_null() {
        Value::Object(serde_json::Map::new())
    } else {
        pack(input)
    };
    inspect(&input)?;
    Ok(input)
}

pub(super) fn parent(attribute: &str) -> Result<Option<String>> {
    let segments = parse(attribute)?;
    if segments.len() < 2 {
        return Ok(None);
    }
    let mut parent = String::new();
    for segment in &segments[..segments.len() - 1] {
        let Segment::Key(key) = segment else {
            return Err(invalid());
        };
        parent = join(&parent, key)?;
    }
    Ok(Some(parent))
}

pub(super) fn forget(input: &mut Value, segments: &[Segment]) -> Result<()> {
    let Some((Segment::Key(key), rest)) = segments.split_first() else {
        return Err(invalid());
    };
    match input {
        Value::Object(fields) => {
            if rest.is_empty() {
                fields.remove(key);
            } else if let Some(value) = fields.get_mut(key) {
                forget(value, rest)?;
            }
        }
        Value::Array(fields) => {
            if let Some(index) = key
                .parse::<usize>()
                .ok()
                .filter(|i| i.to_string() == *key && *i < fields.len())
            {
                if rest.is_empty() {
                    if index + 1 == fields.len() {
                        fields.pop();
                    } else {
                        let mut map: serde_json::Map<_, _> = std::mem::take(fields)
                            .into_iter()
                            .enumerate()
                            .map(|(i, value)| (i.to_string(), value))
                            .collect();
                        map.remove(key);
                        *input = Value::Object(map);
                    }
                } else {
                    forget(&mut fields[index], rest)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}
