//! 同一属性存储的原始基线、变化记录与 cast 感知比较。
//!
//! ```
//! use openrust::database::eloquent::{ModelAttributes, ModelMetadata};
//! use serde_json::{Map, json};
//! fn main() -> openrust::Result<()> {
//!     let metadata = ModelMetadata::new("records", "app::Record");
//!     let attributes = ModelAttributes::new(Map::from_iter([("id".into(), json!("00042"))]));
//!     attributes.set_raw_attribute("id", json!(42))?;
//!     assert!(attributes.is_clean(&metadata, &[])?);
//!     attributes.set_raw_attribute("name", json!("Ada"))?;
//!     attributes.sync_changes(&metadata)?;
//!     assert!(attributes.was_changed(&["name"])?);
//!     assert_eq!(attributes.get_raw_original("name")?, None);
//!     attributes.sync_original()?;
//!     assert!(attributes.is_clean(&metadata, &[])?);
//!     Ok(())
//! }
//! ```
//!
//! `sync_changes` 只发布变化记录，`sync_original` 只确认当前内存基线；二者均不保存数据库。
//! 比较不调用 getter/setter/哈希；未决可回滚写入、运行中的 setter 或过期快照返回 409。
//! 逐字段同步遇缺项报错并保留此前已同步字段，模型保存会话仍待对应。

use super::ModelDateContext;
use super::{
    AttributeCast, AttributeGetter, AttributeMap, AttributeValue, ModelAttributes, ModelMetadata,
    State, from_json, into_json, read_conflict, replace_raw, state_error,
};
use crate::{
    Error, Result,
    database::eloquent::attribute_cast::{array_cast_equivalent, cast_float_value},
    support::{ArrayKey, InputNumber},
};
use serde_json::{Map, Value};
use std::borrow::Cow;

struct Snapshot {
    raw: AttributeMap,
    original: AttributeMap,
    revision: u64,
    original_revision: u64,
}

impl ModelAttributes {
    /// 读取原始基线：先查完整字面键，再按点路径查找；星号是字面键。
    ///
    /// 缺项返回 `None`，显式 null 返回 `Some(Value::Null)`，保留选中值的原始形状。
    pub fn get_raw_original(&self, name: &str) -> Result<Option<Value>> {
        self.get_raw_original_value(name)?
            .map(AttributeValue::try_into_json)
            .transpose()
    }

    /// 读取有类型的原始基线值；日期对象保留身份。
    pub fn get_raw_original_value(&self, name: &str) -> Result<Option<AttributeValue>> {
        let state = self.state.read().map_err(|_| state_error())?;
        Ok(original_value(&state.original, name).map(Cow::into_owned))
    }
    /// 按原值点路径读取，只有缺项才返回默认值；显式 null 不使用默认值。
    pub fn get_raw_original_or(&self, name: &str, default: Value) -> Result<Value> {
        Ok(self.get_raw_original(name)?.unwrap_or(default))
    }
    /// 读取完整数据库原始基线；Rust显式拆分PHP可选key的不同返回类型。
    pub fn get_raw_original_attributes(&self) -> Result<Map<String, Value>> {
        into_json(
            self.state
                .read()
                .map_err(|_| state_error())?
                .original
                .clone(),
        )
    }
    /// 转换原始基线中的指定属性；缺项的默认值也经过转换，显式 null 保留。
    ///
    /// 使用模型声明的 `metadata` 和访问器选择器。选择器及访问器在释放属性锁后执行，
    /// 访问器收到同一份原始基线。当前实例临时注册的访问器不参与。
    /// 此入口显式选择单键，包括空键或 `"0"`；整表读取使用
    /// [`Self::get_original_attributes`]。
    pub fn get_original<'a>(
        &self,
        metadata: &ModelMetadata,
        name: &str,
        default: Option<&Value>,
        declared_getter: impl FnOnce(&str) -> Option<&'a dyn AttributeGetter>,
    ) -> Result<Option<Value>> {
        let original = self
            .state
            .read()
            .map_err(|_| state_error())?
            .original
            .clone();
        let value = original_value(&original, name)
            .map(Cow::into_owned)
            .or_else(|| default.cloned().map(AttributeValue::from));
        transform_original(
            metadata,
            name,
            value,
            &original,
            declared_getter(name),
            &self.dates,
        )?
        .map(AttributeValue::try_into_json)
        .transpose()
    }

    /// 转换所有原始属性后显式读取 JSON 值。
    pub fn get_original_attributes<'a>(
        &self,
        metadata: &ModelMetadata,
        declared_getter: impl Fn(&str) -> Option<&'a dyn AttributeGetter>,
    ) -> Result<Map<String, Value>> {
        into_json(self.get_original_attribute_values(metadata, declared_getter)?)
    }

    /// 用同一原始快照转换所有已存在的顶层键，并保留日期对象。
    pub fn get_original_attribute_values<'a>(
        &self,
        metadata: &ModelMetadata,
        declared_getter: impl Fn(&str) -> Option<&'a dyn AttributeGetter>,
    ) -> Result<AttributeMap> {
        let original = self
            .state
            .read()
            .map_err(|_| state_error())?
            .original
            .clone();
        original
            .iter()
            .map(|(name, value)| {
                transform_original(
                    metadata,
                    name,
                    Some(value.clone()),
                    &original,
                    declared_getter(name),
                    &self.dates,
                )
                .map(|converted| {
                    (
                        name.clone(),
                        converted.unwrap_or_else(|| Value::Null.into()),
                    )
                })
            })
            .collect()
    }

    /// 读取最近一次显式 `sync_changes` 记录的原始变化，不重新计算 `dirty`。
    pub fn get_changes(&self) -> Result<Map<String, Value>> {
        into_json(self.get_changes_values()?)
    }

    /// 读取最近记录的有类型属性变化，保留对象身份。
    pub fn get_changes_values(&self) -> Result<AttributeMap> {
        Ok(self
            .state
            .read()
            .map_err(|_| state_error())?
            .changes
            .clone())
    }
    /// 读取最近 `sync_changes` 变化字段对应的旧 `original` 交集。
    pub fn get_previous(&self) -> Result<Map<String, Value>> {
        into_json(self.get_previous_values()?)
    }

    /// 读取最近记录的有类型旧原值，保留对象身份。
    pub fn get_previous_values(&self) -> Result<AttributeMap> {
        Ok(self
            .state
            .read()
            .map_err(|_| state_error())?
            .previous
            .clone())
    }
    /// 按固定cast规则比较当前原值与 `original`，不调用getter、setter或哈希。
    ///
    /// 未完成的可回滚赋值/异步setter或比较期间版本变化返回409，不报告伪 `clean`。
    pub fn get_dirty(&self, metadata: &ModelMetadata) -> Result<Map<String, Value>> {
        into_json(self.get_dirty_values(metadata)?)
    }

    /// 读取尚未保存的有类型原值，保留日期对象。
    pub fn get_dirty_values(&self, metadata: &ModelMetadata) -> Result<AttributeMap> {
        let snapshot = self.persistence_snapshot()?;
        let dirty = dirty(&snapshot, metadata, &self.dates)?;
        let state = self.state.read().map_err(|_| state_error())?;
        snapshot.check(&state)?;
        Ok(dirty)
    }
    /// 判断指定原始字段是否与original等价；缺 `original`字段不等价。
    pub fn original_is_equivalent(&self, metadata: &ModelMetadata, name: &str) -> Result<bool> {
        let snapshot = self.persistence_snapshot()?;
        let equivalent = equivalent(
            metadata,
            name,
            &snapshot.raw,
            &snapshot.original,
            &self.dates,
        )?;
        let state = self.state.read().map_err(|_| state_error())?;
        snapshot.check(&state)?;
        Ok(equivalent)
    }
    /// 判断全部或指定字段是否 `dirty`；空字段列表表示全部。
    pub fn is_dirty(&self, metadata: &ModelMetadata, names: &[&str]) -> Result<bool> {
        Ok(has_changes(&self.get_dirty_values(metadata)?, names))
    }
    /// 判断全部或指定字段是否 `clean`；不稳定状态仍返回错误。
    pub fn is_clean(&self, metadata: &ModelMetadata, names: &[&str]) -> Result<bool> {
        self.is_dirty(metadata, names).map(|dirty| !dirty)
    }
    /// 判断最近显式 `sync_changes` 是否包含全部或指定字段，空列表表示任意变化。
    pub fn was_changed(&self, names: &[&str]) -> Result<bool> {
        let state = self.state.read().map_err(|_| state_error())?;
        Ok(has_changes(&state.changes, names))
    }
    /// 丢弃当前属性的未保存变化，恢复原始基线并清空 `changes` 和 `previous`。
    ///
    /// 不执行 SQL 或访问器，也不更新原始基线。存在未决可回滚写入或运行中的
    /// setter 时返回冲突，保留全部当前状态。
    pub fn discard_changes(&self) -> Result<&Self> {
        let (changes, previous, retired) = {
            let mut state = self.state.write().map_err(|_| state_error())?;
            ensure_stable(&state)?;
            let original = state.original.clone();
            let retired = replace_raw(&mut state, original)?;
            (
                std::mem::take(&mut state.changes),
                std::mem::take(&mut state.previous),
                retired,
            )
        };
        drop((changes, previous, retired));
        Ok(self)
    }

    // 关系加载须看到回读值，但加载成功前保留原始基线。
    pub(in crate::database::eloquent) fn install_refreshed_raw(
        &self,
        raw: Map<String, Value>,
    ) -> Result<()> {
        let mut state = self.state.write().map_err(|_| state_error())?;
        ensure_stable(&state)?;
        let retired = replace_raw(&mut state, from_json(raw))?;
        drop(state);
        drop(retired);
        Ok(())
    }

    // 无关系需要重载时，一次更新当前属性与原始基线，保留上次保存的变化记录。
    pub(in crate::database::eloquent) fn install_refreshed_attributes(
        &self,
        raw: Map<String, Value>,
    ) -> Result<()> {
        let raw = from_json(raw);
        let retired = {
            let mut state = self.state.write().map_err(|_| state_error())?;
            ensure_stable(&state)?;
            let revision = next_original_revision(&state)?;
            let retired_raw = replace_raw(&mut state, raw.clone())?;
            let retired = (std::mem::replace(&mut state.original, raw), retired_raw);
            state.original_revision = revision;
            retired
        };
        drop(retired);
        Ok(())
    }

    /// 把稳定当前原值同步为 `original`，不清 `changes`/`previous`，不改变原始属性修改版本。
    pub fn sync_original(&self) -> Result<&Self> {
        self.flush_casts(None)?;
        let previous = {
            let mut state = self.state.write().map_err(|_| state_error())?;
            ensure_stable(&state)?;
            let revision = next_original_revision(&state)?;
            let raw = state.raw.clone();
            let previous = std::mem::replace(&mut state.original, raw);
            state.original_revision = revision;
            previous
        };
        drop(previous);
        Ok(self)
    }
    /// 同步单个原始字段；缺项明确错误，不把缺项写成null。
    pub fn sync_original_attribute(&self, name: &str) -> Result<&Self> {
        self.sync_original_attributes(&[name])
    }
    /// 按输入顺序同步字段；后项缺失保留已同步前缀，重复键按输入处理。
    pub fn sync_original_attributes(&self, names: &[&str]) -> Result<&Self> {
        self.flush_casts(None)?;
        let mut retired = Vec::new();
        let mut state = self.state.write().map_err(|_| state_error())?;
        ensure_stable(&state)?;
        let mut revision = None;
        for name in names {
            let value = state
                .raw
                .get(*name)
                .ok_or_else(|| persistence_error("model_attribute_original_missing"))?;
            let next = match revision {
                Some(revision) => revision,
                None => next_original_revision(&state)?,
            };
            let value = value.clone();
            if let Some(previous) = state.original.insert((*name).to_owned(), value) {
                retired.push(previous);
            }
            state.original_revision = next;
            revision = Some(next);
        }
        drop(state);
        drop(retired);
        Ok(self)
    }
    /// 发布当前`dirty` 与旧 `original`交集；不把当前属性标为 `clean`。
    pub fn sync_changes(&self, metadata: &ModelMetadata) -> Result<&Self> {
        let snapshot = self.persistence_snapshot()?;
        let changes = dirty(&snapshot, metadata, &self.dates)?;
        self.publish_change_set(&snapshot, changes)
    }

    // SQL 使用已绑定的变更；写后发布不能再次执行访问器或扩展这批列。
    pub(in crate::database::eloquent) fn sync_written_changes(
        &self,
        changes: AttributeMap,
    ) -> Result<&Self> {
        self.publish_change_set(&self.stable_snapshot()?, changes)
    }

    fn publish_change_set(&self, snapshot: &Snapshot, changes: AttributeMap) -> Result<&Self> {
        let previous = snapshot
            .original
            .iter()
            .filter(|(key, _)| changes.contains_key(*key))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        self.publish_changes(snapshot, changes, previous)?;
        Ok(self)
    }
    fn publish_changes(
        &self,
        snapshot: &Snapshot,
        changes: AttributeMap,
        previous: AttributeMap,
    ) -> Result<()> {
        let retired = {
            let mut state = self.state.write().map_err(|_| state_error())?;
            snapshot.check(&state)?;
            (
                std::mem::replace(&mut state.changes, changes),
                std::mem::replace(&mut state.previous, previous),
            )
        };
        drop(retired);
        Ok(())
    }
    fn persistence_snapshot(&self) -> Result<Snapshot> {
        self.flush_casts(None)?;
        self.stable_snapshot()
    }

    fn stable_snapshot(&self) -> Result<Snapshot> {
        let state = self.state.read().map_err(|_| state_error())?;
        ensure_stable(&state)?;
        Ok(Snapshot {
            raw: state.raw.clone(),
            original: state.original.clone(),
            revision: state.revision,
            original_revision: state.original_revision,
        })
    }
}

impl Snapshot {
    fn check(&self, state: &State) -> Result<()> {
        ensure_stable(state)?;
        if self.revision != state.revision || self.original_revision != state.original_revision {
            return Err(read_conflict("model_attribute_snapshot_conflict"));
        }
        Ok(())
    }
}
fn ensure_stable(state: &State) -> Result<()> {
    if state.pending_count != 0 || state.active_setters != 0 {
        return Err(read_conflict("model_attribute_update_pending"));
    }
    Ok(())
}
fn next_original_revision(state: &State) -> Result<u64> {
    state
        .original_revision
        .checked_add(1)
        .ok_or_else(|| persistence_error("model_attribute_original_revision_exhausted"))
}
fn persistence_error(code: &'static str) -> Error {
    Error::custom_with_status(
        http::StatusCode::INTERNAL_SERVER_ERROR,
        code,
        "模型持久属性快照无法完成",
    )
}
fn has_changes(changes: &AttributeMap, names: &[&str]) -> bool {
    if names.is_empty() {
        !changes.is_empty()
    } else {
        names.iter().any(|name| changes.contains_key(*name))
    }
}
fn dirty(
    snapshot: &Snapshot,
    metadata: &ModelMetadata,
    dates: &ModelDateContext,
) -> Result<AttributeMap> {
    let mut changes = AttributeMap::new();
    for (key, value) in &snapshot.raw {
        if !equivalent(metadata, key, &snapshot.raw, &snapshot.original, dates)? {
            changes.insert(key.clone(), value.clone());
        }
    }
    Ok(changes)
}
fn equivalent(
    metadata: &ModelMetadata,
    name: &str,
    current: &AttributeMap,
    original: &AttributeMap,
    dates: &ModelDateContext,
) -> Result<bool> {
    if !original.contains_key(name) {
        return Ok(false);
    }
    let Some(original) = original_value(original, name) else {
        return Ok(false);
    };
    let current =
        original_value(current, name).unwrap_or(Cow::Borrowed(&AttributeValue::Json(Value::Null)));
    if strict_attribute_equal(&current, &original) {
        return Ok(true);
    }
    if current.is_null() {
        return Ok(false);
    }
    let cast = metadata.effective_cast(name)?;
    if metadata.is_implicit_date(name)
        || cast
            .as_ref()
            .is_some_and(|cast| cast.is_date_read() && *cast != AttributeCast::Timestamp)
    {
        return Ok(strict_attribute_equal(
            &dates.store(metadata, current.into_owned())?,
            &dates.store(metadata, original.into_owned())?,
        ));
    }
    if let Some(AttributeCast::Class(caster)) = &cast
        && let Some(equal) = caster.compare(name, &original, &current)?
    {
        return Ok(equal);
    }
    #[cfg(feature = "encryption")]
    if let Some(AttributeCast::Encrypted(cast)) = &cast {
        return cast.equivalent(&current, &original);
    }
    let (Ok(current), Ok(original)) = (current.as_json(), original.as_json()) else {
        return Ok(false);
    };
    match cast {
        Some(AttributeCast::Float) => {
            if original.is_null() {
                return Ok(false);
            }
            let left = cast_float_value(current)?;
            let right = cast_float_value(original)?;
            Ok((left - right).abs() < f64::EPSILON * 4.0)
        }
        Some(AttributeCast::Array) => array_cast_equivalent(current, original),
        None | Some(AttributeCast::Class(_)) => Ok(InputNumber::from_value(current).is_some()
            && InputNumber::from_value(original).is_some()
            && crate::support::scalars::string(current)
                == crate::support::scalars::string(original)),
        Some(_) => {
            let left = metadata
                .read_cast_attribute(name, Some(current.clone()))?
                .unwrap_or(Value::Null);
            let right = metadata
                .read_cast_attribute(name, Some(original.clone()))?
                .unwrap_or(Value::Null);
            Ok(strict_equal(&left, &right))
        }
    }
}

fn strict_attribute_equal(left: &AttributeValue, right: &AttributeValue) -> bool {
    match (left, right) {
        (AttributeValue::Date(left), AttributeValue::Date(right)) => left.same_object(right),
        (AttributeValue::Object(left), AttributeValue::Object(right)) => left.same_object(right),
        (AttributeValue::Json(left), AttributeValue::Json(right)) => strict_equal(left, right),
        _ => false,
    }
}

fn transform_original(
    metadata: &ModelMetadata,
    name: &str,
    raw: Option<AttributeValue>,
    original: &AttributeMap,
    getter: Option<&dyn AttributeGetter>,
    dates: &ModelDateContext,
) -> Result<Option<AttributeValue>> {
    if let Some(getter) = getter {
        return getter.get(raw.as_ref(), original);
    }
    metadata.cast_value_with_snapshot(name, raw, original, dates)
}

// Arr::get只在根尝试完整键；逐段查找不解释通配符，不转换被选值。
fn original_value<'a>(values: &'a AttributeMap, name: &str) -> Option<Cow<'a, AttributeValue>> {
    if let Some(value) = values.get(name) {
        return Some(Cow::Borrowed(value));
    }
    let (first, rest) = name.split_once('.')?;
    let value = values.get(first)?.as_json().ok()?;
    nested_value(value, rest)
        .cloned()
        .map(AttributeValue::from)
        .map(Cow::Owned)
}

fn nested_value<'a>(mut value: &'a Value, path: &str) -> Option<&'a Value> {
    for segment in path.split('.') {
        value = match value {
            Value::Object(values) => values.get(segment)?,
            Value::Array(values) => {
                let ArrayKey::Integer(index) = ArrayKey::from(segment) else {
                    return None;
                };
                values.get(usize::try_from(index).ok()?)?
            }
            _ => return None,
        };
    }
    Some(value)
}

fn strict_number_equal(left: &Value, right: &Value) -> bool {
    match (
        InputNumber::from_value(left),
        InputNumber::from_value(right),
    ) {
        (Some(InputNumber::Integer(left)), Some(InputNumber::Integer(right))) => left == right,
        (Some(InputNumber::Float(left)), Some(InputNumber::Float(right))) => {
            left.partial_cmp(&right) == Some(std::cmp::Ordering::Equal)
        }
        _ => false,
    }
}

// 未解码raw对象/数组不互换；数字先按现有PHP64载体分类。
fn strict_equal(left: &Value, right: &Value) -> bool {
    let mut pending = vec![(left, right)];
    while let Some((left, right)) = pending.pop() {
        match (left, right) {
            (Value::Null, Value::Null) => {}
            (Value::Bool(left), Value::Bool(right)) if left == right => {}
            (Value::String(left), Value::String(right)) if left == right => {}
            (Value::Number(_), Value::Number(_)) if strict_number_equal(left, right) => {}
            (Value::Array(left), Value::Array(right)) if left.len() == right.len() => {
                pending.extend(left.iter().zip(right));
            }
            (Value::Object(left), Value::Object(right)) if left.len() == right.len() => {
                for ((left_key, left), (right_key, right)) in left.iter().zip(right) {
                    if left_key != right_key {
                        return false;
                    }
                    pending.push((left, right));
                }
            }
            _ => return false,
        }
    }
    true
}
#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
