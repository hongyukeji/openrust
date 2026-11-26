use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use super::{Cache, CacheError, CacheFuture, CacheResult, Repository, Value};

type Callback =
    dyn for<'a> Fn(&'a Repository, Vec<Value>) -> CacheFuture<'a, CacheResult<Value>> + Send + Sync;

/// 绑定实际仓储的异步缓存宏，可在回调中继续调用该仓储。
#[derive(Clone)]
pub struct CacheMacro(Arc<Callback>);

impl CacheMacro {
    pub fn new(
        callback: impl for<'a> Fn(&'a Repository, Vec<Value>) -> CacheFuture<'a, CacheResult<Value>>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self(Arc::new(callback))
    }
}

#[derive(Default)]
pub(super) struct CacheMacros(RwLock<HashMap<String, CacheMacro>>);

impl Repository {
    pub(super) fn with_macros(mut self, macros: Arc<CacheMacros>) -> Self {
        self.macros = macros;
        self
    }

    pub fn r#macro(
        &self,
        name: impl Into<String>,
        callback: impl for<'a> Fn(&'a Repository, Vec<Value>) -> CacheFuture<'a, CacheResult<Value>>
        + Send
        + Sync
        + 'static,
    ) {
        self.mixin([(name, CacheMacro::new(callback))], true);
    }

    pub fn mixin<K: Into<String>>(
        &self,
        macros: impl IntoIterator<Item = (K, CacheMacro)>,
        replace: bool,
    ) {
        use std::collections::hash_map::Entry;
        for (name, callback) in macros {
            let mut registered = self
                .macros
                .0
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let retired = match registered.entry(name.into()) {
                Entry::Vacant(entry) => {
                    entry.insert(callback);
                    None
                }
                Entry::Occupied(mut entry) if replace => Some(entry.insert(callback)),
                Entry::Occupied(_) => Some(callback),
            };
            drop(registered);
            drop(retired);
        }
    }

    pub fn has_macro(&self, name: &str) -> bool {
        self.macros
            .0
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains_key(name)
    }

    pub fn flush_macros(&self) {
        let retired = std::mem::take(
            &mut *self
                .macros
                .0
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        drop(retired);
    }

    pub async fn macro_call(&self, name: &str, arguments: Vec<Value>) -> CacheResult<Value> {
        let callback = self
            .macros
            .0
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(name)
            .cloned()
            .ok_or_else(|| CacheError::UnknownMacro {
                name: name.to_owned(),
            })?;
        (callback.0)(self, arguments).await
    }
}

impl Cache {
    pub fn r#macro(
        name: impl Into<String>,
        callback: impl for<'a> Fn(&'a Repository, Vec<Value>) -> CacheFuture<'a, CacheResult<Value>>
        + Send
        + Sync
        + 'static,
    ) -> CacheResult<()> {
        Self::store(None)?.r#macro(name, callback);
        Ok(())
    }
    pub fn mixin<K: Into<String>>(
        macros: impl IntoIterator<Item = (K, CacheMacro)>,
        replace: bool,
    ) -> CacheResult<()> {
        Self::store(None)?.mixin(macros, replace);
        Ok(())
    }
    pub fn has_macro(name: &str) -> CacheResult<bool> {
        Ok(Self::store(None)?.has_macro(name))
    }
    pub fn flush_macros() -> CacheResult<()> {
        Self::store(None)?.flush_macros();
        Ok(())
    }
    pub async fn macro_call(name: &str, arguments: Vec<Value>) -> CacheResult<Value> {
        Self::store(None)?.macro_call(name, arguments).await
    }
}
