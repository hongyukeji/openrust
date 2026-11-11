use std::{collections::BTreeMap, sync::Arc};

use crate::database::{Connection, Row, Value as SqlValue, query::Record as SqlRecord};

use super::{
    CacheError, CacheResult, Value, backend,
    engine::{Change, Engine, Record, key_digest, now},
    store::StoreCore,
};

/// Stores values and locks in named database tables.
pub struct DatabaseStore(StoreCore);

super::store::impl_store!(DatabaseStore);

impl DatabaseStore {
    pub fn new(
        connection: Connection,
        table: String,
        lock_connection: Connection,
        lock_table: String,
        prefix: String,
    ) -> CacheResult<Self> {
        if !valid_table(&table) || !valid_table(&lock_table) {
            return Err(CacheError::InvalidConfiguration);
        }
        let data = DatabaseBackend {
            connection,
            table,
            column: "value",
        };
        let locks = DatabaseBackend {
            connection: lock_connection,
            table: lock_table,
            column: "owner",
        };
        Ok(Self(StoreCore::new(
            Engine::Database(Arc::new(data)),
            Engine::Database(Arc::new(locks)),
            prefix,
            true,
        )))
    }
}

pub(super) struct DatabaseBackend {
    connection: Connection,
    table: String,
    column: &'static str,
}

impl DatabaseBackend {
    pub(super) async fn read(&self, key: &str) -> CacheResult<Option<Record>> {
        let key = key_digest(key);
        let row = self
            .connection
            .table(&self.table)
            .useWritePdo()
            .r#where("key", "=", key.as_str())
            .first()
            .await
            .map_err(backend)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let time = now()?;
        if expiration(&row).ok_or(CacheError::InvalidValue)? <= time {
            self.connection
                .table(&self.table)
                .r#where("key", "=", key)
                .r#where("expiration", "<=", time)
                .delete()
                .await
                .map_err(backend)?;
            return Ok(None);
        }
        decode(row, self.column).map(Some)
    }

    pub(super) async fn change<T: Send + 'static>(
        &self,
        key: &str,
        edit: impl Fn(Option<Record>) -> CacheResult<(Change, T)> + Send + Sync + 'static,
    ) -> CacheResult<T> {
        let key: Arc<str> = key_digest(key).into();
        let table: Arc<str> = self.table.as_str().into();
        let column = self.column;
        let edit = Arc::new(edit);
        self.connection
            .transaction(
                move |tx| {
                    let (key, table, edit) =
                        (Arc::clone(&key), Arc::clone(&table), Arc::clone(&edit));
                    Box::pin(async move {
                        // The no-op conflict update locks the unique row, including competing insertions.
                        tx.table(&table)
                            .upsert(
                                &[row(&key, column, String::new(), 0)],
                                &["key"],
                                Some(&["key"]),
                            )
                            .await?;
                        let current = tx
                            .table(&table)
                            .r#where("key", "=", key.as_ref())
                            .lockForUpdate()
                            .first()
                            .await?;
                        let result = (|| {
                            let time = now()?;
                            let current = current
                                .filter(|row| expiration(row).is_some_and(|expiry| expiry > time));
                            let present = current.is_some();
                            let current = current.map(|row| decode(row, column)).transpose()?;
                            let (change, value) = edit(current)?;
                            let change = match change {
                                Change::Keep if !present => SqlChange::Remove,
                                Change::Keep => SqlChange::Keep,
                                Change::Remove => SqlChange::Remove,
                                Change::Write(entry) => {
                                    SqlChange::Write(encode(&entry, column)?, entry.expires)
                                }
                            };
                            Ok((change, value))
                        })();
                        match result {
                            Ok((SqlChange::Keep, result)) => Ok(Ok(result)),
                            Ok((SqlChange::Remove, result)) => {
                                tx.table(&table)
                                    .r#where("key", "=", key.as_ref())
                                    .delete()
                                    .await?;
                                Ok(Ok(result))
                            }
                            Ok((SqlChange::Write(value, expiration), result)) => {
                                tx.table(&table)
                                    .r#where("key", "=", key.as_ref())
                                    .update(BTreeMap::from([
                                        (column.into(), value.into()),
                                        ("expiration".into(), expiration.into()),
                                    ]))
                                    .await?;
                                Ok(Ok(result))
                            }
                            Err(error) => {
                                tx.table(&table)
                                    .r#where("key", "=", key.as_ref())
                                    .r#where("expiration", "=", 0i64)
                                    .delete()
                                    .await?;
                                Ok(Err(error))
                            }
                        }
                    })
                },
                3,
            )
            .await
            .map_err(backend)?
    }

    pub(super) async fn flush(&self) -> CacheResult<()> {
        self.connection
            .table(&self.table)
            .delete()
            .await
            .map_err(backend)?;
        Ok(())
    }
}

enum SqlChange {
    Keep,
    Remove,
    Write(String, i64),
}

fn row(key: &str, column: &str, value: String, expiration: i64) -> SqlRecord {
    BTreeMap::from([
        ("key".into(), key.into()),
        (column.into(), value.into()),
        ("expiration".into(), expiration.into()),
    ])
}

fn expiration(row: &Row) -> Option<i64> {
    match row.get("expiration") {
        Some(SqlValue::Integer(value)) => Some(*value),
        Some(SqlValue::Unsigned(value)) => (*value).try_into().ok(),
        _ => None,
    }
}

fn encode(entry: &Record, column: &str) -> CacheResult<String> {
    if column == "owner" {
        return entry
            .value
            .as_str()
            .map(str::to_owned)
            .ok_or(CacheError::InvalidOwner);
    }
    serde_json::to_string(&entry.value).map_err(backend)
}

fn decode(row: Row, column: &str) -> CacheResult<Record> {
    let expires = expiration(&row).ok_or(CacheError::InvalidValue)?;
    let Some(SqlValue::Text(value)) = row.get(column) else {
        return Err(CacheError::InvalidValue);
    };
    let value = if column == "owner" {
        Value::from(value.as_str())
    } else {
        serde_json::from_str(value).map_err(backend)?
    };
    Ok(Record { value, expires })
}

fn valid_table(name: &str) -> bool {
    !name.is_empty()
        && name.split('.').all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        })
}
