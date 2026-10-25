//! Personal access token issuance and revocation over the shared database.

use std::fmt;

use crate::core::{Error, Result};
use crate::db::sea_orm::{ConnectionTrait, DbBackend, Statement};
use crate::db::{DatabaseConnection, Db};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};

/// Database-backed personal access token lifecycle operations.
#[derive(Clone)]
pub struct PersonalAccessTokens {
    db: Db,
}

impl fmt::Debug for PersonalAccessTokens {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PersonalAccessTokens")
    }
}

impl PersonalAccessTokens {
    /// Creates a token service over the application's shared database.
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// Creates an opaque token and returns the bearer value once.
    ///
    /// Only the SHA-256 digest and a short lookup prefix are persisted.
    pub async fn issue(
        &self,
        user_id: i64,
        name: &str,
        abilities: &[String],
        expires_at: Option<DateTime<Utc>>,
    ) -> Result<IssuedPersonalAccessToken> {
        validate_input(user_id, name, abilities, expires_at)?;
        let connection = self.connection()?;
        let backend = connection.get_database_backend();
        let account_sql = match backend {
            DbBackend::Postgres => "SELECT id FROM tb_users WHERE id = $1 AND disabled_at IS NULL",
            DbBackend::MySql => "SELECT id FROM tb_users WHERE id = ? AND disabled_at IS NULL",
            _ => return Err(unsupported_backend()),
        };
        if connection
            .query_one_raw(Statement::from_sql_and_values(
                backend,
                &crate::auth::resolve_auth_sql(&self.db, account_sql)?,
                [user_id.into()],
            ))
            .await
            .map_err(Error::internal)?
            .is_none()
        {
            return Err(Error::BadRequest(
                "personal access token account does not exist or is disabled".into(),
            ));
        }
        let value = format!("openrust_pat_{}", random_token()?);
        let prefix = value.chars().take(24).collect::<String>();
        let hash = sha256_hex(&value);
        let abilities = serde_json::to_string(abilities).map_err(Error::internal)?;
        let sql = match backend {
            DbBackend::Postgres => {
                "INSERT INTO tb_personal_access_tokens (user_id, name, token_prefix, token_hash, abilities, expires_at, created_at) VALUES ($1, $2, $3, $4, $5::jsonb, $6, CURRENT_TIMESTAMP)"
            }
            DbBackend::MySql => {
                "INSERT INTO tb_personal_access_tokens (user_id, name, token_prefix, token_hash, abilities, expires_at, created_at) VALUES (?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP)"
            }
            _ => return Err(unsupported_backend()),
        };
        connection
            .execute_raw(Statement::from_sql_and_values(
                backend,
                &crate::auth::resolve_auth_sql(&self.db, sql)?,
                [
                    user_id.into(),
                    name.trim().to_owned().into(),
                    prefix.into(),
                    hash.into(),
                    abilities.into(),
                    expires_at.into(),
                ],
            ))
            .await
            .map_err(Error::internal)?;
        Ok(IssuedPersonalAccessToken { value })
    }

    /// Lists token metadata owned by the account without exposing token hashes or bearer values.
    pub async fn list_for_user(&self, user_id: i64) -> Result<Vec<PersonalAccessTokenInfo>> {
        if user_id <= 0 {
            return Err(Error::BadRequest("user ID must be positive".into()));
        }
        let connection = self.connection()?;
        let backend = connection.get_database_backend();
        let sql = match backend {
            DbBackend::Postgres => {
                "SELECT id, name, token_prefix, abilities, expires_at, revoked_at, last_used_at, created_at FROM tb_personal_access_tokens WHERE user_id = $1 ORDER BY id"
            }
            DbBackend::MySql => {
                "SELECT id, name, token_prefix, abilities, expires_at, revoked_at, last_used_at, created_at FROM tb_personal_access_tokens WHERE user_id = ? ORDER BY id"
            }
            _ => return Err(unsupported_backend()),
        };
        connection
            .query_all_raw(Statement::from_sql_and_values(
                backend,
                &crate::auth::resolve_auth_sql(&self.db, sql)?,
                [user_id.into()],
            ))
            .await
            .map_err(Error::internal)?
            .into_iter()
            .map(|row| {
                let abilities = row
                    .try_get::<serde_json::Value>("", "abilities")
                    .map_err(Error::internal)?
                    .as_array()
                    .ok_or_else(|| Error::internal(InvalidAbilities))?
                    .iter()
                    .map(|value| {
                        value
                            .as_str()
                            .map(str::to_owned)
                            .ok_or_else(|| Error::internal(InvalidAbilities))
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(PersonalAccessTokenInfo {
                    id: row.try_get("", "id").map_err(Error::internal)?,
                    name: row.try_get("", "name").map_err(Error::internal)?,
                    token_prefix: row.try_get("", "token_prefix").map_err(Error::internal)?,
                    abilities,
                    expires_at: row.try_get("", "expires_at").map_err(Error::internal)?,
                    revoked_at: row.try_get("", "revoked_at").map_err(Error::internal)?,
                    last_used_at: row.try_get("", "last_used_at").map_err(Error::internal)?,
                    created_at: row.try_get("", "created_at").map_err(Error::internal)?,
                })
            })
            .collect()
    }

    /// Revokes a token only when it belongs to `user_id`.
    ///
    /// Returns `true` when this call changed an active token to revoked.
    pub async fn revoke(&self, user_id: i64, token_id: i64) -> Result<bool> {
        if user_id <= 0 || token_id <= 0 {
            return Err(Error::BadRequest(
                "user ID and personal access token ID must be positive".into(),
            ));
        }
        let connection = self.connection()?;
        let backend = connection.get_database_backend();
        let sql = match backend {
            DbBackend::Postgres => {
                "UPDATE tb_personal_access_tokens SET revoked_at = CURRENT_TIMESTAMP WHERE user_id = $1 AND id = $2 AND revoked_at IS NULL"
            }
            DbBackend::MySql => {
                "UPDATE tb_personal_access_tokens SET revoked_at = CURRENT_TIMESTAMP WHERE user_id = ? AND id = ? AND revoked_at IS NULL"
            }
            _ => return Err(unsupported_backend()),
        };
        let result = connection
            .execute_raw(Statement::from_sql_and_values(
                backend,
                &crate::auth::resolve_auth_sql(&self.db, sql)?,
                [user_id.into(), token_id.into()],
            ))
            .await
            .map_err(Error::internal)?;
        Ok(result.rows_affected() == 1)
    }

    /// Revokes all active personal access tokens for an account.
    pub async fn revoke_all_for_user(&self, user_id: i64) -> Result<u64> {
        if user_id <= 0 {
            return Err(Error::BadRequest("user ID must be positive".into()));
        }
        let connection = self.connection()?;
        let backend = connection.get_database_backend();
        let sql = match backend {
            DbBackend::Postgres => {
                "UPDATE tb_personal_access_tokens SET revoked_at = CURRENT_TIMESTAMP WHERE user_id = $1 AND revoked_at IS NULL"
            }
            DbBackend::MySql => {
                "UPDATE tb_personal_access_tokens SET revoked_at = CURRENT_TIMESTAMP WHERE user_id = ? AND revoked_at IS NULL"
            }
            _ => return Err(unsupported_backend()),
        };
        let result = connection
            .execute_raw(Statement::from_sql_and_values(
                backend,
                &crate::auth::resolve_auth_sql(&self.db, sql)?,
                [user_id.into()],
            ))
            .await
            .map_err(Error::internal)?;
        Ok(result.rows_affected())
    }

    fn connection(&self) -> Result<DatabaseConnection> {
        let connection = self.db.connection()?;
        match connection.get_database_backend() {
            DbBackend::Postgres | DbBackend::MySql => Ok(connection),
            _ => Err(unsupported_backend()),
        }
    }
}

/// A newly issued personal access token that must be delivered to the client once.
pub struct IssuedPersonalAccessToken {
    value: String,
}

impl IssuedPersonalAccessToken {
    /// Exposes the bearer secret for the response that delivers it to the client.
    pub fn expose(&self) -> &str {
        &self.value
    }
}

impl fmt::Debug for IssuedPersonalAccessToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("IssuedPersonalAccessToken")
            .field("value", &"[REDACTED]")
            .finish()
    }
}

/// Non-secret personal access token metadata returned by account-scoped listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonalAccessTokenInfo {
    /// Database record identifier.
    pub id: i64,
    /// User-provided descriptive name.
    pub name: String,
    /// Short non-secret prefix used to identify the credential.
    pub token_prefix: String,
    /// Exact abilities assigned to this token.
    pub abilities: Vec<String>,
    /// Optional expiry instant.
    pub expires_at: Option<DateTime<Utc>>,
    /// Revocation instant, if revoked.
    pub revoked_at: Option<DateTime<Utc>>,
    /// Most recent successful authentication instant.
    pub last_used_at: Option<DateTime<Utc>>,
    /// Creation instant.
    pub created_at: DateTime<Utc>,
}

fn validate_input(
    user_id: i64,
    name: &str,
    abilities: &[String],
    expires_at: Option<DateTime<Utc>>,
) -> Result<()> {
    if user_id <= 0 {
        return Err(Error::BadRequest("user ID must be positive".into()));
    }
    if name.trim().is_empty() || name.trim().chars().count() > 120 {
        return Err(Error::BadRequest(
            "personal access token name must contain 1 to 120 characters".into(),
        ));
    }
    if abilities.len() > 64
        || abilities
            .iter()
            .any(|ability| ability.trim().is_empty() || ability.len() > 128)
    {
        return Err(Error::BadRequest(
            "personal access token abilities must contain at most 64 non-empty values of at most 128 bytes each".into(),
        ));
    }
    if expires_at.is_some_and(|expiry| expiry <= Utc::now()) {
        return Err(Error::BadRequest(
            "personal access token expiry must be in the future".into(),
        ));
    }
    Ok(())
}

fn random_token() -> Result<String> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(Error::internal)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn sha256_hex(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(value.as_bytes());
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn unsupported_backend() -> Error {
    Error::custom(
        "unsupported_auth_database",
        "personal access tokens require PostgreSQL or MySQL",
    )
}

#[derive(Debug)]
struct InvalidAbilities;

impl fmt::Display for InvalidAbilities {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("personal token abilities must be a JSON array of strings")
    }
}

impl std::error::Error for InvalidAbilities {}
