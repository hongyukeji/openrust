use std::fmt;

use indexmap::IndexMap;
use serde_json::Value;

use crate::{
    http::{HttpError, Request, UploadedFile, UploadedFileValue},
    support::{ArrayKey, data_path_segments},
};

/// A request field containing a JSON scalar, an ordered array, or a received file.
#[derive(Clone)]
pub enum RequestValue {
    Value(Value),
    Array(IndexMap<ArrayKey, Self>),
    File(UploadedFile),
}

impl fmt::Debug for RequestValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Value(_) => formatter.write_str("Value([redacted])"),
            Self::Array(fields) => formatter.debug_tuple("Array").field(&fields.len()).finish(),
            Self::File(_) => formatter.write_str("File([redacted])"),
        }
    }
}

impl RequestValue {
    /// Reads an exact field key before attempting to resolve a dotted path.
    pub fn get(&self, key: &str) -> Option<&Self> {
        self.direct(key).or_else(|| {
            data_path_segments(key)
                .ok()?
                .into_iter()
                .try_fold(self, Self::direct)
        })
    }

    fn direct(&self, key: &str) -> Option<&Self> {
        match self {
            Self::Array(fields) => fields.get(&ArrayKey::from(key)),
            _ => None,
        }
    }

    fn set(&mut self, path: &[&str], value: Self) {
        let Some((&key, rest)) = path.split_first() else {
            *self = value;
            return;
        };
        if !matches!(self, Self::Array(_)) {
            *self = Self::Array(IndexMap::new());
        }
        if let Self::Array(fields) = self {
            fields
                .entry(ArrayKey::from(key))
                .or_insert(Self::Value(Value::Null))
                .set(rest, value);
        }
    }

    fn from_input(input: Value) -> Self {
        match input {
            Value::Array(items) => Self::Array(
                (0_i64..)
                    .zip(items)
                    .map(|(index, item)| (ArrayKey::Integer(index), Self::from_input(item)))
                    .collect(),
            ),
            Value::Object(fields) => Self::Array(
                fields
                    .into_iter()
                    .map(|(key, item)| (ArrayKey::from(key), Self::from_input(item)))
                    .collect(),
            ),
            value => Self::Value(value),
        }
    }

    fn from_upload(upload: UploadedFileValue) -> Self {
        match upload {
            UploadedFileValue::File(file) => Self::File(file),
            UploadedFileValue::Null => Self::Value(Value::Null),
            UploadedFileValue::Files(fields) => Self::Array(
                fields
                    .into_iter()
                    .map(|(key, field)| (ArrayKey::from(key), Self::from_upload(field)))
                    .collect(),
            ),
        }
    }

    fn merge_files(&mut self, upload: UploadedFileValue) {
        if let (Self::Array(input), UploadedFileValue::Files(files)) = (self, upload) {
            for (key, upload) in files {
                match input.entry(ArrayKey::from(key)) {
                    indexmap::map::Entry::Occupied(mut item) => item.get_mut().merge_files(upload),
                    indexmap::map::Entry::Vacant(item) => {
                        item.insert(Self::from_upload(upload));
                    }
                }
            }
        }
    }
}

impl Request {
    /// Combines normal input and uploaded files, preserving input precedence and key order.
    pub async fn all(&mut self) -> Result<RequestValue, HttpError> {
        let values = self
            .input(None)
            .await?
            .unwrap_or_else(|| Value::Array(Vec::new()));
        let mut input = RequestValue::from_input(values);
        input.merge_files(self.allFiles().await?);
        Ok(input)
    }

    /// Selects fields from all input and files. Missing fields become explicit null.
    #[allow(
        non_snake_case,
        reason = "Typed form of Laravel's all with field selection"
    )]
    pub async fn allOnly(&mut self, keys: &[&str]) -> Result<RequestValue, HttpError> {
        let input = self.all().await?;
        if keys.is_empty() {
            return Ok(input);
        }
        let mut selected = RequestValue::Array(IndexMap::new());
        for key in keys {
            let path = data_path_segments(key).map_err(|_| HttpError::InvalidRequest)?;
            let field = input
                .get(key)
                .cloned()
                .unwrap_or(RequestValue::Value(Value::Null));
            selected.set(&path, field);
        }
        Ok(selected)
    }
}
