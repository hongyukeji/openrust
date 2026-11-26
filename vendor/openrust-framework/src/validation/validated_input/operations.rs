use std::{collections::BTreeMap, ops::Index};

use crate::{
    contracts::support::{ArrayAccess, Arrayable, ValidatedData},
    http::{RequestValue, UploadedFile},
    support::{ArrayKey, Collection},
    validation::{Data, ValidationError, ValidationResult, Value, path},
};

use super::ValidatedInput;

impl ValidatedInput {
    /// 包装已核定数据；此构造器本身不执行字段验证。
    pub fn new(input: Value) -> ValidationResult<Self> {
        let (input, files) = Data::new(input)?.into_parts();
        let next_index =
            Collection::from_entries(entries(&input).into_iter().map(|(key, _)| (key, ())))
                .append_state();
        Ok(Self {
            input,
            files,
            next_index,
        })
    }

    pub fn from_request_value(input: &RequestValue) -> ValidationResult<Self> {
        let (input, files) = Data::from_request_value(input)?.into_parts();
        let next_index =
            Collection::from_entries(entries(&input).into_iter().map(|(key, _)| (key, ())))
                .append_state();
        Ok(Self {
            input,
            files,
            next_index,
        })
    }

    pub fn only(&self, keys: &[&str]) -> ValidationResult<Self> {
        let mut selected = serde_json::Map::new();
        for key in keys {
            let parts = path::parse(key)?;
            if parts.iter().any(|part| matches!(part, path::Segment::Any)) {
                return Err(ValidationError::InvalidPath);
            }
            if let Some(value) = self.get(key) {
                super::insert(&mut selected, &parts, value);
            }
        }
        let mut output = Self {
            input: super::shape(selected, &self.input),
            files: BTreeMap::new(),
            next_index: crate::support::NextIndex::Unset,
        };
        output.files = self
            .files
            .iter()
            .filter(|(key, _)| output.get(key).is_some())
            .map(|(key, file)| (key.clone(), file.clone()))
            .collect();
        output.next_index = Collection::from_entries(output.iter()).append_state();
        Ok(output)
    }

    pub fn except(&self, keys: &[&str]) -> ValidationResult<Self> {
        let mut output = self.clone();
        for key in keys {
            path::forget(&mut output.input, &path::parse(key)?)?;
            let prefix = format!("{key}.");
            output
                .files
                .retain(|path, _| path != key && !path.starts_with(&prefix));
        }
        Ok(output)
    }

    pub fn merge(&self, items: Value) -> ValidationResult<Self> {
        // 先复用原输入预算检查，再执行数组合并。
        let (items, _) = Data::new(items)?.into_parts();
        self.merge_fields(RequestValue::from_input(items))
    }

    pub fn merge_fields(&self, items: RequestValue) -> ValidationResult<Self> {
        let items = match items {
            RequestValue::Array(items) => items,
            RequestValue::Value(value) if value.is_array() || value.is_object() => {
                let RequestValue::Array(items) = RequestValue::from_input(value) else {
                    unreachable!("已核定数组");
                };
                items
            }
            _ => return Err(ValidationError::InvalidPath),
        };
        if items.is_empty() {
            return Ok(self.clone());
        }
        let merged = self.collect().merge(Collection::from_entries(items));
        let input = serde_json::to_value(&merged).map_err(|source| ValidationError::Execution {
            source: Box::new(source),
        })?;
        let mut output = Self::new(input)?;
        for (key, value) in merged {
            gather_files(&value, &escape(&key.to_string()), &mut output.files)?;
        }
        Ok(output)
    }

    pub fn collect(&self) -> Collection<RequestValue> {
        let mut values = Collection::from_entries(self);
        values.restore_append_state(self.next_index);
        values
    }

    pub fn to_array(&self) -> Value {
        self.input.clone()
    }

    /// 只借用JSON投影，不复制字段值；文件对象仍可通过file读取。
    pub fn iter(&self) -> impl Iterator<Item = (ArrayKey, &Value)> {
        entries(&self.input).into_iter()
    }

    pub fn offset_exists(&self, key: &str) -> bool {
        self.get(key).is_some()
    }
    pub fn offset_get(&self, key: &str) -> Option<RequestValue> {
        self.get(key)
            .map(|value| typed(value.clone(), key, &self.files))
    }
    pub fn offset_set(
        &mut self,
        key: Option<ArrayKey>,
        value: RequestValue,
    ) -> ValidationResult<()> {
        let mut values = self.collect();
        match key {
            Some(key) => {
                values.put(key, value);
            }
            None => {
                values
                    .push(value)
                    .map_err(|source| ValidationError::Execution {
                        source: Box::new(source),
                    })?;
            }
        }
        let input = serde_json::to_value(&values).map_err(|source| ValidationError::Execution {
            source: Box::new(source),
        })?;
        let mut next = Self::new(input)?;
        next.next_index = values.append_state();
        for (key, value) in values {
            gather_files(&value, &escape(&key.to_string()), &mut next.files)?;
        }
        *self = next;
        Ok(())
    }
    pub fn offset_unset(&mut self, key: &str) {
        // 动态offset赋值／删除使用字面键，与路径读取区分。
        if let Value::Object(fields) = &mut self.input {
            fields.remove(key);
        } else if let Value::Array(fields) = &mut self.input
            && let Ok(index) = key.parse::<usize>()
            && index.to_string() == key
            && index < fields.len()
        {
            let mut values: serde_json::Map<_, _> = std::mem::take(fields)
                .into_iter()
                .enumerate()
                .map(|(i, value)| (i.to_string(), value))
                .collect();
            values.remove(key);
            self.input = Value::Object(values);
        }
        let key = escape(key);
        let prefix = format!("{key}.");
        self.files
            .retain(|path, _| path != &key && !path.starts_with(&prefix));
    }
}

impl Arrayable for ValidatedInput {
    type Array = Value;
    fn to_array(&self) -> Value {
        self.to_array()
    }
}
impl ArrayAccess for ValidatedInput {
    fn offset_exists(&self, key: &str) -> bool {
        self.offset_exists(key)
    }
    fn offset_get(&self, key: &str) -> Option<RequestValue> {
        self.offset_get(key)
    }
    fn offset_set(&mut self, key: Option<ArrayKey>, value: RequestValue) -> ValidationResult<()> {
        self.offset_set(key, value)
    }
    fn offset_unset(&mut self, key: &str) {
        self.offset_unset(key)
    }
}
impl ValidatedData for ValidatedInput {}

impl serde::Serialize for ValidatedInput {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.input.serialize(serializer)
    }
}
static NULL: Value = Value::Null;
impl Index<&str> for ValidatedInput {
    type Output = Value;
    fn index(&self, key: &str) -> &Value {
        self.get(key).unwrap_or(&NULL)
    }
}
impl Index<usize> for ValidatedInput {
    type Output = Value;
    fn index(&self, key: usize) -> &Value {
        self.get(&key.to_string()).unwrap_or(&NULL)
    }
}

/// 借用迭代器按需取得拥有值，未遍历字段不会预先复制。
pub struct ValidatedInputIter<'a> {
    source: &'a ValidatedInput,
    fields: std::vec::IntoIter<(ArrayKey, &'a Value)>,
}
impl Iterator for ValidatedInputIter<'_> {
    type Item = (ArrayKey, RequestValue);
    fn next(&mut self) -> Option<Self::Item> {
        self.fields.next().map(|(key, value)| {
            let value = typed(value.clone(), &escape(&key.to_string()), &self.source.files);
            (key, value)
        })
    }
}
impl<'a> IntoIterator for &'a ValidatedInput {
    type Item = (ArrayKey, RequestValue);
    type IntoIter = ValidatedInputIter<'a>;
    fn into_iter(self) -> Self::IntoIter {
        ValidatedInputIter {
            source: self,
            fields: entries(&self.input).into_iter(),
        }
    }
}
impl IntoIterator for ValidatedInput {
    type Item = (ArrayKey, RequestValue);
    type IntoIter = std::vec::IntoIter<Self::Item>;
    fn into_iter(self) -> Self::IntoIter {
        let Self { input, files, .. } = self;
        let fields = match input {
            Value::Object(fields) => fields
                .into_iter()
                .map(|(key, value)| (ArrayKey::from(key), value))
                .collect::<Vec<_>>(),
            Value::Array(fields) => (0_i64..)
                .zip(fields)
                .map(|(key, value)| (ArrayKey::Integer(key), value))
                .collect(),
            _ => Vec::new(),
        };
        fields
            .into_iter()
            .map(|(key, value)| {
                let value = typed(value, &escape(&key.to_string()), &files);
                (key, value)
            })
            .collect::<Vec<_>>()
            .into_iter()
    }
}

fn entries(value: &Value) -> Vec<(ArrayKey, &Value)> {
    match value {
        Value::Object(fields) => fields
            .iter()
            .map(|(key, value)| (ArrayKey::from(key.as_str()), value))
            .collect(),
        Value::Array(fields) => (0_i64..)
            .zip(fields)
            .map(|(key, value)| (ArrayKey::Integer(key), value))
            .collect(),
        _ => Vec::new(),
    }
}
fn escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('.', "\\.")
        .replace('*', "\\*")
}
fn typed(value: Value, prefix: &str, files: &BTreeMap<String, UploadedFile>) -> RequestValue {
    if let Some(file) = files.get(prefix) {
        return RequestValue::File(file.clone());
    }
    let child = format!("{prefix}.");
    if !files.keys().any(|key| key.starts_with(&child)) {
        return RequestValue::from_input(value);
    }
    match value {
        Value::Object(fields) => RequestValue::Array(
            fields
                .into_iter()
                .map(|(key, value)| {
                    let path = format!("{prefix}.{}", escape(&key));
                    (ArrayKey::from(key), typed(value, &path, files))
                })
                .collect(),
        ),
        Value::Array(fields) => RequestValue::Array(
            (0_i64..)
                .zip(fields)
                .map(|(key, value)| {
                    (
                        ArrayKey::Integer(key),
                        typed(value, &format!("{prefix}.{key}"), files),
                    )
                })
                .collect(),
        ),
        value => RequestValue::Value(value),
    }
}
fn gather_files(
    value: &RequestValue,
    prefix: &str,
    files: &mut BTreeMap<String, UploadedFile>,
) -> ValidationResult<()> {
    match value {
        RequestValue::File(file) => {
            path::parse(prefix)?;
            files.insert(prefix.to_owned(), file.clone());
        }
        RequestValue::Array(fields) => {
            for (key, value) in fields {
                if contains_file(value) {
                    gather_files(
                        value,
                        &format!("{prefix}.{}", escape(&key.to_string())),
                        files,
                    )?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}
fn contains_file(value: &RequestValue) -> bool {
    match value {
        RequestValue::File(_) => true,
        RequestValue::Array(fields) => fields.values().any(contains_file),
        _ => false,
    }
}
