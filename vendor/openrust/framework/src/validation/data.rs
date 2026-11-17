use std::collections::BTreeMap;

use serde_json::Value;

use crate::http::{HttpError, Request, UploadedFile};

use super::{ValidationError, ValidationResult, path};

/// Structured request data supplied to a validator.
#[derive(Clone, Debug)]
pub struct Data {
    input: Value,
    uploads: BTreeMap<String, UploadedFile>,
    upload_shape: Value,
}

#[allow(
    non_snake_case,
    reason = "Matches Laravel's request-based validation API"
)]
impl Data {
    /// Builds validation input from a JSON object or array.
    pub fn new(input: Value) -> ValidationResult<Self> {
        check(&input)?;
        Ok(Self {
            input,
            uploads: BTreeMap::new(),
            upload_shape: Value::Null,
        })
    }

    /// Combines request input and the typed uploads from the multipart parser.
    pub async fn fromRequest(request: &mut Request) -> Result<Self, HttpError> {
        let mut data = Self::new(
            request
                .input(None)
                .await?
                .unwrap_or_else(|| serde_json::json!({})),
        )?;
        let (indices, files) = request.validation_uploads().await?;
        if !files.is_empty() {
            data.upload_shape = collect_uploads(indices, files, "", &mut data.uploads)?;
            merge_uploads(&mut data.input, &data.upload_shape);
            check(&data.input)?;
        }
        Ok(data)
    }

    /// Replaces the scalar input while retaining uploaded files for validation.
    pub fn replace(&mut self, mut input: Value) -> ValidationResult<()> {
        check(&input)?;
        if !self.uploads.is_empty() {
            merge_uploads(&mut input, &self.upload_shape);
            check(&input)?;
        }
        self.input = input;
        Ok(())
    }

    /// Returns the JSON input, representing typed file leaves as null.
    pub fn as_value(&self) -> &Value {
        &self.input
    }

    /// Returns an upload by its dotted validation path.
    pub fn file(&self, attribute: &str) -> Option<&UploadedFile> {
        self.uploads.get(attribute)
    }

    pub(super) fn uploads(&self) -> &BTreeMap<String, UploadedFile> {
        &self.uploads
    }

    /// Resolves a dot path or wildcard against this input.
    ///
    /// An absent child beneath a matched parent is returned with no value.
    /// An explicit JSON null remains a supplied value.
    pub fn attributes(&self, attribute: &str) -> ValidationResult<Vec<(String, Option<&Value>)>> {
        path::expand(
            &self.input,
            &path::parse(attribute)?,
            &mut path::Budget::default(),
        )
    }
}

fn check(input: &Value) -> ValidationResult<()> {
    if !(input.is_object() || input.is_array()) {
        return Err(ValidationError::InvalidPath);
    }
    path::inspect(input)
}

fn collect_uploads(
    indices: &Value,
    files: &[UploadedFile],
    prefix: &str,
    output: &mut BTreeMap<String, UploadedFile>,
) -> ValidationResult<Value> {
    Ok(match indices {
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, child)| {
                    Ok((
                        key.clone(),
                        collect_uploads(child, files, &path::join(prefix, key)?, output)?,
                    ))
                })
                .collect::<ValidationResult<_>>()?,
        ),
        Value::Array(fields) => Value::Array(
            fields
                .iter()
                .enumerate()
                .map(|(index, child)| {
                    collect_uploads(
                        child,
                        files,
                        &path::join(prefix, &index.to_string())?,
                        output,
                    )
                })
                .collect::<ValidationResult<_>>()?,
        ),
        Value::String(index) => {
            let file = index
                .parse::<usize>()
                .ok()
                .and_then(|index| files.get(index))
                .ok_or(ValidationError::InvalidPath)?;
            output.insert(prefix.to_owned(), file.clone());
            // Keep an internal leaf marker until merging so sparse arrays retain their text inputs.
            Value::Bool(true)
        }
        _ => Value::Null,
    })
}

fn merge_uploads(input: &mut Value, shape: &Value) {
    match shape {
        Value::Object(fields) => {
            if !input.is_object() {
                let map = match std::mem::take(input) {
                    Value::Array(values) => values
                        .into_iter()
                        .enumerate()
                        .map(|(index, value)| (index.to_string(), value))
                        .collect(),
                    _ => serde_json::Map::new(),
                };
                *input = Value::Object(map);
            }
            let input = input.as_object_mut().expect("object input");
            for (key, child) in fields {
                merge_uploads(input.entry(key).or_insert(Value::Null), child);
            }
        }
        Value::Array(fields) => {
            if let Value::Object(input) = input {
                for (index, child) in fields
                    .iter()
                    .enumerate()
                    .filter(|(_, child)| !child.is_null())
                {
                    merge_uploads(input.entry(index.to_string()).or_insert(Value::Null), child);
                }
            } else {
                if !input.is_array() {
                    *input = Value::Array(Vec::new());
                }
                let input = input.as_array_mut().expect("array input");
                input.resize(input.len().max(fields.len()), Value::Null);
                for (input, shape) in input.iter_mut().zip(fields) {
                    merge_uploads(input, shape);
                }
            }
        }
        Value::Bool(true) => *input = Value::Null,
        _ => {}
    }
}
