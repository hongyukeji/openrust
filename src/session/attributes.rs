//! Laravel Session 的唯一属性树与 flash 状态转换。
//!
//! 固定依据：framework d52c3b3129e4c8e213c97cdc68f94b0b7d27f181 的
//! `Session/Store.php`（233–240、280–348、393–403、471–545）及
//! `Collections/Arr.php`（426–459、497–524、752–755）。
//! 路径写入、删除与资源限制复用既有数组引擎；Session 读取没有通配符展开。

/*
Laravel 派生行为的许可声明：
The MIT License (MIT)

Copyright (c) Taylor Otwell

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
*/

use crate::{
    Error, Result,
    support::{data, scalars},
};
use http::StatusCode;
use serde_json::{Map, Value};
use std::collections::HashSet;

const MAX_KEYS: usize = 1024;
const MAX_DEPTH: usize = 64;
const MAX_KEY_BYTES: usize = 20 * 1024 * 1024;

/// 当前请求共享的 Session 属性；快照只由同一棵值树产生。
#[derive(Clone)]
pub(crate) struct Attributes(Value);

impl std::fmt::Debug for Attributes {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Attributes").finish_non_exhaustive()
    }
}

impl Attributes {
    /// 从数组属性构造；根空数组统一表示为 `[]`。
    pub(crate) fn new(value: Value) -> Result<Self> {
        if !value.is_array() && !value.is_object() {
            return Err(invalid());
        }
        validate(&value)?;
        Ok(Self(empty_root(value)))
    }

    /// 返回完整属性快照，不输出内部共享引用。
    pub(crate) fn all(&self) -> Value {
        self.0.clone()
    }

    /// 完整字面键优先，其次 dot；显式 null 保留为 `Some(Null)`。
    pub(crate) fn get(&self, key: &str) -> Result<Option<Value>> {
        check_lookup_keys(&self.0, &[key])?;
        Ok(read(&self.0, key).cloned())
    }

    /// 所有键均存在；null 也存在，空列表为 true。
    pub(crate) fn exists(&self, keys: &[&str]) -> Result<bool> {
        check_lookup_keys(&self.0, keys)?;
        Ok(keys.iter().all(|key| read(&self.0, key).is_some()))
    }

    /// 所有键均存在且非 null；false、0、空串保留为存在。
    pub(crate) fn has(&self, keys: &[&str]) -> Result<bool> {
        check_lookup_keys(&self.0, keys)?;
        Ok(keys
            .iter()
            .all(|key| read(&self.0, key).is_some_and(|value| !value.is_null())))
    }

    /// 至少一个键存在且非 null；空列表为 false。
    pub(crate) fn has_any(&self, keys: &[&str]) -> Result<bool> {
        check_lookup_keys(&self.0, keys)?;
        Ok(keys
            .iter()
            .any(|key| read(&self.0, key).is_some_and(|value| !value.is_null())))
    }

    /// 设置 dot 路径；失败不改变原属性。
    pub(crate) fn put(&mut self, key: &str, value: Value) -> Result<()> {
        self.change(|candidate| set(candidate, key, value))
    }

    /// 按给定顺序设置多项；其中任意项失败时全部回滚。
    pub(crate) fn put_many(&mut self, values: &[(String, Value)]) -> Result<()> {
        if values.len() > MAX_KEYS {
            return Err(limits());
        }
        let keys = values
            .iter()
            .map(|(key, _)| key.as_str())
            .collect::<Vec<_>>();
        check_keys(&keys)?;
        self.change(|candidate| {
            for (key, value) in values {
                set(candidate, key, value.clone())?;
            }
            Ok(())
        })
    }

    /// 返回原值并删除；null 与缺失不同。
    pub(crate) fn pull(&mut self, key: &str) -> Result<Option<Value>> {
        check_keys(&[key])?;
        self.change(|candidate| {
            let value = read(candidate, key).cloned();
            forget(candidate, key)?;
            Ok(value)
        })
    }

    /// 删除完整字面键或 dot 路径；缺失键不报错。
    pub(crate) fn forget(&mut self, keys: &[&str]) -> Result<()> {
        check_keys(keys)?;
        self.change(|candidate| {
            for key in keys {
                forget(candidate, key)?;
            }
            Ok(())
        })
    }

    /// 清空全部属性，包含 token 和 flash 元数据。
    pub(crate) fn flush(&mut self) -> Result<()> {
        self.change(|candidate| {
            *candidate = Value::Array(Vec::new());
            Ok(())
        })
    }

    /// 立即可读且延续到下一次请求的值；保留 new 列表的重复项。
    pub(crate) fn flash(&mut self, key: &str, value: Value) -> Result<()> {
        check_keys(&[key])?;
        self.change(|candidate| {
            set(candidate, key, value)?;
            push(candidate, "_flash.new", Value::String(key.to_owned()))?;
            remove_old(candidate, &[Value::String(key.to_owned())])
        })
    }

    /// 仅在当前请求可读；下一次 age 删除。
    pub(crate) fn now(&mut self, key: &str, value: Value) -> Result<()> {
        check_keys(&[key])?;
        self.change(|candidate| {
            set(candidate, key, value)?;
            push(candidate, "_flash.old", Value::String(key.to_owned()))
        })
    }

    /// 将所有 old 合并到 new，并清空 old；包括由 now 标记的键。
    pub(crate) fn reflash(&mut self) -> Result<()> {
        self.change(|candidate| {
            let old = array(read_or_empty(candidate, "_flash.old"))?;
            merge_new(candidate, old)?;
            set(candidate, "_flash.old", Value::Array(Vec::new()))
        })
    }

    /// 保留指定键；空列表不等于 reflash。
    pub(crate) fn keep(&mut self, keys: &[&str]) -> Result<()> {
        check_keys(keys)?;
        self.change(|candidate| {
            let keys = keys
                .iter()
                .map(|key| Value::String((*key).to_owned()))
                .collect::<Vec<_>>();
            merge_new(candidate, array(Value::Array(keys.clone()))?)?;
            remove_old(candidate, &keys)
        })
    }

    /// 推进一次 flash 年龄；每次显式 save 都应调用一次，不做幂等去重。
    pub(crate) fn age(&mut self) -> Result<()> {
        self.change(|candidate| {
            let old = read_or_empty(candidate, "_flash.old");
            // Store::forget 先执行 PHP (array) cast，不要求整项元数据是字符串列表。
            for key in cast_values(old) {
                forget_php_key(candidate, &key)?;
            }
            let new = read_or_empty(candidate, "_flash.new");
            set(candidate, "_flash.old", new)?;
            set(candidate, "_flash.new", Value::Array(Vec::new()))
        })
    }

    /// 按顶层字面键选择，并保持原属性顺序；不解释 dot。
    pub(crate) fn only(&self, keys: &[&str]) -> Result<Value> {
        check_keys(keys)?;
        let mut result = Map::new();
        for (key, value) in entries(self.0.clone()) {
            if keys.contains(&key.as_str()) {
                result.insert(key, value);
            }
        }
        let value = packed(result);
        validate(&value)?;
        Ok(value)
    }

    /// 从独立快照删除字面键或 dot 路径，不修改当前属性。
    pub(crate) fn except(&self, keys: &[&str]) -> Result<Value> {
        check_keys(keys)?;
        let mut result = self.0.clone();
        for key in keys {
            forget(&mut result, key)?;
        }
        validate(&result)?;
        Ok(empty_root(result))
    }

    fn change<T>(&mut self, mutation: impl FnOnce(&mut Value) -> Result<T>) -> Result<T> {
        let mut candidate = self.0.clone();
        let result = mutation(&mut candidate)?;
        validate(&candidate)?;
        self.0 = empty_root(candidate);
        Ok(result)
    }
}

fn read<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    if let Some(value) = direct(value, key) {
        return Some(value);
    }
    let mut current = value;
    for part in key.split('.') {
        current = direct(current, part)?;
    }
    Some(current)
}

fn direct<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    match value {
        Value::Object(values) => values.get(key),
        Value::Array(values) => key
            .parse::<usize>()
            .ok()
            .filter(|index| index.to_string() == key)
            .and_then(|index| values.get(index)),
        _ => None,
    }
}

fn read_or_empty(value: &Value, key: &str) -> Value {
    read(value, key)
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()))
}

fn check_keys(keys: &[&str]) -> Result<()> {
    if keys.len() > MAX_KEYS {
        return Err(limits());
    }
    let mut bytes = 0_usize;
    for key in keys {
        bytes = bytes.checked_add(key.len()).ok_or_else(limits)?;
        if bytes > MAX_KEY_BYTES {
            return Err(limits());
        }
    }
    Ok(())
}

fn check_path(key: &str) -> Result<()> {
    if key.split('.').take(MAX_DEPTH + 1).count() > MAX_DEPTH {
        return Err(limits());
    }
    Ok(())
}

fn check_lookup_keys(value: &Value, keys: &[&str]) -> Result<()> {
    check_keys(keys)?;
    for key in keys {
        if direct(value, key).is_none() {
            check_path(key)?;
        }
    }
    Ok(())
}

fn set(value: &mut Value, key: &str, child: Value) -> Result<()> {
    check_keys(&[key])?;
    check_path(key)?;
    data::set(value, key, child).map_err(|error| map_error(&error))
}
fn forget(value: &mut Value, key: &str) -> Result<()> {
    check_lookup_keys(value, &[key])?;
    if direct(value, key).is_some() {
        // data::forget 在字面命中前检查 dot 深度；此处按已命中的顶层键删除。
        let mut values = entries(std::mem::take(value));
        values.shift_remove(key);
        *value = packed(values);
        Ok(())
    } else {
        data::forget(value, key).map_err(|error| map_error(&error))
    }
}
fn validate(value: &Value) -> Result<()> {
    data::validate_limits(value).map_err(|error| map_error(&error))
}
fn map_error(error: &Error) -> Error {
    if error.code() == "input_limits_exceeded" {
        limits()
    } else {
        invalid()
    }
}
fn invalid() -> Error {
    Error::custom_with_status(
        StatusCode::INTERNAL_SERVER_ERROR,
        "session_data_invalid",
        "Session 属性无效",
    )
}
fn limits() -> Error {
    Error::custom_with_status(
        StatusCode::PAYLOAD_TOO_LARGE,
        "session_limits_exceeded",
        "Session 属性超过资源限制",
    )
}
fn empty_root(value: Value) -> Value {
    if value.as_object().is_some_and(Map::is_empty) {
        Value::Array(Vec::new())
    } else {
        value
    }
}
fn entries(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(values) => values,
        Value::Array(values) => values
            .into_iter()
            .enumerate()
            .map(|(index, value)| (index.to_string(), value))
            .collect(),
        _ => Map::new(),
    }
}
fn packed(values: Map<String, Value>) -> Value {
    if values
        .keys()
        .enumerate()
        .all(|(index, key)| key == &index.to_string())
    {
        Value::Array(values.into_values().collect())
    } else {
        Value::Object(values)
    }
}
fn array(value: Value) -> Result<Map<String, Value>> {
    if !value.is_array() && !value.is_object() {
        return Err(invalid());
    }
    Ok(entries(value))
}

fn push(attributes: &mut Value, key: &str, child: Value) -> Result<()> {
    let value = read_or_empty(attributes, key);
    // PHP 8.5 [] 追加接受 null 和 false；false 的非致命弃用警告不改写其结果。
    let mut values = if matches!(value, Value::Null | Value::Bool(false)) {
        Map::new()
    } else {
        array(value)?
    };
    let maximum = values
        .keys()
        .filter_map(|key| {
            key.parse::<i64>()
                .ok()
                .filter(|index| index.to_string() == *key)
        })
        .max();
    let next = maximum.map_or(Ok(0), |index| index.checked_add(1).ok_or_else(invalid))?;
    values.insert(next.to_string(), child);
    set(attributes, key, packed(values))
}

fn merge_new(attributes: &mut Value, old: Map<String, Value>) -> Result<()> {
    let new = array(read_or_empty(attributes, "_flash.new"))?;
    let mut merged = Map::new();
    let mut next = 0_usize;
    for (key, value) in new.into_iter().chain(old) {
        if key
            .parse::<i64>()
            .is_ok_and(|integer| integer.to_string() == key)
        {
            merged.insert(next.to_string(), value);
            next += 1;
        } else {
            merged.insert(key, value);
        }
    }
    // array_unique 默认 SORT_STRING；保留第一次的值和整数键，不调用 values()。
    let mut seen = HashSet::new();
    merged.retain(|_, value| seen.insert(scalars::string(value)));
    set(attributes, "_flash.new", packed(merged))
}

fn remove_old(attributes: &mut Value, keys: &[Value]) -> Result<()> {
    let mut old = array(read_or_empty(attributes, "_flash.old"))?;
    let removed = keys.iter().map(scalars::string).collect::<HashSet<_>>();
    old.retain(|_, value| !removed.contains(&scalars::string(value)));
    set(attributes, "_flash.old", packed(old))
}

fn cast_values(value: Value) -> Vec<Value> {
    match value {
        Value::Null => Vec::new(),
        Value::Array(values) => values,
        Value::Object(values) => values.into_values().collect(),
        value => vec![value],
    }
}

fn forget_php_key(attributes: &mut Value, value: &Value) -> Result<()> {
    let direct_key = match value {
        Value::String(value) => value.clone(),
        Value::Null => String::new(),
        Value::Bool(value) => i64::from(*value).to_string(),
        Value::Number(_) => scalars::integer(value).to_string(),
        Value::Array(_) | Value::Object(_) => return Err(invalid()),
    };
    if direct(attributes, &direct_key).is_some() {
        forget(attributes, &direct_key)
    } else {
        forget_path(attributes, &scalars::string(value))
    }
}

// PHP 非字符串 key 的直接检查使用整数转换，随后 explode 才用字符串转换；
// fallback 不能再次把整个字符串当 literal key 检查（例如 float 1.25）。
fn forget_path(attributes: &mut Value, key: &str) -> Result<()> {
    check_keys(&[key])?;
    check_path(key)?;
    let mut parts = key.split('.').peekable();
    let mut current = attributes;
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            return forget(current, part);
        }
        let child = match current {
            Value::Object(values) => values.get_mut(part),
            Value::Array(values) => part
                .parse::<usize>()
                .ok()
                .filter(|index| index.to_string() == part)
                .and_then(|index| values.get_mut(index)),
            _ => None,
        };
        let Some(child) = child else {
            return Ok(());
        };
        current = child;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // 以下正常 Store 期望来自固定源码/上游 SessionStoreTest；PHP Store 本身未运行。
    // 元数据弱转另以既存 PHP 8.5.0 -n 的 array_diff/unique/key_exists 内置函数探查。
    #[test]
    fn literal_dot_null_and_wildcard_match_arr_get() -> Result<()> {
        let mut attributes = Attributes::new(json!({
            "a.b": "literal", "a": {"b": "nested"}, "nil": null,
            "items": {"*": "star", "{first}": "first", "one": "ordinary"}
        }))?;
        assert_eq!(attributes.get("a.b")?, Some(json!("literal")));
        assert_eq!(attributes.get("nil")?, Some(Value::Null));
        assert_eq!(attributes.get("missing")?, None);
        assert_eq!(attributes.get("items.*")?, Some(json!("star")));
        assert_eq!(attributes.get("items.{first}")?, Some(json!("first")));
        attributes.put("a.b", json!("new"))?;
        assert_eq!(attributes.get("a.b")?, Some(json!("literal")));
        assert_eq!(attributes.pull("a.b")?, Some(json!("literal")));
        assert_eq!(attributes.get("a.b")?, Some(json!("new")));
        attributes.forget(&["a.b"])?;
        assert_eq!(attributes.get("a.b")?, None);
        Ok(())
    }

    #[test]
    fn dotted_literal_keys_do_not_consume_path_depth_and_real_paths_remain_bounded() -> Result<()> {
        let key = format!("{}a", "a.".repeat(MAX_DEPTH));
        let mut values = Map::new();
        values.insert(key.clone(), json!("literal"));
        let mut attributes = Attributes::new(Value::Object(values))?;
        assert_eq!(attributes.get(&key)?, Some(json!("literal")));
        assert!(attributes.exists(&[&key])?);
        assert!(attributes.has(&[&key])?);
        assert!(attributes.has_any(&[&key])?);
        assert_eq!(attributes.only(&[&key])?, attributes.all());
        assert_eq!(attributes.except(&[&key])?, json!([]));
        let before = attributes.all();
        assert_eq!(
            attributes
                .put(&key, json!("replacement"))
                .err()
                .map(|error| error.code().to_owned()),
            Some("session_limits_exceeded".to_owned())
        );
        assert_eq!(attributes.all(), before);
        let missing = format!("{key}.missing");
        assert_eq!(
            attributes
                .forget(&[&key, &missing])
                .err()
                .map(|error| error.code().to_owned()),
            Some("session_limits_exceeded".to_owned())
        );
        assert_eq!(attributes.all(), before);
        assert_eq!(attributes.pull(&key)?, Some(json!("literal")));
        assert_eq!(attributes.all(), json!([]));
        assert_eq!(attributes.only(&[&key])?, json!([]));
        for error in [
            attributes.get(&key).err(),
            attributes.exists(&[&key]).err(),
            attributes.has(&[&key]).err(),
            attributes.has_any(&[&key]).err(),
            attributes.pull(&key).err(),
            attributes.forget(&[&key]).err(),
            attributes.except(&[&key]).err(),
        ] {
            assert_eq!(error.ok_or_else(invalid)?.code(), "session_limits_exceeded");
        }
        assert_eq!(attributes.all(), json!([]));
        attributes = Attributes::new(before)?;
        attributes.forget(&[&key])?;
        assert_eq!(attributes.all(), json!([]));
        Ok(())
    }

    #[test]
    fn null_falsy_multi_key_and_empty_presence_are_distinct() -> Result<()> {
        let mut attributes = Attributes::new(json!({"nil":null,"f":false,"zero":0,"empty":""}))?;
        assert!(!attributes.has(&["nil"])?);
        assert!(attributes.exists(&["nil"])?);
        assert!(attributes.has(&["f", "zero", "empty"])?);
        assert!(!attributes.has(&["f", "nil"])?);
        assert!(!attributes.exists(&["f", "missing"])?);
        assert!(attributes.has(&[])?);
        assert!(attributes.exists(&[])?);
        assert!(!attributes.has_any(&[])?);
        assert!(attributes.has_any(&["nil", "f"])?);
        assert_eq!(attributes.pull("nil")?, Some(Value::Null));
        assert_eq!(attributes.pull("nil")?, None);
        Ok(())
    }

    #[test]
    fn only_is_top_level_and_except_uses_literal_then_dot_in_original_order() -> Result<()> {
        let attributes = Attributes::new(json!({"first":1,"a.b":2,"a":{"b":3,"c":4},"last":5}))?;
        let only = attributes.only(&["last", "a.b", "a.c", "first"])?;
        assert_eq!(only, json!({"first":1,"a.b":2,"last":5}));
        assert_eq!(
            only.as_object()
                .map(|values| values.keys().map(String::as_str).collect::<Vec<_>>()),
            Some(vec!["first", "a.b", "last"])
        );
        assert_eq!(
            attributes.except(&["a.b"])?,
            json!({"first":1,"a":{"b":3,"c":4},"last":5})
        );
        assert_eq!(attributes.get("a.b")?, Some(json!(2)));
        let indexed = Attributes::new(json!(["zero", "one"]))?;
        assert_eq!(indexed.only(&["1"])?, json!({"1":"one"}));
        assert_eq!(indexed.only(&[])?, json!([]));
        Ok(())
    }

    #[test]
    fn writes_preserve_values_and_bulk_is_atomic() -> Result<()> {
        let mut attributes = Attributes::new(json!({"a":null}))?;
        attributes.put("a.b", json!("  value  "))?;
        attributes.put_many(&[("a.b".into(), json!(2)), ("a".into(), json!(1))])?;
        assert_eq!(attributes.get("a")?, Some(json!(1)));
        let before = attributes.all();
        let too_deep = std::iter::repeat_n("x", MAX_DEPTH + 1)
            .collect::<Vec<_>>()
            .join(".");
        let error = attributes
            .put_many(&[("first".into(), json!(true)), (too_deep, json!(false))])
            .err()
            .ok_or_else(invalid)?;
        assert_eq!(error.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(error.code(), "session_limits_exceeded");
        assert_eq!(attributes.all(), before);
        attributes.put("raw", json!("  value  "))?;
        assert_eq!(attributes.get("raw")?, Some(json!("  value  ")));
        attributes.flush()?;
        assert_eq!(attributes.all(), json!([]));
        assert_eq!(Attributes::new(json!({}))?.all(), json!([]));
        Ok(())
    }

    #[test]
    fn flash_survives_one_age_and_now_does_not() -> Result<()> {
        let mut attributes = Attributes::new(json!([]))?;
        attributes.flash("status", json!("ok"))?;
        attributes.now("only_now", json!(0))?;
        assert_eq!(attributes.get("status")?, Some(json!("ok")));
        assert_eq!(attributes.get("only_now")?, Some(json!(0)));
        attributes.age()?;
        assert_eq!(attributes.get("status")?, Some(json!("ok")));
        assert_eq!(attributes.get("only_now")?, None);
        assert_eq!(attributes.get("_flash.old")?, Some(json!(["status"])));
        attributes.age()?;
        assert_eq!(attributes.get("status")?, None);
        Ok(())
    }

    #[test]
    fn flash_now_order_and_explicit_repeated_aging_are_observable() -> Result<()> {
        let mut first = Attributes::new(json!([]))?;
        first.flash("x", json!(1))?;
        first.now("x", json!(2))?;
        first.age()?;
        assert_eq!(first.get("x")?, None);
        assert_eq!(first.get("_flash.old")?, Some(json!(["x"])));
        let mut second = Attributes::new(json!([]))?;
        second.now("x", json!(1))?;
        second.flash("x", json!(2))?;
        second.age()?;
        assert_eq!(second.get("x")?, Some(json!(2)));
        second.age()?;
        assert_eq!(second.get("x")?, None);
        Ok(())
    }

    #[test]
    fn reflash_promotes_now_and_keep_empty_is_not_reflash() -> Result<()> {
        let mut attributes = Attributes::new(json!([]))?;
        attributes.now("x", json!(1))?;
        attributes.reflash()?;
        attributes.age()?;
        assert_eq!(attributes.get("x")?, Some(json!(1)));
        attributes.keep(&[])?;
        attributes.age()?;
        assert_eq!(attributes.get("x")?, None);
        attributes.put("ordinary", json!(7))?;
        attributes.keep(&["ordinary", "absent"])?;
        assert_eq!(attributes.get("absent")?, None);
        attributes.age()?;
        assert_eq!(attributes.get("ordinary")?, Some(json!(7)));
        attributes.age()?;
        assert_eq!(attributes.get("ordinary")?, None);
        Ok(())
    }

    #[test]
    fn diff_and_unique_preserve_sparse_indices_and_flash_duplicates() -> Result<()> {
        let mut attributes =
            Attributes::new(json!({"keep":1,"drop":2,"_flash":{"old":["keep","drop"],"new":[]}}))?;
        attributes.keep(&["keep"])?;
        assert_eq!(attributes.get("_flash.old")?, Some(json!({"1":"drop"})));
        attributes.age()?;
        assert_eq!(attributes.get("keep")?, Some(json!(1)));
        assert_eq!(attributes.get("drop")?, None);
        let mut unique = Attributes::new(json!({"_flash":{"new":["a"],"old":["a","b"]}}))?;
        unique.reflash()?;
        assert_eq!(unique.get("_flash.new")?, Some(json!({"0":"a","2":"b"})));
        let mut duplicate = Attributes::new(json!([]))?;
        duplicate.flash("x", json!(1))?;
        duplicate.flash("x", json!(2))?;
        assert_eq!(duplicate.get("_flash.new")?, Some(json!(["x", "x"])));
        Ok(())
    }

    #[test]
    fn metadata_scalar_comparisons_follow_real_php_builtin_probe() -> Result<()> {
        let mut attributes = Attributes::new(
            json!({"_flash":{"new":[],"old":[1,"1",true,false,null,0,0.0,1.25,[1],{"x":1}]}}),
        )?;
        attributes.reflash()?;
        assert_eq!(
            attributes.get("_flash.new")?,
            Some(json!({"0":1,"3":false,"5":0,"7":1.25,"8":[1]}))
        );
        attributes.age()?;
        let before = attributes.all();
        let error = attributes.age().err().ok_or_else(invalid)?;
        assert_eq!(error.code(), "session_data_invalid");
        assert_eq!(error.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(attributes.all(), before);
        Ok(())
    }

    #[test]
    fn age_accepts_numeric_bool_and_null_keys_and_preserves_float_fallback() -> Result<()> {
        let mut attributes = Attributes::new(
            json!({"0":"zero","":"empty","1":"one","keep":1,"_flash":{"old":[false,null,true],"new":[]}}),
        )?;
        attributes.age()?;
        assert_eq!(
            attributes.all(),
            json!({"keep":1,"_flash":{"old":[],"new":[]}})
        );
        let mut float =
            Attributes::new(json!({"1.25":"literal","_flash":{"old":[1.25],"new":[]}}))?;
        float.age()?;
        assert_eq!(float.get("1.25")?, Some(json!("literal")));
        let mut scalar =
            Attributes::new(json!({"victim":true,"_flash":{"old":"victim","new":true}}))?;
        scalar.age()?;
        assert_eq!(scalar.get("victim")?, None);
        assert_eq!(scalar.get("_flash.old")?, Some(json!(true)));
        Ok(())
    }

    #[test]
    fn push_accepts_php_null_false_and_next_negative_integer_key() -> Result<()> {
        for value in [Value::Null, Value::Bool(false)] {
            let mut attributes = Attributes::new(json!({"_flash":{"new":value,"old":[]}}))?;
            attributes.flash("x", json!(1))?;
            assert_eq!(attributes.get("_flash.new")?, Some(json!(["x"])));
        }
        let mut attributes = Attributes::new(json!({"_flash":{"new":{"-5":"before"},"old":[]}}))?;
        attributes.flash("x", json!(1))?;
        assert_eq!(
            attributes.get("_flash.new")?,
            Some(json!({"-5":"before","-4":"x"}))
        );
        Ok(())
    }

    #[test]
    fn manually_trimmed_flash_metadata_uses_visible_json_next_index() -> Result<()> {
        let mut attributes = Attributes::new(json!({"_flash":{"new":["a","b"],"old":[]}}))?;
        attributes.forget(&["_flash.new.1"])?;
        attributes.flash("x", json!(1))?;
        // JSON 值不保留 PHP 数组删除后的隐藏 next-free index；这里明确使用可见键。
        // PHP 同序列追加结果为 {0:"a",2:"x"}，本表示为连续数组 ["a","x"]。
        assert_eq!(attributes.get("_flash.new")?, Some(json!(["a", "x"])));
        Ok(())
    }

    #[test]
    fn malformed_metadata_and_all_resource_failures_are_atomic_and_safe() -> Result<()> {
        let mut attributes =
            Attributes::new(json!({"secret":"SENSITIVE","_flash":{"new":[],"old":null}}))?;
        let before = attributes.all();
        let error = attributes.flash("x", json!(1)).err().ok_or_else(invalid)?;
        assert_eq!(error.code(), "session_data_invalid");
        assert_eq!(attributes.all(), before);
        assert!(!format!("{attributes:?}").contains("SENSITIVE"));
        assert!(!format!("{error:?}").contains("SENSITIVE"));
        let too_many = vec!["x"; MAX_KEYS + 1];
        assert_eq!(
            attributes
                .forget(&too_many)
                .err()
                .map(|error| error.code().to_owned()),
            Some("session_limits_exceeded".into())
        );
        assert_eq!(attributes.all(), before);
        let full = (0..MAX_KEYS)
            .map(|index| (format!("k{index}"), Value::Null))
            .collect::<Map<_, _>>();
        let mut full = Attributes::new(Value::Object(full))?;
        let before = full.all();
        assert_eq!(
            full.put("extra", json!(1))
                .err()
                .map(|error| error.code().to_owned()),
            Some("session_limits_exceeded".into())
        );
        assert_eq!(full.all(), before);
        let long = "x".repeat(MAX_KEY_BYTES + 1);
        assert_eq!(
            full.get(&long).err().map(|error| error.code().to_owned()),
            Some("session_limits_exceeded".into())
        );
        for invalid_value in [Value::Null, json!(1), json!("secret")] {
            assert_eq!(
                Attributes::new(invalid_value)
                    .err()
                    .map(|error| error.code().to_owned()),
                Some("session_data_invalid".into())
            );
        }
        Ok(())
    }
}
