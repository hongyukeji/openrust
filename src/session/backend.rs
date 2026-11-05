//! 复用现数据库与Redis Session表、namespace和截止时间。

use std::{collections::HashMap, sync::Arc};

use chrono::{DateTime, Utc};
use time::OffsetDateTime;
use tokio::sync::Mutex;

use super::store::{digest_id, invalid_record, limit_error, unavailable};
use super::{SessionId, SessionRecord, SessionStore, SessionWriteMode, SessionWriteOutcome};
#[cfg(feature = "db-sqlite")]
#[path = "sqlite_guarded.rs"]
mod sqlite_guarded;

use crate::{
    Result,
    core::BoxFuture,
    db::{
        Db,
        sea_orm::{ConnectionTrait, DbBackend, SqlErr, Statement},
    },
};

/// 显式增强数据库 driver；哈希身份、受限 JSON envelope 与绝对截止复用原生表。
///
/// 必须预先登记增强 `expires_at` 迁移；不是原生 database handler 的 Base64 编码。
#[derive(Clone)]
pub struct GuardedDatabaseSessionStore {
    db: Db,
    table: String,
}

impl std::fmt::Debug for GuardedDatabaseSessionStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GuardedDatabaseSessionStore")
    }
}

impl GuardedDatabaseSessionStore {
    /// 复用已有Db，不建立第二连接池或迁移仓储。
    pub fn new(db: Db) -> Self {
        Self {
            db,
            table: "sessions".to_owned(),
        }
    }

    /// 选择同一公开表名解析器中的增强 Session 表；不会创建或改写表。
    ///
    /// # Errors
    ///
    /// 表名无法由当前 Db 解析时安全拒绝。
    pub fn for_table(mut self, table: impl Into<String>) -> Result<Self> {
        let table = table.into();
        self.db
            .table_names()
            .resolve(&table)
            .map_err(|_| unavailable())?;
        self.table = table;
        Ok(self)
    }
}

#[cfg(feature = "auth-fortify")]
impl super::store::CredentialSessionReader for GuardedDatabaseSessionStore {
    fn load_authorized_for_credential<'a>(
        &'a self,
        tx: &'a crate::db::DatabaseTransaction,
        id: &'a SessionId,
        maximum: usize,
    ) -> BoxFuture<'a, Result<Option<SessionRecord>>> {
        Box::pin(async move {
            let table = self.db.quoted_table_name(&self.table)?;
            let backend = tx.get_database_backend();
            let (length, budget, marker, lock) = match backend {
                DbBackend::Postgres => ("octet_length(payload)::bigint", "$1", "$2", " FOR UPDATE"),
                DbBackend::MySql => (
                    "CAST(OCTET_LENGTH(payload) AS SIGNED)",
                    "?",
                    "?",
                    " FOR UPDATE",
                ),
                DbBackend::Sqlite => ("length(CAST(payload AS BLOB))", "?", "?", ""),
                _ => return Err(unavailable()),
            };
            let sql = format!(
                "SELECT id, CASE WHEN {length}<={budget} THEN payload ELSE NULL END AS bounded_payload, {length} AS payload_length, expires_at FROM {table} WHERE id={marker}{lock}"
            );
            let row = tx
                .query_one_raw(Statement::from_sql_and_values(
                    backend,
                    sql,
                    [
                        i64::try_from(maximum).map_err(|_| limit_error())?.into(),
                        digest_id(id).into(),
                    ],
                ))
                .await
                .map_err(|_| unavailable())?;
            let Some(row) = row else {
                return Ok(None);
            };
            if row
                .try_get::<String>("", "id")
                .map_err(|_| invalid_record())?
                != digest_id(id)
            {
                return Ok(None);
            }
            let length: i64 = row
                .try_get("", "payload_length")
                .map_err(|_| invalid_record())?;
            if usize::try_from(length).map_err(|_| invalid_record())? > maximum {
                return Err(limit_error());
            }
            let expiry: DateTime<Utc> = row
                .try_get("", "expires_at")
                .map_err(|_| invalid_record())?;
            if expiry <= Utc::now() {
                return Ok(None);
            }
            let cutoff = OffsetDateTime::from_unix_timestamp(expiry.timestamp())
                .map_err(|_| invalid_record())?
                .replace_nanosecond(expiry.timestamp_subsec_nanos())
                .map_err(|_| invalid_record())?;
            let payload: String = row
                .try_get("", "bounded_payload")
                .map_err(|_| invalid_record())?;
            let mut record =
                SessionRecord::decode(id.clone(), payload.as_bytes(), cutoff, maximum)?;
            record.expiry_date = record.expiry_date.min(cutoff);
            Ok(Some(record))
        })
    }
}

impl SessionStore for GuardedDatabaseSessionStore {
    #[cfg(feature = "auth-fortify")]
    fn credential_parent_reader(
        &self,
        db: &Db,
    ) -> Result<&dyn super::store::CredentialSessionReader> {
        if !self.db.same_handle(db) {
            return Err(super::store::unsupported_guarantee());
        }
        Ok(self)
    }

    fn gc(&self, _lifetime_seconds: i64) -> BoxFuture<'_, Result<u64>> {
        Box::pin(async move {
            let connection = self.db.connection().map_err(|_| unavailable())?;
            let backend = connection.get_database_backend();
            let table = self
                .db
                .quoted_table_name(&self.table)
                .map_err(|_| unavailable())?;
            #[cfg(feature = "db-sqlite")]
            if backend == DbBackend::Sqlite {
                return sqlite_guarded::gc(connection.get_sqlite_connection_pool(), &table).await;
            }
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
            let table = self
                .db
                .quoted_table_name(&self.table)
                .map_err(|_| unavailable())?;
            #[cfg(feature = "db-sqlite")]
            if backend == DbBackend::Sqlite {
                return sqlite_guarded::load(
                    connection.get_sqlite_connection_pool(),
                    &table,
                    id,
                    max_bytes,
                    empty_expiry,
                )
                .await;
            }
            let maximum = i64::try_from(max_bytes).map_err(|_| limit_error())?;
            // 同一次查询检查长度并有界返回，避免length/GET分离的TOCTOU。
            let sql = match backend {
                DbBackend::Postgres => format!(
                    "SELECT CASE WHEN octet_length(payload) <= $1 THEN payload ELSE NULL END AS bounded_payload, octet_length(payload)::bigint AS payload_length, expires_at FROM {table} WHERE id = $2"
                ),
                DbBackend::MySql => format!(
                    "SELECT CASE WHEN OCTET_LENGTH(payload) <= ? THEN payload ELSE NULL END AS bounded_payload, CAST(OCTET_LENGTH(payload) AS SIGNED) AS payload_length, expires_at FROM {table} WHERE id = ?"
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
                .try_get::<String>("", "bounded_payload")
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
                    format!("DELETE FROM {table} WHERE id={marker} AND expires_at<=CURRENT_TIMESTAMP"),
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
            let table = self
                .db
                .quoted_table_name(&self.table)
                .map_err(|_| unavailable())?;
            let expiry = DateTime::<Utc>::from_timestamp(
                normalized_expiry.unix_timestamp(),
                normalized_expiry.nanosecond(),
            )
            .ok_or_else(invalid_record)?;
            #[cfg(feature = "db-sqlite")]
            if backend == DbBackend::Sqlite {
                return sqlite_guarded::write(
                    connection.get_sqlite_connection_pool(),
                    &table,
                    &record.id,
                    payload,
                    expiry,
                    mode,
                )
                .await;
            }
            let activity = OffsetDateTime::now_utc().unix_timestamp();
            let sql = match (backend, mode) {
                (DbBackend::Postgres, SessionWriteMode::Insert) => format!(
                    "INSERT INTO {table} (id,payload,expires_at,last_activity) VALUES ($1,$2,$3,$4)"
                ),
                (DbBackend::MySql, SessionWriteMode::Insert) => format!(
                    "INSERT INTO {table} (id,payload,expires_at,last_activity) VALUES (?,?,?,?)"
                ),
                (DbBackend::Postgres, SessionWriteMode::Update) => format!(
                    "UPDATE {table} SET payload=$2,expires_at=$3,last_activity=$4 WHERE id=$1 AND expires_at>CURRENT_TIMESTAMP"
                ),
                (DbBackend::MySql, SessionWriteMode::Update) => {
                    format!(
                        "UPDATE {table} SET payload=?,expires_at=?,last_activity=? WHERE id=? AND expires_at>CURRENT_TIMESTAMP(6)"
                    )
                }
                _ => return Err(unavailable()),
            };
            let values = if backend == DbBackend::MySql && mode == SessionWriteMode::Update {
                vec![
                    payload.into(),
                    expiry.into(),
                    activity.into(),
                    digest_id(&record.id).into(),
                ]
            } else {
                vec![
                    digest_id(&record.id).into(),
                    payload.into(),
                    expiry.into(),
                    activity.into(),
                ]
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
            let table = self
                .db
                .quoted_table_name(&self.table)
                .map_err(|_| unavailable())?;
            let placeholder = match backend {
                DbBackend::Postgres => "$1",
                DbBackend::MySql | DbBackend::Sqlite => "?",
                _ => return Err(unavailable()),
            };
            connection
                .execute_raw(Statement::from_sql_and_values(
                    backend,
                    format!("DELETE FROM {table} WHERE id={placeholder}"),
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

#[cfg(feature = "auth-fortify")]
impl super::store::CredentialSessionReader for MemorySessionStore {
    fn load_authorized_for_credential<'a>(
        &'a self,
        _tx: &'a crate::db::DatabaseTransaction,
        id: &'a SessionId,
        maximum: usize,
    ) -> BoxFuture<'a, Result<Option<SessionRecord>>> {
        self.load_authorized(id, maximum)
    }
}

impl SessionStore for MemorySessionStore {
    #[cfg(feature = "auth-fortify")]
    fn credential_parent_reader(
        &self,
        _db: &Db,
    ) -> Result<&dyn super::store::CredentialSessionReader> {
        Ok(self)
    }

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

// 授权读取不使用请求的回退期限；以读取前时钟加服务端剩余TTL作为保守上界。
#[cfg(feature = "session-redis")]
fn redis_authorized_record(
    id: &SessionId,
    bytes: &[u8],
    ttl: i64,
    observed: OffsetDateTime,
    maximum: usize,
) -> Result<Option<SessionRecord>> {
    match ttl {
        -2 | 0..=999 => return Ok(None),
        // 无TTL的键不能证明服务端授权截止，也不能退回请求内期限。
        -1 => return Err(invalid_record()),
        value if value < -2 => return Err(invalid_record()),
        _ => {}
    }
    if bytes.len() > maximum {
        return Err(limit_error());
    }
    let deadline = observed
        .checked_add(time::Duration::milliseconds(ttl))
        .ok_or_else(invalid_record)?
        // 截止只缩短；整秒避免改写纳秒字段后重新编码超过原始有界载荷预算。
        .replace_nanosecond(0)
        .map_err(|_| invalid_record())?;
    let mut record = SessionRecord::decode(id.clone(), bytes, deadline, maximum)?;
    record.expiry_date = record.expiry_date.min(deadline);
    record.validate_for(super::SessionStoreSemantics::Guarded, maximum)?;
    if record.expiry_date <= OffsetDateTime::now_utc() {
        return Ok(None);
    }
    Ok(Some(record))
}

#[cfg(all(feature = "auth-fortify", feature = "session-redis"))]
impl super::store::CredentialSessionReader for RedisSessionStore {
    fn load_authorized_for_credential<'a>(
        &'a self,
        _tx: &'a crate::db::DatabaseTransaction,
        id: &'a SessionId,
        maximum: usize,
    ) -> BoxFuture<'a, Result<Option<SessionRecord>>> {
        self.load_authorized(id, maximum)
    }
}

#[cfg(feature = "session-redis")]
impl SessionStore for RedisSessionStore {
    #[cfg(feature = "auth-fortify")]
    fn credential_parent_reader(
        &self,
        _db: &Db,
    ) -> Result<&dyn super::store::CredentialSessionReader> {
        Ok(self)
    }

    fn load_authorized<'a>(
        &'a self,
        id: &'a SessionId,
        max_bytes: usize,
    ) -> BoxFuture<'a, Result<Option<SessionRecord>>> {
        Box::pin(async move {
            let observed = OffsetDateTime::now_utc();
            let mut connection = self.connection.clone();
            let (bytes, ttl): (Vec<u8>, i64) = redis::cmd("EVAL")
                .arg("return {redis.call('GETRANGE', KEYS[1], 0, ARGV[1]), redis.call('PTTL', KEYS[1])}")
                .arg(1)
                .arg(self.key(id))
                .arg(max_bytes)
                .query_async(&mut connection)
                .await
                .map_err(|_| unavailable())?;
            redis_authorized_record(id, &bytes, ttl, observed, max_bytes)
        })
    }

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

    #[cfg(feature = "session-redis")]
    #[test]
    fn redis_authorized_deadline_is_bounded_by_ttl_and_record_expiry() -> Result<()> {
        let now = OffsetDateTime::now_utc();
        let id = SessionId::generate()?;
        let mut record = SessionRecord {
            id: id.clone(),
            data: serde_json::json!({"identity": 42}),
            expiry_date: now + time::Duration::seconds(90),
        };
        let bytes = record.encode(4096)?;
        let loaded =
            redis_authorized_record(&id, &bytes, 30_000, now, 4096)?.ok_or_else(invalid_record)?;
        assert_eq!(
            loaded.expiry_date,
            (now + time::Duration::seconds(30))
                .replace_nanosecond(0)
                .map_err(|_| invalid_record())?
        );
        record.expiry_date = now + time::Duration::seconds(10);
        let bytes = record.encode(4096)?;
        let loaded =
            redis_authorized_record(&id, &bytes, 30_000, now, 4096)?.ok_or_else(invalid_record)?;
        assert_eq!(loaded.expiry_date, record.expiry_date);
        assert!(redis_authorized_record(&id, &bytes, -1, now, 4096).is_err());
        assert!(redis_authorized_record(&id, &bytes, -2, now, 4096)?.is_none());
        assert!(redis_authorized_record(&id, &bytes, 0, now, 4096)?.is_none());
        assert!(redis_authorized_record(&id, &bytes, 30_000, now, 1).is_err());
        Ok(())
    }

    #[cfg(feature = "session-redis")]
    #[test]
    fn redis_authorized_exact_wire_budget_and_subsecond_ttl() -> Result<()> {
        let now = OffsetDateTime::now_utc();
        let id = SessionId::generate()?;
        let record = SessionRecord {
            id: id.clone(),
            data: serde_json::json!({"identity": 42}),
            // 最短纳秒编码使未经取整的TTL截止更容易增大重编码载荷。
            expiry_date: (now + time::Duration::seconds(90))
                .replace_nanosecond(1)
                .map_err(|_| invalid_record())?,
        };
        let bytes = record.encode(4096)?;
        let maximum = bytes.len();
        let loaded = redis_authorized_record(&id, &bytes, 30_000, now, maximum)?
            .ok_or_else(invalid_record)?;
        assert_eq!(loaded.data, record.data);
        assert!(loaded.expiry_date < record.expiry_date);
        assert_eq!(loaded.expiry_date.nanosecond(), 0);
        assert!(loaded.expiry_date <= now + time::Duration::seconds(30));
        loaded.validate_for(super::super::SessionStoreSemantics::Guarded, maximum)?;
        assert!(redis_authorized_record(&id, &bytes, 999, now, maximum)?.is_none());
        assert!(redis_authorized_record(&id, &bytes, 1, now, maximum)?.is_none());
        assert!(
            redis_authorized_record(&id, &bytes, 30_000, now, maximum - 1)
                .is_err_and(|error| error.code() == "session_limits_exceeded")
        );
        Ok(())
    }

    #[cfg(feature = "session-redis")]
    #[tokio::test]
    #[ignore = "需要显式真实Redis连接；实服务验收必须单独执行"]
    async fn redis_authorized_read_observes_ttl_without_refresh_and_rejects_persistent_key()
    -> Result<()> {
        fn require(condition: bool) -> Result<()> {
            if condition {
                Ok(())
            } else {
                Err(invalid_record())
            }
        }
        let url = std::env::var("OPENRUST_TEST_REDIS_URL").map_err(|_| unavailable())?;
        let id = SessionId::generate()?;
        let mut random = [0_u8; 8];
        getrandom::fill(&mut random).map_err(crate::Error::internal)?;
        let namespace = format!("authread-{:x}", u64::from_be_bytes(random));
        let store = RedisSessionStore::connect(url, namespace).await?;
        let outcome: Result<()> = async {
            let record = SessionRecord {
                id: id.clone(),
                data: serde_json::json!({"identity": 42}),
                expiry_date: OffsetDateTime::now_utc() + time::Duration::seconds(60),
            };
            require(
                store.write(&record, SessionWriteMode::Insert, 4096).await?
                    == SessionWriteOutcome::Written,
            )?;
            let mut connection = store.connection.clone();
            let key = store.key(&id);
            let shortened: bool = redis::cmd("PEXPIRE")
                .arg(&key)
                .arg(10_000)
                .query_async(&mut connection)
                .await
                .map_err(|_| unavailable())?;
            require(shortened)?;
            let before: i64 = redis::cmd("PTTL")
                .arg(&key)
                .query_async(&mut connection)
                .await
                .map_err(|_| unavailable())?;
            let loaded = store
                .load_authorized(&id, 4096)
                .await?
                .ok_or_else(invalid_record)?;
            let after: i64 = redis::cmd("PTTL")
                .arg(&key)
                .query_async(&mut connection)
                .await
                .map_err(|_| unavailable())?;
            require(after > 0 && after <= before)?;
            require(loaded.expiry_date > OffsetDateTime::now_utc())?;
            require(loaded.expiry_date <= record.expiry_date)?;
            require(
                loaded.expiry_date
                    <= OffsetDateTime::now_utc() + time::Duration::milliseconds(before),
            )?;
            let persisted: bool = redis::cmd("PERSIST")
                .arg(&key)
                .query_async(&mut connection)
                .await
                .map_err(|_| unavailable())?;
            require(persisted)?;
            require(
                store
                    .load_authorized(&id, 4096)
                    .await
                    .is_err_and(|error| error.code() == "session_store_data_invalid"),
            )?;
            Ok(())
        }
        .await;
        let cleanup = store.delete(&id).await;
        outcome?;
        cleanup
    }

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
