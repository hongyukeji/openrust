use std::collections::BTreeMap;

use openrust::database::{Connection, Row, Transaction, Value, query::Record};
use serde_json::Value as Json;
use uuid::Uuid;

use crate::{
    SettingsError as Error, SettingsFuture, SettingsRepository, SettingsResult, SettingsSnapshot,
    SettingsUpdate, StoredProperty,
};

// A reserved property provides a stable row lock even when every override is reset.
const VERSION: &str = "__settings_version";
pub(crate) const MAX_PROPERTIES: usize = 256;
pub(crate) const MAX_BYTES: usize = 1_048_576;

#[derive(Clone)]
pub struct DatabaseSettingsRepository {
    connection: Connection,
    table: String,
}
impl DatabaseSettingsRepository {
    pub fn new(connection: Connection) -> Self {
        Self {
            connection,
            table: "settings".into(),
        }
    }

    pub fn with_table(mut self, table: impl Into<String>) -> SettingsResult<Self> {
        let table = table.into();
        if !identifier(&table) {
            return Err(Error::InvalidDefinition);
        }
        self.table = table;
        Ok(self)
    }

    async fn apply(
        &self,
        tx: &mut Transaction,
        group: &str,
        expected: &SettingsSnapshot,
        changes: &BTreeMap<String, SettingsUpdate>,
    ) -> SettingsResult<SettingsSnapshot> {
        if !identifier(group) || changes.keys().any(|name| !property(name)) {
            return Err(Error::InvalidDefinition);
        }
        if changes.len() > MAX_PROPERTIES {
            return Err(Error::LimitExceeded);
        }
        // A no-op UPSERT acquires the write lock even for existing groups. In
        // particular, MySQL INSERT IGNORE's shared lock must not be upgraded by
        // competing writers. It also acquires SQLite's write lock before reading.
        tx.table(&self.table)
            .upsert(
                &[record(group, VERSION, Json::Null, true)],
                &["group", "name"],
                Some(&["name"]),
            )
            .await
            .map_err(|_| Error::Database)?;
        let rows = tx
            .table(&self.table)
            .r#where("group", "=", group)
            .order_by("name", "asc")
            .lock_for_update()
            .limit((MAX_PROPERTIES + 2) as u64)
            .get()
            .await
            .map_err(|_| Error::Database)?;
        let mut current = snapshot(rows, group)?;
        if &current != expected {
            return Err(Error::Conflict);
        }
        for (name, change) in changes {
            let existing = current.properties.get(name);
            match change {
                SettingsUpdate::Set(payload) => {
                    if existing.is_some_and(|p| p.locked) {
                        return Err(Error::Locked);
                    }
                    check_payload(payload)?;
                    current.properties.insert(
                        name.clone(),
                        StoredProperty {
                            payload: payload.clone(),
                            locked: false,
                        },
                    );
                }
                SettingsUpdate::Reset => {
                    if existing.is_some_and(|p| p.locked) {
                        return Err(Error::Locked);
                    }
                    current.properties.remove(name);
                }
                SettingsUpdate::Lock | SettingsUpdate::Unlock => {
                    let p = current
                        .properties
                        .get_mut(name)
                        .ok_or(Error::UnknownProperty)?;
                    p.locked = matches!(change, SettingsUpdate::Lock);
                }
            }
        }
        if current.properties.len() > MAX_PROPERTIES {
            return Err(Error::LimitExceeded);
        }
        let unique: std::collections::BTreeSet<_> = current
            .properties
            .keys()
            .map(|key| key.to_ascii_lowercase())
            .collect();
        if unique.len() != current.properties.len() {
            return Err(Error::InvalidDefinition);
        }
        let bytes = current.properties.values().try_fold(0usize, |total, p| {
            let length = serde_json::to_vec(&p.payload)
                .map_err(|_| Error::InvalidValue)?
                .len();
            Ok::<_, Error>(total.saturating_add(length))
        })?;
        if bytes > MAX_BYTES - 64 {
            return Err(Error::LimitExceeded);
        }
        // Validation above completes before any property mutation; the surrounding
        // transaction/savepoint also rolls back the newly inserted lock row on error.
        for (name, change) in changes {
            if matches!(change, SettingsUpdate::Reset) {
                tx.table(&self.table)
                    .r#where("group", "=", group)
                    .r#where("name", "=", name.as_str())
                    .delete()
                    .await
                    .map_err(|_| Error::Database)?;
            } else {
                let p = &current.properties[name];
                tx.table(&self.table)
                    .upsert(
                        &[record(group, name, p.payload.clone(), p.locked)],
                        &["group", "name"],
                        Some(&["payload", "locked", "updated_at"]),
                    )
                    .await
                    .map_err(|_| Error::Database)?;
            }
        }
        let version = Uuid::new_v4().to_string();
        tx.table(&self.table)
            .r#where("group", "=", group)
            .r#where("name", "=", VERSION)
            .update(record_values(Json::String(version.clone()), true))
            .await
            .map_err(|_| Error::Database)?;
        current.version = Some(version);
        Ok(current)
    }
}

impl SettingsRepository for DatabaseSettingsRepository {
    fn load<'a>(&'a self, group: &'a str) -> SettingsFuture<'a, SettingsSnapshot> {
        Box::pin(async move {
            if !identifier(group) {
                return Err(Error::InvalidDefinition);
            }
            let rows = self
                .connection
                .table(&self.table)
                .use_write_pdo()
                .r#where("group", "=", group)
                .limit((MAX_PROPERTIES + 2) as u64)
                .get()
                .await
                .map_err(|_| Error::Database)?;
            snapshot(rows, group)
        })
    }
    fn save<'a>(
        &'a self,
        group: &'a str,
        expected: &'a SettingsSnapshot,
        changes: &'a BTreeMap<String, SettingsUpdate>,
    ) -> SettingsFuture<'a, SettingsSnapshot> {
        Box::pin(async move {
            let mut tx = self
                .connection
                .begin_transaction()
                .await
                .map_err(|_| Error::Database)?;
            match self.apply(&mut tx, group, expected, changes).await {
                Ok(result) => {
                    tx.commit().await.map_err(|_| Error::Commit)?;
                    Ok(result)
                }
                Err(error) => {
                    tx.roll_back(None).await.map_err(|_| Error::Database)?;
                    Err(error)
                }
            }
        })
    }
    fn save_in<'a>(
        &'a self,
        tx: &'a mut Transaction,
        group: &'a str,
        expected: &'a SettingsSnapshot,
        changes: &'a BTreeMap<String, SettingsUpdate>,
    ) -> SettingsFuture<'a, SettingsSnapshot> {
        Box::pin(async move {
            let level = tx.transaction_level();
            if level == 0 {
                return Err(Error::Database);
            }
            tx.begin_transaction().await.map_err(|_| Error::Database)?;
            match self.apply(tx, group, expected, changes).await {
                Ok(result) => {
                    tx.commit().await.map_err(|_| Error::Commit)?;
                    Ok(result)
                }
                Err(error) => {
                    tx.roll_back(Some(level))
                        .await
                        .map_err(|_| Error::Database)?;
                    Err(error)
                }
            }
        })
    }
}

fn snapshot(rows: Vec<Row>, group: &str) -> SettingsResult<SettingsSnapshot> {
    if rows.len() > MAX_PROPERTIES + 1 {
        return Err(Error::LimitExceeded);
    }
    let mut result = SettingsSnapshot::default();
    let mut bytes = 0usize;
    for row in rows {
        if row.get("group") != Some(&Value::from(group)) {
            return Err(Error::InvalidDefinition);
        }
        let Some(Value::Text(name)) = row.get("name") else {
            return Err(Error::InvalidValue);
        };
        let payload = match row.get("payload") {
            Some(Value::Json(v)) => v.clone(),
            Some(Value::Text(v)) => {
                if v.len() > MAX_BYTES {
                    return Err(Error::LimitExceeded);
                }
                serde_json::from_str(v).map_err(|_| Error::InvalidValue)?
            }
            _ => return Err(Error::InvalidValue),
        };
        bytes = bytes.saturating_add(
            serde_json::to_vec(&payload)
                .map_err(|_| Error::InvalidValue)?
                .len(),
        );
        if bytes > MAX_BYTES {
            return Err(Error::LimitExceeded);
        }
        let locked = match row.get("locked") {
            Some(Value::Bool(v)) => *v,
            Some(Value::Integer(0)) => false,
            Some(Value::Integer(1)) => true,
            Some(Value::Unsigned(0)) => false,
            Some(Value::Unsigned(1)) => true,
            _ => return Err(Error::InvalidValue),
        };
        if name == VERSION {
            result.version = match payload {
                Json::Null => None,
                Json::String(v) if Uuid::parse_str(&v).is_ok() => Some(v),
                _ => return Err(Error::InvalidValue),
            };
            continue;
        }
        if !property(name)
            || result
                .properties
                .insert(name.clone(), StoredProperty { payload, locked })
                .is_some()
        {
            return Err(Error::InvalidDefinition);
        }
    }
    Ok(result)
}

fn record(group: &str, name: &str, payload: Json, locked: bool) -> Record {
    let mut values = record_values(payload, locked);
    values.insert("group".into(), Value::from(group));
    values.insert("name".into(), Value::from(name));
    values.insert(
        "created_at".into(),
        Value::from(timestamp_text(jiff::Timestamp::now())),
    );
    values
}
fn record_values(payload: Json, locked: bool) -> Record {
    BTreeMap::from([
        ("payload".into(), Value::Json(payload)),
        ("locked".into(), Value::Bool(locked)),
        (
            "updated_at".into(),
            Value::from(timestamp_text(jiff::Timestamp::now())),
        ),
    ])
}
fn timestamp_text(value: jiff::Timestamp) -> String {
    let nanoseconds = value.subsec_nanosecond().unsigned_abs();
    // Stored timestamps retain whole milliseconds, microseconds or nanoseconds.
    let format = if nanoseconds == 0 {
        "%Y-%m-%d %H:%M:%S"
    } else if nanoseconds.is_multiple_of(1_000_000) {
        "%Y-%m-%d %H:%M:%S%.3f"
    } else if nanoseconds.is_multiple_of(1_000) {
        "%Y-%m-%d %H:%M:%S%.6f"
    } else {
        "%Y-%m-%d %H:%M:%S%.9f"
    };
    value.strftime(format).to_string()
}

#[cfg(test)]
#[path = "../tests/unit/database_timestamps.rs"]
mod timestamp_tests;

pub(crate) fn identifier(value: &str) -> bool {
    let mut bytes = value.bytes();
    !value.is_empty()
        && value.len() <= 128
        && bytes
            .next()
            .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
}
pub(crate) fn property(value: &str) -> bool {
    identifier(value) && !value.to_ascii_lowercase().starts_with("__settings_")
}
pub(crate) fn check_payload(payload: &Json) -> SettingsResult<()> {
    if serde_json::to_vec(payload)
        .map_err(|_| Error::InvalidValue)?
        .len()
        > MAX_BYTES
    {
        return Err(Error::LimitExceeded);
    }
    Ok(())
}
