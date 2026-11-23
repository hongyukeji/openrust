use serde_json::{Map, Value};

use super::{Collection, CollectionError, CollectionResult};
use crate::support::{Arr, data, data_get, data_has};

impl Collection<Value> {
    pub fn select(self, keys: &[&str]) -> CollectionResult<Self> {
        Ok(Self::from_value(Arr::select(&self.to_array()?, keys)))
    }

    pub fn value(&self, path: &str) -> Option<Value> {
        self.items
            .values()
            .find(|item| data_has(item, path))
            .and_then(|item| data_get(item, Some(path)))
    }

    pub fn where_null(self) -> Self {
        self.filter(|value, _| value.is_null())
    }

    pub fn where_not_null(self) -> Self {
        self.reject(|value, _| value.is_null())
    }

    pub fn where_null_at(self, path: &str) -> Self {
        self.filter(|value, _| data_get(value, Some(path)).is_none_or(|value| value.is_null()))
    }

    pub fn where_not_null_at(self, path: &str) -> Self {
        self.reject(|value, _| data_get(value, Some(path)).is_none_or(|value| value.is_null()))
    }

    pub fn dot(self) -> CollectionResult<Self> {
        self.dot_with_depth(None)
    }

    pub fn dot_with_depth(self, depth: Option<usize>) -> CollectionResult<Self> {
        Ok(Self::from_entries(Arr::dot_with_depth(
            &self.to_array()?,
            "",
            depth,
        )?))
    }

    pub fn undot(self) -> CollectionResult<Self> {
        let items: Map<_, _> = self
            .into_iter()
            .map(|(key, item)| (key.to_string(), item))
            .collect();
        Ok(Self::from_value(Arr::undot(&items)?))
    }

    pub fn flatten(self, depth: Option<usize>) -> CollectionResult<Self> {
        Ok(Self::make(Arr::flatten(&self.to_array()?, depth).map_err(
            |error| CollectionError::Source(Box::new(error)),
        )?))
    }

    pub fn merge_recursive(self, right: Self) -> CollectionResult<Self> {
        let left = self.to_array()?;
        let right = right.to_array()?;
        validate(&left)?;
        validate(&right)?;
        let result = merge(left, right, false);
        validate(&result)?;
        Ok(Self::from_value(result))
    }

    pub fn replace_recursive(self, right: Self) -> CollectionResult<Self> {
        let left = self.to_array()?;
        let right = right.to_array()?;
        validate(&left)?;
        validate(&right)?;
        Ok(Self::from_value(merge(left, right, true)))
    }
}

fn validate(value: &Value) -> CollectionResult<()> {
    data::check_depth(value, 0).map_err(Into::into)
}

fn merge(left: Value, right: Value, replace: bool) -> Value {
    let mut left = Collection::from_value(if replace {
        left
    } else {
        as_merge_operand(left)
    });
    let right = Collection::from_value(if replace {
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
        } else if let Some(existing) = left.get_mut(&key) {
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
