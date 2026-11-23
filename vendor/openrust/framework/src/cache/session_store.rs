use std::sync::{Arc, Weak};

use serde::Deserialize;

use crate::{
    container::Container,
    http::request_context,
    session::{CacheAttributes, get_attribute, insert_attribute, remove_attribute},
};

use super::{
    CacheError, CacheResult,
    engine::{Change, Engine, Record, now},
    store::StoreCore,
};

/// Stores cache items in the current HTTP session.
pub struct SessionStore(StoreCore, Arc<SessionBackend>);

super::store::impl_store!(SessionStore);

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl SessionStore {
    pub fn new(application: Weak<Container>, namespace: String) -> CacheResult<Self> {
        if namespace.is_empty() {
            return Err(CacheError::InvalidConfiguration);
        }
        let backend = Arc::new(SessionBackend {
            application,
            namespace,
        });
        Ok(Self(
            StoreCore::without_locks(Engine::Session(backend.clone())),
            backend,
        ))
    }

    /// Returns the stored tree of cache entries, including expiry metadata.
    pub fn all(&self) -> CacheResult<serde_json::Value> {
        let attributes = self.1.attributes()?;
        let attributes = attributes.lock().expect("session cache poisoned");
        Ok(get_attribute(&attributes, &self.1.namespace)
            .cloned()
            .unwrap_or_else(|| serde_json::Value::Object(serde_json::Map::new())))
    }

    pub fn itemKey(&self, key: &str) -> String {
        self.1.item_key(key)
    }

    pub fn getPrefix(&self) -> &str {
        ""
    }
}

pub(super) struct SessionBackend {
    application: Weak<Container>,
    namespace: String,
}

impl SessionBackend {
    fn attributes(&self) -> CacheResult<CacheAttributes> {
        self.application
            .upgrade()
            .and_then(|owner| request_context::session_cache(&owner))
            .ok_or(CacheError::SessionUnavailable)
    }

    fn item_key(&self, key: &str) -> String {
        format!("{}.{key}", self.namespace)
    }

    pub(super) fn forget(&self, key: &str) -> CacheResult<bool> {
        let attributes = self.attributes()?;
        let mut attributes = attributes.lock().expect("session cache poisoned");
        Ok(remove_attribute(&mut attributes, &self.item_key(key)).is_some())
    }

    pub(super) fn change<T>(
        &self,
        key: &str,
        edit: impl FnOnce(Option<Record>) -> CacheResult<(Change, T)>,
    ) -> CacheResult<T> {
        let attributes = self.attributes()?;
        let mut attributes = attributes.lock().expect("session cache poisoned");
        let key = self.item_key(key);
        let mut record: Option<Record> =
            get_attribute(&attributes, &key).and_then(|value| Record::deserialize(value).ok());
        let time = now()?;
        if record.as_ref().is_some_and(|record| record.expires <= time) {
            remove_attribute(&mut attributes, &key);
            record = None;
        }
        let (change, result) = edit(record)?;
        match change {
            Change::Keep => {}
            Change::Write(record) => {
                let value = serde_json::to_value(record).map_err(super::backend)?;
                insert_attribute(&mut attributes, &key, value);
            }
            Change::Remove => {
                remove_attribute(&mut attributes, &key);
            }
        }
        Ok(result)
    }

    pub(super) fn flush(&self) -> CacheResult<()> {
        let attributes = self.attributes()?;
        let mut attributes = attributes.lock().expect("session cache poisoned");
        remove_attribute(&mut attributes, &self.namespace);
        Ok(())
    }
}
