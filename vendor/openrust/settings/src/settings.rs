use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::Arc,
};

use openrust::{
    contracts::{config::Repository as Config, encryption::Encrypter},
    database::Transaction,
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Map, Value};
use tokio::sync::Mutex;

use crate::database_settings_repository::{MAX_PROPERTIES, check_payload, identifier, property};
use crate::{
    SettingsError as Error, SettingsRepository, SettingsResult, SettingsSnapshot, SettingsUpdate,
};

/// Application-defined, stable JSON object schema. Never use conditional field omission.
/// Validation errors must not contain property values or credentials.
pub trait SettingsGroup: Serialize + DeserializeOwned + Clone + Send + Sync + 'static {
    fn group() -> &'static str;
    fn defaults(config: &dyn Config) -> SettingsResult<Self>;
    fn encrypted() -> &'static [&'static str] {
        &[]
    }
    fn validate(&self) -> SettingsResult<()> {
        Ok(())
    }
}

/// A request/job scoped typed settings instance. It deliberately cannot be serialized.
/// `get` returns trusted internal data; HTTP responses need an explicit public projection.
pub struct Settings<T: SettingsGroup> {
    repository: Arc<dyn SettingsRepository>,
    encrypter: Option<Arc<dyn Encrypter>>,
    defaults: Map<String, Value>,
    state: Mutex<State<T>>,
}
struct State<T> {
    values: T,
    snapshot: SettingsSnapshot,
    changes: BTreeMap<String, SettingsUpdate>,
}
impl<T: SettingsGroup> fmt::Debug for Settings<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Settings")
            .field("group", &T::group())
            .finish_non_exhaustive()
    }
}

impl<T: SettingsGroup> Settings<T> {
    pub async fn load(
        repository: Arc<dyn SettingsRepository>,
        config: &dyn Config,
        encrypter: Option<Arc<dyn Encrypter>>,
    ) -> SettingsResult<Self> {
        if !identifier(T::group()) {
            return Err(Error::InvalidDefinition);
        }
        let defaults = object(&T::defaults(config)?)?;
        if defaults.is_empty()
            || defaults.len() > MAX_PROPERTIES
            || defaults.keys().any(|k| !property(k))
        {
            return Err(Error::InvalidDefinition);
        }
        let unique: BTreeSet<_> = defaults
            .keys()
            .map(|key| key.to_ascii_lowercase())
            .collect();
        if unique.len() != defaults.len() {
            return Err(Error::InvalidDefinition);
        }
        let encrypted: BTreeSet<_> = T::encrypted().iter().copied().collect();
        if encrypted.len() != T::encrypted().len()
            || encrypted.iter().any(|key| !defaults.contains_key(*key))
        {
            return Err(Error::InvalidDefinition);
        }
        if !encrypted.is_empty() && encrypter.is_none() {
            return Err(Error::Encryption);
        }
        let snapshot = repository.load(T::group()).await?;
        let values = decode::<T>(&defaults, &snapshot, encrypter.as_deref())?;
        Ok(Self {
            repository,
            encrypter,
            defaults,
            state: Mutex::new(State {
                values,
                snapshot,
                changes: BTreeMap::new(),
            }),
        })
    }

    pub async fn get(&self) -> T {
        self.state.lock().await.values.clone()
    }

    /// Replaces declared top-level properties. Nested objects/arrays are replaced whole.
    /// Backend field patches must be validated and merged explicitly by the application.
    pub async fn fill(&self, properties: Value) -> SettingsResult<()> {
        let Value::Object(properties) = properties else {
            return Err(Error::InvalidValue);
        };
        let mut state = self.state.lock().await;
        let mut values = object(&state.values)?;
        for (name, value) in &properties {
            if !self.defaults.contains_key(name) {
                return Err(Error::UnknownProperty);
            }
            if state
                .snapshot
                .properties
                .get(name)
                .is_some_and(|p| p.locked)
            {
                return Err(Error::Locked);
            }
            check_payload(value)?;
            values.insert(name.clone(), value.clone());
        }
        let candidate = validated::<T>(values)?;
        // Mutate only after every field and the complete candidate pass validation.
        state.values = candidate;
        for (name, value) in properties {
            state.changes.insert(name, SettingsUpdate::Set(value));
        }
        Ok(())
    }

    pub async fn reset(&self, name: &str) -> SettingsResult<()> {
        let default = self.defaults.get(name).ok_or(Error::UnknownProperty)?;
        let mut state = self.state.lock().await;
        if state
            .snapshot
            .properties
            .get(name)
            .is_some_and(|p| p.locked)
        {
            return Err(Error::Locked);
        }
        let mut values = object(&state.values)?;
        values.insert(name.into(), default.clone());
        let candidate = validated::<T>(values)?;
        state.values = candidate;
        state.changes.insert(name.into(), SettingsUpdate::Reset);
        Ok(())
    }

    /// Saves validated changes and updates this instance only after successful commit.
    /// Concurrent stale editors receive Conflict and must refresh explicitly.
    pub async fn save(&self) -> SettingsResult<()> {
        let mut state = self.state.lock().await;
        if state.changes.is_empty() {
            return Ok(());
        }
        state.values.validate()?;
        let changes = self.encode(&state.changes)?;
        let snapshot = self
            .repository
            .save(T::group(), &state.snapshot, &changes)
            .await?;
        state.snapshot = snapshot;
        state.changes.clear();
        Ok(())
    }

    /// Writes in a caller-owned transaction. It leaves this instance unpublished.
    /// After outer commit call refresh; after rollback pending edits remain retryable.
    /// The transaction must target this repository's database and migrated table.
    pub async fn save_in(&self, transaction: &mut Transaction) -> SettingsResult<()> {
        let state = self.state.lock().await;
        if state.changes.is_empty() {
            return Ok(());
        }
        state.values.validate()?;
        let changes = self.encode(&state.changes)?;
        self.repository
            .save_in(transaction, T::group(), &state.snapshot, &changes)
            .await?;
        Ok(())
    }

    /// Explicitly discards local edits. A failed load preserves the current instance.
    pub async fn refresh(&self) -> SettingsResult<()> {
        let mut state = self.state.lock().await;
        let snapshot = self.repository.load(T::group()).await?;
        let values = decode::<T>(&self.defaults, &snapshot, self.encrypter.as_deref())?;
        state.values = values;
        state.snapshot = snapshot;
        state.changes.clear();
        Ok(())
    }

    pub async fn is_locked(&self, name: &str) -> SettingsResult<bool> {
        if !self.defaults.contains_key(name) {
            return Err(Error::UnknownProperty);
        }
        Ok(self
            .state
            .lock()
            .await
            .snapshot
            .properties
            .get(name)
            .is_some_and(|p| p.locked))
    }

    /// Trusted code may lock persisted properties; save pending edits first.
    pub async fn lock(&self, name: &str) -> SettingsResult<()> {
        self.set_locked(name, true).await
    }
    pub async fn unlock(&self, name: &str) -> SettingsResult<()> {
        self.set_locked(name, false).await
    }
    async fn set_locked(&self, name: &str, locked: bool) -> SettingsResult<()> {
        if !self.defaults.contains_key(name) {
            return Err(Error::UnknownProperty);
        }
        let mut state = self.state.lock().await;
        if !state.changes.is_empty() {
            return Err(Error::InvalidValue);
        }
        let changes = BTreeMap::from([(
            name.into(),
            if locked {
                SettingsUpdate::Lock
            } else {
                SettingsUpdate::Unlock
            },
        )]);
        state.snapshot = self
            .repository
            .save(T::group(), &state.snapshot, &changes)
            .await?;
        Ok(())
    }

    /// Rewrites persisted secrets with the current encrypter key. Unlock them first.
    /// Missing values stay absent. No secret is returned or written to logs.
    pub async fn re_encrypt(&self) -> SettingsResult<()> {
        let mut state = self.state.lock().await;
        if !state.changes.is_empty() {
            return Err(Error::InvalidValue);
        }
        let values = object(&state.values)?;
        let mut changes = BTreeMap::new();
        for name in T::encrypted() {
            if let Some(property) = state.snapshot.properties.get(*name) {
                if property.locked {
                    return Err(Error::Locked);
                }
                changes.insert((*name).into(), SettingsUpdate::Set(values[*name].clone()));
            }
        }
        if changes.is_empty() {
            return Ok(());
        }
        let changes = self.encode(&changes)?;
        state.snapshot = self
            .repository
            .save(T::group(), &state.snapshot, &changes)
            .await?;
        Ok(())
    }

    fn encode(
        &self,
        changes: &BTreeMap<String, SettingsUpdate>,
    ) -> SettingsResult<BTreeMap<String, SettingsUpdate>> {
        changes
            .iter()
            .map(|(name, update)| {
                let update = match update {
                    SettingsUpdate::Set(value) if T::encrypted().contains(&name.as_str()) => {
                        let plaintext =
                            serde_json::to_vec(value).map_err(|_| Error::InvalidValue)?;
                        let ciphertext = self
                            .encrypter
                            .as_ref()
                            .ok_or(Error::Encryption)?
                            .encrypt_string(&plaintext)
                            .map_err(|_| Error::Encryption)?;
                        SettingsUpdate::Set(Value::String(ciphertext))
                    }
                    update => update.clone(),
                };
                Ok((name.clone(), update))
            })
            .collect()
    }
}

fn object(value: &impl Serialize) -> SettingsResult<Map<String, Value>> {
    let Value::Object(properties) = serde_json::to_value(value).map_err(|_| Error::InvalidValue)?
    else {
        return Err(Error::InvalidDefinition);
    };
    check_payload(&Value::Object(properties.clone()))?;
    Ok(properties)
}
fn validated<T: SettingsGroup>(values: Map<String, Value>) -> SettingsResult<T> {
    let values: T =
        serde_json::from_value(Value::Object(values)).map_err(|_| Error::InvalidValue)?;
    values.validate()?;
    Ok(values)
}
fn decode<T: SettingsGroup>(
    defaults: &Map<String, Value>,
    snapshot: &SettingsSnapshot,
    encrypter: Option<&dyn Encrypter>,
) -> SettingsResult<T> {
    let mut values = defaults.clone();
    // Older applications ignore newer properties while repository CAS preserves them.
    for (name, property) in &snapshot.properties {
        if !defaults.contains_key(name) {
            continue;
        }
        let value = if T::encrypted().contains(&name.as_str()) {
            let ciphertext = property.payload.as_str().ok_or(Error::Decryption)?;
            let bytes = encrypter
                .ok_or(Error::Decryption)?
                .decrypt_string(ciphertext)
                .map_err(|_| Error::Decryption)?;
            serde_json::from_slice(&bytes).map_err(|_| Error::Decryption)?
        } else {
            property.payload.clone()
        };
        values.insert(name.clone(), value);
    }
    validated::<T>(values)
}
