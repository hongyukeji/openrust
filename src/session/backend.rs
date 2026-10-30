//! 复用现数据库与Redis Session表、namespace和截止时间。

use std::{collections::HashMap, sync::Arc};

use chrono::{DateTime, Utc};
use time::OffsetDateTime;
use tokio::sync::Mutex;

use super::store::{digest_id, invalid_record, limit_error, unavailable};
use super::{SessionId, SessionRecord, SessionStore, SessionWriteMode, SessionWriteOutcome};
use crate::{
    Result,
    core::BoxFuture,
    db::{
        Db,
        sea_orm::{ConnectionTrait, DbBackend, SqlErr, Statement},
    },
};

/// 基于应用现有数据库连接及逻辑sessions表的共享store。
#[derive(Clone)]
pub struct DatabaseSessionStore {
    db: Db,
}

impl std::fmt::Debug for DatabaseSessionStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DatabaseSessionStore")
    }
}

impl DatabaseSessionStore {
    /// 复用已有Db，不建立第二连接池或迁移仓储。
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

impl SessionStore for DatabaseSessionStore {
    fn gc(&self, _lifetime_seconds: i64) -> BoxFuture<'_, Result<u64>> {
        Box::pin(async move {
            let connection = self.db.connection().map_err(|_| unavailable())?;
            let backend = connection.get_database_backend();
            let table = self.db.quoted_table_name("sessions")?;
            let now = match backend {
                DbBackend::Postgres => "CURRENT_TIMESTAMP",
                DbBackend::MySql => "CURRENT_TIMESTAMP(6)",
                _ => return Err(unavailable()),
            };
            connection
                .execute_raw(Statement::from_string(
                    backend,
                    format!("DELETE FROM {table} WHERE expires_at <= {now}"),
                ))
                .await
                .map(|result| result.rows_affected())
                .map_err(|_| unavailable())
        })
    }

    fn load<'a>(
        &'a self,
        id: &'a SessionId,
        max_bytes: usize,
        empty_expiry: OffsetDateTime,
    ) -> BoxFuture<'a, Result<Option<SessionRecord>>> {
        Box::pin(async move {
            let connection = self.db.connection().map_err(|_| unavailable())?;
            let backend = connection.get_database_backend();
            let table = self.db.quoted_table_name("sessions")?;
            let maximum = i64::try_from(max_bytes).map_err(|_| limit_error())?;
            // 同一次查询检查长度并有界返回，避免length/GET分离的TOCTOU。
            let sql = match backend {
                DbBackend::Postgres => format!(
                    "SELECT CASE WHEN octet_length(data) <= $1 THEN data ELSE NULL END AS payload, octet_length(data)::bigint AS payload_length, expires_at FROM {table} WHERE id_hash = $2"
                ),
                DbBackend::MySql => format!(
                    "SELECT CASE WHEN OCTET_LENGTH(data) <= ? THEN data ELSE NULL END AS payload, CAST(OCTET_LENGTH(data) AS SIGNED) AS payload_length, expires_at FROM {table} WHERE id_hash = ?"
                ),
                _ => return Err(unavailable()),
            };
            let row = connection
                .query_one_raw(Statement::from_sql_and_values(
                    backend,
                    sql,
                    [maximum.into(), digest_id(id).into()],
                ))
                .await
                .map_err(|_| unavailable())?;
            let Some(row) = row else {
                return Ok(None);
            };
            let length = row
                .try_get::<i64>("", "payload_length")
                .map_err(|_| invalid_record())?;
            if usize::try_from(length).map_err(|_| invalid_record())? > max_bytes {
                return Err(limit_error());
            }
            let payload = row
                .try_get::<String>("", "payload")
                .map_err(|_| invalid_record())?;
            let expiry = row
                .try_get::<DateTime<Utc>>("", "expires_at")
                .map_err(|_| invalid_record())?;
            let expiry = OffsetDateTime::from_unix_timestamp(expiry.timestamp())
                .map_err(|_| invalid_record())?
                .replace_nanosecond(expiry.timestamp_subsec_nanos())
                .map_err(|_| invalid_record())?;
            if expiry <= OffsetDateTime::now_utc() {
                let marker = if backend == DbBackend::Postgres {
                    "$1"
                } else {
                    "?"
                };
                connection.execute_raw(Statement::from_sql_and_values(backend,
                    format!("DELETE FROM {table} WHERE id_hash={marker} AND expires_at<=CURRENT_TIMESTAMP"),
                    [digest_id(id).into()])).await.map_err(|_| unavailable())?;
                return Ok(None);
            }
            let mut record = SessionRecord::decode(
                id.clone(),
                payload.as_bytes(),
                expiry.min(empty_expiry),
                max_bytes,
            )?;
            // SQL时间戳可能只保微秒；列截止是持久授权的上界，不能被envelope延长。
            record.expiry_date = record.expiry_date.min(expiry);
            Ok(Some(record))
        })
    }

    fn write<'a>(
        &'a self,
        record: &'a SessionRecord,
        mode: SessionWriteMode,
        max_bytes: usize,
    ) -> BoxFuture<'a, Result<SessionWriteOutcome>> {
        Box::pin(async move {
            let normalized_expiry = record
                .expiry_date
                .replace_nanosecond(record.expiry_date.nanosecond() / 1000 * 1000)
                .map_err(|_| invalid_record())?;
            let bytes = record.encode_with_expiry(max_bytes, normalized_expiry)?;
            let payload = String::from_utf8(bytes).map_err(|_| invalid_record())?;
            let connection = self.db.connection().map_err(|_| unavailable())?;
            let backend = connection.get_database_backend();
            let table = self.db.quoted_table_name("sessions")?;
            let expiry = DateTime::<Utc>::from_timestamp(
                normalized_expiry.unix_timestamp(),
                normalized_expiry.nanosecond(),
            )
            .ok_or_else(invalid_record)?;
            let sql = match (backend, mode) {
                (DbBackend::Postgres, SessionWriteMode::Insert) => format!(
                    "INSERT INTO {table} (id_hash,data,expires_at,created_at) VALUES ($1,$2,$3,CURRENT_TIMESTAMP)"
                ),
                (DbBackend::MySql, SessionWriteMode::Insert) => format!(
                    "INSERT INTO {table} (id_hash,data,expires_at,created_at) VALUES (?,?,?,CURRENT_TIMESTAMP)"
                ),
                (DbBackend::Postgres, SessionWriteMode::Update) => format!(
                    "UPDATE {table} SET data=$2,expires_at=$3 WHERE id_hash=$1 AND expires_at>CURRENT_TIMESTAMP"
                ),
                (DbBackend::MySql, SessionWriteMode::Update) => {
                    format!(
                        "UPDATE {table} SET data=?,expires_at=? WHERE id_hash=? AND expires_at>CURRENT_TIMESTAMP(6)"
                    )
                }
                _ => return Err(unavailable()),
            };
            let values = if backend == DbBackend::MySql && mode == SessionWriteMode::Update {
                vec![payload.into(), expiry.into(), digest_id(&record.id).into()]
            } else {
                vec![digest_id(&record.id).into(), payload.into(), expiry.into()]
            };
            let result = connection
                .execute_raw(Statement::from_sql_and_values(backend, sql, values))
                .await;
            match result {
                Ok(result) if result.rows_affected() > 0 => Ok(SessionWriteOutcome::Written),
                Ok(_) => Ok(if mode == SessionWriteMode::Insert {
                    SessionWriteOutcome::Conflict
                } else {
                    SessionWriteOutcome::Missing
                }),
                Err(error)
                    if mode == SessionWriteMode::Insert
                        && matches!(
                            error.sql_err(),
                            Some(SqlErr::UniqueConstraintViolation(_))
                        ) =>
                {
                    Ok(SessionWriteOutcome::Conflict)
                }
                Err(_) => Err(unavailable()),
            }
        })
    }

    fn delete<'a>(&'a self, id: &'a SessionId) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let connection = self.db.connection().map_err(|_| unavailable())?;
            let backend = connection.get_database_backend();
            let table = self.db.quoted_table_name("sessions")?;
            let placeholder = match backend {
                DbBackend::Postgres => "$1",
                DbBackend::MySql => "?",
                _ => return Err(unavailable()),
            };
            connection
                .execute_raw(Statement::from_sql_and_values(
                    backend,
                    format!("DELETE FROM {table} WHERE id_hash={placeholder}"),
                    [digest_id(id).into()],
                ))
                .await
                .map_err(|_| unavailable())?;
            Ok(())
        })
    }
}

/// 有界进程内测试/array store；只有显式安装才会启用。
#[derive(Clone)]
pub struct MemorySessionStore {
    records: Arc<Mutex<HashMap<SessionId, SessionRecord>>>,
    maximum: usize,
}

impl Default for MemorySessionStore {
    fn default() -> Self {
        Self {
            records: Arc::default(),
            maximum: 1024,
        }
    }
}

impl std::fmt::Debug for MemorySessionStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MemorySessionStore([REDACTED])")
    }
}

impl SessionStore for MemorySessionStore {
    fn gc(&self, _lifetime_seconds: i64) -> BoxFuture<'_, Result<u64>> {
        Box::pin(async move {
            let mut records = self.records.lock().await;
            let before = records.len();
            let now = OffsetDateTime::now_utc();
            records.retain(|_, record| record.expiry_date > now);
            u64::try_from(before - records.len()).map_err(|_| limit_error())
        })
    }

    fn load<'a>(
        &'a self,
        id: &'a SessionId,
        max_bytes: usize,
        _empty_expiry: OffsetDateTime,
    ) -> BoxFuture<'a, Result<Option<SessionRecord>>> {
        Box::pin(async move {
            let mut records = self.records.lock().await;
            if records
                .get(id)
                .is_some_and(|record| record.expiry_date <= OffsetDateTime::now_utc())
            {
                records.remove(id);
            }
            let Some(record) = records.get(id) else {
                return Ok(None);
            };
            record.encode(max_bytes)?;
            Ok(Some(record.clone()))
        })
    }
    fn write<'a>(
        &'a self,
        record: &'a SessionRecord,
        mode: SessionWriteMode,
        max_bytes: usize,
    ) -> BoxFuture<'a, Result<SessionWriteOutcome>> {
        Box::pin(async move {
            record.encode(max_bytes)?;
            let mut records = self.records.lock().await;
            let exists = records
                .get(&record.id)
                .is_some_and(|old| old.expiry_date > OffsetDateTime::now_utc());
            if mode == SessionWriteMode::Insert && exists {
                return Ok(SessionWriteOutcome::Conflict);
            }
            if mode == SessionWriteMode::Update && !exists {
                return Ok(SessionWriteOutcome::Missing);
            }
            if !exists && records.len() >= self.maximum {
                records.retain(|_, r| r.expiry_date > OffsetDateTime::now_utc());
                if records.len() >= self.maximum {
                    return Err(limit_error());
                }
            }
            records.insert(record.id.clone(), record.clone());
            Ok(SessionWriteOutcome::Written)
        })
    }
    fn delete<'a>(&'a self, id: &'a SessionId) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.records.lock().await.remove(id);
            Ok(())
        })
    }
}

/// 使用既有 Redis 连接管理器与应用隔离 namespace 的共享 Session store。
#[cfg(feature = "session-redis")]
#[derive(Clone)]
pub struct RedisSessionStore {
    connection: redis::aio::ConnectionManager,
    namespace: String,
}

#[cfg(feature = "session-redis")]
impl std::fmt::Debug for RedisSessionStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RedisSessionStore([REDACTED])")
    }
}

#[cfg(feature = "session-redis")]
impl RedisSessionStore {
    /// 连接已有Redis服务；URL不保存在日志或错误信息中。
    pub async fn connect(dsn: impl Into<String>, namespace: impl Into<String>) -> Result<Self> {
        let namespace = namespace.into();
        if namespace.is_empty()
            || namespace.len() > 63
            || !namespace
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
        {
            return Err(invalid_record());
        }
        let client = redis::Client::open(dsn.into()).map_err(|_| unavailable())?;
        let connection = client
            .get_connection_manager()
            .await
            .map_err(|_| unavailable())?;
        Ok(Self {
            connection,
            namespace,
        })
    }
    fn key(&self, id: &SessionId) -> String {
        format!("openrust:session:{}:{}", self.namespace, digest_id(id))
    }
}

#[cfg(feature = "session-redis")]
impl SessionStore for RedisSessionStore {
    fn load<'a>(
        &'a self,
        id: &'a SessionId,
        max_bytes: usize,
        empty_expiry: OffsetDateTime,
    ) -> BoxFuture<'a, Result<Option<SessionRecord>>> {
        Box::pin(async move {
            let mut connection = self.connection.clone();
            // 一次有界读取与TTL快照；独立STRLEN/GET会有TOCTOU。
            let observed = OffsetDateTime::now_utc();
            let (bytes, ttl): (Vec<u8>, i64) = redis::cmd("EVAL")
                .arg("return {redis.call('GETRANGE', KEYS[1], 0, ARGV[1]), redis.call('PTTL', KEYS[1])}")
                .arg(1).arg(self.key(id)).arg(max_bytes)
                .query_async(&mut connection).await.map_err(|_| unavailable())?;
            if ttl == -2 {
                return Ok(None);
            }
            if bytes.len() > max_bytes {
                return Err(limit_error());
            }
            let fallback = if ttl >= 0 {
                observed
                    .checked_add(time::Duration::milliseconds(ttl))
                    .ok_or_else(invalid_record)?
            } else {
                empty_expiry
            };
            let fallback = fallback.min(empty_expiry);
            let mut record = SessionRecord::decode(id.clone(), &bytes, fallback, max_bytes)?;
            if ttl >= 0 {
                record.expiry_date = record.expiry_date.min(fallback);
            }
            Ok(Some(record))
        })
    }
    fn write<'a>(
        &'a self,
        record: &'a SessionRecord,
        mode: SessionWriteMode,
        max_bytes: usize,
    ) -> BoxFuture<'a, Result<SessionWriteOutcome>> {
        Box::pin(async move {
            let bytes = record.encode(max_bytes)?;
            let ttl = (record.expiry_date - OffsetDateTime::now_utc()).whole_milliseconds();
            if ttl <= 0 {
                return Ok(SessionWriteOutcome::Missing);
            }
            let ttl = u64::try_from(ttl).map_err(|_| invalid_record())?;
            let mut connection = self.connection.clone();
            let result: Option<String> = redis::cmd("SET")
                .arg(self.key(&record.id))
                .arg(bytes)
                .arg("PX")
                .arg(ttl)
                .arg(if mode == SessionWriteMode::Insert {
                    "NX"
                } else {
                    "XX"
                })
                .query_async(&mut connection)
                .await
                .map_err(|_| unavailable())?;
            Ok(if result.is_some() {
                SessionWriteOutcome::Written
            } else if mode == SessionWriteMode::Insert {
                SessionWriteOutcome::Conflict
            } else {
                SessionWriteOutcome::Missing
            })
        })
    }
    fn delete<'a>(&'a self, id: &'a SessionId) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let mut connection = self.connection.clone();
            let _: i64 = redis::cmd("DEL")
                .arg(self.key(id))
                .query_async(&mut connection)
                .await
                .map_err(|_| unavailable())?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn memory_gc_counts_absolute_deadlines_and_prevents_update_resurrection() -> Result<()> {
        let store = MemorySessionStore::default();
        let now = OffsetDateTime::now_utc();
        let mut records = Vec::new();
        for expiry in [
            now - time::Duration::seconds(1),
            now,
            now + time::Duration::days(3),
        ] {
            let record = SessionRecord {
                id: SessionId::generate()?,
                data: serde_json::json!({"kept": true}),
                expiry_date: expiry,
            };
            assert_eq!(
                store.write(&record, SessionWriteMode::Insert, 4096).await?,
                SessionWriteOutcome::Written
            );
            records.push(record);
        }
        assert_eq!(store.gc(1).await?, 2);
        assert_eq!(store.gc(i64::MAX).await?, 0);
        for record in &records[..2] {
            let renewed = SessionRecord {
                expiry_date: now + time::Duration::days(4),
                ..record.clone()
            };
            assert_eq!(
                store
                    .write(&renewed, SessionWriteMode::Update, 4096)
                    .await?,
                SessionWriteOutcome::Missing
            );
        }
        assert!(
            store
                .load(&records[2].id, 4096, now + time::Duration::days(4))
                .await?
                .is_some()
        );
        Ok(())
    }
}
