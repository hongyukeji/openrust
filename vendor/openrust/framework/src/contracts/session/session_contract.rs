use std::collections::BTreeMap;

use crate::session::{SessionFuture, SessionResult, Value};

/// Accesses the state loaded for an HTTP request.
pub trait Session: Send + Sync {
    fn get_name(&self) -> &str;
    fn set_name(&mut self, name: &str) -> SessionResult<()>;
    fn get_id(&self) -> &str;
    fn set_id(&mut self, id: Option<&str>) -> SessionResult<()>;
    fn is_started(&self) -> bool;
    fn start(&mut self) -> SessionFuture<'_, SessionResult<()>>;
    fn save(&mut self) -> SessionFuture<'_, SessionResult<()>>;

    fn all(&self) -> &BTreeMap<String, Value>;
    fn exists(&self, key: &str) -> bool;
    fn has(&self, key: &str) -> bool;
    fn get(&self, key: &str) -> Option<&Value>;
    fn pull(&mut self, key: &str) -> Option<Value>;
    fn put(&mut self, key: &str, value: Value) -> SessionResult<()>;
    fn forget(&mut self, keys: &[&str]);
    fn flush(&mut self);

    fn flash(&mut self, key: &str, value: Value) -> SessionResult<()>;
    fn reflash(&mut self);
    fn keep(&mut self, keys: &[&str]);

    fn token(&self) -> Option<&str>;
    fn regenerate_token(&mut self) -> SessionResult<()>;
    fn migrate(&mut self, destroy: bool) -> SessionFuture<'_, SessionResult<()>>;
    fn regenerate(&mut self, destroy: bool) -> SessionFuture<'_, SessionResult<()>>;
    fn invalidate(&mut self) -> SessionFuture<'_, SessionResult<()>>;
}
