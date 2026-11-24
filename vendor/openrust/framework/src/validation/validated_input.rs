mod operations;
pub use operations::ValidatedInputIter;

use std::collections::BTreeMap;

use serde_json::Map;

use crate::http::UploadedFile;

use super::{
    Data, ValidationResult, Value,
    path::{self, Segment},
};

/// Contains only the attributes selected by the validator.
#[derive(Clone, Debug)]
pub struct ValidatedInput {
    input: Value,
    next_index: crate::support::NextIndex,
    files: BTreeMap<String, UploadedFile>,
}

impl ValidatedInput {
    pub fn all(&self) -> &Value {
        &self.input
    }

    pub fn into_value(self) -> Value {
        self.input
    }

    pub fn get(&self, attribute: &str) -> Option<&Value> {
        let segments = path::parse(attribute).ok()?;
        segments.iter().try_fold(&self.input, |value, segment| {
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

    pub fn file(&self, attribute: &str) -> Option<&UploadedFile> {
        self.files.get(attribute)
    }

    pub(super) fn project(
        data: &Data,
        fields: &[(String, Option<&Value>)],
        array_fields: &std::collections::BTreeSet<String>,
    ) -> ValidationResult<Self> {
        let mut selected = Map::new();
        let declarations = fields
            .iter()
            .map(|(field, _)| field.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        for (field, value) in fields {
            let Some(value) = value else { continue };
            if array_fields.contains(field) && (value.is_object() || value.is_array()) {
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
        let mut result = Self {
            input: shape(selected, data.as_value()),
            files: BTreeMap::new(),
            next_index: crate::support::NextIndex::Unset,
        };
        result.files = data
            .uploads()
            .iter()
            .filter(|(path, _)| result.get(path).is_some())
            .map(|(path, file)| (path.clone(), file.clone()))
            .collect();
        result.next_index = crate::support::Collection::from_entries(result.iter()).append_state();
        Ok(result)
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
