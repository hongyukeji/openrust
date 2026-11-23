use crate::{Error, Result};
use openrust::{
    container::ServiceKey, contracts::container::BindingResolutionError, queue::Payload,
    support::facades::Facade,
};
use std::sync::RwLock;

pub type TagCallback = fn(&Payload) -> Vec<String>;
#[derive(Default)]
pub(crate) struct TagManager {
    callbacks: RwLock<Vec<TagCallback>>,
}
struct TagStore;
impl Facade for TagStore {
    type Service = TagManager;
    fn get_facade_accessor() -> ServiceKey {
        ServiceKey::typed::<TagManager>()
    }
}
/// Explicit job tags are resolved in the current application scope.
pub struct Tags;
impl Tags {
    pub fn forPayload(payload: &Payload) -> Result<Vec<String>> {
        let manager = TagStore::get_facade_root().map_err(|_| Error::Unavailable)?;
        let mut tags = Vec::new();
        for callback in manager
            .callbacks
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
        {
            for tag in callback(payload) {
                if !tag.is_empty() && !tags.contains(&tag) {
                    tags.push(tag);
                }
            }
        }
        Ok(tags)
    }
    pub fn forModel<M: openrust::database::eloquent::Model>(model: &M) -> Result<String> {
        let key = match model.get_key().ok_or(Error::InvalidRecord)? {
            openrust::database::Value::Text(value) => value.clone(),
            openrust::database::Value::Integer(value) => value.to_string(),
            openrust::database::Value::Unsigned(value) => value.to_string(),
            _ => return Err(Error::InvalidRecord),
        };
        Ok(format!("{}:{}", std::any::type_name::<M>(), key))
    }
}
impl crate::Horizon {
    pub fn tag(callback: TagCallback) -> std::result::Result<(), BindingResolutionError> {
        TagStore::get_facade_root()?
            .callbacks
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(callback);
        Ok(())
    }
}
