//! 有类型的 Eloquent 属性访问器及对象缓存。

use super::{
    AttributeMap, AttributeValue, ModelAttributes, Result, apply_attribute_patch, apply_patch,
    next_revision, read_conflict, state_error,
};
use crate::database::eloquent::ModelDate;
use serde_json::Value;
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};

type Getter =
    dyn Fn(Option<&AttributeValue>, &AttributeMap) -> Result<AttributeValue> + Send + Sync;
type Setter = dyn Fn(AttributeValue, &AttributeMap) -> Result<AttributeMap> + Send + Sync;

/// 同步模型属性描述符，支持有类型的 getter、setter 与对象缓存。
pub struct ModelAttribute {
    getter: Option<Box<Getter>>,
    setter: Option<Box<Setter>>,
    cache_objects: bool,
    cache_scalars: bool,
}

impl Default for ModelAttribute {
    fn default() -> Self {
        Self {
            getter: None,
            setter: None,
            cache_objects: true,
            cache_scalars: false,
        }
    }
}

impl ModelAttribute {
    /// 创建可选 getter 和 setter 的属性描述符。
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置在模型锁外执行的 getter。
    #[must_use]
    pub fn get(
        mut self,
        getter: impl Fn(Option<&AttributeValue>, &AttributeMap) -> Result<AttributeValue>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.getter = Some(Box::new(getter));
        self
    }

    /// 设置返回多字段写入补丁的同步 setter。
    #[must_use]
    pub fn set(
        mut self,
        setter: impl Fn(AttributeValue, &AttributeMap) -> Result<AttributeMap> + Send + Sync + 'static,
    ) -> Self {
        self.setter = Some(Box::new(setter));
        self
    }

    /// 同时缓存普通值。
    #[must_use]
    pub fn should_cache(mut self) -> Self {
        self.cache_scalars = true;
        self
    }

    /// 每次读取重新执行对象 getter，除非显式开启全部值缓存。
    #[must_use]
    pub fn without_object_caching(mut self) -> Self {
        self.cache_objects = false;
        self
    }

    fn caches(&self, value: &AttributeValue) -> bool {
        self.cache_scalars || self.cache_objects && value.as_date().is_some()
    }

    pub(super) fn has_getter(&self) -> bool {
        self.getter.is_some()
    }

    pub(super) fn has_setter(&self) -> bool {
        self.setter.is_some()
    }

    pub(super) fn read(
        &self,
        value: Option<&AttributeValue>,
        raw: &AttributeMap,
    ) -> Result<AttributeValue> {
        self.getter.as_ref().map_or_else(
            || Ok(value.cloned().unwrap_or_else(|| Value::Null.into())),
            |getter| getter(value, raw),
        )
    }

    pub(super) fn write(
        &self,
        name: &str,
        value: AttributeValue,
        raw: &AttributeMap,
    ) -> Result<AttributeMap> {
        if let Some(setter) = &self.setter {
            setter(value, raw)
        } else {
            Ok(AttributeMap::from_iter([(name.to_owned(), value)]))
        }
    }
}

struct CallbackScope<'a> {
    store: &'a ModelAttributes,
    name: &'a str,
}

impl Drop for CallbackScope<'_> {
    fn drop(&mut self) {
        self.store
            .marked_callbacks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(self.name);
    }
}

pub(super) type CallbackNames = Mutex<BTreeSet<String>>;

impl ModelAttributes {
    /// 登记有类型的属性描述符，并使该字段旧缓存失效。
    pub fn register_attribute(
        &self,
        name: impl Into<String>,
        descriptor: ModelAttribute,
    ) -> Result<()> {
        let name = name.into();
        let old = {
            let mut state = self.state.write().map_err(|_| state_error())?;
            let revision = next_revision(&state)?;
            state.marked_cache.shift_remove(&name);
            state.setter_versions.insert(name.clone(), revision);
            state.revision = revision;
            state.marked.insert(name, Arc::new(descriptor))
        };
        drop(old);
        Ok(())
    }

    fn enter_attribute_callback<'a>(&'a self, name: &'a str) -> Result<CallbackScope<'a>> {
        if !self
            .marked_callbacks
            .lock()
            .map_err(|_| state_error())?
            .insert(name.to_owned())
        {
            return Err(read_conflict("model_attribute_callback_pending"));
        }
        Ok(CallbackScope { store: self, name })
    }

    pub(super) fn read_marked(
        &self,
        name: &str,
        descriptor: &ModelAttribute,
        raw: &AttributeMap,
    ) -> Result<AttributeValue> {
        let _callback = self.enter_attribute_callback(name)?;
        descriptor.read(raw.get(name), raw)
    }

    pub(super) fn write_marked(
        &self,
        name: &str,
        descriptor: &ModelAttribute,
        value: AttributeValue,
        raw: &AttributeMap,
    ) -> Result<AttributeMap> {
        let _callback = self.enter_attribute_callback(name)?;
        descriptor.write(name, value, raw)
    }

    pub(super) fn get_marked(
        &self,
        name: &str,
        descriptor: &ModelAttribute,
        input: (Option<&AttributeValue>, &AttributeMap),
        cached: Option<AttributeValue>,
        revision: u64,
        committed: bool,
    ) -> Result<AttributeValue> {
        if let Some(value) = cached {
            return Ok(value);
        }
        let value = {
            let _callback = self.enter_attribute_callback(name)?;
            descriptor.read(input.0, input.1)?
        };
        if !committed && descriptor.caches(&value) {
            let mut state = self.state.write().map_err(|_| state_error())?;
            if revision != state.revision {
                return Err(read_conflict("model_attribute_read_conflict"));
            }
            if let Some(cached) = state.marked_cache.get(name) {
                return Ok(cached.clone());
            }
            if state.pending_count == 0 && state.active_setters == 0 {
                let revision = next_revision(&state)?;
                state.marked_cache.insert(name.to_owned(), value.clone());
                state.revision = revision;
            }
        }
        Ok(value)
    }

    pub(super) fn set_marked(
        &self,
        name: &str,
        descriptor: &ModelAttribute,
        value: AttributeValue,
        raw: &AttributeMap,
        revision: u64,
    ) -> Result<()> {
        let date_revision = date_revision(&value)?;
        let patch = self.write_marked(name, descriptor, value.clone(), raw)?;
        let mut state = self.state.write().map_err(|_| state_error())?;
        if state.revision != revision || date_revision != self::date_revision(&value)? {
            return Err(read_conflict("model_attribute_mutation_conflict"));
        }
        apply_patch(&mut state, patch)?;
        state.marked_cache.shift_remove(name);
        if descriptor.caches(&value) {
            state.marked_cache.insert(name.to_owned(), value);
        }
        Ok(())
    }

    pub(super) fn flush_marked(&self, only: Option<&str>) -> Result<()> {
        let (pending, mut snapshot, revision) = {
            let state = self.state.read().map_err(|_| state_error())?;
            let pending = state
                .marked_cache
                .iter()
                .filter(|(name, value)| {
                    !value.is_null() && only.is_none_or(|key| key == name.as_str())
                })
                .filter_map(|(name, value)| {
                    let descriptor = state.marked.get(name)?;
                    (!descriptor.has_getter() || descriptor.has_setter())
                        .then(|| (name.clone(), value.clone(), Arc::clone(descriptor)))
                })
                .collect::<Vec<_>>();
            if pending.is_empty() {
                return Ok(());
            }
            if state.pending_count != 0 || state.active_setters != 0 {
                return Err(read_conflict("model_attribute_update_pending"));
            }
            (pending, state.raw.clone(), state.revision)
        };
        let versions = pending
            .iter()
            .map(|(_, value, _)| date_revision(value))
            .collect::<Result<Vec<_>>>()?;
        let mut patch = AttributeMap::new();
        for (name, value, descriptor) in &pending {
            let fields = self.write_marked(name, descriptor, value.clone(), &snapshot)?;
            snapshot.extend(fields.clone());
            patch.extend(fields);
        }
        let mut state = self.state.write().map_err(|_| state_error())?;
        if revision != state.revision || state.pending_count != 0 || state.active_setters != 0 {
            return Err(read_conflict("model_attribute_mutation_conflict"));
        }
        for ((_, value, _), version) in pending.iter().zip(versions) {
            if version != date_revision(value)? {
                return Err(read_conflict("model_attribute_mutation_conflict"));
            }
        }
        // 缓存回写同步存储值，继续保留同一访问器对象。
        apply_attribute_patch(&mut state, patch, true)
    }
}

fn date_revision(value: &AttributeValue) -> Result<Option<u64>> {
    value.as_date().map(ModelDate::revision).transpose()
}
