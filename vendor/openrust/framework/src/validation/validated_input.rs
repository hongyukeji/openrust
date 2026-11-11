use serde_json::Map;

use super::{
    ValidationResult, Value,
    path::{self, Segment},
};

/// Contains only the attributes selected by the validator.
#[derive(Clone, Debug)]
pub struct ValidatedInput(Value);

impl ValidatedInput {
    pub fn all(&self) -> &Value {
        &self.0
    }

    pub fn into_value(self) -> Value {
        self.0
    }

    pub fn get(&self, attribute: &str) -> Option<&Value> {
        let segments = path::parse(attribute).ok()?;
        segments.iter().try_fold(&self.0, |value, segment| {
            let Segment::Key(key) = segment else {
                return None;
            };
            match value {
                Value::Array(items) => items.get(key.parse::<usize>().ok()?),
                Value::Object(items) => items.get(key),
                _ => None,
            }
        })
    }

    pub(super) fn project(
        input: &Value,
        fields: &[(String, Option<&Value>)],
    ) -> ValidationResult<Self> {
        let mut selected = Map::new();
        let declarations = fields
            .iter()
            .map(|(field, _)| field.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        for (field, value) in fields {
            let Some(value) = value else { continue };
            if value.is_object() || value.is_array() {
                let prefix = format!("{field}.");
                if declarations
                    .range(prefix.as_str()..)
                    .next()
                    .is_some_and(|path| path.starts_with(&prefix))
                {
                    continue;
                }
            }
            let segments = path::parse(field)?;
            insert(&mut selected, &segments, value);
        }
        Ok(Self(shape(selected, input)))
    }
}

fn insert(map: &mut Map<String, Value>, path: &[Segment], value: &Value) {
    let Some((Segment::Key(key), rest)) = path.split_first() else {
        return;
    };
    if rest.is_empty() {
        map.insert(key.clone(), value.clone());
    } else if let Some(child) = map
        .entry(key)
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
    {
        insert(child, rest, value);
    }
}

fn shape(mut map: Map<String, Value>, input: &Value) -> Value {
    for (key, value) in &mut map {
        if !value.is_object() {
            continue;
        }
        let source = match input {
            Value::Array(items) => key.parse::<usize>().ok().and_then(|index| items.get(index)),
            Value::Object(items) => items.get(key),
            _ => None,
        };
        if let Some(source) = source
            && let Value::Object(children) = std::mem::take(value)
        {
            *value = shape(children, source);
        }
    }
    if input.is_array() && (0..map.len()).all(|index| map.contains_key(&index.to_string())) {
        Value::Array(
            (0..map.len())
                .map(|index| map.remove(&index.to_string()).unwrap_or(Value::Null))
                .collect(),
        )
    } else {
        Value::Object(map)
    }
}
