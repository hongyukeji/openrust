//! Shared, single-use refresh-token families.

use std::fmt;

use crate::core::{Error, Result};
use crate::db::sea_orm::{ConnectionTrait, DatabaseTransaction, DbBackend, Statement};
use crate::db::{DatabaseConnection, Db};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};

/// Database-backed refresh-token lifecycle operations.
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
    /// Creates a refresh-token service over the application's shared database.
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// Creates a refresh token with a caller-selected family expiry and returns it once.
    ///
    /// Only its SHA-256 digest is persisted. The caller owns the policy that selects the expiry.
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
        let token_hash = sha256_hex(&token);
        let db = self.db.clone();
        self.db
            .transaction(|transaction| {
                let db = db.clone();
                Box::pin(async move {
                    lock_user(transaction, &db, user_id).await?;
                    insert_token(
                        transaction,
                        &db,
                        user_id,
                        &family_id,
                        &token_hash,
                        expires_at,
                    )
                    .await
                })
            })
            .await?;
        Ok(IssuedRefreshToken { value: token })
    }

    /// Atomically consumes a refresh token and returns its single successor.
    ///
    /// Reusing a consumed token commits family-wide revocation and returns `Unauthorized`.
    pub async fn rotate(&self, token: &str) -> Result<RefreshRotation> {
        let next_token = random_token()?;
        let next_hash = sha256_hex(&next_token);
        let token_hash = sha256_hex(token);
        let db = self.db.clone();
        let decision = self
            .db
            .transaction(|transaction| {
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

    /// Revokes every token in the family containing `token`.
    pub async fn revoke(&self, token: &str) -> Result<()> {
        self.connection()?;
        let token_hash = sha256_hex(token);
        let db = self.db.clone();
        self.db
            .transaction(|transaction| {
                let db = db.clone();
                Box::pin(async move {
                    let backend = transaction.get_database_backend();
                    let sql = match backend {
                        DbBackend::Postgres => "SELECT user_id, family_id FROM tb_auth_refresh_tokens WHERE token_hash = $1",
                        DbBackend::MySql => "SELECT user_id, family_id FROM tb_auth_refresh_tokens WHERE token_hash = ?",
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

    /// Revokes all refresh-token families for an account and returns the affected row count.
    pub async fn revoke_all_for_user(&self, user_id: i64) -> Result<u64> {
        self.connection()?;
        if user_id <= 0 {
            return Err(Error::BadRequest("user ID must be positive".into()));
        }
        let db = self.db.clone();
        self.db
            .transaction(|transaction| {
                Box::pin(async move {
                    revoke_all_for_user_in_transaction(transaction, &db, user_id).await
                })
            })
            .await
    }

    pub(crate) async fn user_id_for_token(&self, token: &str) -> Result<i64> {
        let connection = self.connection()?;
        let backend = connection.get_database_backend();
        let sql = match backend {
            DbBackend::Postgres => {
                "SELECT user_id FROM tb_auth_refresh_tokens WHERE token_hash = $1 AND consumed_at IS NULL AND revoked_at IS NULL AND expires_at > CURRENT_TIMESTAMP"
            }
            DbBackend::MySql => {
                "SELECT user_id FROM tb_auth_refresh_tokens WHERE token_hash = ? AND consumed_at IS NULL AND revoked_at IS NULL AND expires_at > CURRENT_TIMESTAMP(6)"
            }
            _ => return Err(unsupported_backend()),
        };
        let row = connection
            .query_one_raw(Statement::from_sql_and_values(
                backend,
                &crate::auth::resolve_auth_sql(&self.db, sql)?,
                [sha256_hex(token).into()],
            ))
            .await
            .map_err(Error::internal)?
            .ok_or(Error::Unauthorized)?;
        row.try_get::<i64>("", "user_id").map_err(Error::internal)
    }

    fn connection(&self) -> Result<DatabaseConnection> {
        let connection = self.db.connection()?;
        match connection.get_database_backend() {
            DbBackend::Postgres | DbBackend::MySql => Ok(connection),
            _ => Err(unsupported_backend()),
        }
    }
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

/// A refresh credential returned only when a token is issued or rotated.
pub struct IssuedRefreshToken {
    value: String,
}

impl IssuedRefreshToken {
    /// Exposes the bearer secret for the response that delivers it to the client.
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

/// The account, successor refresh credential and unchanged family expiry after rotation.
pub struct RefreshRotation {
    user_id: i64,
    token: IssuedRefreshToken,
    expires_at: DateTime<Utc>,
}

impl RefreshRotation {
    /// Returns the account that owns the token family.
    pub const fn user_id(&self) -> i64 {
        self.user_id
    }

    /// Returns the one-time successor credential.
    pub const fn token(&self) -> &IssuedRefreshToken {
        &self.token
    }

    /// Returns the family expiry, which rotation does not extend.
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

// Keep the lock, validation, consume and successor insert together to preserve transaction order.
#[allow(clippy::too_many_lines)]
async fn rotate_in_transaction(
    transaction: &DatabaseTransaction,
    db: &Db,
    token_hash: &str,
    next_hash: &str,
) -> Result<RotationDecision> {
    let backend = transaction.get_database_backend();
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

async fn lock_user(transaction: &DatabaseTransaction, db: &Db, user_id: i64) -> Result<()> {
    let backend = transaction.get_database_backend();
    let sql = match backend {
        DbBackend::Postgres => "SELECT id FROM tb_users WHERE id = $1 FOR UPDATE",
        DbBackend::MySql => "SELECT id FROM tb_users WHERE id = ? FOR UPDATE",
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

async fn revoke_family_in_transaction(
    transaction: &DatabaseTransaction,
    db: &Db,
    backend: DbBackend,
    family_id: &str,
) -> Result<()> {
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
        "refresh tokens require PostgreSQL or MySQL",
    )
}
