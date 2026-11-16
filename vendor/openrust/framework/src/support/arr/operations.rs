use std::{borrow::Cow, cmp::Ordering};

use serde_json::{Map, Value};

use super::{Arr, ArrValueError};
use crate::support::data::{self, DataPathError};

type Entry<'a> = (Cow<'a, str>, &'a Value);

fn entries(value: &Value) -> Box<dyn DoubleEndedIterator<Item = Entry<'_>> + '_> {
    match value {
        Value::Array(items) => Box::new(
            items
                .iter()
                .enumerate()
                .map(|(key, item)| (key.to_string().into(), item)),
        ),
        Value::Object(items) => {
            Box::new(items.iter().map(|(key, item)| (key.as_str().into(), item)))
        }
        _ => Box::new(std::iter::empty()),
    }
}

fn pack<'a>(items: impl Iterator<Item = Entry<'a>>) -> Value {
    data::packed(
        items
            .map(|(key, item)| (key.into_owned(), item.clone()))
            .collect(),
    )
}

fn as_key(value: Value) -> Result<String, ArrValueError> {
    match value {
        Value::String(key) => Ok(key),
        Value::Null => Ok(String::new()),
        Value::Bool(key) => Ok(u8::from(key).to_string()),
        Value::Number(key) if key.is_f64() => {
            let key = key.as_f64().expect("float");
            if (-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&key) {
                Ok(format!("{:.0}", key.trunc() + 0.0))
            } else {
                Err(ArrValueError::Expected("an integer key"))
            }
        }
        Value::Number(key) => Ok(key.to_string()),
        _ => Err(ArrValueError::Expected("a scalar key")),
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's Arr API")]
impl Arr {
    pub fn arrayable(value: &Value) -> bool {
        Self::accessible(value)
    }

    /// Converts a serializable Rust value to ordered JSON and wraps a scalar.
    pub fn from(value: impl serde::Serialize) -> serde_json::Result<Value> {
        serde_json::to_value(value).map(Self::wrap)
    }

    pub fn collapse(value: &Value) -> Value {
        data::collapse(entries(value).map(|(_, item)| item.clone()))
    }

    pub fn crossJoin<T: Clone>(arrays: &[&[T]]) -> Vec<Vec<T>> {
        arrays.iter().fold(vec![Vec::new()], |products, array| {
            products
                .into_iter()
                .flat_map(|product| {
                    array.iter().map(move |item| {
                        let mut next = product.clone();
                        next.push(item.clone());
                        next
                    })
                })
                .collect()
        })
    }

    pub fn divide(value: &Value) -> (Vec<String>, Vec<&Value>) {
        entries(value)
            .map(|(key, item)| (key.into_owned(), item))
            .unzip()
    }

    pub fn every(value: &Value, mut predicate: impl FnMut(&Value, &str) -> bool) -> bool {
        entries(value).all(|(key, item)| predicate(item, &key))
    }

    pub fn some(value: &Value, mut predicate: impl FnMut(&Value, &str) -> bool) -> bool {
        entries(value).any(|(key, item)| predicate(item, &key))
    }

    pub fn except(value: &Value, keys: &[&str]) -> Result<Value, DataPathError> {
        let mut result = value.clone();
        for key in keys {
            Self::forget(&mut result, key)?;
        }
        Ok(result)
    }

    pub fn only(value: &Value, keys: &[&str]) -> Value {
        pack(entries(value).filter(|(key, _)| keys.contains(&key.as_ref())))
    }

    pub fn onlyValues(value: &Value, values: &[Value], strict: bool) -> Value {
        Self::r#where(value, |item, _| {
            values
                .iter()
                .any(|candidate| equal(item, candidate, strict))
        })
    }

    pub fn exceptValues(value: &Value, values: &[Value], strict: bool) -> Value {
        Self::reject(value, |item, _| {
            values
                .iter()
                .any(|candidate| equal(item, candidate, strict))
        })
    }

    pub fn first(value: &Value) -> Option<&Value> {
        Self::firstWhere(value, |_, _| true)
    }

    pub fn last(value: &Value) -> Option<&Value> {
        Self::lastWhere(value, |_, _| true)
    }

    pub fn firstWhere(
        value: &Value,
        mut predicate: impl FnMut(&Value, &str) -> bool,
    ) -> Option<&Value> {
        entries(value).find_map(|(key, item)| predicate(item, &key).then_some(item))
    }

    pub fn lastWhere(
        value: &Value,
        mut predicate: impl FnMut(&Value, &str) -> bool,
    ) -> Option<&Value> {
        entries(value)
            .rev()
            .find_map(|(key, item)| predicate(item, &key).then_some(item))
    }

    pub fn sole(value: &Value) -> Result<&Value, ArrValueError> {
        Self::soleWhere(value, |_, _| true)
    }

    pub fn soleWhere(
        value: &Value,
        mut predicate: impl FnMut(&Value, &str) -> bool,
    ) -> Result<&Value, ArrValueError> {
        let mut items =
            entries(value).filter_map(|(key, item)| predicate(item, &key).then_some(item));
        let first = items.next().ok_or(ArrValueError::Missing)?;
        let count = 1 + items.count();
        if count == 1 {
            Ok(first)
        } else {
            Err(ArrValueError::Multiple(count))
        }
    }

    pub fn flatten(value: &Value, depth: Option<usize>) -> Result<Vec<Value>, DataPathError> {
        data::check_depth(value, 0)?;
        let mut output = Vec::new();
        let mut stack: Vec<_> = entries(value)
            .rev()
            .map(|(_, item)| (item, depth.filter(|depth| *depth > 0).unwrap_or(usize::MAX)))
            .collect();
        while let Some((item, depth)) = stack.pop() {
            if depth > 0 && Self::accessible(item) {
                stack.extend(entries(item).rev().map(|(_, child)| (child, depth - 1)));
            } else {
                output.push(item.clone());
            }
        }
        Ok(output)
    }

    pub fn join<T: AsRef<str>>(items: &[T], glue: &str, final_glue: &str) -> String {
        let parts: Vec<_> = items.iter().map(AsRef::as_ref).collect();
        if parts.len() < 2 || final_glue.is_empty() {
            return parts.join(glue);
        }
        let (last, rest) = parts.split_last().expect("at least two items");
        format!("{}{final_glue}{last}", rest.join(glue))
    }

    pub fn keyBy(value: &Value, mut project: impl FnMut(&Value, &str) -> String) -> Value {
        data::packed(
            entries(value)
                .map(|(key, item)| (project(item, &key), item.clone()))
                .collect(),
        )
    }

    pub fn prependKeysWith(value: &Value, prefix: &str) -> Value {
        Self::keyBy(value, |_, key| format!("{prefix}{key}"))
    }

    pub fn map(value: &Value, mut callback: impl FnMut(&Value, &str) -> Value) -> Value {
        data::packed(
            entries(value)
                .map(|(key, item)| {
                    let output = callback(item, &key);
                    (key.into_owned(), output)
                })
                .collect(),
        )
    }

    pub fn mapWithKeys<I: IntoIterator<Item = (String, Value)>>(
        value: &Value,
        mut callback: impl FnMut(&Value, &str) -> I,
    ) -> Value {
        data::packed(
            entries(value)
                .flat_map(|(key, item)| callback(item, &key))
                .collect(),
        )
    }

    /// Passes a list chunk and its outer key as explicit callback arguments.
    pub fn mapSpread(
        value: &Value,
        mut callback: impl FnMut(&[Value], &str) -> Value,
    ) -> Result<Value, ArrValueError> {
        entries(value)
            .map(|(key, item)| {
                let items = item
                    .as_array()
                    .ok_or(ArrValueError::Expected("a list chunk"))?;
                let result = callback(items, &key);
                Ok((key.into_owned(), result))
            })
            .collect::<Result<Map<_, _>, _>>()
            .map(data::packed)
    }

    pub fn select(value: &Value, keys: &[&str]) -> Value {
        Self::map(value, |item, _| {
            data::packed(
                keys.iter()
                    .filter_map(|&key| {
                        data::direct(item, key).map(|item| (key.to_owned(), item.clone()))
                    })
                    .collect(),
            )
        })
    }

    pub fn pluck(
        value: &Value,
        path: &str,
        key_path: Option<&str>,
    ) -> Result<Value, ArrValueError> {
        let project = |item| data::data_get(item, Some(path)).unwrap_or(Value::Null);
        match key_path {
            None => Ok(Value::Array(
                entries(value).map(|(_, item)| project(item)).collect(),
            )),
            Some(key_path) => entries(value)
                .map(|(_, item)| {
                    let key = as_key(data::data_get(item, Some(key_path)).unwrap_or(Value::Null))?;
                    Ok((key, project(item)))
                })
                .collect::<Result<Map<_, _>, _>>()
                .map(data::packed),
        }
    }

    pub fn prepend(value: &Value, item: Value, key: Option<&str>) -> Value {
        let mut result = Map::new();
        result.insert(key.unwrap_or("0").to_owned(), item);
        let mut next = 1;
        for (old_key, value) in entries(value) {
            let old_key = if key.is_none() && data::integer_key(&old_key) {
                let key = next.to_string();
                next += 1;
                key
            } else {
                old_key.into_owned()
            };
            result.entry(old_key).or_insert_with(|| value.clone());
        }
        data::packed(result)
    }

    pub fn push(
        value: &mut Value,
        key: Option<&str>,
        items: impl IntoIterator<Item = Value>,
    ) -> Result<(), ArrValueError> {
        let mut target = key
            .map_or(Some(&*value), |key| Self::get(value, key))
            .map_or_else(
                || Ok(Value::Array(Vec::new())),
                |value| {
                    Self::accessible(value)
                        .then(|| value.clone())
                        .ok_or(ArrValueError::Expected("an array or object"))
                },
            )?;
        match &mut target {
            Value::Array(target) => target.extend(items),
            Value::Object(target) => {
                let mut max = target
                    .keys()
                    .filter(|key| data::integer_key(key))
                    .filter_map(|key| key.parse::<i64>().ok())
                    .max()
                    .unwrap_or(-1);
                for item in items {
                    max = max.checked_add(1).ok_or(ArrValueError::KeyOverflow)?;
                    target.insert(max.to_string(), item);
                }
            }
            _ => unreachable!("validated array or object"),
        }
        data::check_depth(&target, 0)?;
        if let Some(key) = key {
            Self::set(value, key, target)?;
        } else {
            *value = target;
        }
        Ok(())
    }

    pub fn randItem(value: &Value) -> Result<&Value, ArrValueError> {
        let items: Vec<_> = entries(value).map(|(_, item)| item).collect();
        if items.is_empty() {
            return Err(ArrValueError::Missing);
        }
        Ok(items[random_index(items.len())?])
    }

    /// Samples without replacement. Key-preserving samples retain source order.
    pub fn random(
        value: &Value,
        count: usize,
        preserve_keys: bool,
    ) -> Result<Value, ArrValueError> {
        let items: Vec<_> = entries(value).collect();
        if count > items.len() {
            return Err(ArrValueError::SampleSize);
        }
        let mut indices: Vec<_> = (0..items.len()).collect();
        for index in 0..count {
            indices.swap(index, index + random_index(items.len() - index)?);
        }
        indices.truncate(count);
        indices.sort_unstable();
        if preserve_keys {
            Ok(pack(indices.into_iter().map(|index| items[index].clone())))
        } else {
            Ok(Value::Array(
                indices
                    .into_iter()
                    .map(|index| items[index].1.clone())
                    .collect(),
            ))
        }
    }

    pub fn shuffle(value: &Value) -> Result<Vec<Value>, ArrValueError> {
        let mut items: Vec<_> = entries(value).map(|(_, item)| item.clone()).collect();
        for index in (1..items.len()).rev() {
            items.swap(index, random_index(index + 1)?);
        }
        Ok(items)
    }

    pub fn sortBy<K: Ord>(
        value: &Value,
        mut project: impl FnMut(&Value, &str) -> K,
        descending: bool,
    ) -> Value {
        let mut items: Vec<_> = entries(value).collect();
        items.sort_by_cached_key(|(key, item)| {
            let key = project(item, key);
            OrderedKey { key, descending }
        });
        pack(items.into_iter())
    }

    pub fn sort(value: &Value) -> Value {
        Self::sortValues(value, false)
    }

    pub fn sortDesc(value: &Value) -> Value {
        Self::sortValues(value, true)
    }

    fn sortValues(value: &Value, descending: bool) -> Value {
        let mut items: Vec<_> = entries(value).collect();
        items.sort_by(|(_, left), (_, right)| order(left, right, descending));
        pack(items.into_iter())
    }

    pub fn sortRecursive(value: &Value, descending: bool) -> Result<Value, DataPathError> {
        data::check_depth(value, 0)?;
        Ok(sort_recursive(value, descending))
    }

    pub fn sortRecursiveDesc(value: &Value) -> Result<Value, DataPathError> {
        Self::sortRecursive(value, true)
    }

    pub fn take(value: &Value, limit: isize) -> Value {
        let count = limit.unsigned_abs();
        let items: Vec<_> = entries(value).collect();
        let selected = if limit >= 0 {
            &items[..count.min(items.len())]
        } else {
            &items[items.len().saturating_sub(count)..]
        };
        let mut index = 0;
        data::packed(
            selected
                .iter()
                .map(|(key, item)| {
                    let key = if data::integer_key(key) {
                        let key = index.to_string();
                        index += 1;
                        key
                    } else {
                        key.to_string()
                    };
                    (key, (*item).clone())
                })
                .collect(),
        )
    }

    pub fn r#where(value: &Value, mut predicate: impl FnMut(&Value, &str) -> bool) -> Value {
        pack(entries(value).filter(|(key, item)| predicate(item, key)))
    }

    pub fn reject(value: &Value, mut predicate: impl FnMut(&Value, &str) -> bool) -> Value {
        Self::r#where(value, |item, key| !predicate(item, key))
    }

    pub fn whereNotNull(value: &Value) -> Value {
        Self::reject(value, |item, _| item.is_null())
    }

    pub fn partition(
        value: &Value,
        mut predicate: impl FnMut(&Value, &str) -> bool,
    ) -> (Value, Value) {
        let (pass, fail): (Vec<_>, Vec<_>) =
            entries(value).partition(|(key, item)| predicate(item, key));
        (pack(pass.into_iter()), pack(fail.into_iter()))
    }

    pub fn toCssClasses(value: &Value) -> String {
        css(value, false)
    }

    pub fn toCssStyles(value: &Value) -> String {
        css(value, true)
    }
}

#[derive(Eq, PartialEq)]
struct OrderedKey<K> {
    key: K,
    descending: bool,
}

impl<K: Ord> Ord for OrderedKey<K> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.descending.cmp(&other.descending).then_with(|| {
            if self.descending {
                other.key.cmp(&self.key)
            } else {
                self.key.cmp(&other.key)
            }
        })
    }
}
impl<K: Ord> PartialOrd for OrderedKey<K> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub(in crate::support) fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_f64() != Some(0.0),
        Value::String(value) => !matches!(value.as_str(), "" | "0"),
        Value::Array(value) => !value.is_empty(),
        Value::Object(value) => !value.is_empty(),
    }
}

fn equal(left: &Value, right: &Value, strict: bool) -> bool {
    if strict || left == right {
        return left == right;
    }
    if left.is_boolean() || right.is_boolean() {
        return truthy(left) == truthy(right);
    }
    if left.is_null() || right.is_null() {
        let other = if left.is_null() { right } else { left };
        return other.as_str().map_or_else(|| !truthy(other), str::is_empty);
    }
    match (left, right) {
        (Value::Array(left), Value::Array(right)) => {
            left.len() == right.len() && left.iter().zip(right).all(|(a, b)| equal(a, b, false))
        }
        (Value::Object(left), Value::Object(right)) => {
            left.len() == right.len()
                && left.iter().all(|(key, value)| {
                    right
                        .get(key)
                        .is_some_and(|other| equal(value, other, false))
                })
        }
        _ => match (numeric(left), numeric(right)) {
            (Some(left), Some(right)) => left == right,
            _ => false,
        },
    }
}

fn numeric(value: &Value) -> Option<f64> {
    value.as_f64().or_else(|| {
        value
            .as_str()?
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
    })
}

fn order(left: &Value, right: &Value, descending: bool) -> Ordering {
    // A total order keeps mixed JSON input well-defined. Numeric strings are
    // sorted with numbers and equal keys retain input order.
    let rank = |value: &Value| match value {
        Value::Null => 0,
        Value::Bool(_) => 1,
        Value::Number(_) => 2,
        Value::String(_) if numeric(value).is_some() => 2,
        Value::String(_) => 3,
        Value::Array(_) => 4,
        Value::Object(_) => 5,
    };
    let result = rank(left)
        .cmp(&rank(right))
        .then_with(|| match (numeric(left), numeric(right)) {
            (Some(left_number), Some(right_number)) => match (left.as_i64(), right.as_i64()) {
                (Some(left), Some(right)) => left.cmp(&right),
                _ => left_number.total_cmp(&right_number),
            },
            _ => match (left.as_str(), right.as_str()) {
                (Some(left), Some(right)) => left.cmp(right),
                _ => left.to_string().cmp(&right.to_string()),
            },
        });
    if descending { result.reverse() } else { result }
}

fn sort_recursive(value: &Value, descending: bool) -> Value {
    let mut items: Vec<_> = entries(value)
        .map(|(key, item)| (key, sort_recursive(item, descending)))
        .collect();
    if !Arr::accessible(value) {
        return value.clone();
    }
    if Arr::isList(value) {
        items.sort_by(|(_, left), (_, right)| order(left, right, descending));
        Value::Array(items.into_iter().map(|(_, item)| item).collect())
    } else {
        items.sort_by(|(left, _), (right, _)| {
            let integer = |key: &str| {
                key.parse::<i64>()
                    .ok()
                    .filter(|number| number.to_string() == key)
            };
            let ordering = match (integer(left), integer(right)) {
                (Some(left), Some(right)) => left.cmp(&right),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => left.cmp(right),
            };
            if descending {
                ordering.reverse()
            } else {
                ordering
            }
        });
        data::packed(
            items
                .into_iter()
                .map(|(key, item)| (key.into_owned(), item))
                .collect(),
        )
    }
}

fn css(value: &Value, style: bool) -> String {
    let render = |text: String| {
        if style {
            crate::support::Str::finish(&text, ";")
        } else {
            text
        }
    };
    let mut output = Vec::new();
    let wrapped = Arr::wrap(value.clone());
    for (key, item) in entries(&wrapped) {
        if key.trim().parse::<f64>().is_ok_and(f64::is_finite) {
            let text = match item {
                Value::String(text) => text.clone(),
                Value::Null | Value::Bool(false) => String::new(),
                Value::Bool(true) => "1".into(),
                item => item.to_string(),
            };
            output.push(render(text));
        } else if truthy(item) {
            output.push(render(key.into_owned()));
        }
    }
    output.join(" ")
}

pub(in crate::support) fn random_index(length: usize) -> Result<usize, ArrValueError> {
    let threshold = length.wrapping_neg() % length;
    loop {
        let mut bytes = [0; size_of::<usize>()];
        aws_lc_rs::rand::fill(&mut bytes).map_err(|_| ArrValueError::RandomUnavailable)?;
        let sample = usize::from_ne_bytes(bytes);
        if sample >= threshold {
            return Ok(sample % length);
        }
    }
}
