//! PHP 数组键语义的有序集合；过滤不会隐式重排索引。

use crate::Result;
use serde::{
    Serialize, Serializer,
    ser::{SerializeMap, SerializeSeq},
};
use serde_json::Value;
use std::fmt;

/// PHP 数组的整数键或字符串键。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ArrayKey {
    /// 有符号整数索引，允许负数与稀疏索引。
    Integer(i64),
    /// 非标准十进制整数形式的字符串键。
    String(String),
}
impl From<String> for ArrayKey {
    fn from(key: String) -> Self {
        match key.parse::<i64>() {
            Ok(integer) if integer.to_string() == key => Self::Integer(integer),
            _ => Self::String(key),
        }
    }
}
impl From<&str> for ArrayKey {
    fn from(key: &str) -> Self {
        Self::from(key.to_owned())
    }
}
impl From<i64> for ArrayKey {
    fn from(key: i64) -> Self {
        Self::Integer(key)
    }
}
impl fmt::Display for ArrayKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Integer(key) => key.fmt(f),
            Self::String(key) => key.fmt(f),
        }
    }
}
impl ArrayKey {
    fn normalized(self) -> Self {
        match self {
            Self::String(key) => Self::from(key),
            key @ Self::Integer(_) => key,
        }
    }
}

/// 拥有所有权的有序集合；空集合也仍是一个对象，而非 PHP 假值数组。
///
/// Rust 中 `all` 借用键值条目；`to_value` 按 PHP 顺序数组或关联数组形状输出。
#[derive(Clone, PartialEq, Eq)]
pub struct Collection<T = Value> {
    entries: Vec<(ArrayKey, T)>,
}
impl<T> fmt::Debug for Collection<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Collection").finish_non_exhaustive()
    }
}
impl<T> Default for Collection<T> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
}
impl<T> Collection<T> {
    /// 借用全部键值条目，保持迭代顺序与稀疏键。
    pub fn all(&self) -> &[(ArrayKey, T)] {
        &self.entries
    }
    /// 按直接键读取；点号没有路径含义。
    pub fn get(&self, key: impl Into<ArrayKey>) -> Option<&T> {
        let key = key.into().normalized();
        self.entries
            .iter()
            .find_map(|(candidate, value)| (candidate == &key).then_some(value))
    }
    /// 按直接键可变借用；点号没有路径含义。
    pub(crate) fn get_mut(&mut self, key: impl Into<ArrayKey>) -> Option<&mut T> {
        let key = key.into().normalized();
        self.entries
            .iter_mut()
            .find_map(|(candidate, value)| (candidate == &key).then_some(value))
    }
    /// 借用按原顺序排列的键值迭代器。
    pub fn iter(&self) -> std::slice::Iter<'_, (ArrayKey, T)> {
        self.entries.iter()
    }
    /// 判断是否没有条目。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    /// 判断是否至少有一个条目。
    pub fn is_not_empty(&self) -> bool {
        !self.is_empty()
    }
}
impl<T: Serialize> Collection<T> {
    /// 原位替换或末尾插入一个键；预算失败时保持原集合不变。
    pub fn try_put(&mut self, key: impl Into<ArrayKey>, value: T) -> Result<()> {
        let key = key.into().normalized();
        let replacing = self
            .entries
            .iter()
            .position(|(candidate, _)| candidate == &key);
        let mut candidate =
            Vec::with_capacity(self.entries.len() + usize::from(replacing.is_none()));
        for (index, (existing_key, existing_value)) in self.entries.iter().enumerate() {
            candidate.push((
                existing_key.clone(),
                if Some(index) == replacing {
                    &value
                } else {
                    existing_value
                },
            ));
        }
        if replacing.is_none() {
            candidate.push((key.clone(), &value));
        }
        let _ = Collection::try_from_entries(candidate)?;
        if let Some(index) = replacing {
            self.entries[index].1 = value;
        } else {
            self.entries.push((key, value));
        }
        Ok(())
    }

    /// 按 PHP 数组的下一整数键追加；预算失败保持原集合不变。
    pub fn try_push(&mut self, value: T) -> Result<()> {
        let next = self
            .entries
            .iter()
            .filter_map(|(key, _)| match key {
                ArrayKey::Integer(index) => Some(*index),
                ArrayKey::String(_) => None,
            })
            .max()
            .map_or(Some(0), |index| index.checked_add(1));
        let index = next.ok_or_else(super::data::limit_error)?;
        self.try_put(ArrayKey::Integer(index), value)
    }

    /// 删除直接键，不重排其余条目。
    pub fn forget(&mut self, key: impl Into<ArrayKey>) -> Option<T> {
        let key = key.into().normalized();
        self.entries
            .iter()
            .position(|(candidate, _)| candidate == &key)
            .map(|index| self.entries.remove(index).1)
    }

    /// 从有序条目构造；重复键后值覆盖前值，错误不产生部分结果。
    ///
    /// 应用 Serialize 执行一次后校验展开树预算；不保证任意应用序列化器的分配预先受限。
    /// 非有限浮点在序列化过程中拒绝，不会静默变为 null。
    pub fn try_from_entries(entries: impl IntoIterator<Item = (ArrayKey, T)>) -> Result<Self> {
        let mut result = Self::default();
        for (key, value) in entries {
            let key = key.normalized();
            if let Some(index) = result
                .entries
                .iter()
                .position(|(candidate, _)| candidate == &key)
            {
                result.entries[index].1 = value;
            } else {
                result.entries.push((key, value));
            }
            if result.entries.len() > 1024 {
                return Err(super::data::limit_error());
            }
        }
        let _ = result.to_value()?;
        Ok(result)
    }
    /// 按 PHP 数组形状输出，并在一次应用序列化后校验展开树预算。
    ///
    /// 连续的 0 起始数字键输出 JSON 数组，其余输出对象；自定义 Serialize
    /// 后续改变输出时仍重新校验。直接 Serialize 仅拦截非有限数，不承诺此预算。
    pub fn to_value(&self) -> Result<Value> {
        let value = serde_json::to_value(self).map_err(crate::Error::internal)?;
        super::data::validate_limits(&value)?;
        Ok(value)
    }
    /// 映射值并保留原键，随后校验新结果的资源预算。
    pub fn map<U: Serialize>(
        self,
        mut callback: impl FnMut(&T, &ArrayKey) -> U,
    ) -> Result<Collection<U>> {
        Collection::try_from_entries(self.entries.into_iter().map(|(key, value)| {
            let mapped = callback(&value, &key);
            (key, mapped)
        }))
    }
    /// 过滤值但保留原键，只有 `values` 会重新编号。
    pub fn filter(self, mut callback: impl FnMut(&T, &ArrayKey) -> bool) -> Result<Self> {
        Self::try_from_entries(
            self.entries
                .into_iter()
                .filter(|(key, value)| callback(value, key)),
        )
    }
    /// 丢弃原键，明确按 0 起始连续数字键重新编号。
    pub fn values(self) -> Result<Self> {
        Self::try_from_entries(
            self.entries
                .into_iter()
                .zip(0_i64..)
                .map(|((_, value), index)| (ArrayKey::Integer(index), value)),
        )
    }
}
impl Collection<Value> {
    /// 按 PHP 数组转换构造；null 为空，其他标量包装为一个元素。
    pub fn from_value(value: Value) -> Result<Self> {
        super::data::validate_limits(&value)?;
        let entries = match super::data::cast_json(value) {
            Value::Array(values) => values
                .into_iter()
                .zip(0_i64..)
                .map(|(value, index)| (ArrayKey::Integer(index), value))
                .collect(),
            Value::Object(values) => values
                .into_iter()
                .map(|(key, value)| (ArrayKey::from(key), value))
                .collect(),
            _ => Vec::new(),
        };
        Self::try_from_entries(entries)
    }
}
impl<T: Serialize> Serialize for Collection<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let sequential = self
            .entries
            .iter()
            .zip(0_i64..)
            .all(|((key, _), index)| *key == ArrayKey::Integer(index));
        if sequential {
            let mut output = serializer.serialize_seq(Some(self.entries.len()))?;
            for (_, value) in &self.entries {
                output.serialize_element(&super::finite::Finite(value))?;
            }
            output.end()
        } else {
            let mut output = serializer.serialize_map(Some(self.entries.len()))?;
            for (key, value) in &self.entries {
                output.serialize_entry(&key.to_string(), &super::finite::Finite(value))?;
            }
            output.end()
        }
    }
}
impl<T> IntoIterator for Collection<T> {
    type Item = (ArrayKey, T);
    type IntoIter = std::vec::IntoIter<Self::Item>;
    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter()
    }
}
impl<'a, T> IntoIterator for &'a Collection<T> {
    type Item = &'a (ArrayKey, T);
    type IntoIter = std::slice::Iter<'a, (ArrayKey, T)>;
    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter()
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "集合转换测试的固定 fixture 构造失败直接中止"
)]
mod tests {
    use super::*;
    use std::{cell::Cell, rc::Rc};
    #[test]
    fn nonfinite_values_fail_in_constructor_and_map_without_changing_finite_null() {
        use crate::support::InputNumber;
        for value in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
            let error = Collection::try_from_entries([(0.into(), InputNumber::Float(value))])
                .expect_err("finite boundary");
            assert_eq!(error.code(), "internal_error");
            let source = Collection::from_value(serde_json::json!([1])).expect("source");
            assert!(source.map(|_, _| InputNumber::Float(value)).is_err());
        }
        let source = Collection::from_value(serde_json::json!([null, 1.5])).expect("source");
        assert_eq!(
            source.to_value().expect("finite and null"),
            serde_json::json!([null, 1.5])
        );
    }
    struct Growing {
        calls: Rc<Cell<usize>>,
        len: Rc<Cell<usize>>,
    }
    impl Serialize for Growing {
        fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
            self.calls.set(self.calls.get() + 1);
            let mut seq = serializer.serialize_seq(Some(self.len.get()))?;
            for _ in 0..self.len.get() {
                seq.serialize_element(&0)?;
            }
            seq.end()
        }
    }
    #[test]
    fn changing_serialize_revalidates_output_once_and_does_not_publish_partial_value() {
        let calls = Rc::new(Cell::new(0));
        let len = Rc::new(Cell::new(1));
        let source = Collection::try_from_entries([(
            0.into(),
            Growing {
                calls: calls.clone(),
                len: len.clone(),
            },
        )])
        .expect("source");
        assert_eq!(calls.get(), 1);
        let previous = source.to_value().expect("initial output");
        assert_eq!(calls.get(), 2);
        len.set(1025);
        let error = source
            .to_value()
            .expect_err("changed output exceeds same data budget");
        assert_eq!(calls.get(), 3);
        assert_eq!(error.code(), "input_limits_exceeded");
        assert_eq!(error.status(), http::StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(previous, serde_json::json!([[0]]));
        assert_eq!(source.all().len(), 1);
        len.set(1);
        assert_eq!(source.to_value().expect("source unchanged"), previous);
        assert_eq!(calls.get(), 4);
    }

    #[derive(Serialize)]
    enum Nested {
        Entry { values: Vec<Option<f64>> },
    }
    #[test]
    fn nonfinite_rejection_reaches_nested_enum_struct_sequence_and_option() {
        let input = Nested::Entry {
            values: vec![None, Some(f64::INFINITY)],
        };
        assert!(Collection::try_from_entries([("nested".into(), input)]).is_err());
    }
    struct Changing {
        calls: Rc<Cell<usize>>,
        value: Rc<Cell<f64>>,
    }
    impl Serialize for Changing {
        fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
            self.calls.set(self.calls.get() + 1);
            serializer.serialize_f64(self.value.get())
        }
    }
    #[test]
    fn custom_serialize_is_called_once_per_operation_and_later_nonfinite_is_rejected() {
        let calls = Rc::new(Cell::new(0));
        let value = Rc::new(Cell::new(1.5));
        let source = Collection::try_from_entries([(
            0.into(),
            Changing {
                calls: calls.clone(),
                value: value.clone(),
            },
        )])
        .expect("source");
        assert_eq!(calls.get(), 1);
        assert_eq!(source.to_value().expect("finite"), serde_json::json!([1.5]));
        assert_eq!(calls.get(), 2);
        value.set(f64::INFINITY);
        assert!(source.to_value().is_err());
        assert_eq!(calls.get(), 3);
        assert!(serde_json::to_string(&source).is_err());
        assert_eq!(calls.get(), 4);
    }
}
