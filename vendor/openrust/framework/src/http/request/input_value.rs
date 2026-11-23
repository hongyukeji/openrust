use std::fmt;

use indexmap::IndexMap;
use serde_json::Value;

use crate::{
    container::ParameterValue,
    http::{HttpError, Request, UploadedFile, UploadedFileValue},
    support::{
        ArrayKey, Collection, DATA_MAX_DEPTH, Fluent, FluentError, data_check_depth,
        data_path_segments,
    },
};

/// A dynamic request field or the bound object resolved from its route parameter.
#[derive(Clone, Debug)]
pub enum RequestField {
    Value(RequestValue),
    RouteModel(ParameterValue),
}

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

    fn set(&mut self, path: &[&str], value: Self, expand_wildcards: bool) {
        let Some((&key, rest)) = path.split_first() else {
            *self = value;
            return;
        };
        if !matches!(self, Self::Array(_)) {
            *self = Self::Array(IndexMap::new());
        }
        if let Self::Array(fields) = self {
            if expand_wildcards && key == "*" {
                for field in fields.values_mut() {
                    field.set(rest, value.clone(), true);
                }
            } else {
                fields
                    .entry(ArrayKey::from(key))
                    .or_insert(Self::Value(Value::Null))
                    .set(rest, value, expand_wildcards);
            }
        }
    }

    fn check_depth(&self, depth: usize) -> Result<(), FluentError> {
        match self {
            Self::Array(_) if depth >= DATA_MAX_DEPTH => Err(FluentError::Depth),
            Self::Array(fields) => fields
                .values()
                .try_for_each(|field| field.check_depth(depth + 1)),
            Self::Value(value) => Ok(data_check_depth(value, depth)?),
            Self::File(_) => Ok(()),
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

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl Fluent<RequestValue> {
    /// Reads a nested attribute, with the same selectors as request input.
    pub fn get(&self, key: &str) -> Option<RequestValue> {
        self.all().data(key)
    }

    /// Reads a top-level attribute by its exact key.
    pub fn value(&self, key: &str) -> Option<&RequestValue> {
        self.all().direct(key)
    }

    pub fn has(&self, key: &str) -> bool {
        self.all().get(key).is_some()
    }

    /// Updates this Fluent bag using a dot path or wildcard and preserves typed uploads.
    pub fn set(&mut self, key: &str, value: RequestValue) -> Result<&mut Self, FluentError> {
        let path = data_path_segments(key)?;
        value.check_depth(0)?;
        let mut attributes = self.to_array();
        attributes.set(&path, value, true);
        attributes.check_depth(0)?;
        *self = Self::from_attributes(attributes);
        Ok(self)
    }

    /// Returns an independent bag rooted at the selected attribute.
    pub fn scope(&self, key: &str) -> Self {
        let value = match self.get(key) {
            None | Some(RequestValue::Value(Value::Null)) => RequestValue::Array(IndexMap::new()),
            Some(value @ RequestValue::Array(_)) => value,
            Some(value) => RequestValue::Array([(ArrayKey::Integer(0), value)].into()),
        };
        Self::from_attributes(value)
    }

    pub fn is_empty(&self) -> bool {
        matches!(self.all(), RequestValue::Array(fields) if fields.is_empty())
    }

    pub fn is_not_empty(&self) -> bool {
        !self.is_empty()
    }
}

impl Request {
    /// Reads merged input or files, then falls back to the current route's parameter.
    pub async fn field(&mut self, key: &str) -> Result<Option<RequestField>, HttpError> {
        if let Some(value) = self.all().await?.get(key).cloned() {
            return Ok(Some(RequestField::Value(value)));
        }
        if let Some(model) = self.route_models.get(key) {
            return Ok(Some(RequestField::RouteModel(model.clone())));
        }
        Ok(self
            .route(key)
            .map(|value| RequestField::Value(RequestValue::Value(Value::String(value.to_owned())))))
    }

    /// Combines normal input and uploaded files, preserving input precedence and key order.
    pub async fn all(&mut self) -> Result<RequestValue, HttpError> {
        let values = self
            .input(None)
            .await?
            .unwrap_or_else(|| Value::Array(Vec::new()));
        let mut input = RequestValue::from_input(values);
        input.merge_files(self.all_files().await?);
        Ok(input)
    }

    /// Selects fields from all input and files. Missing fields become explicit null.
    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    pub async fn all_only(&mut self, keys: &[&str]) -> Result<RequestValue, HttpError> {
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
            selected.set(&path, field, false);
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
                selected.set(&path, field, false);
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

    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    pub async fn array_only(&mut self, keys: &[&str]) -> Result<RequestValue, HttpError> {
        self.only(keys).await
    }

    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    pub async fn collect_only(
        &mut self,
        keys: &[&str],
    ) -> Result<Collection<RequestValue>, HttpError> {
        let RequestValue::Array(fields) = self.only(keys).await? else {
            unreachable!("only builds a keyed array");
        };
        Ok(Collection::fromEntries(fields))
    }

    /// Wraps selected input and uploads in an independent typed Fluent bag.
    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    pub async fn fluent_only(&mut self, keys: &[&str]) -> Result<Fluent<RequestValue>, HttpError> {
        Ok(Fluent::from_attributes(self.only(keys).await?))
    }
}
