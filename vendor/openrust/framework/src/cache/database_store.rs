use std::{collections::BTreeMap, sync::Arc};

use crate::database::{Connection, Row, Value as SqlValue, query::Record as SqlRecord};

use super::{
    CacheError, CacheResult, Value, backend,
    engine::{Change, Engine, FOREVER, Record, key_digest, now},
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
        // A bare name and a schema-qualified name can refer to the same table.
        let data_name = format!(
            "{}{}",
            connection.get_table_prefix(),
            table.rsplit('.').next().unwrap_or(&table)
        );
        let lock_name = format!(
            "{}{}",
            lock_connection.get_table_prefix(),
            lock_table.rsplit('.').next().unwrap_or(&lock_table)
        );
        let separate_locks = !data_name.eq_ignore_ascii_case(&lock_name);
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
            separate_locks,
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
            .use_write_pdo()
            .r#where("key", "=", key.as_str())
            .first()
            .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let time = now()?;
        if expiration(&row).ok_or(CacheError::InvalidValue)? <= time {
            self.connection
                .table(&self.table)
                .r#where("key", "=", key)
                .r#where("expiration", "<=", time / 1000)
                .delete()
                .await?;
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
                            .lock_for_update()
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
                                Change::Write(entry) => SqlChange::Write(
                                    encode(&entry, column)?,
                                    database_expiration(entry.expires)?,
                                ),
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
            .await?
    }

    pub(super) async fn flush(&self) -> CacheResult<()> {
        self.connection.table(&self.table).delete().await?;
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
        Some(SqlValue::Integer(value)) => value.checked_mul(1000),
        Some(SqlValue::Unsigned(value)) => i64::try_from(*value).ok()?.checked_mul(1000),
        _ => None,
    }
}

/*
 * 标准缓存迁移的 expiration 为整数秒；共享引擎使用毫秒，仅在数据库边界转换。
 * 永久缓存按 框架 DatabaseStore 保存十年，避免将无限哨兵写入整数列。
 * 亚秒租约向上取整，防止存入后立即过期。
 */
#[allow(
    non_snake_case,
    reason = "Names the database expiration adapter explicitly"
)]
fn database_expiration(expires: i64) -> CacheResult<i64> {
    if expires == FOREVER {
        (now()? / 1000)
            .checked_add(315_360_000)
            .ok_or(CacheError::InvalidValue)
    } else {
        expires
            .checked_add(999)
            .map(|value| value / 1000)
            .ok_or(CacheError::InvalidValue)
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
