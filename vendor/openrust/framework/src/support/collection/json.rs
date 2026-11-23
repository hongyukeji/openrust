use serde_json::{Map, Value};

use super::{Collection, CollectionError, CollectionResult};
use crate::support::{Arr, data, data_get, data_has};

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl Collection<Value> {
    pub fn select(self, keys: &[&str]) -> CollectionResult<Self> {
        Ok(Self::fromValue(Arr::select(&self.toArray()?, keys)))
    }

    pub fn value(&self, path: &str) -> Option<Value> {
        self.items
            .values()
            .find(|item| data_has(item, path))
            .and_then(|item| data_get(item, Some(path)))
    }

    pub fn whereNull(self) -> Self {
        self.filter(|value, _| value.is_null())
    }

    pub fn whereNotNull(self) -> Self {
        self.reject(|value, _| value.is_null())
    }

    pub fn whereNullAt(self, path: &str) -> Self {
        self.filter(|value, _| data_get(value, Some(path)).is_none_or(|value| value.is_null()))
    }

    pub fn whereNotNullAt(self, path: &str) -> Self {
        self.reject(|value, _| data_get(value, Some(path)).is_none_or(|value| value.is_null()))
    }

    pub fn dot(self) -> CollectionResult<Self> {
        self.dotWithDepth(None)
    }

    pub fn dotWithDepth(self, depth: Option<usize>) -> CollectionResult<Self> {
        Ok(Self::fromEntries(Arr::dotWithDepth(
            &self.toArray()?,
            "",
            depth,
        )?))
    }

    pub fn undot(self) -> CollectionResult<Self> {
        let items: Map<_, _> = self
            .into_iter()
            .map(|(key, item)| (key.to_string(), item))
            .collect();
        Ok(Self::fromValue(Arr::undot(&items)?))
    }

    pub fn flatten(self, depth: Option<usize>) -> CollectionResult<Self> {
        Ok(Self::make(Arr::flatten(&self.toArray()?, depth).map_err(
            |error| CollectionError::Source(Box::new(error)),
        )?))
    }

    pub fn mergeRecursive(self, right: Self) -> CollectionResult<Self> {
        let left = self.toArray()?;
        let right = right.toArray()?;
        validate(&left)?;
        validate(&right)?;
        let result = merge(left, right, false);
        validate(&result)?;
        Ok(Self::fromValue(result))
    }

    pub fn replaceRecursive(self, right: Self) -> CollectionResult<Self> {
        let left = self.toArray()?;
        let right = right.toArray()?;
        validate(&left)?;
        validate(&right)?;
        Ok(Self::fromValue(merge(left, right, true)))
    }
}

fn validate(value: &Value) -> CollectionResult<()> {
    data::check_depth(value, 0).map_err(Into::into)
}

fn merge(left: Value, right: Value, replace: bool) -> Value {
    let mut left = Collection::fromValue(if replace {
        left
    } else {
        as_merge_operand(left)
    });
    let right = Collection::fromValue(if replace {
        right
    } else {
        as_merge_operand(right)
    });
    if !replace {
        left = left.merge(Collection::new());
    }
    for (key, item) in right {
        if !replace && matches!(key, super::ArrayKey::Integer(_)) {
            left.push(item).expect("renumbered values fit integer keys");
        } else if let Some(existing) = left.getMut(&key) {
            let nested = existing.is_object() || existing.is_array();
            if !replace || (nested && (item.is_array() || item.is_object())) {
                *existing = merge(std::mem::take(existing), item, replace);
            } else {
                *existing = item;
            }
        } else {
            left.put(key, item);
        }
    }
    let values: Map<_, _> = left
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect();
    data::packed(values)
}

// In a colliding string slot, null is a scalar operand just like any other.
fn as_merge_operand(value: Value) -> Value {
    if value.is_null() {
        Value::Array(vec![value])
    } else {
        value
    }
}
