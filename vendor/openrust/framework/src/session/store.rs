use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use serde_json::Map;

use crate::contracts::session::{Session, SessionHandler};

use super::{SessionError, SessionFuture, SessionResult, Value};

type Attributes = BTreeMap<String, Value>;
pub(crate) type CacheAttributes = Arc<Mutex<Attributes>>;

// Session's attributes API lends references. Cache values are shared separately
// with the request's repository and saved in the same session payload.
const CACHE_PAYLOAD_KEY: &str = "\0session.cache";

pub(super) const MAX_PAYLOAD_BYTES: usize = 1024 * 1024;
const ID_LENGTH: usize = 40;
pub(crate) const PASSWORD_CONFIRMED_AT: &str = "auth.password_confirmed_at";

pub(super) fn valid_id(id: &str) -> bool {
    id.len() == ID_LENGTH && id.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

pub(crate) fn unix_seconds() -> i64 {
    crate::support::timestamp_seconds(jiff::Timestamp::now())
}

/// The state of one HTTP session, with a request-owned storage handler.
pub struct Store {
    name: String,
    id: String,
    handler: Box<dyn SessionHandler>,
    attributes: Attributes,
    cache: CacheAttributes,
    started: bool,
}

impl Store {
    pub fn new(name: impl Into<String>, handler: Box<dyn SessionHandler>) -> SessionResult<Self> {
        let mut store = Self {
            name: String::new(),
            id: random_id()?,
            handler,
            attributes: Attributes::new(),
            cache: Arc::default(),
            started: false,
        };
        store.setName(&name.into())?;
        Ok(store)
    }

    /// Returns the cache repository for the current HTTP session.
    pub fn cache(&self) -> crate::cache::CacheResult<Arc<crate::cache::Repository>> {
        use crate::{
            cache::CacheError, http::request_context, support::facades::current_container,
        };

        let active = current_container()
            .and_then(|owner| request_context::session_cache(&owner))
            .filter(|cache| Arc::ptr_eq(cache, &self.cache));
        active.ok_or(CacheError::SessionUnavailable)?;
        crate::cache::Cache::store(Some("session"))
    }

    pub(crate) fn cache_attributes(&self) -> &CacheAttributes {
        &self.cache
    }

    pub async fn gc(&mut self, lifetime: std::time::Duration) -> SessionResult<Option<u64>> {
        self.handler.gc(lifetime).await
    }

    /// Flashes input so it is available on the following request.
    #[allow(non_snake_case, reason = "Matches Laravel's Session API")]
    pub fn flashInput(&mut self, input: Value) -> SessionResult<()> {
        self.flash("_old_input", input)
    }

    /// Retrieves all old input or one nested field.
    #[allow(non_snake_case, reason = "Matches Laravel's Session API")]
    pub fn getOldInput(&self, key: Option<&str>) -> Option<&Value> {
        let input = self.get("_old_input")?;
        match key {
            Some(key) => crate::support::Arr::get(input, key),
            None => Some(input),
        }
    }

    /// Checks for non-null old input at a key, or any entries in the old input bag.
    #[allow(non_snake_case, reason = "Matches Laravel's Session API")]
    pub fn hasOldInput(&self, key: Option<&str>) -> bool {
        match key {
            Some(key) => self
                .getOldInput(Some(key))
                .is_some_and(|value| !value.is_null()),
            None => match self.getOldInput(None) {
                Some(Value::Array(items)) => !items.is_empty(),
                Some(Value::Object(items)) => !items.is_empty(),
                _ => false,
            },
        }
    }

    /// Flashes one error bag while preserving other bags from this request.
    #[allow(non_snake_case, reason = "Matches Laravel's error bag conventions")]
    pub fn flashErrors(
        &mut self,
        bag: &str,
        messages: &crate::support::MessageBag,
    ) -> SessionResult<()> {
        let mut errors = self
            .get("errors")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        errors.insert(bag.to_owned(), serde_json::json!(messages.messages()));
        self.flash("errors", Value::Object(errors))
    }

    #[allow(non_snake_case, reason = "Matches Laravel's Session API")]
    pub fn previousUrl(&self) -> Option<&str> {
        self.get("_previous.url").and_then(Value::as_str)
    }

    #[allow(non_snake_case, reason = "Matches Laravel's Session API")]
    pub fn setPreviousUrl(&mut self, url: &str) -> SessionResult<()> {
        self.put("_previous.url", url.into())
    }

    #[allow(non_snake_case, reason = "Matches Laravel's Session API")]
    pub fn previousUri(&self) -> Result<Option<crate::support::Uri>, crate::support::InvalidUri> {
        self.previousUrl().map(crate::support::Uri::of).transpose()
    }

    #[allow(non_snake_case, reason = "Matches Laravel's Session API")]
    pub fn previousRoute(&self) -> Option<&str> {
        self.get("_previous.route").and_then(Value::as_str)
    }

    #[allow(non_snake_case, reason = "Matches Laravel's Session API")]
    pub fn setPreviousRoute(&mut self, name: Option<&str>) -> SessionResult<()> {
        self.put("_previous.route", name.into())
    }

    /// Records that the password for this session was confirmed.
    #[allow(non_snake_case, reason = "Matches Laravel's Session API")]
    pub fn passwordConfirmed(&mut self) -> SessionResult<()> {
        self.put(PASSWORD_CONFIRMED_AT, unix_seconds().into())
    }

    fn flash_keys(&self, key: &str) -> Vec<String> {
        flash_keys(&self.attributes, key)
    }

    fn set_flash(&mut self, key: &str, keys: Vec<String>) {
        insert(
            &mut self.attributes,
            key,
            Value::Array(keys.into_iter().map(Value::String).collect()),
        );
    }
}

impl Session for Store {
    fn getName(&self) -> &str {
        &self.name
    }

    fn setName(&mut self, name: &str) -> SessionResult<()> {
        self.name = name.to_owned();
        Ok(())
    }

    fn getId(&self) -> &str {
        &self.id
    }

    fn setId(&mut self, id: Option<&str>) -> SessionResult<()> {
        self.id = match id.filter(|id| valid_id(id)) {
            Some(id) => id.to_owned(),
            None => random_id()?,
        };
        Ok(())
    }

    fn isStarted(&self) -> bool {
        self.started
    }

    fn start(&mut self) -> SessionFuture<'_, SessionResult<()>> {
        Box::pin(async move {
            if self.started {
                return Ok(());
            }
            if let Some(payload) = self.handler.read(&self.id).await? {
                if payload.len() > MAX_PAYLOAD_BYTES {
                    return Err(SessionError::PayloadTooLarge);
                }
                if let Ok(mut attributes) = serde_json::from_slice::<Attributes>(&payload) {
                    if let Some(Value::Object(cache)) = attributes.remove(CACHE_PAYLOAD_KEY) {
                        self.cache
                            .lock()
                            .expect("session cache poisoned")
                            .extend(cache);
                    }
                    self.attributes.extend(attributes);
                }
            }
            if self.token().is_none() {
                self.regenerateToken()?;
            }
            self.started = true;
            Ok(())
        })
    }

    fn save(&mut self) -> SessionFuture<'_, SessionResult<()>> {
        Box::pin(async move {
            let mut attributes = self.attributes.clone();
            for key in flash_keys(&attributes, "_flash.old") {
                remove(&mut attributes, &key);
            }
            let new = flash_keys(&attributes, "_flash.new");
            insert(
                &mut attributes,
                "_flash.old",
                Value::Array(new.into_iter().map(Value::String).collect()),
            );
            insert(&mut attributes, "_flash.new", Value::Array(Vec::new()));
            let cache = {
                let cache = self.cache.lock().expect("session cache poisoned");
                (!cache.is_empty()).then(|| Value::Object(cache.clone().into_iter().collect()))
            };
            if let Some(cache) = cache {
                attributes.insert(CACHE_PAYLOAD_KEY.into(), cache);
            }
            let payload = serde_json::to_vec(&attributes).map_err(|_| SessionError::Encoding)?;
            if payload.len() > MAX_PAYLOAD_BYTES {
                return Err(SessionError::PayloadTooLarge);
            }
            self.handler.write(&self.id, &payload).await?;
            attributes.remove(CACHE_PAYLOAD_KEY);
            self.attributes = attributes;
            self.started = false;
            Ok(())
        })
    }

    fn all(&self) -> &BTreeMap<String, Value> {
        &self.attributes
    }
    fn exists(&self, key: &str) -> bool {
        self.get(key).is_some()
    }
    fn has(&self, key: &str) -> bool {
        self.get(key).is_some_and(|value| !value.is_null())
    }

    fn get(&self, key: &str) -> Option<&Value> {
        get(&self.attributes, key)
    }

    fn pull(&mut self, key: &str) -> Option<Value> {
        remove(&mut self.attributes, key)
    }

    fn put(&mut self, key: &str, value: Value) -> SessionResult<()> {
        insert(&mut self.attributes, key, value);
        Ok(())
    }

    fn forget(&mut self, keys: &[&str]) {
        for key in keys {
            self.pull(key);
        }
    }

    fn flush(&mut self) {
        self.attributes.clear();
        self.cache.lock().expect("session cache poisoned").clear();
    }

    fn flash(&mut self, key: &str, value: Value) -> SessionResult<()> {
        self.put(key, value)?;
        self.keep(&[key]);
        Ok(())
    }

    fn reflash(&mut self) {
        let old = self.flash_keys("_flash.old");
        self.keep(&old.iter().map(String::as_str).collect::<Vec<_>>());
    }

    fn keep(&mut self, keys: &[&str]) {
        let mut new = self.flash_keys("_flash.new");
        for key in keys {
            if !new.iter().any(|existing| existing == key) {
                new.push((*key).to_owned());
            }
        }
        let mut old = self.flash_keys("_flash.old");
        old.retain(|key| !keys.contains(&key.as_str()));
        self.set_flash("_flash.new", new);
        self.set_flash("_flash.old", old);
    }

    fn token(&self) -> Option<&str> {
        self.get("_token").and_then(Value::as_str)
    }

    fn regenerateToken(&mut self) -> SessionResult<()> {
        self.put("_token", random_id()?.into())
    }

    fn migrate(&mut self, destroy: bool) -> SessionFuture<'_, SessionResult<()>> {
        Box::pin(async move {
            let next = random_id()?;
            if destroy {
                self.handler.destroy(&self.id).await?;
            }
            self.id = next;
            Ok(())
        })
    }

    fn regenerate(&mut self, destroy: bool) -> SessionFuture<'_, SessionResult<()>> {
        Box::pin(async move {
            let token = random_id()?;
            self.migrate(destroy).await?;
            self.put("_token", token.into())
        })
    }

    fn invalidate(&mut self) -> SessionFuture<'_, SessionResult<()>> {
        Box::pin(async move {
            self.migrate(true).await?;
            self.flush();
            Ok(())
        })
    }
}

pub(crate) fn get<'a>(attributes: &'a BTreeMap<String, Value>, key: &str) -> Option<&'a Value> {
    attributes.get(key).or_else(|| {
        let (root, rest) = key.split_once('.')?;
        rest.split('.').try_fold(attributes.get(root)?, child)
    })
}

fn flash_keys(attributes: &BTreeMap<String, Value>, key: &str) -> Vec<String> {
    get(attributes, key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

fn child<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    match value {
        Value::Object(map) => map.get(key),
        Value::Array(items) => items.get(key.parse::<usize>().ok()?),
        _ => None,
    }
}

pub(crate) fn insert(attributes: &mut BTreeMap<String, Value>, key: &str, value: Value) {
    if let Some((root, rest)) = key.split_once('.') {
        let mut parent = attributes.entry(root.to_owned()).or_insert(Value::Null);
        let mut segments = rest.split('.').peekable();
        while let Some(segment) = segments.next() {
            if !parent.is_object() {
                // JSON arrays keep their numeric keys when changed by a dotted path.
                *parent = match std::mem::take(parent) {
                    Value::Array(items) => Value::Object(
                        items
                            .into_iter()
                            .enumerate()
                            .map(|(index, value)| (index.to_string(), value))
                            .collect(),
                    ),
                    _ => Value::Object(Map::new()),
                };
            }
            let map = parent.as_object_mut().expect("object created above");
            if segments.peek().is_none() {
                map.insert(segment.to_owned(), value);
                break;
            }
            parent = map.entry(segment.to_owned()).or_insert(Value::Null);
        }
    } else {
        attributes.insert(key.to_owned(), value);
    }
}

pub(crate) fn remove(attributes: &mut BTreeMap<String, Value>, key: &str) -> Option<Value> {
    if let Some(value) = attributes.remove(key) {
        return Some(value);
    }
    let (root, rest) = key.split_once('.')?;
    let mut parent = attributes.get_mut(root)?;
    let mut segments = rest.split('.').peekable();
    while let Some(segment) = segments.next() {
        let last = segments.peek().is_none();
        match parent {
            Value::Object(map) if last => return map.remove(segment),
            Value::Array(items) if last => {
                let index = segment.parse::<usize>().ok()?;
                if index >= items.len() {
                    return None;
                }
                let mut indexed: Map<_, _> = std::mem::take(items)
                    .into_iter()
                    .enumerate()
                    .map(|(index, value)| (index.to_string(), value))
                    .collect();
                let removed = indexed.remove(segment);
                *parent = Value::Object(indexed);
                return removed;
            }
            Value::Object(map) => parent = map.get_mut(segment)?,
            Value::Array(items) => parent = items.get_mut(segment.parse::<usize>().ok()?)?,
            _ => return None,
        }
    }
    None
}

fn random_id() -> SessionResult<String> {
    const ALPHANUMERIC: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut id = String::with_capacity(ID_LENGTH);
    let mut bytes = [0u8; 64];
    while id.len() < ID_LENGTH {
        aws_lc_rs::rand::fill(&mut bytes).map_err(|_| SessionError::Random)?;
        for byte in bytes
            .into_iter()
            .filter(|byte| *byte < 248)
            .take(ID_LENGTH - id.len())
        {
            id.push(char::from(
                ALPHANUMERIC[usize::from(byte) % ALPHANUMERIC.len()],
            ));
        }
    }
    Ok(id)
}
