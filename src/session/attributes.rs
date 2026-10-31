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
    support::{DataValue, data, scalars},
};
use http::StatusCode;
use serde_json::{Map, Value};
use std::collections::HashSet;

const MAX_KEYS: usize = 1024;
const MAX_DEPTH: usize = 64;
const MAX_KEY_BYTES: usize = 20 * 1024 * 1024;

/// 当前请求共享的 Session 属性；快照只由同一棵值树产生。
#[derive(Clone)]
pub(crate) enum Attributes {
    Pure(DataValue),
    Rich(DataValue),
}

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
        Ok(Self::Pure(DataValue::from_json(empty_root(value))?))
    }

    /// 返回完整属性快照，不输出内部共享引用。
    pub(crate) fn all(&self) -> Result<Value> {
        match self {
            Self::Pure(value) | Self::Rich(value) => value.to_json(),
        }
    }

    /// 包含共享对象身份的完整属性快照。
    pub(crate) fn all_value(&self) -> DataValue {
        match self {
            Self::Pure(value) | Self::Rich(value) => value.clone(),
        }
    }

    /// 只供请求内容量预检；对象占位值不允许传给 store。
    pub(crate) fn budget_json(&self) -> Result<Value> {
        match self {
            Self::Pure(value) | Self::Rich(value) => value.budget_json(),
        }
    }

    /// 按 PHP `array_replace` 合并已加载的顶层属性，保留现存对象身份。
    pub(crate) fn merge_loaded(&mut self, incoming: Value) -> Result<()> {
        if matches!(self, Self::Pure(_)) {
            *self = Self::new(super::merge_attributes(self.all()?, incoming)?)?;
            return Ok(());
        }
        let DataValue::Array(entries) = DataValue::from_json(incoming)? else {
            return Err(invalid());
        };
        let mut candidate = self.all_value();
        let DataValue::Array(ref mut current) = candidate else {
            return Err(invalid());
        };
        for (key, value) in entries {
            if let Some((_, existing)) = current.iter_mut().find(|(existing, _)| existing == &key) {
                *existing = value;
            } else {
                current.push((key, value));
            }
        }
        candidate.validate()?;
        *self = Self::Rich(candidate);
        Ok(())
    }

    /// 原始对象与纯数据沿同一属性树读取。
    pub(crate) fn get_value(&self, key: &str) -> Result<Option<DataValue>> {
        self.check_lookup_paths(&[key])?;
        match self {
            Self::Pure(value) | Self::Rich(value) => Ok(value.get(key)?.cloned()),
        }
    }

    fn check_lookup_paths(&self, keys: &[&str]) -> Result<()> {
        check_keys(keys)?;
        let value = match self {
            Self::Pure(value) | Self::Rich(value) => value,
        };
        for key in keys {
            if value.direct(key).is_none() {
                check_path(key)?;
            }
        }
        Ok(())
    }

    /// 原始对象写入；所有变更在同一树内原子发布。
    pub(crate) fn put_value(&mut self, key: &str, value: DataValue) -> Result<()> {
        check_keys(&[key])?;
        let mut candidate = self.all_value();
        candidate.set(key, value)?;
        *self = Self::Rich(candidate);
        Ok(())
    }

    /// 顺序批量原始写入，任一失败则不发布部分结果。
    pub(crate) fn put_many_value(&mut self, values: &[(String, DataValue)]) -> Result<()> {
        check_keys(
            &values
                .iter()
                .map(|(key, _)| key.as_str())
                .collect::<Vec<_>>(),
        )?;
        let mut candidate = self.clone();
        for (key, value) in values {
            candidate.put_value(key, value.clone())?;
        }
        *self = candidate;
        Ok(())
    }

    /// 在原始 PHP 数组末尾追加，保留已有对象身份。
    pub(crate) fn push_value(&mut self, key: &str, value: DataValue) -> Result<()> {
        let current = self.get_value(key)?.unwrap_or(DataValue::Array(Vec::new()));
        let mut entries = match current {
            DataValue::Array(entries) => entries,
            DataValue::Null | DataValue::Bool(false) => Vec::new(),
            _ => return Err(invalid()),
        };
        let next = entries
            .iter()
            .filter_map(|(key, _)| match key {
                crate::support::ArrayKey::Integer(value) => Some(*value),
                crate::support::ArrayKey::String(_) => None,
            })
            .max()
            .map_or(Some(0), |value| value.checked_add(1))
            .ok_or_else(invalid)?;
        entries.push((crate::support::ArrayKey::Integer(next), value));
        self.put_value(key, DataValue::Array(entries))
    }

    /// 返回原始值并删除，保留对象共享身份。
    pub(crate) fn pull_value(&mut self, key: &str) -> Result<Option<DataValue>> {
        let value = self.get_value(key)?;
        self.forget(&[key])?;
        Ok(value)
    }

    /// 按顶层字面键选择，不对对象作编码投影。
    pub(crate) fn only_value(&self, keys: &[&str]) -> Result<DataValue> {
        check_keys(keys)?;
        let DataValue::Array(entries) = self.all_value() else {
            return Err(invalid());
        };
        Ok(DataValue::Array(
            entries
                .into_iter()
                .filter(|(key, _)| keys.contains(&key.to_string().as_str()))
                .collect(),
        ))
    }

    /// 从独立值快照移除指定键。
    pub(crate) fn except_value(&self, keys: &[&str]) -> Result<DataValue> {
        check_keys(keys)?;
        let mut candidate = self.all_value();
        for key in keys {
            candidate.forget(key)?;
        }
        Ok(candidate)
    }

    /// 通用对象闪存写入，使用同一 `_flash` 元数据。
    pub(crate) fn flash_value(&mut self, key: &str, value: DataValue) -> Result<()> {
        let mut candidate = self.clone();
        candidate.put_value(key, value)?;
        candidate.rich_push_flash("_flash.new", key)?;
        candidate.rich_remove_old(key)?;
        *self = candidate;
        Ok(())
    }

    /// 原始对象仅在当前请求可读，本次保存按 old flash 老化。
    pub(crate) fn now_value(&mut self, key: &str, value: DataValue) -> Result<()> {
        let mut candidate = self.clone();
        candidate.put_value(key, value)?;
        candidate.rich_push_flash("_flash.old", key)?;
        *self = candidate;
        Ok(())
    }

    /// 原始旧输入读取；对象 clone 保持同一 Arc 身份。
    pub(crate) fn old_input_value(&self, key: Option<&str>) -> Result<Option<DataValue>> {
        let bag = self
            .get_value("_old_input")?
            .unwrap_or(DataValue::Array(Vec::new()));
        if !bag.is_array() {
            return Err(invalid());
        }
        match key {
            None => Ok(Some(bag)),
            Some(key) => Ok(bag.get(key)?.cloned()),
        }
    }

    /// 对象写入旧输入并登记 flash，不建立额外旁路。
    pub(crate) fn flash_input_value(&mut self, value: DataValue) -> Result<()> {
        if !value.is_array() {
            return Err(invalid());
        }
        self.flash_value("_old_input", value)
    }

    fn rich_push_flash(&mut self, path: &str, key: &str) -> Result<()> {
        let mut current = self
            .get_value(path)?
            .unwrap_or(DataValue::Array(Vec::new()));
        let DataValue::Array(ref mut entries) = current else {
            return Err(invalid());
        };
        let next = entries
            .iter()
            .filter_map(|(key, _)| match key {
                crate::support::ArrayKey::Integer(value) => Some(*value),
                crate::support::ArrayKey::String(_) => None,
            })
            .max()
            .map_or(Some(0), |value| value.checked_add(1))
            .ok_or_else(invalid)?;
        entries.push((
            crate::support::ArrayKey::Integer(next),
            DataValue::String(key.to_owned()),
        ));
        self.put_value(path, current)
    }

    fn rich_remove_old(&mut self, key: &str) -> Result<()> {
        let old = self
            .get_value("_flash.old")?
            .unwrap_or(DataValue::Array(Vec::new()));
        let DataValue::Array(mut entries) = old else {
            return Err(invalid());
        };
        entries.retain(
            |(_, value)| !matches!(value, DataValue::String(candidate) if candidate == key),
        );
        self.put_value("_flash.old", DataValue::Array(entries))
    }

    /// 完整字面键优先，其次 dot；显式 null 保留为 `Some(Null)`。
    pub(crate) fn get(&self, key: &str) -> Result<Option<Value>> {
        self.get_value(key)?
            .map(|value| value.to_json())
            .transpose()
    }

    /// 先定位顶层旧输入 bag，再在 bag 内按字面键优先读取。
    pub(crate) fn old_input(&self, key: Option<&str>) -> Result<Option<Value>> {
        if matches!(self, Self::Rich(_)) {
            return self
                .old_input_value(key)?
                .map(|value| value.to_json())
                .transpose();
        }
        let bag = self
            .get("_old_input")?
            .unwrap_or_else(|| Value::Array(Vec::new()));
        if !bag.is_array() && !bag.is_object() {
            return Err(invalid());
        }
        match key {
            None => Ok(Some(bag)),
            Some(key) => {
                check_lookup_keys(&bag, &[key])?;
                Ok(read(&bag, key).cloned())
            }
        }
    }

    /// 所有键均存在；null 也存在，空列表为 true。
    pub(crate) fn exists(&self, keys: &[&str]) -> Result<bool> {
        self.check_lookup_paths(keys)?;
        keys.iter().try_fold(true, |result, key| {
            Ok(result && self.get_value(key)?.is_some())
        })
    }

    /// 所有键均存在且非 null；false、0、空串保留为存在。
    pub(crate) fn has(&self, keys: &[&str]) -> Result<bool> {
        self.check_lookup_paths(keys)?;
        keys.iter().try_fold(true, |result, key| {
            Ok(result && self.get_value(key)?.is_some_and(|value| !value.is_null()))
        })
    }

    /// 至少一个键存在且非 null；空列表为 false。
    pub(crate) fn has_any(&self, keys: &[&str]) -> Result<bool> {
        self.check_lookup_paths(keys)?;
        for key in keys {
            if self.get_value(key)?.is_some_and(|value| !value.is_null()) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// 设置 dot 路径；失败不改变原属性。
    pub(crate) fn put(&mut self, key: &str, value: Value) -> Result<()> {
        if matches!(self, Self::Rich(_)) {
            return self.put_value(key, DataValue::from_json(value)?);
        }
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
        if matches!(self, Self::Rich(_)) {
            let mut candidate = self.clone();
            for (key, value) in values {
                candidate.put(key, value.clone())?;
            }
            *self = candidate;
            return Ok(());
        }
        self.change(|candidate| {
            for (key, value) in values {
                set(candidate, key, value.clone())?;
            }
            Ok(())
        })
    }

    /// 返回原值并删除；null 与缺失不同。
    pub(crate) fn pull(&mut self, key: &str) -> Result<Option<Value>> {
        if matches!(self, Self::Rich(_)) {
            let value = self.get(key)?;
            self.forget(&[key])?;
            return Ok(value);
        }
        check_keys(&[key])?;
        self.change(|candidate| {
            let value = read(candidate, key).cloned();
            forget(candidate, key)?;
            Ok(value)
        })
    }

    /// PHP 数组追加；null/false 可转为空数组，其他标量拒绝。
    pub(crate) fn push(&mut self, key: &str, value: Value) -> Result<()> {
        check_keys(&[key])?;
        if matches!(self, Self::Rich(_)) {
            let current = self.get_value(key)?.unwrap_or(DataValue::Array(Vec::new()));
            let mut entries = match current {
                DataValue::Array(entries) => entries,
                DataValue::Null | DataValue::Bool(false) => Vec::new(),
                _ => return Err(invalid()),
            };
            let next = entries
                .iter()
                .filter_map(|(key, _)| match key {
                    crate::support::ArrayKey::Integer(value) => Some(*value),
                    crate::support::ArrayKey::String(_) => None,
                })
                .max()
                .map_or(Some(0), |value| value.checked_add(1))
                .ok_or_else(invalid)?;
            entries.push((
                crate::support::ArrayKey::Integer(next),
                DataValue::from_json(value)?,
            ));
            return self.put_value(key, DataValue::Array(entries));
        }
        self.change(|candidate| push(candidate, key, value))
    }

    /// 按 PHP 数值或数组加法计算并写入；失败保持原属性。
    pub(crate) fn increment(&mut self, key: &str, amount: &Value) -> Result<Value> {
        if matches!(self, Self::Rich(_)) {
            let previous = self.get(key)?.unwrap_or_else(|| Value::from(0));
            let result = scalars::add(&previous, amount).ok_or_else(invalid)?;
            self.put(key, result.clone())?;
            return Ok(result);
        }
        let previous = self.get(key)?.unwrap_or_else(|| Value::from(0));
        let value = scalars::add(&previous, amount).ok_or_else(invalid)?;
        self.put(key, value.clone())?;
        Ok(value)
    }

    /// 删除完整字面键或 dot 路径；缺失键不报错。
    pub(crate) fn forget(&mut self, keys: &[&str]) -> Result<()> {
        check_keys(keys)?;
        if let Self::Rich(value) = self {
            let mut candidate = value.clone();
            for key in keys {
                candidate.forget(key)?;
            }
            *value = candidate;
            return Ok(());
        }
        self.change(|candidate| {
            for key in keys {
                forget(candidate, key)?;
            }
            Ok(())
        })
    }

    /// 清空全部属性，包含 token 和 flash 元数据。
    pub(crate) fn flush(&mut self) -> Result<()> {
        if matches!(self, Self::Rich(_)) {
            *self = Self::Pure(DataValue::Array(Vec::new()));
            return Ok(());
        }
        self.change(|candidate| {
            *candidate = Value::Array(Vec::new());
            Ok(())
        })
    }

    /// 立即可读且延续到下一次请求的值；保留 new 列表的重复项。
    pub(crate) fn flash(&mut self, key: &str, value: Value) -> Result<()> {
        if matches!(self, Self::Rich(_)) {
            let mut candidate = self.clone();
            candidate.put(key, value)?;
            candidate.rich_push_flash("_flash.new", key)?;
            candidate.rich_remove_old(key)?;
            *self = candidate;
            return Ok(());
        }
        check_keys(&[key])?;
        self.change(|candidate| {
            set(candidate, key, value)?;
            push(candidate, "_flash.new", Value::String(key.to_owned()))?;
            remove_old(candidate, &[Value::String(key.to_owned())])
        })
    }

    /// 旧输入只接受 PHP array 对应的 JSON 数组或对象；失败不改变 flash 元数据。
    pub(crate) fn flash_input(&mut self, value: Value) -> Result<()> {
        if !value.is_array() && !value.is_object() {
            return Err(invalid());
        }
        self.flash("_old_input", value)
    }

    /// 仅在当前请求可读；下一次 age 删除。
    pub(crate) fn now(&mut self, key: &str, value: Value) -> Result<()> {
        if matches!(self, Self::Rich(_)) {
            let mut candidate = self.clone();
            candidate.put(key, value)?;
            candidate.rich_push_flash("_flash.old", key)?;
            *self = candidate;
            return Ok(());
        }
        check_keys(&[key])?;
        self.change(|candidate| {
            set(candidate, key, value)?;
            push(candidate, "_flash.old", Value::String(key.to_owned()))
        })
    }

    /// 将所有 old 合并到 new，并清空 old；包括由 now 标记的键。
    pub(crate) fn reflash(&mut self) -> Result<()> {
        if matches!(self, Self::Rich(_)) {
            return self.rich_flash_update(|metadata| {
                let old = array(read_or_empty(metadata, "_flash.old"))?;
                merge_new(metadata, old)?;
                set(metadata, "_flash.old", Value::Array(Vec::new()))
            });
        }
        self.change(|candidate| {
            let old = array(read_or_empty(candidate, "_flash.old"))?;
            merge_new(candidate, old)?;
            set(candidate, "_flash.old", Value::Array(Vec::new()))
        })
    }

    /// 保留指定键；空列表不等于 reflash。
    pub(crate) fn keep(&mut self, keys: &[&str]) -> Result<()> {
        check_keys(keys)?;
        if matches!(self, Self::Rich(_)) {
            return self.rich_flash_update(|metadata| {
                let keys = keys
                    .iter()
                    .map(|key| Value::String((*key).to_owned()))
                    .collect::<Vec<_>>();
                merge_new(metadata, array(Value::Array(keys.clone()))?)?;
                remove_old(metadata, &keys)
            });
        }
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
        if matches!(self, Self::Rich(_)) {
            let mut candidate = self.clone();
            let old = candidate
                .get_value("_flash.old")?
                .unwrap_or(DataValue::Array(Vec::new()))
                .to_json()?;
            for key in cast_values(old) {
                candidate.rich_forget_php_key(&key)?;
            }
            let new = candidate
                .get_value("_flash.new")?
                .unwrap_or(DataValue::Array(Vec::new()));
            candidate.put_value("_flash.old", new)?;
            candidate.put_value("_flash.new", DataValue::Array(Vec::new()))?;
            *self = candidate;
            return Ok(());
        }
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
        if let Self::Rich(value) = self {
            let DataValue::Array(entries) = value else {
                return Err(invalid());
            };
            return DataValue::Array(
                entries
                    .iter()
                    .filter(|(key, _)| keys.contains(&key.to_string().as_str()))
                    .cloned()
                    .collect(),
            )
            .to_json();
        }
        let Self::Pure(value) = self else {
            return Err(invalid());
        };
        let mut result = Map::new();
        for (key, value) in entries(value.to_json()?) {
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
        if let Self::Rich(value) = self {
            let mut result = value.clone();
            for key in keys {
                result.forget(key)?;
            }
            return result.to_json();
        }
        let Self::Pure(value) = self else {
            return Err(invalid());
        };
        let mut result = value.to_json()?;
        for key in keys {
            forget(&mut result, key)?;
        }
        validate(&result)?;
        Ok(empty_root(result))
    }

    fn change<T>(&mut self, mutation: impl FnOnce(&mut Value) -> Result<T>) -> Result<T> {
        let Self::Pure(value) = self else {
            return Err(invalid());
        };
        let mut candidate = value.to_json()?;
        let result = mutation(&mut candidate)?;
        validate(&candidate)?;
        *value = DataValue::from_json(empty_root(candidate))?;
        Ok(result)
    }

    fn rich_flash_update(&mut self, mutation: impl FnOnce(&mut Value) -> Result<()>) -> Result<()> {
        let flash = self
            .get_value("_flash")?
            .unwrap_or(DataValue::Array(Vec::new()))
            .to_json()?;
        let mut metadata = Value::Object(Map::new());
        set(&mut metadata, "_flash", flash)?;
        mutation(&mut metadata)?;
        let updated = read(&metadata, "_flash").cloned().ok_or_else(invalid)?;
        self.put_value("_flash", DataValue::from_json(updated)?)
    }

    fn rich_forget_php_key(&mut self, value: &Value) -> Result<()> {
        let direct_key = match value {
            Value::String(value) => value.clone(),
            Value::Null => String::new(),
            Value::Bool(value) => i64::from(*value).to_string(),
            Value::Number(_) => scalars::integer(value).to_string(),
            Value::Array(_) | Value::Object(_) => return Err(invalid()),
        };
        let key = if self.all_value().direct(&direct_key).is_some() {
            direct_key
        } else {
            scalars::string(value)
        };
        self.forget(&[&key])
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
        assert_eq!(attributes.only(&[&key])?, attributes.all()?);
        assert_eq!(attributes.except(&[&key])?, json!([]));
        let before = attributes.all()?;
        assert_eq!(
            attributes
                .put(&key, json!("replacement"))
                .err()
                .map(|error| error.code().to_owned()),
            Some("session_limits_exceeded".to_owned())
        );
        assert_eq!(attributes.all()?, before);
        let missing = format!("{key}.missing");
        assert_eq!(
            attributes
                .forget(&[&key, &missing])
                .err()
                .map(|error| error.code().to_owned()),
            Some("session_limits_exceeded".to_owned())
        );
        assert_eq!(attributes.all()?, before);
        assert_eq!(attributes.pull(&key)?, Some(json!("literal")));
        assert_eq!(attributes.all()?, json!([]));
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
        assert_eq!(attributes.all()?, json!([]));
        attributes = Attributes::new(before)?;
        attributes.forget(&[&key])?;
        assert_eq!(attributes.all()?, json!([]));
        Ok(())
    }

    #[test]
    fn multi_key_presence_prechecks_every_path_before_short_circuit() -> Result<()> {
        let deep = std::iter::repeat_n("x", MAX_DEPTH + 1)
            .collect::<Vec<_>>()
            .join(".");
        let attributes = Attributes::new(json!({"present":1,"nil":null}))?;
        for keys in [
            ["missing", deep.as_str()],
            [deep.as_str(), "missing"],
            ["nil", deep.as_str()],
            [deep.as_str(), "nil"],
            ["present", deep.as_str()],
            [deep.as_str(), "present"],
        ] {
            for result in [
                attributes.exists(&keys),
                attributes.has(&keys),
                attributes.has_any(&keys),
            ] {
                assert_eq!(
                    result.err().map(|error| error.code().to_owned()),
                    Some("session_limits_exceeded".to_owned())
                );
            }
        }
        let mut values = Map::new();
        values.insert(deep.clone(), Value::Bool(true));
        values.insert("nil".into(), Value::Null);
        let literal = Attributes::new(Value::Object(values))?;
        for keys in [["nil", deep.as_str()], [deep.as_str(), "nil"]] {
            assert!(literal.exists(&keys)?);
            assert!(!literal.has(&keys)?);
            assert!(literal.has_any(&keys)?);
        }
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
        let before = attributes.all()?;
        let too_deep = std::iter::repeat_n("x", MAX_DEPTH + 1)
            .collect::<Vec<_>>()
            .join(".");
        let error = attributes
            .put_many(&[("first".into(), json!(true)), (too_deep, json!(false))])
            .err()
            .ok_or_else(invalid)?;
        assert_eq!(error.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(error.code(), "session_limits_exceeded");
        assert_eq!(attributes.all()?, before);
        attributes.put("raw", json!("  value  "))?;
        assert_eq!(attributes.get("raw")?, Some(json!("  value  ")));
        attributes.flush()?;
        assert_eq!(attributes.all()?, json!([]));
        assert_eq!(Attributes::new(json!({}))?.all()?, json!([]));
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
        let before = attributes.all()?;
        let error = attributes.age().err().ok_or_else(invalid)?;
        assert_eq!(error.code(), "session_data_invalid");
        assert_eq!(error.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(attributes.all()?, before);
        Ok(())
    }

    #[test]
    fn age_accepts_numeric_bool_and_null_keys_and_preserves_float_fallback() -> Result<()> {
        let mut attributes = Attributes::new(
            json!({"0":"zero","":"empty","1":"one","keep":1,"_flash":{"old":[false,null,true],"new":[]}}),
        )?;
        attributes.age()?;
        assert_eq!(
            attributes.all()?,
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
        let before = attributes.all()?;
        let error = attributes.flash("x", json!(1)).err().ok_or_else(invalid)?;
        assert_eq!(error.code(), "session_data_invalid");
        assert_eq!(attributes.all()?, before);
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
        assert_eq!(attributes.all()?, before);
        let full = (0..MAX_KEYS)
            .map(|index| (format!("k{index}"), Value::Null))
            .collect::<Map<_, _>>();
        let mut full = Attributes::new(Value::Object(full))?;
        let before = full.all()?;
        assert_eq!(
            full.put("extra", json!(1))
                .err()
                .map(|error| error.code().to_owned()),
            Some("session_limits_exceeded".into())
        );
        assert_eq!(full.all()?, before);
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
