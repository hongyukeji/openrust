//! 共享且一次性使用的 refresh token 家族。

use std::fmt;

use crate::core::{Error, Result};
use crate::database::sea_orm::{ConnectionTrait, DatabaseTransaction, DbBackend, Statement};
use crate::database::{DatabaseConnection, Db};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};

/// 基于数据库的 refresh token 生命周期操作。
#[derive(Clone)]
pub struct RefreshTokens {
    db: Db,
}

impl fmt::Debug for RefreshTokens {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RefreshTokens")
    }
}

impl RefreshTokens {
    /// 基于应用共享数据库创建 refresh token 服务。
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    #[cfg(any(feature = "auth-fortify", feature = "auth-oidc"))]
    pub(crate) fn require_store(&self, db: &Db) -> Result<()> {
        if !self.db.same_handle(db) {
            return Err(Error::Unauthorized);
        }
        for table in ["users", "auth_refresh_tokens", "personal_access_tokens"] {
            if self.db.table_name(table)? != db.table_name(table)? {
                return Err(Error::Unauthorized);
            }
        }
        Ok(())
    }

    /// 使用调用方指定的家族有效期创建 refresh token，并一次性返回该 token。
    ///
    /// 仅持久化其 SHA-256 摘要。有效期策略由调用方决定。
    pub async fn issue(
        &self,
        user_id: i64,
        expires_at: DateTime<Utc>,
    ) -> Result<IssuedRefreshToken> {
        if user_id <= 0 || expires_at <= Utc::now() {
            return Err(Error::BadRequest(
                "refresh token user ID must be positive and expiry must be in the future".into(),
            ));
        }
        let token = random_token()?;
        let family_id = random_hex(16)?;
        self.connection()?;
        let db = self.db.clone();
        self.db
            .transaction_for_update(|transaction| {
                let db = db.clone();
                let token = token.clone();
                let family_id = family_id.clone();
                Box::pin(async move {
                    issue_in_transaction(transaction, &db, user_id, expires_at, &token, &family_id)
                        .await
                })
            })
            .await?;
        Ok(IssuedRefreshToken { value: token })
    }

    pub(crate) async fn issue_in_transaction(
        &self,
        transaction: &DatabaseTransaction,
        user_id: i64,
        expires_at: DateTime<Utc>,
    ) -> Result<IssuedRefreshToken> {
        if user_id <= 0 || expires_at <= Utc::now() {
            return Err(Error::BadRequest(
                "refresh token user ID must be positive and expiry must be in the future".into(),
            ));
        }
        let token = random_token()?;
        let family_id = random_hex(16)?;
        issue_in_transaction(
            transaction,
            &self.db,
            user_id,
            expires_at,
            &token,
            &family_id,
        )
        .await?;
        Ok(IssuedRefreshToken { value: token })
    }

    /// 原子消费 refresh token 并返回其唯一后继 token。
    ///
    /// 重用已消费的 token 会提交家族级撤销，并返回 `Unauthorized`。
    pub async fn rotate(&self, token: &str) -> Result<RefreshRotation> {
        let next_token = random_token()?;
        let next_hash = sha256_hex(&next_token);
        let token_hash = sha256_hex(token);
        let db = self.db.clone();
        let decision = self
            .db
            .transaction_for_update(|transaction| {
                let db = db.clone();
                Box::pin(async move {
                    rotate_in_transaction(transaction, &db, &token_hash, &next_hash).await
                })
            })
            .await?;
        match decision {
            RotationDecision::Rotated {
                user_id,
                expires_at,
            } => Ok(RefreshRotation {
                user_id,
                token: IssuedRefreshToken { value: next_token },
                expires_at,
            }),
            RotationDecision::Rejected => Err(Error::Unauthorized),
        }
    }

    /// 撤销包含 `token` 的家族中的所有 token。
    pub async fn revoke(&self, token: &str) -> Result<()> {
        self.connection()?;
        let token_hash = sha256_hex(token);
        let db = self.db.clone();
        self.db
            .transaction_for_update(|transaction| {
                let db = db.clone();
                Box::pin(async move {
                    let backend = transaction.get_database_backend();
                    let sql = match backend {
                        DbBackend::Postgres => "SELECT user_id, family_id FROM tb_auth_refresh_tokens WHERE token_hash = $1",
                        DbBackend::MySql | DbBackend::Sqlite => "SELECT user_id, family_id FROM tb_auth_refresh_tokens WHERE token_hash = ?",
                        _ => return Err(unsupported_backend()),
                    };
                    let sql = crate::auth::resolve_auth_sql(&db, sql)?;
                    let row = transaction
                        .query_one_raw(Statement::from_sql_and_values(
                            backend,
                            &sql,
                            [token_hash.into()],
                        ))
                        .await
                        .map_err(Error::internal)?
                        .ok_or(Error::Unauthorized)?;
                    let user_id = row.try_get::<i64>("", "user_id").map_err(Error::internal)?;
                    let family_id = row
                        .try_get::<String>("", "family_id")
                        .map_err(Error::internal)?;
                    lock_user(transaction, &db, user_id).await?;
                    revoke_family_in_transaction(transaction, &db, backend, &family_id).await
                })
            })
            .await
    }

    /// 撤销账户的所有 refresh token 家族，并返回受影响的行数。
    pub async fn revoke_all_for_user(&self, user_id: i64) -> Result<u64> {
        self.connection()?;
        if user_id <= 0 {
            return Err(Error::BadRequest("user ID must be positive".into()));
        }
        let db = self.db.clone();
        self.db
            .transaction_for_update(|transaction| {
                Box::pin(async move {
                    revoke_all_for_user_in_transaction(transaction, &db, user_id).await
                })
            })
            .await
    }

    // 这里只取签名候选的账户，不提前过滤已消费/过期状态；
    // rotate的同一事务必须观察重放并提交家族撤销，不能被前置查询短路。
    pub(crate) async fn owner_id_for_rotation(&self, token: &str) -> Result<i64> {
        let connection = self.connection()?;
        let backend = connection.get_database_backend();
        let sql = match backend {
            DbBackend::Postgres => "SELECT user_id FROM tb_auth_refresh_tokens WHERE token_hash=$1",
            DbBackend::MySql | DbBackend::Sqlite => {
                "SELECT user_id FROM tb_auth_refresh_tokens WHERE token_hash=?"
            }
            _ => return Err(unsupported_backend()),
        };
        let row = connection
            .query_one_raw(Statement::from_sql_and_values(
                backend,
                crate::auth::resolve_auth_sql(&self.db, sql)?,
                [sha256_hex(token).into()],
            ))
            .await
            .map_err(Error::internal)?
            .ok_or(Error::Unauthorized)?;
        row.try_get("", "user_id").map_err(Error::internal)
    }

    fn connection(&self) -> Result<DatabaseConnection> {
        let connection = self.db.connection()?;
        match connection.get_database_backend() {
            DbBackend::Postgres | DbBackend::MySql | DbBackend::Sqlite => Ok(connection),
            _ => Err(unsupported_backend()),
        }
    }
}

async fn issue_in_transaction(
    transaction: &DatabaseTransaction,
    db: &Db,
    user_id: i64,
    expires_at: DateTime<Utc>,
    token: &str,
    family_id: &str,
) -> Result<()> {
    if user_id <= 0 || expires_at <= Utc::now() {
        return Err(Error::BadRequest(
            "refresh token user ID must be positive and expiry must be in the future".into(),
        ));
    }
    lock_user(transaction, db, user_id).await?;
    insert_token(
        transaction,
        db,
        user_id,
        family_id,
        &sha256_hex(token),
        expires_at,
    )
    .await
}

pub(crate) async fn revoke_all_for_user_in_transaction(
    transaction: &DatabaseTransaction,
    db: &Db,
    user_id: i64,
) -> Result<u64> {
    if user_id <= 0 {
        return Err(Error::BadRequest("user ID must be positive".into()));
    }
    lock_user(transaction, db, user_id).await?;
    let backend = transaction.get_database_backend();
    if backend == DbBackend::Sqlite {
        let sql = crate::auth::resolve_auth_sql(
            db,
            "UPDATE tb_auth_refresh_tokens SET revoked_at=? WHERE user_id=? AND revoked_at IS NULL",
        )?;
        let result = transaction
            .execute_raw(Statement::from_sql_and_values(
                backend,
                sql,
                [
                    crate::database::dialect::utc_value(backend, Utc::now()),
                    user_id.into(),
                ],
            ))
            .await
            .map_err(Error::internal)?;
        return Ok(result.rows_affected());
    }
    let sql = match backend {
        DbBackend::Postgres => {
            "UPDATE tb_auth_refresh_tokens SET revoked_at = CURRENT_TIMESTAMP WHERE user_id = $1 AND revoked_at IS NULL"
        }
        DbBackend::MySql => {
            "UPDATE tb_auth_refresh_tokens SET revoked_at = CURRENT_TIMESTAMP WHERE user_id = ? AND revoked_at IS NULL"
        }
        _ => return Err(unsupported_backend()),
    };
    let sql = crate::auth::resolve_auth_sql(db, sql)?;
    let result = transaction
        .execute_raw(Statement::from_sql_and_values(
            backend,
            &sql,
            [user_id.into()],
        ))
        .await
        .map_err(Error::internal)?;
    Ok(result.rows_affected())
}

/// 仅在签发或轮换 token 时返回的 refresh 凭据。
pub struct IssuedRefreshToken {
    value: String,
}

impl IssuedRefreshToken {
    /// 暴露 bearer 密钥，供响应发送给客户端。
    pub fn expose(&self) -> &str {
        &self.value
    }
}

impl fmt::Debug for IssuedRefreshToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("IssuedRefreshToken")
            .field("value", &"[REDACTED]")
            .finish()
    }
}

/// 轮换后返回的账户、后继 refresh 凭据及未改变的家族有效期。
pub struct RefreshRotation {
    user_id: i64,
    token: IssuedRefreshToken,
    expires_at: DateTime<Utc>,
}

impl RefreshRotation {
    /// 返回拥有此 token 家族的账户。
    pub const fn user_id(&self) -> i64 {
        self.user_id
    }

    /// 返回一次性后继凭据。
    pub const fn token(&self) -> &IssuedRefreshToken {
        &self.token
    }

    /// 返回轮换不会延长的家族有效期。
    pub const fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }

    pub(crate) fn into_parts(self) -> (i64, IssuedRefreshToken, DateTime<Utc>) {
        (self.user_id, self.token, self.expires_at)
    }
}

impl fmt::Debug for RefreshRotation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RefreshRotation")
            .field("user_id", &self.user_id)
            .field("token", &self.token)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

enum RotationDecision {
    Rotated {
        user_id: i64,
        expires_at: DateTime<Utc>,
    },
    Rejected,
}

// 将锁定、校验、消费和插入后继 token 放在同一事务中，以保持事务顺序。
#[allow(clippy::too_many_lines)]
async fn rotate_in_transaction(
    transaction: &DatabaseTransaction,
    db: &Db,
    token_hash: &str,
    next_hash: &str,
) -> Result<RotationDecision> {
    let backend = transaction.get_database_backend();
    if backend == DbBackend::Sqlite {
        return rotate_sqlite(transaction, db, token_hash, next_hash).await;
    }
    let owner_sql = match backend {
        DbBackend::Postgres => "SELECT user_id FROM tb_auth_refresh_tokens WHERE token_hash = $1",
        DbBackend::MySql => "SELECT user_id FROM tb_auth_refresh_tokens WHERE token_hash = ?",
        _ => return Err(unsupported_backend()),
    };
    let owner_sql = crate::auth::resolve_auth_sql(db, owner_sql)?;
    let Some(owner) = transaction
        .query_one_raw(Statement::from_sql_and_values(
            backend,
            &owner_sql,
            [token_hash.to_owned().into()],
        ))
        .await
        .map_err(Error::internal)?
    else {
        return Ok(RotationDecision::Rejected);
    };
    let owner_id = owner
        .try_get::<i64>("", "user_id")
        .map_err(Error::internal)?;
    lock_user(transaction, db, owner_id).await?;
    let select_sql = match backend {
        DbBackend::Postgres => {
            "SELECT user_id, family_id, expires_at, consumed_at, revoked_at, expires_at > CURRENT_TIMESTAMP AS unexpired FROM tb_auth_refresh_tokens WHERE token_hash = $1 FOR UPDATE"
        }
        DbBackend::MySql => {
            "SELECT user_id, family_id, expires_at, consumed_at, revoked_at, expires_at > CURRENT_TIMESTAMP AS unexpired FROM tb_auth_refresh_tokens WHERE token_hash = ? FOR UPDATE"
        }
        _ => return Err(unsupported_backend()),
    };
    let select_sql = crate::auth::resolve_auth_sql(db, select_sql)?;
    let Some(row) = transaction
        .query_one_raw(Statement::from_sql_and_values(
            backend,
            &select_sql,
            [token_hash.to_owned().into()],
        ))
        .await
        .map_err(Error::internal)?
    else {
        return Ok(RotationDecision::Rejected);
    };
    let user_id = row.try_get::<i64>("", "user_id").map_err(Error::internal)?;
    let family_id = row
        .try_get::<String>("", "family_id")
        .map_err(Error::internal)?;
    let expires_at = row
        .try_get::<DateTime<Utc>>("", "expires_at")
        .map_err(Error::internal)?;
    let consumed_at = row
        .try_get::<Option<DateTime<Utc>>>("", "consumed_at")
        .map_err(Error::internal)?;
    let revoked_at = row
        .try_get::<Option<DateTime<Utc>>>("", "revoked_at")
        .map_err(Error::internal)?;
    let unexpired = row
        .try_get::<bool>("", "unexpired")
        .map_err(Error::internal)?;
    if consumed_at.is_some() || revoked_at.is_some() || !unexpired {
        revoke_family_in_transaction(transaction, db, backend, &family_id).await?;
        return Ok(RotationDecision::Rejected);
    }

    let update_sql = match backend {
        DbBackend::Postgres => {
            "UPDATE tb_auth_refresh_tokens SET consumed_at = CURRENT_TIMESTAMP, replaced_by_hash = $1 WHERE token_hash = $2 AND consumed_at IS NULL AND revoked_at IS NULL AND expires_at > CURRENT_TIMESTAMP"
        }
        DbBackend::MySql => {
            "UPDATE tb_auth_refresh_tokens SET consumed_at = CURRENT_TIMESTAMP, replaced_by_hash = ? WHERE token_hash = ? AND consumed_at IS NULL AND revoked_at IS NULL AND expires_at > CURRENT_TIMESTAMP"
        }
        _ => return Err(unsupported_backend()),
    };
    let update_sql = crate::auth::resolve_auth_sql(db, update_sql)?;
    let update = transaction
        .execute_raw(Statement::from_sql_and_values(
            backend,
            &update_sql,
            [next_hash.to_owned().into(), token_hash.to_owned().into()],
        ))
        .await
        .map_err(Error::internal)?;
    if update.rows_affected() != 1 {
        revoke_family_in_transaction(transaction, db, backend, &family_id).await?;
        return Ok(RotationDecision::Rejected);
    }

    let insert_sql = match backend {
        DbBackend::Postgres => {
            "INSERT INTO tb_auth_refresh_tokens (user_id, family_id, token_hash, expires_at, created_at) VALUES ($1, $2, $3, $4, CURRENT_TIMESTAMP)"
        }
        DbBackend::MySql => {
            "INSERT INTO tb_auth_refresh_tokens (user_id, family_id, token_hash, expires_at, created_at) VALUES (?, ?, ?, ?, CURRENT_TIMESTAMP)"
        }
        _ => return Err(unsupported_backend()),
    };
    let insert_sql = crate::auth::resolve_auth_sql(db, insert_sql)?;
    transaction
        .execute_raw(Statement::from_sql_and_values(
            backend,
            &insert_sql,
            [
                user_id.into(),
                family_id.into(),
                next_hash.to_owned().into(),
                expires_at.into(),
            ],
        ))
        .await
        .map_err(Error::internal)?;
    Ok(RotationDecision::Rotated {
        user_id,
        expires_at,
    })
}

// 首次 owner 读取前已经持有 Immediate 写事务；精确时间在取得写所有权后采样。
async fn rotate_sqlite(
    transaction: &DatabaseTransaction,
    db: &Db,
    token_hash: &str,
    next_hash: &str,
) -> Result<RotationDecision> {
    let now = Utc::now();
    let sql = crate::auth::resolve_auth_sql(
        db,
        "SELECT user_id,family_id,expires_at,consumed_at,revoked_at FROM tb_auth_refresh_tokens WHERE token_hash=?",
    )?;
    let Some(row) = transaction
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [token_hash.into()],
        ))
        .await
        .map_err(Error::internal)?
    else {
        return Ok(RotationDecision::Rejected);
    };
    let user_id = row.try_get::<i64>("", "user_id").map_err(Error::internal)?;
    lock_user(transaction, db, user_id).await?;
    let family = row
        .try_get::<String>("", "family_id")
        .map_err(Error::internal)?;
    let expiry = row
        .try_get::<DateTime<Utc>>("", "expires_at")
        .map_err(Error::internal)?;
    let consumed = row
        .try_get::<Option<DateTime<Utc>>>("", "consumed_at")
        .map_err(Error::internal)?;
    let revoked = row
        .try_get::<Option<DateTime<Utc>>>("", "revoked_at")
        .map_err(Error::internal)?;
    if consumed.is_some() || revoked.is_some() || expiry <= now {
        revoke_family_in_transaction(transaction, db, DbBackend::Sqlite, &family).await?;
        return Ok(RotationDecision::Rejected);
    }
    // expiry 条件由同一写事务内的 Chrono 比较兑现，不混比外部 RFC3339/空格文本。
    let sql = crate::auth::resolve_auth_sql(
        db,
        "UPDATE tb_auth_refresh_tokens SET consumed_at=?,replaced_by_hash=? WHERE token_hash=? AND consumed_at IS NULL AND revoked_at IS NULL",
    )?;
    let updated = transaction
        .execute_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [
                crate::database::dialect::utc_value(DbBackend::Sqlite, now),
                next_hash.into(),
                token_hash.into(),
            ],
        ))
        .await
        .map_err(Error::internal)?;
    if updated.rows_affected() != 1 {
        revoke_family_in_transaction(transaction, db, DbBackend::Sqlite, &family).await?;
        return Ok(RotationDecision::Rejected);
    }
    insert_token(transaction, db, user_id, &family, next_hash, expiry).await?;
    Ok(RotationDecision::Rotated {
        user_id,
        expires_at: expiry,
    })
}

async fn lock_user(transaction: &DatabaseTransaction, db: &Db, user_id: i64) -> Result<()> {
    let backend = transaction.get_database_backend();
    let sql = match backend {
        DbBackend::Postgres => "SELECT id FROM tb_users WHERE id = $1 FOR UPDATE",
        DbBackend::MySql => "SELECT id FROM tb_users WHERE id = ? FOR UPDATE",
        // SQLite 调用方须已由 transaction_for_update 在首次读取前取得写所有权。
        DbBackend::Sqlite => "SELECT id FROM tb_users WHERE id = ?",
        _ => return Err(unsupported_backend()),
    };
    let sql = crate::auth::resolve_auth_sql(db, sql)?;
    transaction
        .query_one_raw(Statement::from_sql_and_values(
            backend,
            &sql,
            [user_id.into()],
        ))
        .await
        .map_err(Error::internal)?
        .ok_or(Error::Unauthorized)?;
    Ok(())
}

async fn insert_token(
    transaction: &DatabaseTransaction,
    db: &Db,
    user_id: i64,
    family_id: &str,
    token_hash: &str,
    expires_at: DateTime<Utc>,
) -> Result<()> {
    let backend = transaction.get_database_backend();
    if backend == DbBackend::Sqlite {
        let sql = crate::auth::resolve_auth_sql(
            db,
            "INSERT INTO tb_auth_refresh_tokens (user_id,family_id,token_hash,expires_at,created_at) VALUES (?,?,?,?,?)",
        )?;
        transaction
            .execute_raw(Statement::from_sql_and_values(
                backend,
                sql,
                [
                    user_id.into(),
                    family_id.into(),
                    token_hash.into(),
                    crate::database::dialect::utc_value(backend, expires_at),
                    crate::database::dialect::utc_value(backend, Utc::now()),
                ],
            ))
            .await
            .map_err(Error::internal)?;
        return Ok(());
    }
    let sql = match backend {
        DbBackend::Postgres => {
            "INSERT INTO tb_auth_refresh_tokens (user_id, family_id, token_hash, expires_at, created_at) VALUES ($1, $2, $3, $4, CURRENT_TIMESTAMP)"
        }
        DbBackend::MySql => {
            "INSERT INTO tb_auth_refresh_tokens (user_id, family_id, token_hash, expires_at, created_at) VALUES (?, ?, ?, ?, CURRENT_TIMESTAMP)"
        }
        _ => return Err(unsupported_backend()),
    };
    let sql = crate::auth::resolve_auth_sql(db, sql)?;
    transaction
        .execute_raw(Statement::from_sql_and_values(
            backend,
            &sql,
            [
                user_id.into(),
                family_id.to_owned().into(),
                token_hash.to_owned().into(),
                expires_at.into(),
            ],
        ))
        .await
        .map_err(Error::internal)?;
    Ok(())
}

pub(crate) async fn revoke_family_in_transaction(
    transaction: &DatabaseTransaction,
    db: &Db,
    backend: DbBackend,
    family_id: &str,
) -> Result<()> {
    let owner_sql = match backend {
        DbBackend::Postgres => {
            "SELECT user_id FROM tb_auth_refresh_tokens WHERE family_id = $1 LIMIT 1"
        }
        DbBackend::MySql | DbBackend::Sqlite => {
            "SELECT user_id FROM tb_auth_refresh_tokens WHERE family_id = ? LIMIT 1"
        }
        _ => return Err(unsupported_backend()),
    };
    let owner_sql = crate::auth::resolve_auth_sql(db, owner_sql)?;
    let Some(owner) = transaction
        .query_one_raw(Statement::from_sql_and_values(
            backend,
            &owner_sql,
            [family_id.to_owned().into()],
        ))
        .await
        .map_err(Error::internal)?
    else {
        return Ok(());
    };
    let owner_id = owner
        .try_get::<i64>("", "user_id")
        .map_err(Error::internal)?;
    // Rotation locks the account row before inserting its successor. Share that
    // serialization point so this family update cannot miss a concurrent insert.
    lock_user(transaction, db, owner_id).await?;
    if backend == DbBackend::Sqlite {
        let sql = crate::auth::resolve_auth_sql(
            db,
            "UPDATE tb_auth_refresh_tokens SET revoked_at=? WHERE family_id=? AND revoked_at IS NULL",
        )?;
        transaction
            .execute_raw(Statement::from_sql_and_values(
                backend,
                sql,
                [
                    crate::database::dialect::utc_value(backend, Utc::now()),
                    family_id.into(),
                ],
            ))
            .await
            .map_err(Error::internal)?;
        return Ok(());
    }
    let sql = match backend {
        DbBackend::Postgres => {
            "UPDATE tb_auth_refresh_tokens SET revoked_at = CURRENT_TIMESTAMP WHERE family_id = $1 AND revoked_at IS NULL"
        }
        DbBackend::MySql => {
            "UPDATE tb_auth_refresh_tokens SET revoked_at = CURRENT_TIMESTAMP WHERE family_id = ? AND revoked_at IS NULL"
        }
        _ => return Err(unsupported_backend()),
    };
    let sql = crate::auth::resolve_auth_sql(db, sql)?;
    transaction
        .execute_raw(Statement::from_sql_and_values(
            backend,
            &sql,
            [family_id.to_owned().into()],
        ))
        .await
        .map_err(Error::internal)?;
    Ok(())
}

// 调用方已按统一锁序持有expected_user_id账户行；此处不能再锁另一个账户。
#[cfg(feature = "auth-oidc")]
pub(crate) async fn revoke_family_for_locked_owner_in_transaction(
    transaction: &DatabaseTransaction,
    db: &Db,
    backend: DbBackend,
    family_id: &str,
    expected_user_id: i64,
) -> Result<()> {
    if backend == DbBackend::Postgres {
        let isolation = transaction
            .query_one_raw(Statement::from_string(
                backend,
                "SELECT current_setting('transaction_isolation') AS isolation",
            ))
            .await
            .map_err(Error::internal)?
            .ok_or(Error::Unauthorized)?;
        let isolation: String = isolation
            .try_get("", "isolation")
            .map_err(Error::internal)?;
        // 旧快照中的空结果不能证明当前家族不存在；不擅改调用方事务隔离级别。
        if isolation != "read committed" {
            return Err(Error::custom_with_status(
                crate::http::axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "auth_refresh_isolation_unsupported",
                "已锁账户的家族撤销当前要求PostgreSQL READ COMMITTED事务",
            ));
        }
    }
    let owner_sql = match backend {
        DbBackend::Postgres => {
            "SELECT user_id FROM tb_auth_refresh_tokens WHERE family_id=$1 ORDER BY token_hash FOR UPDATE"
        }
        DbBackend::MySql => {
            "SELECT user_id FROM tb_auth_refresh_tokens WHERE family_id=? ORDER BY token_hash FOR UPDATE"
        }
        DbBackend::Sqlite => {
            "SELECT user_id FROM tb_auth_refresh_tokens WHERE family_id=? ORDER BY token_hash"
        }
        _ => return Err(unsupported_backend()),
    };
    // MySQL的locking read必须读当前值，不能用此前consistent snapshot把新家族判成不存在。
    // PostgreSQL调用链沿默认READ COMMITTED；不把本接口当作提升事务隔离级别的机制。
    let rows = transaction
        .query_all_raw(Statement::from_sql_and_values(
            backend,
            crate::auth::resolve_auth_sql(db, owner_sql)?,
            [family_id.into()],
        ))
        .await
        .map_err(Error::internal)?;
    if rows.is_empty() {
        return Ok(());
    }
    for row in rows {
        if row.try_get::<i64>("", "user_id").map_err(Error::internal)? != expected_user_id {
            return Err(Error::Unauthorized);
        }
    }
    let (sql, values): (&str, Vec<crate::database::sea_orm::Value>) = match backend {
        DbBackend::Postgres => (
            "UPDATE tb_auth_refresh_tokens SET revoked_at=CURRENT_TIMESTAMP WHERE family_id=$1 AND user_id=$2 AND revoked_at IS NULL",
            vec![family_id.into(), expected_user_id.into()],
        ),
        DbBackend::MySql => (
            "UPDATE tb_auth_refresh_tokens SET revoked_at=CURRENT_TIMESTAMP WHERE family_id=? AND user_id=? AND revoked_at IS NULL",
            vec![family_id.into(), expected_user_id.into()],
        ),
        DbBackend::Sqlite => (
            "UPDATE tb_auth_refresh_tokens SET revoked_at=? WHERE family_id=? AND user_id=? AND revoked_at IS NULL",
            vec![
                crate::database::dialect::utc_value(backend, Utc::now()),
                family_id.into(),
                expected_user_id.into(),
            ],
        ),
        _ => return Err(unsupported_backend()),
    };
    transaction
        .execute_raw(Statement::from_sql_and_values(
            backend,
            crate::auth::resolve_auth_sql(db, sql)?,
            values,
        ))
        .await
        .map_err(Error::internal)?;
    Ok(())
}

fn random_token() -> Result<String> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(Error::internal)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn random_hex(length: usize) -> Result<String> {
    let mut bytes = vec![0_u8; length];
    getrandom::fill(&mut bytes).map_err(Error::internal)?;
    Ok(hex(&bytes))
}

fn sha256_hex(value: &str) -> String {
    hex(&Sha256::digest(value.as_bytes()))
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
        "unsupported_auth_database",
        "refresh tokens require PostgreSQL, MySQL or SQLite",
    )
}

#[cfg(all(test, feature = "db-sqlite"))]
mod sqlite_tests {
    use super::*;
    use crate::database::sea_orm_migration::{MigrationTrait, MigratorTrait};
    use crate::{support::BoxFuture, testing::TestDatabase};

    struct RefreshMigrator;
    impl MigratorTrait for RefreshMigrator {
        fn migrations() -> Vec<Box<dyn MigrationTrait>> {
            vec![
                Box::new(crate::auth::AuthMigration),
                Box::new(crate::auth::AuthRefreshTokenMigration),
            ]
        }
    }
    fn check(condition: bool, message: &'static str) -> Result<()> {
        if condition {
            Ok(())
        } else {
            Err(Error::custom("sqlite_refresh_contract_failed", message))
        }
    }
    async fn owned<F>(callback: F) -> Result<()>
    where
        F: for<'a> FnOnce(&'a Db, i64) -> BoxFuture<'a, Result<()>>,
    {
        let guard = TestDatabase::sqlite_file::<RefreshMigrator>().await?;
        let db = Db::default();
        let outcome = async {
            db.initialize(guard.connection()?)?;
            let table = db.quoted_table_name("users")?;
            let now = crate::database::dialect::utc_value(DbBackend::Sqlite, Utc::now());
            let inserted = db.connection()?.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite,
                format!("INSERT INTO {table}(email,password_hash,created_at,updated_at) VALUES (?,?,?,?)"),
                ["refresh-owned@example.test".into(),"unused-test-hash".into(),now.clone(),now]))
                .await.map_err(Error::internal)?;
            callback(&db, i64::try_from(inserted.last_insert_id()).map_err(Error::internal)?).await
        }.await;
        drop(db);
        let cleanup = guard.close().await;
        outcome?;
        cleanup
    }

    #[tokio::test]
    async fn sqlite_refresh_rotation_is_single_use_and_replay_revokes_family() -> Result<()> {
        owned(|db, user| {
            Box::pin(async move {
                let refresh = RefreshTokens::new(db.clone());
                let deadline = Utc::now() + chrono::Duration::hours(1);
                let issued = refresh.issue(user, deadline).await?;
                let rotated = refresh.rotate(issued.expose()).await?;
                check(rotated.user_id() == user, "轮换改变账户")?;
                check(
                    rotated.expires_at().timestamp_micros() == deadline.timestamp_micros(),
                    "轮换延长或改变家族微秒截止",
                )?;
                check(
                    refresh.rotate(issued.expose()).await.is_err(),
                    "已消费原 token 被重复接受",
                )?;
                check(
                    refresh.rotate(rotated.token().expose()).await.is_err(),
                    "重放后家族后继未撤销",
                )?;
                let other = refresh.issue(user, deadline).await?;
                check(
                    refresh.revoke_all_for_user(user).await? > 0,
                    "账户撤销未命中任何记录",
                )?;
                check(
                    refresh.rotate(other.expose()).await.is_err(),
                    "账户撤销后 token 仍可用",
                )?;
                Ok(())
            })
        })
        .await
    }

    #[tokio::test]
    async fn sqlite_refresh_concurrent_rotation_has_one_successor_and_offset_expiry_is_exact()
    -> Result<()> {
        owned(|db, user| {
            Box::pin(async move {
                let refresh = RefreshTokens::new(db.clone());
                let issued = refresh
                    .issue(user, Utc::now() + chrono::Duration::hours(1))
                    .await?;
                let (first, second) = tokio::join!(
                    refresh.rotate(issued.expose()),
                    refresh.rotate(issued.expose())
                );
                check(
                    usize::from(first.is_ok()) + usize::from(second.is_ok()) == 1,
                    "并发轮换不止一个成功后继或全部失败",
                )?;
                let winner = [first, second]
                    .into_iter()
                    .find_map(std::result::Result::ok)
                    .ok_or(Error::Unauthorized)?;
                check(
                    refresh.rotate(winner.token().expose()).await.is_err(),
                    "并发重放未撤销已产生的家族后继",
                )?;
                let expired = refresh
                    .issue(user, Utc::now() + chrono::Duration::hours(1))
                    .await?;
                let zone = chrono::FixedOffset::east_opt(8 * 3600).ok_or(Error::Unauthorized)?;
                let precise = (Utc::now() - chrono::Duration::microseconds(1))
                    .with_timezone(&zone)
                    .to_rfc3339_opts(chrono::SecondsFormat::Micros, false);
                let table = db.quoted_table_name("auth_refresh_tokens")?;
                db.connection()?
                    .execute_raw(Statement::from_sql_and_values(
                        DbBackend::Sqlite,
                        format!("UPDATE {table} SET expires_at=? WHERE token_hash=?"),
                        [precise.into(), sha256_hex(expired.expose()).into()],
                    ))
                    .await
                    .map_err(Error::internal)?;
                check(
                    refresh.rotate(expired.expose()).await.is_err(),
                    "带offset已过微秒截止被词法比较延长",
                )?;
                Ok(())
            })
        })
        .await
    }
}

#[cfg(all(
    test,
    feature = "auth-oidc",
    any(feature = "db-mysql", feature = "db-postgres")
))]
mod locked_owner_tests {
    use super::*;
    use crate::{
        database::{
            sea_orm::{IsolationLevel, TransactionTrait},
            sea_orm_migration::{MigrationTrait, MigratorTrait},
        },
        testing::TestDatabase,
    };

    struct Migrator;
    impl MigratorTrait for Migrator {
        fn migrations() -> Vec<Box<dyn MigrationTrait>> {
            vec![
                Box::new(crate::auth::AuthMigration),
                Box::new(crate::auth::AuthRefreshTokenMigration),
            ]
        }
    }

    async fn unrevoked(connection: &DatabaseConnection, family: &str) -> Result<i64> {
        let backend = connection.get_database_backend();
        let sql = match backend {
            DbBackend::Postgres => {
                "SELECT COUNT(*) AS count FROM auth_refresh_tokens WHERE family_id=$1 AND revoked_at IS NULL"
            }
            _ => {
                "SELECT COUNT(*) AS count FROM auth_refresh_tokens WHERE family_id=? AND revoked_at IS NULL"
            }
        };
        connection
            .query_one_raw(Statement::from_sql_and_values(
                backend,
                sql,
                [family.into()],
            ))
            .await
            .map_err(Error::internal)?
            .ok_or(Error::Unauthorized)?
            .try_get("", "count")
            .map_err(Error::internal)
    }

    #[tokio::test]
    #[ignore = "需要真实MySQL/PostgreSQL及OPENRUST_TEST_DATABASE_URL；MySQL显式REPEATABLE READ验证旧快照"]
    #[allow(
        clippy::too_many_lines,
        reason = "同一真实事务覆盖快照后提交家族、owner错配及幂等撤销"
    )]
    async fn current_family_read_revokes_after_old_snapshot_and_rejects_wrong_owner() -> Result<()>
    {
        let url = crate::test_database_environment::database_url().map_err(Error::internal)?;
        let database = TestDatabase::create::<Migrator>(&url).await?;
        let outcome: Result<()> = async {
            let connection = database.connection()?;
            let backend = connection.get_database_backend();
            assert!(matches!(backend, DbBackend::MySql | DbBackend::Postgres));
            connection.execute_unprepared("INSERT INTO users(id,email,password_hash,created_at,updated_at) VALUES(1,'one@example.test','hash',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP),(2,'two@example.test','hash',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)").await.map_err(Error::internal)?;
            let db = Db::default(); db.initialize(connection.clone())?;
            let isolation = if backend == DbBackend::MySql { IsolationLevel::RepeatableRead } else { IsolationLevel::ReadCommitted };
            let transaction = connection.begin_with_config(Some(isolation), None).await.map_err(Error::internal)?;
            let snapshot_sql = "SELECT COUNT(*) AS count FROM auth_refresh_tokens";
            assert_eq!(transaction.query_one_raw(Statement::from_string(backend, snapshot_sql)).await.map_err(Error::internal)?.ok_or(Error::Unauthorized)?.try_get::<i64>("", "count").map_err(Error::internal)?, 0);
            // 第二个池连接在旧事务快照建立之后提交家族，不能靠同事务插入制造假阳性。
            let insert_db = db.clone();
            db.transaction_for_update(move |transaction| {
                let db = insert_db.clone();
                Box::pin(async move {
                    lock_user(transaction, &db, 1).await?;
                    for token in ["first", "second"] {
                        insert_token(transaction, &db, 1, "new-family", &sha256_hex(token), Utc::now()+chrono::Duration::hours(1)).await?;
                    }
                    lock_user(transaction, &db, 2).await?;
                    insert_token(transaction, &db, 2, "other-family", &sha256_hex("other"), Utc::now()+chrono::Duration::hours(1)).await
                })
            }).await?;
            if backend == DbBackend::MySql {
                assert_eq!(transaction.query_one_raw(Statement::from_string(backend, snapshot_sql)).await.map_err(Error::internal)?.ok_or(Error::Unauthorized)?.try_get::<i64>("", "count").map_err(Error::internal)?, 0);
            }
            lock_user(&transaction, &db, 1).await?;
            revoke_family_for_locked_owner_in_transaction(&transaction, &db, backend, "new-family", 1).await?;
            revoke_family_for_locked_owner_in_transaction(&transaction, &db, backend, "new-family", 1).await?;
            revoke_family_for_locked_owner_in_transaction(&transaction, &db, backend, "already-cleaned", 1).await?;
            transaction.commit().await.map_err(Error::internal)?;
            assert_eq!(unrevoked(&connection, "new-family").await?, 0);
            assert_eq!(unrevoked(&connection, "other-family").await?, 1);
            if backend == DbBackend::Postgres {
                let unsupported = connection.begin_with_config(Some(IsolationLevel::RepeatableRead), None).await.map_err(Error::internal)?;
                lock_user(&unsupported, &db, 1).await?;
                let error = revoke_family_for_locked_owner_in_transaction(&unsupported, &db, backend, "missing-in-old-snapshot", 1).await.err().ok_or(Error::Unauthorized)?;
                assert_eq!(error.code(), "auth_refresh_isolation_unsupported");
                unsupported.rollback().await.map_err(Error::internal)?;
            }
            let transaction = connection.begin().await.map_err(Error::internal)?;
            lock_user(&transaction, &db, 1).await?;
            assert!(matches!(revoke_family_for_locked_owner_in_transaction(&transaction, &db, backend, "other-family", 1).await, Err(Error::Unauthorized)));
            transaction.rollback().await.map_err(Error::internal)?;
            assert_eq!(unrevoked(&connection, "other-family").await?, 1);
            Ok(())
        }.await;
        let closed = database.close().await;
        outcome?;
        closed
    }
}

/// 同一事务中签发的受控 refresh 凭据。
#[cfg(feature = "auth-fortify")]
pub(crate) struct ManagedRefreshIssue {
    pub token: IssuedRefreshToken,
    pub family_id: String,
}
/// 仅能由当前事务的真实锁定行构造，不能用于另一个事务。
#[cfg(feature = "auth-fortify")]
pub(crate) struct LockedRefreshToken<'a> {
    transaction: &'a DatabaseTransaction,
    db: Db,
    token_hash: String,
    user_id: i64,
    family_id: String,
    expires_at: DateTime<Utc>,
    context: super::credential::CredentialFactorContext,
}
#[cfg(feature = "auth-fortify")]
impl LockedRefreshToken<'_> {
    pub(crate) const fn user_id(&self) -> i64 {
        self.user_id
    }
    pub(crate) fn family_id(&self) -> &str {
        &self.family_id
    }
    pub(crate) const fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }
    pub(crate) const fn context(&self) -> &super::credential::CredentialFactorContext {
        &self.context
    }
}
/// 消费锁票后返回同一家族的唯一后继凭据。
#[cfg(feature = "auth-fortify")]
pub(crate) struct ManagedRefreshRotation {
    pub user_id: i64,
    pub token: IssuedRefreshToken,
    pub family_id: String,
    pub expires_at: DateTime<Utc>,
    pub context: super::credential::CredentialFactorContext,
}
#[cfg(feature = "auth-fortify")]
impl fmt::Debug for ManagedRefreshIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ManagedRefreshIssue([REDACTED])")
    }
}
#[cfg(feature = "auth-fortify")]
impl fmt::Debug for LockedRefreshToken<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("LockedRefreshToken([REDACTED])")
    }
}
#[cfg(feature = "auth-fortify")]
impl fmt::Debug for ManagedRefreshRotation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ManagedRefreshRotation([REDACTED])")
    }
}

#[cfg(feature = "auth-fortify")]
impl RefreshTokens {
    pub(crate) async fn issue_context_in_transaction(
        &self,
        transaction: &DatabaseTransaction,
        user_id: i64,
        expires_at: DateTime<Utc>,
        context: &super::credential::CredentialFactorContext,
    ) -> Result<ManagedRefreshIssue> {
        if expires_at <= Utc::now() {
            return Err(Error::Unauthorized);
        }
        let encoded = context.encode(user_id)?;
        ensure_context_isolation(transaction).await?;
        lock_user(transaction, &self.db, user_id).await?;
        let value = random_token()?;
        let family_id = random_hex(16)?;
        insert_context_token(
            transaction,
            &self.db,
            user_id,
            &family_id,
            &sha256_hex(&value),
            expires_at,
            &encoded,
        )
        .await?;
        Ok(ManagedRefreshIssue {
            token: IssuedRefreshToken { value },
            family_id,
        })
    }

    // SQLite调用方须在首次读取前通过transaction_for_update取得写所有权。
    // replay返回None，外层必须提交撤销，不能把None转成事务内Err。
    #[cfg(test)]
    pub(crate) async fn lock_context_in_transaction<'a>(
        &self,
        transaction: &'a DatabaseTransaction,
        raw: &str,
    ) -> Result<Option<LockedRefreshToken<'a>>> {
        self.lock_context_in_scope_in_transaction(transaction, raw, None)
            .await
    }

    pub(crate) async fn lock_context_in_scope_in_transaction<'a>(
        &self,
        transaction: &'a DatabaseTransaction,
        raw: &str,
        scope: Option<&super::credential::CredentialGuardBinding>,
    ) -> Result<Option<LockedRefreshToken<'a>>> {
        ensure_context_isolation(transaction).await?;
        if raw.is_empty() || raw.len() > 1024 {
            return Ok(None);
        }
        let hash = sha256_hex(raw);
        let backend = transaction.get_database_backend();
        let marker = if backend == DbBackend::Postgres {
            "$1"
        } else {
            "?"
        };
        let owner = transaction
            .query_one_raw(Statement::from_sql_and_values(
                backend,
                crate::auth::resolve_auth_sql(
                    &self.db,
                    &format!(
                        "SELECT user_id FROM tb_auth_refresh_tokens WHERE token_hash={marker}"
                    ),
                )?,
                [hash.clone().into()],
            ))
            .await
            .map_err(Error::internal)?;
        let Some(owner) = owner else {
            return Ok(None);
        };
        let user_id: i64 = owner.try_get("", "user_id").map_err(Error::internal)?;
        if user_id <= 0 {
            return Err(Error::Unauthorized);
        }
        lock_user(transaction, &self.db, user_id).await?;
        let row = self
            .current_context_row_in_transaction(transaction, &hash)
            .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        if row.try_get::<i64>("", "user_id").map_err(Error::internal)? != user_id {
            return Err(Error::Unauthorized);
        }
        let family_id: String = row.try_get("", "family_id").map_err(Error::internal)?;
        validate_family_id(&family_id)?;
        let expires_at: DateTime<Utc> = row.try_get("", "expires_at").map_err(Error::internal)?;
        let encoded: Option<String> = row.try_get("", "factor_context").map_err(Error::internal)?;
        let decoded = encoded
            .as_deref()
            .ok_or(Error::Unauthorized)
            .and_then(|encoded| {
                super::credential::CredentialFactorContext::decode(encoded, user_id)
            });
        let context = match decoded {
            Ok(context) => context,
            Err(error) => {
                if !self
                    .has_current_family_scope(transaction, user_id, &family_id, scope)
                    .await?
                {
                    return Err(error);
                }
                revoke_context_family(transaction, &self.db, user_id, &family_id).await?;
                return Ok(None);
            }
        };
        if context.scope.as_ref() != scope {
            return Err(Error::Unauthorized);
        }
        if row
            .try_get::<Option<DateTime<Utc>>>("", "consumed_at")
            .map_err(Error::internal)?
            .is_some()
            || row
                .try_get::<Option<DateTime<Utc>>>("", "revoked_at")
                .map_err(Error::internal)?
                .is_some()
            || !context_unexpired(&row, backend, expires_at)?
        {
            revoke_context_family(transaction, &self.db, user_id, &family_id).await?;
            return Ok(None);
        }
        Ok(Some(LockedRefreshToken {
            transaction,
            db: self.db.clone(),
            token_hash: hash,
            user_id,
            family_id,
            expires_at,
            context,
        }))
    }

    // 调用方已持有同一用户锁；损坏祖先只能凭当前有效叶证明允许撤销的作用域。
    async fn has_current_family_scope(
        &self,
        transaction: &DatabaseTransaction,
        user_id: i64,
        family_id: &str,
        scope: Option<&super::credential::CredentialGuardBinding>,
    ) -> Result<bool> {
        let query = self
            .db
            .table("auth_refresh_tokens")?
            .using_transaction(transaction)
            .where_eq("user_id", user_id)?
            .where_eq("family_id", family_id)?
            .where_null("consumed_at")?
            .where_null("revoked_at")?;
        let query = if transaction.get_database_backend() == DbBackend::Sqlite {
            query
        } else {
            query.lock_for_update()?
        };
        let mut evidence = false;
        for leaf in query.get().await? {
            let expiry: DateTime<Utc> = leaf.try_get("", "expires_at").map_err(Error::internal)?;
            if expiry <= Utc::now() {
                continue;
            }
            let encoded: Option<String> = leaf
                .try_get("", "factor_context")
                .map_err(Error::internal)?;
            let context = super::credential::CredentialFactorContext::decode(
                encoded.as_deref().ok_or(Error::Unauthorized)?,
                user_id,
            )?;
            if context.scope.as_ref() != scope {
                return Err(Error::Unauthorized);
            }
            evidence = true;
        }
        Ok(evidence)
    }

    pub(crate) async fn revoke_scoped(
        &self,
        raw: &str,
        scope: &super::credential::CredentialGuardBinding,
    ) -> Result<()> {
        let this = self.clone();
        let raw = raw.to_owned();
        let scope = scope.clone();
        self.db
            .transaction_for_update(move |transaction| {
                Box::pin(async move {
                    if let Some(locked) = this
                        .lock_context_in_scope_in_transaction(transaction, &raw, Some(&scope))
                        .await?
                    {
                        revoke_context_family(
                            transaction,
                            &this.db,
                            locked.user_id(),
                            locked.family_id(),
                        )
                        .await?;
                    }
                    Ok(())
                })
            })
            .await
    }

    async fn current_context_row_in_transaction(
        &self,
        transaction: &DatabaseTransaction,
        hash: &str,
    ) -> Result<Option<crate::database::sea_orm::QueryResult>> {
        let backend = transaction.get_database_backend();
        let marker = if backend == DbBackend::Postgres {
            "$1"
        } else {
            "?"
        };
        let suffix = context_lock_suffix(backend)?;
        transaction.query_one_raw(Statement::from_sql_and_values(backend,
            crate::auth::resolve_auth_sql(&self.db, &format!("SELECT user_id,family_id,expires_at,consumed_at,revoked_at,factor_context{} FROM tb_auth_refresh_tokens WHERE token_hash={marker}{suffix}", context_expiry_expression(backend)))?,
            [hash.into()])).await.map_err(Error::internal)
    }

    // 调用方已按users→association顺序锁定user_id；此处只锁真实refresh行与家族。
    // attach拒绝重放但不撤销家族，不能复用轮换入口的replay写副作用。
    #[cfg(all(feature = "auth-fortify", feature = "auth-oidc"))]
    pub(crate) async fn attach_context_in_transaction(
        &self,
        transaction: &DatabaseTransaction,
        user_id: i64,
        raw: &str,
    ) -> Result<(
        String,
        DateTime<Utc>,
        super::credential::CredentialFactorContext,
    )> {
        ensure_context_isolation(transaction).await?;
        if user_id <= 0 || raw.is_empty() || raw.len() > 1_024 {
            return Err(Error::Unauthorized);
        }
        let row = self
            .current_context_row_in_transaction(transaction, &sha256_hex(raw))
            .await?
            .ok_or(Error::Unauthorized)?;
        if row.try_get::<i64>("", "user_id").map_err(Error::internal)? != user_id
            || row
                .try_get::<Option<DateTime<Utc>>>("", "consumed_at")
                .map_err(Error::internal)?
                .is_some()
            || row
                .try_get::<Option<DateTime<Utc>>>("", "revoked_at")
                .map_err(Error::internal)?
                .is_some()
        {
            return Err(Error::Unauthorized);
        }
        let expiry: DateTime<Utc> = row.try_get("", "expires_at").map_err(Error::internal)?;
        if !context_unexpired(&row, transaction.get_database_backend(), expiry)? {
            return Err(Error::Unauthorized);
        }
        let family: String = row.try_get("", "family_id").map_err(Error::internal)?;
        validate_family_id(&family)?;
        let encoded: Option<String> = row.try_get("", "factor_context").map_err(Error::internal)?;
        let context = super::credential::CredentialFactorContext::decode(
            encoded.as_deref().ok_or(Error::Unauthorized)?,
            user_id,
        )?;
        let deadline = self
            .validate_family_for_locked_owner_in_transaction(
                transaction,
                user_id,
                &family,
                &context,
            )
            .await?;
        if deadline != expiry {
            return Err(Error::Unauthorized);
        }
        require_attach_deadline(transaction, deadline).await?;
        Ok((family, deadline, context))
    }

    pub(crate) async fn rotate_context_in_transaction(
        &self,
        locked: LockedRefreshToken<'_>,
    ) -> Result<ManagedRefreshRotation> {
        self.require_store(&locked.db)?;
        let backend = locked.transaction.get_database_backend();
        let value = random_token()?;
        let hash = sha256_hex(&value);
        let (sql, values) = match backend {
            DbBackend::Postgres => (
                "UPDATE tb_auth_refresh_tokens SET consumed_at=CURRENT_TIMESTAMP,replaced_by_hash=$1 WHERE token_hash=$2 AND user_id=$3 AND family_id=$4 AND consumed_at IS NULL AND revoked_at IS NULL AND expires_at>clock_timestamp()",
                vec![
                    hash.clone().into(),
                    locked.token_hash.into(),
                    locked.user_id.into(),
                    locked.family_id.clone().into(),
                ],
            ),
            DbBackend::MySql => (
                "UPDATE tb_auth_refresh_tokens SET consumed_at=CURRENT_TIMESTAMP,replaced_by_hash=? WHERE token_hash=? AND user_id=? AND family_id=? AND consumed_at IS NULL AND revoked_at IS NULL AND expires_at>CURRENT_TIMESTAMP",
                vec![
                    hash.clone().into(),
                    locked.token_hash.into(),
                    locked.user_id.into(),
                    locked.family_id.clone().into(),
                ],
            ),
            DbBackend::Sqlite => {
                if locked.expires_at <= Utc::now() {
                    return Err(Error::Unauthorized);
                }
                (
                    "UPDATE tb_auth_refresh_tokens SET consumed_at=?,replaced_by_hash=? WHERE token_hash=? AND user_id=? AND family_id=? AND consumed_at IS NULL AND revoked_at IS NULL",
                    vec![
                        crate::database::dialect::utc_value(backend, Utc::now()),
                        hash.clone().into(),
                        locked.token_hash.into(),
                        locked.user_id.into(),
                        locked.family_id.clone().into(),
                    ],
                )
            }
            _ => return Err(unsupported_backend()),
        };
        let updated = locked
            .transaction
            .execute_raw(Statement::from_sql_and_values(
                backend,
                crate::auth::resolve_auth_sql(&self.db, sql)?,
                values,
            ))
            .await
            .map_err(Error::internal)?;
        if updated.rows_affected() != 1 {
            return Err(Error::Unauthorized);
        }
        insert_context_token(
            locked.transaction,
            &self.db,
            locked.user_id,
            &locked.family_id,
            &hash,
            locked.expires_at,
            &locked.context.encode(locked.user_id)?,
        )
        .await?;
        Ok(ManagedRefreshRotation {
            user_id: locked.user_id,
            token: IssuedRefreshToken { value },
            family_id: locked.family_id,
            expires_at: locked.expires_at,
            context: locked.context,
        })
    }

    pub(crate) async fn validate_family_in_transaction(
        &self,
        transaction: &DatabaseTransaction,
        user_id: i64,
        family_id: &str,
        context: &super::credential::CredentialFactorContext,
    ) -> Result<DateTime<Utc>> {
        context.validate_for_user(user_id)?;
        validate_family_id(family_id)?;
        ensure_context_isolation(transaction).await?;
        lock_user(transaction, &self.db, user_id).await?;
        self.validate_family_for_locked_owner_in_transaction(
            transaction,
            user_id,
            family_id,
            context,
        )
        .await
    }

    async fn validate_family_for_locked_owner_in_transaction(
        &self,
        transaction: &DatabaseTransaction,
        user_id: i64,
        family_id: &str,
        context: &super::credential::CredentialFactorContext,
    ) -> Result<DateTime<Utc>> {
        let backend = transaction.get_database_backend();
        let marker = if backend == DbBackend::Postgres {
            "$1"
        } else {
            "?"
        };
        let rows = transaction.query_all_raw(Statement::from_sql_and_values(backend,
            crate::auth::resolve_auth_sql(&self.db, &format!("SELECT user_id,factor_context,expires_at,consumed_at,revoked_at{} FROM tb_auth_refresh_tokens WHERE family_id={marker} ORDER BY token_hash{}", context_expiry_expression(backend), context_lock_suffix(backend)?))?, [family_id.into()])).await.map_err(Error::internal)?;
        let mut deadline = None;
        let mut live_leaf = None;
        for row in rows {
            if row.try_get::<i64>("", "user_id").map_err(Error::internal)? != user_id
                || row
                    .try_get::<Option<DateTime<Utc>>>("", "revoked_at")
                    .map_err(Error::internal)?
                    .is_some()
            {
                return Err(Error::Unauthorized);
            }
            let encoded: Option<String> =
                row.try_get("", "factor_context").map_err(Error::internal)?;
            if super::credential::CredentialFactorContext::decode(
                encoded.as_deref().ok_or(Error::Unauthorized)?,
                user_id,
            )? != *context
            {
                return Err(Error::Unauthorized);
            }
            let expiry: DateTime<Utc> = row.try_get("", "expires_at").map_err(Error::internal)?;
            if deadline.is_some_and(|previous| previous != expiry) {
                return Err(Error::Unauthorized);
            }
            deadline = Some(expiry);
            if row
                .try_get::<Option<DateTime<Utc>>>("", "consumed_at")
                .map_err(Error::internal)?
                .is_none()
                && context_unexpired(&row, backend, expiry)?
                && live_leaf.replace(expiry).is_some()
            {
                return Err(Error::Unauthorized);
            }
        }
        live_leaf.ok_or(Error::Unauthorized)
    }
}

#[cfg(all(feature = "auth-fortify", feature = "auth-oidc"))]
async fn require_attach_deadline(
    transaction: &DatabaseTransaction,
    deadline: DateTime<Utc>,
) -> Result<()> {
    let backend = transaction.get_database_backend();
    if backend == DbBackend::Sqlite {
        return if deadline > Utc::now() {
            Ok(())
        } else {
            Err(Error::Unauthorized)
        };
    }
    let sql = match backend {
        DbBackend::Postgres => "SELECT $1>clock_timestamp() AS unexpired",
        DbBackend::MySql => "SELECT ?>CURRENT_TIMESTAMP(6) AS unexpired",
        _ => return Err(unsupported_backend()),
    };
    let row = transaction
        .query_one_raw(Statement::from_sql_and_values(
            backend,
            sql,
            [deadline.into()],
        ))
        .await
        .map_err(Error::internal)?
        .ok_or(Error::Unauthorized)?;
    if row
        .try_get::<bool>("", "unexpired")
        .map_err(Error::internal)?
    {
        Ok(())
    } else {
        Err(Error::Unauthorized)
    }
}

#[cfg(feature = "auth-fortify")]
fn validate_family_id(value: &str) -> Result<()> {
    if value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(Error::Unauthorized)
    }
}
#[cfg(feature = "auth-fortify")]
fn context_lock_suffix(backend: DbBackend) -> Result<&'static str> {
    match backend {
        DbBackend::Postgres | DbBackend::MySql => Ok(" FOR UPDATE"),
        DbBackend::Sqlite => Ok(""),
        _ => Err(unsupported_backend()),
    }
}
#[cfg(feature = "auth-fortify")]
fn context_expiry_expression(backend: DbBackend) -> &'static str {
    match backend {
        DbBackend::Sqlite => "",
        DbBackend::Postgres => ",expires_at>clock_timestamp() AS unexpired",
        _ => ",expires_at>CURRENT_TIMESTAMP AS unexpired",
    }
}
#[cfg(feature = "auth-fortify")]
fn context_unexpired(
    row: &crate::database::sea_orm::QueryResult,
    backend: DbBackend,
    expiry: DateTime<Utc>,
) -> Result<bool> {
    if backend == DbBackend::Sqlite {
        Ok(expiry > Utc::now())
    } else {
        row.try_get("", "unexpired").map_err(Error::internal)
    }
}
#[cfg(feature = "auth-fortify")]
async fn ensure_context_isolation(transaction: &DatabaseTransaction) -> Result<()> {
    if transaction.get_database_backend() == DbBackend::Postgres {
        let row = transaction
            .query_one_raw(Statement::from_string(
                DbBackend::Postgres,
                "SELECT current_setting('transaction_isolation') AS isolation",
            ))
            .await
            .map_err(Error::internal)?
            .ok_or(Error::Unauthorized)?;
        if row
            .try_get::<String>("", "isolation")
            .map_err(Error::internal)?
            != "read committed"
        {
            return Err(Error::custom_with_status(
                crate::http::axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "auth_refresh_isolation_unsupported",
                "受控refresh当前要求PostgreSQL READ COMMITTED事务",
            ));
        }
    }
    context_lock_suffix(transaction.get_database_backend())?;
    Ok(())
}
#[cfg(feature = "auth-fortify")]
async fn insert_context_token(
    transaction: &DatabaseTransaction,
    db: &Db,
    user_id: i64,
    family: &str,
    hash: &str,
    expiry: DateTime<Utc>,
    context: &str,
) -> Result<()> {
    let backend = transaction.get_database_backend();
    let sql = if backend == DbBackend::Postgres {
        "INSERT INTO tb_auth_refresh_tokens(user_id,family_id,token_hash,expires_at,created_at,factor_context) VALUES($1,$2,$3,$4,$5,$6)"
    } else {
        "INSERT INTO tb_auth_refresh_tokens(user_id,family_id,token_hash,expires_at,created_at,factor_context) VALUES(?,?,?,?,?,?)"
    };
    let inserted = transaction
        .execute_raw(Statement::from_sql_and_values(
            backend,
            crate::auth::resolve_auth_sql(db, sql)?,
            [
                user_id.into(),
                family.into(),
                hash.into(),
                crate::database::dialect::utc_value(backend, expiry),
                crate::database::dialect::utc_value(backend, Utc::now()),
                context.into(),
            ],
        ))
        .await
        .map_err(Error::internal)?;
    if inserted.rows_affected() != 1 {
        return Err(Error::custom_with_status(
            crate::http::axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "auth_refresh_insert_failed",
            "受控refresh未保存唯一后继凭据",
        ));
    }
    Ok(())
}
#[cfg(feature = "auth-fortify")]
async fn revoke_context_family(
    transaction: &DatabaseTransaction,
    db: &Db,
    user_id: i64,
    family: &str,
) -> Result<()> {
    let backend = transaction.get_database_backend();
    let sql = if backend == DbBackend::Postgres {
        "UPDATE tb_auth_refresh_tokens SET revoked_at=$1 WHERE family_id=$2 AND user_id=$3 AND revoked_at IS NULL"
    } else {
        "UPDATE tb_auth_refresh_tokens SET revoked_at=? WHERE family_id=? AND user_id=? AND revoked_at IS NULL"
    };
    transaction
        .execute_raw(Statement::from_sql_and_values(
            backend,
            crate::auth::resolve_auth_sql(db, sql)?,
            [
                crate::database::dialect::utc_value(backend, Utc::now()),
                family.into(),
                user_id.into(),
            ],
        ))
        .await
        .map_err(Error::internal)?;
    Ok(())
}

#[cfg(all(
    test,
    feature = "auth-fortify",
    any(feature = "db-sqlite", feature = "db-mysql", feature = "db-postgres")
))]
mod context_tests;

#[cfg(all(
    test,
    feature = "db-sqlite",
    feature = "auth-fortify",
    feature = "auth-oidc"
))]
#[path = "refresh/attach_context_tests.rs"]
mod attach_context_tests;
