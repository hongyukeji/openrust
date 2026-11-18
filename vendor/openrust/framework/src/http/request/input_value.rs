use std::fmt;

use indexmap::IndexMap;
use serde_json::Value;

use crate::{
    http::{HttpError, Request, UploadedFile, UploadedFileValue},
    support::{ArrayKey, Collection, data_path_segments},
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

    pub(super) fn data(&self, key: &str) -> Option<Self> {
        self.read(&data_path_segments(key).ok()?)
    }

    fn read(&self, path: &[&str]) -> Option<Self> {
        let Some((&key, rest)) = path.split_first() else {
            return Some(self.clone());
        };
        let Self::Array(fields) = self else {
            return None;
        };
        match key {
            "*" => {
                let mut selected = IndexMap::new();
                let mut next = 0_i64;
                if rest.contains(&"*") {
                    for field in fields.values() {
                        if let Some(Self::Array(items)) = field.read(rest) {
                            for (key, value) in items {
                                let key = match key {
                                    ArrayKey::Integer(_) => {
                                        let key = ArrayKey::Integer(next);
                                        next += 1;
                                        key
                                    }
                                    key => key,
                                };
                                selected.insert(key, value);
                            }
                        }
                    }
                } else {
                    selected.extend((0_i64..).zip(fields.values()).map(|(index, child)| {
                        let value = child.read(rest).unwrap_or(Self::Value(Value::Null));
                        (ArrayKey::Integer(index), value)
                    }));
                }
                Some(Self::Array(selected))
            }
            "{first}" => fields.first()?.1.read(rest),
            "{last}" => fields.last()?.1.read(rest),
            "\\*" | "\\{first}" | "\\{last}" => self.direct(&key[1..])?.read(rest),
            _ => self.direct(key)?.read(rest),
        }
    }

    fn forget(&mut self, path: &[&str]) {
        if let (Self::Array(fields), Some((&key, rest))) = (self, path.split_first()) {
            let key = ArrayKey::from(key);
            if rest.is_empty() {
                fields.shift_remove(&key);
            } else if let Some(field) = fields.get_mut(&key) {
                field.forget(rest);
            }
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

    /// Selects input and files by dotted path, omitting missing fields.
    pub async fn only(&mut self, keys: &[&str]) -> Result<RequestValue, HttpError> {
        let input = self.all().await?;
        let mut selected = RequestValue::Array(IndexMap::new());
        for key in keys {
            let path = data_path_segments(key).map_err(|_| HttpError::InvalidRequest)?;
            if let Some(field) = input.read(&path) {
                selected.set(&path, field);
            }
        }
        Ok(selected)
    }

    /// Removes exact keys or dotted paths from the combined input and files.
    pub async fn except(&mut self, keys: &[&str]) -> Result<RequestValue, HttpError> {
        let mut input = self.all().await?;
        for key in keys {
            let exact = [*key];
            if input.direct(key).is_some() {
                input.forget(&exact);
            } else {
                let path = data_path_segments(key).map_err(|_| HttpError::InvalidRequest)?;
                input.forget(&path);
            }
        }
        Ok(input)
    }

    #[allow(
        non_snake_case,
        reason = "Typed form of Laravel's array with field selection"
    )]
    pub async fn arrayOnly(&mut self, keys: &[&str]) -> Result<RequestValue, HttpError> {
        self.only(keys).await
    }

    #[allow(
        non_snake_case,
        reason = "Typed form of Laravel's collect with field selection"
    )]
    pub async fn collectOnly(
        &mut self,
        keys: &[&str],
    ) -> Result<Collection<RequestValue>, HttpError> {
        let RequestValue::Array(fields) = self.only(keys).await? else {
            unreachable!("only builds a keyed array");
        };
        Ok(Collection::fromEntries(fields))
    }
}
