//! 数据库共享租约与显式请求幂等；SQLite 租约仅用于同机文件协调。

use std::{fmt, time::Duration};

use crate::{
    core::{Error, Result},
    database::{DatabaseTransaction, Db, DbBackend},
};
use sea_orm::{ConnectionTrait, Statement};
use sea_orm_migration::prelude::*;
use serde_json::Value;
use sha2::{Digest, Sha256};

#[cfg(feature = "db-sqlite")]
mod sqlite_locks;

const MAX_KEY_LEN: usize = 256;
const MAX_FINGERPRINT_LEN: usize = 1024 * 1024;
const MAX_RESPONSE_LEN: usize = 64 * 1024;
const MAX_TTL_MS: u64 = 86_400_000;

/// 为共享租约和请求幂等表创建数据库迁移。
pub struct CoordinationMigration;

impl MigrationName for CoordinationMigration {
    fn name(&self) -> &'static str {
        "m20261026_000004_openrust_coordination"
    }
}

#[async_trait::async_trait]
#[allow(clippy::too_many_lines)] // 将租约与幂等表和清理索引的同一版本迁移保持在一处。
impl MigrationTrait for CoordinationMigration {
    async fn up(&self, manager: &SchemaManager) -> std::result::Result<(), DbErr> {
        let backend = manager.get_database_backend();
        let locks = migration_table_alias("shared_locks", backend)?;
        let idempotency = migration_table_alias("request_idempotency", backend)?;
        manager
            .create_table(
                Table::create()
                    .table(locks)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("key_hash"))
                            .string_len(64)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("owner_token"))
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("expires_at_ms"))
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("fencing_token"))
                            .big_integer()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(idempotency.clone())
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("scope_hash"))
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("key_hash"))
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("request_hash"))
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("owner_token"))
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("state"))
                            .string_len(16)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("lease_until_ms"))
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("expires_at_ms"))
                            .big_integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(Alias::new("response_status")).small_integer())
                    .col(ColumnDef::new(Alias::new("response_body")).text())
                    .primary_key(
                        Index::create()
                            .col(Alias::new("scope_hash"))
                            .col(Alias::new("key_hash")),
                    )
                    .to_owned(),
            )
            .await?;
        let names = crate::database::DatabaseTableNames::current().ok_or_else(|| {
            DbErr::Migration("coordination migration requires a table-name scope".into())
        })?;
        let expiry_index = format!(
            "or_idem_exp_{}",
            &digest(
                names
                    .resolve("request_idempotency")
                    .map_err(|error| DbErr::Migration(error.to_string()))?
                    .as_bytes(),
            )[..16]
        );
        manager
            .create_index(
                Index::create()
                    .name(expiry_index)
                    .table(idempotency)
                    .col(Alias::new("expires_at_ms"))
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> std::result::Result<(), DbErr> {
        let backend = manager.get_database_backend();
        for name in ["request_idempotency", "shared_locks"] {
            manager
                .drop_table(
                    Table::drop()
                        .table(migration_table_alias(name, backend)?)
                        .if_exists()
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }
}

fn migration_table_alias(name: &str, backend: DbBackend) -> std::result::Result<Alias, DbErr> {
    let names = crate::database::DatabaseTableNames::current().ok_or_else(|| {
        DbErr::Migration(
            "coordination migration requires MigrationServiceProvider or TestDatabase scope".into(),
        )
    })?;
    names
        .quote(name, backend)
        .map_err(|error| DbErr::Migration(error.to_string()))?;
    names
        .resolve(name)
        .map(Alias::new)
        .map_err(|error| DbErr::Migration(error.to_string()))
}

/// 使用数据库时钟协调多个应用实例的短期租约。
///
/// SQLite 增强租约仅支持同机共享文件。
#[derive(Clone)]
pub struct SharedLocks {
    db: Db,
}

impl fmt::Debug for SharedLocks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SharedLocks")
    }
}

impl SharedLocks {
    /// 使用应用共享数据库创建租约服务。
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// 尝试获取最长 24 小时的租约；当前租约未过期时返回其数据库到期时间。
    pub async fn acquire(&self, key: &str, ttl: Duration) -> Result<LockAcquire> {
        validate_text("lock key", key, MAX_KEY_LEN)?;
        let ttl_ms = ttl_millis(ttl)?;
        let key_hash = digest(key.as_bytes());
        let owner_token = random_token()?;
        let table = self.db.quoted_table_name("shared_locks")?;
        let backend = self.db.backend()?;
        #[cfg(feature = "db-sqlite")]
        if backend == DbBackend::Sqlite {
            return sqlite_locks::acquire(&self.db, key_hash, owner_token, ttl_ms).await;
        }
        let sql = lock_acquire_sql(backend, &table)?;
        let result = self.db.transaction(|transaction| {
            let key_hash = key_hash.clone();
            let owner_token = owner_token.clone();
            let sql = sql.clone();
            Box::pin(async move {
                if backend == DbBackend::Postgres {
                    transaction.query_one_raw(Statement::from_sql_and_values(backend, &sql, [key_hash.clone().into(), owner_token.clone().into(), ttl_ms.into()]))
                        .await.map_err(Error::internal)?;
                } else {
                    transaction.execute_raw(Statement::from_sql_and_values(backend, &sql, [key_hash.clone().into(), owner_token.clone().into(), ttl_ms.into()]))
                        .await.map_err(Error::internal)?;
                }
                let query = format!("SELECT owner_token, fencing_token, expires_at_ms FROM {table} WHERE key_hash = {} FOR UPDATE", placeholder(backend, 1));
                let row = transaction.query_one_raw(Statement::from_sql_and_values(backend, &query, [key_hash.clone().into()])).await.map_err(Error::internal)?
                    .ok_or_else(|| Error::internal(std::io::Error::other("shared lock row disappeared")))?;
                let stored_owner = row.try_get::<String>("", "owner_token").map_err(Error::internal)?;
                let fencing = row.try_get::<i64>("", "fencing_token").map_err(Error::internal)?;
                let expires_at_ms = row.try_get::<i64>("", "expires_at_ms").map_err(Error::internal)?;
                Ok(if stored_owner == owner_token {
                    LockAcquire::Acquired(LockLease { key_hash, owner_token, fencing_token: fencing, expires_at_ms })
                } else {
                    LockAcquire::Busy { expires_at_ms }
                })
            })
        }).await?;
        Ok(result)
    }

    /// 以当前 owner token 续约；过期或已被接管的租约返回 `false`。
    pub async fn renew(&self, lease: &LockLease, ttl: Duration) -> Result<bool> {
        let ttl_ms = ttl_millis(ttl)?;
        let table = self.db.quoted_table_name("shared_locks")?;
        let backend = self.db.backend()?;
        #[cfg(feature = "db-sqlite")]
        if backend == DbBackend::Sqlite {
            return sqlite_locks::renew(&self.db, lease, ttl_ms).await;
        }
        let sql = match backend {
            DbBackend::Postgres => format!(
                "UPDATE {table} SET expires_at_ms = {} + $3 WHERE key_hash = $1 AND owner_token = $2 AND fencing_token = $4 AND expires_at_ms > {}",
                now_ms_sql(backend),
                now_ms_sql(backend)
            ),
            DbBackend::MySql => format!(
                "UPDATE {table} SET expires_at_ms = {} + ? WHERE key_hash = ? AND owner_token = ? AND fencing_token = ? AND expires_at_ms > {}",
                now_ms_sql(backend),
                now_ms_sql(backend)
            ),
            _ => return Err(unsupported_backend()),
        };
        let values = if backend == DbBackend::Postgres {
            vec![
                lease.key_hash.clone().into(),
                lease.owner_token.clone().into(),
                ttl_ms.into(),
                lease.fencing_token.into(),
            ]
        } else {
            vec![
                ttl_ms.into(),
                lease.key_hash.clone().into(),
                lease.owner_token.clone().into(),
                lease.fencing_token.into(),
            ]
        };
        let result = self
            .db
            .connection()?
            .execute_raw(Statement::from_sql_and_values(backend, &sql, values))
            .await
            .map_err(Error::internal)?;
        Ok(result.rows_affected() == 1)
    }

    /// 仅释放仍由此 lease 持有的锁；旧 owner 不会删除新 owner 的记录。
    pub async fn release(&self, lease: &LockLease) -> Result<bool> {
        let table = self.db.quoted_table_name("shared_locks")?;
        let backend = self.db.backend()?;
        let sql = format!(
            "DELETE FROM {table} WHERE key_hash = {} AND owner_token = {} AND fencing_token = {}",
            placeholder(backend, 1),
            placeholder(backend, 2),
            placeholder(backend, 3)
        );
        let result = self
            .db
            .connection()?
            .execute_raw(Statement::from_sql_and_values(
                backend,
                &sql,
                [
                    lease.key_hash.clone().into(),
                    lease.owner_token.clone().into(),
                    lease.fencing_token.into(),
                ],
            ))
            .await
            .map_err(Error::internal)?;
        Ok(result.rows_affected() == 1)
    }

    /// 在业务事务内确认租约仍有效且未被新 owner 接管。
    pub async fn assert_current_in_transaction(
        &self,
        transaction: &DatabaseTransaction,
        lease: &LockLease,
    ) -> Result<()> {
        let table = self.db.quoted_table_name("shared_locks")?;
        let backend = transaction.get_database_backend();
        #[cfg(feature = "db-sqlite")]
        if backend == DbBackend::Sqlite {
            return sqlite_locks::assert_current_in_transaction(&self.db, transaction, lease).await;
        }
        let sql = format!(
            "SELECT owner_token FROM {table} WHERE key_hash = {} AND owner_token = {} AND fencing_token = {} AND expires_at_ms > {} FOR UPDATE",
            placeholder(backend, 1),
            placeholder(backend, 2),
            placeholder(backend, 3),
            now_ms_sql(backend)
        );
        let row = transaction
            .query_one_raw(Statement::from_sql_and_values(
                backend,
                &sql,
                [
                    lease.key_hash.clone().into(),
                    lease.owner_token.clone().into(),
                    lease.fencing_token.into(),
                ],
            ))
            .await
            .map_err(Error::internal)?;
        if row.is_some() {
            Ok(())
        } else {
            Err(Error::custom(
                "shared_lock_lost",
                "shared lock lease expired or was acquired by another owner",
            ))
        }
    }
}

/// 获取共享租约的结果。
pub enum LockAcquire {
    /// 当前调用获得了 lease。
    Acquired(LockLease),
    /// 其他 owner 持有未过期 lease。
    Busy {
        /// 数据库时钟下的 Unix 毫秒到期时刻。
        expires_at_ms: i64,
    },
}

/// 共享租约的私有 owner 凭据与 fencing generation。
#[derive(Clone)]
pub struct LockLease {
    key_hash: String,
    owner_token: String,
    fencing_token: i64,
    expires_at_ms: i64,
}

impl LockLease {
    /// 返回此 fencing generation；保护写入时应在事务中调用 `assert_current_in_transaction`。
    pub const fn fencing_token(&self) -> i64 {
        self.fencing_token
    }
    /// 返回数据库时钟下的 Unix 毫秒到期时刻。
    pub const fn expires_at_ms(&self) -> i64 {
        self.expires_at_ms
    }
}

impl fmt::Debug for LockLease {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LockLease")
            .field("fencing_token", &self.fencing_token)
            .field("expires_at_ms", &self.expires_at_ms)
            .field("owner_token", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

/// 供变更接口显式执行请求幂等检查与结果重放。
#[derive(Clone)]
pub struct RequestIdempotency {
    db: Db,
}

impl fmt::Debug for RequestIdempotency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RequestIdempotency")
    }
}

impl RequestIdempotency {
    /// 使用应用共享数据库创建幂等服务。
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// 领取、检测冲突或重放一个请求键；处理租约最长为 24 小时。
    pub async fn claim(
        &self,
        scope: &str,
        key: &str,
        fingerprint: &[u8],
        lease_ttl: Duration,
    ) -> Result<IdempotencyClaim> {
        validate_text("idempotency scope", scope, MAX_KEY_LEN)?;
        validate_text("idempotency key", key, MAX_KEY_LEN)?;
        if fingerprint.is_empty() || fingerprint.len() > MAX_FINGERPRINT_LEN {
            return Err(Error::BadRequest(
                "idempotency fingerprint must contain 1..=1048576 bytes".into(),
            ));
        }
        let lease_ms = ttl_millis(lease_ttl)?;
        let scope_hash = digest(scope.as_bytes());
        let key_hash = digest(key.as_bytes());
        let request_hash = digest(fingerprint);
        let owner_token = random_token()?;
        let backend = self.db.backend()?;
        let table = self.db.quoted_table_name("request_idempotency")?;
        let sql = idempotency_claim_sql(backend, &table)?;
        self.db.transaction(|transaction| {
            let scope_hash = scope_hash.clone(); let key_hash = key_hash.clone(); let request_hash = request_hash.clone(); let owner_token = owner_token.clone(); let sql = sql.clone(); let table = table.clone();
            Box::pin(async move {
                let values = if backend == DbBackend::Postgres {
                    vec![scope_hash.clone().into(), key_hash.clone().into(), request_hash.clone().into(), owner_token.clone().into(), lease_ms.into()]
                } else {
                    vec![scope_hash.clone().into(), key_hash.clone().into(), request_hash.clone().into(), owner_token.clone().into(), lease_ms.into(), lease_ms.into()]
                };
                transaction.execute_raw(Statement::from_sql_and_values(backend, &sql, values)).await.map_err(Error::internal)?;
                let query = format!("SELECT request_hash, owner_token, state, lease_until_ms, response_status, response_body FROM {table} WHERE scope_hash = {} AND key_hash = {} FOR UPDATE", placeholder(backend, 1), placeholder(backend, 2));
                let row = transaction.query_one_raw(Statement::from_sql_and_values(backend, &query, [scope_hash.clone().into(), key_hash.clone().into()])).await.map_err(Error::internal)?.ok_or_else(|| Error::internal(std::io::Error::other("idempotency row disappeared")))?;
                let stored_hash = row.try_get::<String>("", "request_hash").map_err(Error::internal)?;
                let stored_owner = row.try_get::<String>("", "owner_token").map_err(Error::internal)?;
                let state = row.try_get::<String>("", "state").map_err(Error::internal)?;
                let lease_until_ms = row.try_get::<i64>("", "lease_until_ms").map_err(Error::internal)?;
                if stored_owner == owner_token {
                    return Ok(IdempotencyClaim::Acquired(IdempotencyLease { scope_hash, key_hash, request_hash, owner_token, lease_until_ms }));
                }
                if stored_hash != request_hash { return Ok(IdempotencyClaim::Conflict); }
                if state == "completed" {
                    let status = row.try_get::<Option<i16>>("", "response_status").map_err(Error::internal)?.ok_or_else(|| Error::internal(std::io::Error::other("completed idempotency record has no status")))?;
                    let body = row.try_get::<Option<String>>("", "response_body").map_err(Error::internal)?.ok_or_else(|| Error::internal(std::io::Error::other("completed idempotency record has no body")))?;
                    let body = serde_json::from_str(&body).map_err(Error::internal)?;
                    return Ok(IdempotencyClaim::Replay(IdempotencyResponse { status: <u16 as TryFrom<i16>>::try_from(status).map_err(Error::internal)?, body }));
                }
                Ok(IdempotencyClaim::InProgress { lease_until_ms })
            })
        }).await
    }

    /// 为仍有效的 owner 延长最长 24 小时的处理租约。
    pub async fn renew(&self, lease: &IdempotencyLease, ttl: Duration) -> Result<bool> {
        let ttl_ms = ttl_millis(ttl)?;
        let backend = self.db.backend()?;
        let table = self.db.quoted_table_name("request_idempotency")?;
        let (sql, values) = if backend == DbBackend::Postgres {
            (
                format!(
                    "UPDATE {table} SET lease_until_ms = {} + $5, expires_at_ms = {} + $5 WHERE scope_hash = $1 AND key_hash = $2 AND request_hash = $3 AND owner_token = $4 AND state = 'in_progress' AND lease_until_ms > {}",
                    now_ms_sql(backend),
                    now_ms_sql(backend),
                    now_ms_sql(backend)
                ),
                vec![
                    lease.scope_hash.clone().into(),
                    lease.key_hash.clone().into(),
                    lease.request_hash.clone().into(),
                    lease.owner_token.clone().into(),
                    ttl_ms.into(),
                ],
            )
        } else if backend == DbBackend::MySql {
            (
                format!(
                    "UPDATE {table} SET lease_until_ms = {} + ?, expires_at_ms = {} + ? WHERE scope_hash = ? AND key_hash = ? AND request_hash = ? AND owner_token = ? AND state = 'in_progress' AND lease_until_ms > {}",
                    now_ms_sql(backend),
                    now_ms_sql(backend),
                    now_ms_sql(backend)
                ),
                vec![
                    ttl_ms.into(),
                    ttl_ms.into(),
                    lease.scope_hash.clone().into(),
                    lease.key_hash.clone().into(),
                    lease.request_hash.clone().into(),
                    lease.owner_token.clone().into(),
                ],
            )
        } else {
            return Err(unsupported_backend());
        };
        let result = self
            .db
            .connection()?
            .execute_raw(Statement::from_sql_and_values(backend, &sql, values))
            .await
            .map_err(Error::internal)?;
        Ok(result.rows_affected() == 1)
    }

    /// 在业务事务中保存最长保留 24 小时的可重放结果；调用方必须同时提交业务写入。
    pub async fn complete_in_transaction(
        &self,
        transaction: &DatabaseTransaction,
        lease: &IdempotencyLease,
        response: &IdempotencyResponse,
        retention: Duration,
    ) -> Result<()> {
        validate_retention(retention)?;
        let body = serde_json::to_string(&response.body).map_err(Error::internal)?;
        if body.len() > MAX_RESPONSE_LEN {
            return Err(Error::BadRequest(
                "idempotency response exceeds 65536 bytes".into(),
            ));
        }
        let backend = transaction.get_database_backend();
        let table = self.db.quoted_table_name("request_idempotency")?;
        let retention_ms = ttl_millis(retention)?;
        let sql = match backend {
            DbBackend::Postgres => format!(
                "UPDATE {table} SET state = 'completed', lease_until_ms = 0, expires_at_ms = {} + $5, response_status = $6, response_body = $7 WHERE scope_hash = $1 AND key_hash = $2 AND request_hash = $3 AND owner_token = $4 AND state = 'in_progress' AND lease_until_ms > {}",
                now_ms_sql(backend),
                now_ms_sql(backend)
            ),
            DbBackend::MySql => format!(
                "UPDATE {table} SET state = 'completed', lease_until_ms = 0, expires_at_ms = {} + ?, response_status = ?, response_body = ? WHERE scope_hash = ? AND key_hash = ? AND request_hash = ? AND owner_token = ? AND state = 'in_progress' AND lease_until_ms > {}",
                now_ms_sql(backend),
                now_ms_sql(backend)
            ),
            _ => return Err(unsupported_backend()),
        };
        let values = if backend == DbBackend::Postgres {
            vec![
                lease.scope_hash.clone().into(),
                lease.key_hash.clone().into(),
                lease.request_hash.clone().into(),
                lease.owner_token.clone().into(),
                retention_ms.into(),
                <i16 as TryFrom<u16>>::try_from(response.status)
                    .map_err(Error::internal)?
                    .into(),
                body.into(),
            ]
        } else {
            vec![
                retention_ms.into(),
                <i16 as TryFrom<u16>>::try_from(response.status)
                    .map_err(Error::internal)?
                    .into(),
                body.into(),
                lease.scope_hash.clone().into(),
                lease.key_hash.clone().into(),
                lease.request_hash.clone().into(),
                lease.owner_token.clone().into(),
            ]
        };
        let result = transaction
            .execute_raw(Statement::from_sql_and_values(backend, &sql, values))
            .await
            .map_err(Error::internal)?;
        if result.rows_affected() == 1 {
            Ok(())
        } else {
            Err(Error::custom(
                "idempotency_lease_lost",
                "request idempotency lease expired or was acquired by another owner",
            ))
        }
    }

    /// 放弃尚未产生业务副作用的处理中记录；已接管的旧 owner 返回 `false`。
    pub async fn release(&self, lease: &IdempotencyLease) -> Result<bool> {
        let backend = self.db.backend()?;
        let table = self.db.quoted_table_name("request_idempotency")?;
        let sql = format!(
            "DELETE FROM {table} WHERE scope_hash = {} AND key_hash = {} AND request_hash = {} AND owner_token = {} AND state = 'in_progress'",
            placeholder(backend, 1),
            placeholder(backend, 2),
            placeholder(backend, 3),
            placeholder(backend, 4)
        );
        let result = self
            .db
            .connection()?
            .execute_raw(Statement::from_sql_and_values(
                backend,
                &sql,
                [
                    lease.scope_hash.clone().into(),
                    lease.key_hash.clone().into(),
                    lease.request_hash.clone().into(),
                    lease.owner_token.clone().into(),
                ],
            ))
            .await
            .map_err(Error::internal)?;
        Ok(result.rows_affected() == 1)
    }

    /// 清理最多 `limit` 条已过期记录；调用方可在维护任务中重复运行以限制每次清理开销。
    pub async fn prune_expired(&self, limit: u16) -> Result<u64> {
        if !(1..=1000).contains(&limit) {
            return Err(Error::BadRequest(
                "idempotency cleanup limit must be in 1..=1000".into(),
            ));
        }
        let backend = self.db.backend()?;
        let table = self.db.quoted_table_name("request_idempotency")?;
        let sql = match backend {
            DbBackend::Postgres => format!(
                "DELETE FROM {table} WHERE ctid IN (SELECT ctid FROM {table} WHERE expires_at_ms <= {} LIMIT $1)",
                now_ms_sql(backend)
            ),
            DbBackend::MySql => format!(
                "DELETE FROM {table} WHERE expires_at_ms <= {} LIMIT ?",
                now_ms_sql(backend)
            ),
            _ => return Err(unsupported_backend()),
        };
        let result = self
            .db
            .connection()?
            .execute_raw(Statement::from_sql_and_values(
                backend,
                &sql,
                [i64::from(limit).into()],
            ))
            .await
            .map_err(Error::internal)?;
        Ok(result.rows_affected())
    }
}

/// 请求幂等处理或回放的结果。
pub enum IdempotencyClaim {
    /// 此调用获得处理 lease。
    Acquired(IdempotencyLease),
    /// 相同请求仍由另一个 owner 处理。
    InProgress {
        /// 数据库时钟下的租约到期时刻。
        lease_until_ms: i64,
    },
    /// 同指纹请求已完成，可返回原结果。
    Replay(IdempotencyResponse),
    /// 同键对应不同请求指纹。
    Conflict,
}

/// 请求幂等的 owner lease；内部凭据不会显示在 Debug 中。
#[derive(Clone)]
pub struct IdempotencyLease {
    scope_hash: String,
    key_hash: String,
    request_hash: String,
    owner_token: String,
    lease_until_ms: i64,
}

impl IdempotencyLease {
    /// 返回数据库时钟下的 Unix 毫秒租约到期时刻。
    pub const fn lease_until_ms(&self) -> i64 {
        self.lease_until_ms
    }
}

impl fmt::Debug for IdempotencyLease {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IdempotencyLease")
            .field("owner_token", &"[REDACTED]")
            .field("lease_until_ms", &self.lease_until_ms)
            .finish_non_exhaustive()
    }
}

/// 幂等响应快照；仅支持有界 JSON 正文。
#[derive(Clone)]
pub struct IdempotencyResponse {
    status: u16,
    body: Value,
}

impl IdempotencyResponse {
    /// 创建 HTTP 状态与 JSON 正文快照。
    pub fn new(status: u16, body: Value) -> Result<Self> {
        if !(200..=599).contains(&status) {
            return Err(Error::BadRequest(
                "idempotency response status must be in 200..=599".into(),
            ));
        }
        if serde_json::to_vec(&body).map_err(Error::internal)?.len() > MAX_RESPONSE_LEN {
            return Err(Error::BadRequest(
                "idempotency response exceeds 65536 bytes".into(),
            ));
        }
        Ok(Self { status, body })
    }
    /// 返回需要重放的 HTTP 状态码。
    pub const fn status(&self) -> u16 {
        self.status
    }
    /// 返回需要重放的 JSON 正文。
    pub fn body(&self) -> &Value {
        &self.body
    }
}

fn lock_acquire_sql(backend: DbBackend, table: &str) -> Result<String> {
    Ok(match backend {
        DbBackend::Postgres => format!(
            "INSERT INTO {table} (key_hash, owner_token, expires_at_ms, fencing_token) VALUES ($1, $2, {} + $3, 1) ON CONFLICT (key_hash) DO UPDATE SET owner_token = EXCLUDED.owner_token, expires_at_ms = EXCLUDED.expires_at_ms, fencing_token = {table}.fencing_token + 1 WHERE {table}.expires_at_ms <= {} RETURNING owner_token, fencing_token, expires_at_ms",
            now_ms_sql(backend),
            now_ms_sql(backend)
        ),
        DbBackend::MySql => format!(
            "INSERT INTO {table} (key_hash, owner_token, expires_at_ms, fencing_token) VALUES (?, ?, {} + ?, 1) ON DUPLICATE KEY UPDATE fencing_token = IF(expires_at_ms <= {}, fencing_token + 1, fencing_token), owner_token = IF(expires_at_ms <= {}, VALUES(owner_token), owner_token), expires_at_ms = IF(expires_at_ms <= {}, VALUES(expires_at_ms), expires_at_ms)",
            now_ms_sql(backend),
            now_ms_sql(backend),
            now_ms_sql(backend),
            now_ms_sql(backend)
        ),
        _ => return Err(unsupported_backend()),
    })
}

fn idempotency_claim_sql(backend: DbBackend, table: &str) -> Result<String> {
    Ok(match backend {
        DbBackend::Postgres => format!(
            "INSERT INTO {table} (scope_hash, key_hash, request_hash, owner_token, state, lease_until_ms, expires_at_ms) VALUES ($1,$2,$3,$4,'in_progress',{} + $5,{} + $5) ON CONFLICT (scope_hash,key_hash) DO UPDATE SET request_hash=EXCLUDED.request_hash, owner_token=EXCLUDED.owner_token, state='in_progress', lease_until_ms=EXCLUDED.lease_until_ms, expires_at_ms=EXCLUDED.expires_at_ms, response_status=NULL, response_body=NULL WHERE {table}.expires_at_ms <= {}",
            now_ms_sql(backend),
            now_ms_sql(backend),
            now_ms_sql(backend)
        ),
        DbBackend::MySql => format!(
            "INSERT INTO {table} (scope_hash,key_hash,request_hash,owner_token,state,lease_until_ms,expires_at_ms) VALUES (?,?,?,?,'in_progress',{} + {},{} + {}) ON DUPLICATE KEY UPDATE request_hash=IF(expires_at_ms <= {},VALUES(request_hash),request_hash), owner_token=IF(expires_at_ms <= {},VALUES(owner_token),owner_token), state=IF(expires_at_ms <= {},'in_progress',state), lease_until_ms=IF(expires_at_ms <= {},VALUES(lease_until_ms),lease_until_ms), response_status=IF(expires_at_ms <= {},NULL,response_status), response_body=IF(expires_at_ms <= {},NULL,response_body), expires_at_ms=IF(expires_at_ms <= {},VALUES(expires_at_ms),expires_at_ms)",
            now_ms_sql(backend),
            placeholder(backend, 5),
            now_ms_sql(backend),
            placeholder(backend, 5),
            now_ms_sql(backend),
            now_ms_sql(backend),
            now_ms_sql(backend),
            now_ms_sql(backend),
            now_ms_sql(backend),
            now_ms_sql(backend),
            now_ms_sql(backend)
        ),
        _ => return Err(unsupported_backend()),
    })
}

fn now_ms_sql(backend: DbBackend) -> &'static str {
    match backend {
        DbBackend::Postgres => {
            "CAST(FLOOR(EXTRACT(EPOCH FROM clock_timestamp()) * 1000) AS BIGINT)"
        }
        DbBackend::MySql => "CAST(UNIX_TIMESTAMP(UTC_TIMESTAMP(6)) * 1000 AS SIGNED)",
        _ => "0",
    }
}
fn placeholder(backend: DbBackend, index: usize) -> String {
    match backend {
        DbBackend::Postgres => format!("${index}"),
        _ => "?".to_owned(),
    }
}
fn ttl_millis(ttl: Duration) -> Result<i64> {
    let millis = <u64 as TryFrom<u128>>::try_from(ttl.as_millis()).map_err(Error::internal)?;
    if millis == 0 || millis > MAX_TTL_MS {
        return Err(Error::BadRequest(
            "lease duration must be between 1 millisecond and 24 hours".into(),
        ));
    }
    <i64 as TryFrom<u64>>::try_from(millis).map_err(Error::internal)
}
fn validate_retention(ttl: Duration) -> Result<()> {
    ttl_millis(ttl).map(|_| ())
}
fn validate_text(name: &str, value: &str, limit: usize) -> Result<()> {
    if value.trim().is_empty()
        || value.len() > limit
        || value.bytes().any(|byte| byte.is_ascii_control())
    {
        Err(Error::BadRequest(format!(
            "{name} must be non-empty, at most {limit} bytes, and contain no control characters"
        )))
    } else {
        Ok(())
    }
}
fn digest(bytes: &[u8]) -> String {
    let hash = Sha256::digest(bytes);
    hex(&hash)
}
fn random_token() -> Result<String> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| {
        Error::custom(
            "random_source_unavailable",
            "operating system random source is unavailable",
        )
    })?;
    Ok(hex(&bytes))
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}
fn unsupported_backend() -> Error {
    Error::custom(
        "unsupported_coordination_backend",
        "database coordination requires PostgreSQL or MySQL",
    )
}
