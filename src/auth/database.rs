use std::sync::OnceLock;

use crate::core::{Error, Result};
use crate::db::sea_orm::{ConnectionTrait, DbBackend, Statement};
use crate::db::{DatabaseConnection, Db};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::auth::{AuthCredential, AuthPrincipal, AuthSource, Authenticator};

#[derive(Default)]
pub(super) struct DatabaseAuthenticator {
    db: OnceLock<Db>,
}

impl DatabaseAuthenticator {
    pub(super) fn initialize(&self, db: Db) -> Result<()> {
        self.db.set(db).map_err(|_| {
            Error::custom(
                "auth_database_already_initialized",
                "database authenticator was initialized more than once",
            )
        })
    }

    async fn authenticate_session(
        &self,
        db: &Db,
        connection: &DatabaseConnection,
        backend: DbBackend,
        credential: &AuthCredential<'_>,
    ) -> Result<AuthPrincipal> {
        let session = credential
            .session()
            .ok_or_else(|| Error::internal(MissingSession))?;
        session.require_authenticated_storage()?;
        let user_id = session
            .get_typed::<i64>(AUTH_USER_ID_KEY)
            .map_err(Error::internal)?
            .ok_or(Error::Unauthorized)?;
        let sql = match backend {
            DbBackend::Postgres => "SELECT id FROM tb_users WHERE id = $1 AND disabled_at IS NULL",
            DbBackend::MySql | DbBackend::Sqlite => {
                "SELECT id FROM tb_users WHERE id = ? AND disabled_at IS NULL"
            }
            _ => return Err(unsupported_auth_database()),
        };
        let statement = Statement::from_sql_and_values(
            backend,
            &crate::auth::resolve_auth_sql(db, sql)?,
            [user_id.into()],
        );
        let row = connection
            .query_one_raw(statement)
            .await
            .map_err(Error::internal)?;
        if row.is_none() {
            return Err(Error::Unauthorized);
        }
        if !is_safe_method(credential.method().as_str()) && !credential.csrf_verified() {
            let submitted = credential
                .csrf_token()
                .ok_or_else(|| Error::Forbidden("invalid CSRF token".into()))?;
            let stored = session
                .get_typed::<String>(AUTH_CSRF_HASH_KEY)
                .map_err(Error::internal)?;
            let Some(stored) = stored else {
                return Err(Error::Forbidden("invalid CSRF token".into()));
            };
            let submitted_hash = sha256_hex(submitted);
            if stored.len() != submitted_hash.len()
                || !bool::from(stored.as_bytes().ct_eq(submitted_hash.as_bytes()))
            {
                return Err(Error::Forbidden("invalid CSRF token".into()));
            }
        }
        Ok(AuthPrincipal::new(user_id, None))
    }

    async fn authenticate_token(
        &self,
        db: &Db,
        connection: &DatabaseConnection,
        backend: DbBackend,
        credential: &AuthCredential<'_>,
    ) -> Result<AuthPrincipal> {
        if backend == DbBackend::Sqlite {
            return self.authenticate_sqlite_token(db, credential).await;
        }
        let value = credential
            .value()
            .ok_or_else(|| Error::internal(MissingToken))?;
        let hash = sha256_hex(value);
        let sql = match backend {
            DbBackend::Postgres => {
                "SELECT p.user_id, p.abilities FROM tb_personal_access_tokens p JOIN tb_users u ON u.id = p.user_id WHERE p.token_hash = $1 AND p.revoked_at IS NULL AND (p.expires_at IS NULL OR p.expires_at > CURRENT_TIMESTAMP) AND u.disabled_at IS NULL"
            }
            DbBackend::MySql => {
                "SELECT p.user_id, p.abilities FROM tb_personal_access_tokens p JOIN tb_users u ON u.id = p.user_id WHERE p.token_hash = ? AND p.revoked_at IS NULL AND (p.expires_at IS NULL OR p.expires_at > CURRENT_TIMESTAMP) AND u.disabled_at IS NULL"
            }
            _ => {
                return Err(Error::custom(
                    "unsupported_auth_database",
                    "authentication requires PostgreSQL, MySQL or SQLite",
                ));
            }
        };
        let statement = Statement::from_sql_and_values(
            backend,
            &crate::auth::resolve_auth_sql(db, sql)?,
            [hash.clone().into()],
        );
        let row = connection
            .query_one_raw(statement)
            .await
            .map_err(Error::internal)?;
        let Some(row) = row else {
            return Err(Error::Unauthorized);
        };
        let user_id = row.try_get::<i64>("", "user_id").map_err(Error::internal)?;
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
        let update = match backend {
            DbBackend::Postgres => {
                "UPDATE tb_personal_access_tokens SET last_used_at = CURRENT_TIMESTAMP WHERE token_hash = $1 AND revoked_at IS NULL"
            }
            DbBackend::MySql => {
                "UPDATE tb_personal_access_tokens SET last_used_at = CURRENT_TIMESTAMP WHERE token_hash = ? AND revoked_at IS NULL"
            }
            _ => {
                return Err(Error::custom(
                    "unsupported_auth_database",
                    "authentication requires PostgreSQL, MySQL or SQLite",
                ));
            }
        };
        connection
            .execute_raw(Statement::from_sql_and_values(
                backend,
                &crate::auth::resolve_auth_sql(db, update)?,
                [hash.into()],
            ))
            .await
            .map_err(Error::internal)?;
        Ok(AuthPrincipal::new(user_id, Some(abilities)))
    }

    async fn authenticate_sqlite_token(
        &self,
        db: &Db,
        credential: &AuthCredential<'_>,
    ) -> Result<AuthPrincipal> {
        let value = credential
            .value()
            .ok_or_else(|| Error::internal(MissingToken))?;
        let hash = sha256_hex(value);
        let database = db.clone();
        db.transaction_for_update(move |transaction| { Box::pin(async move {
            // 同一写事务内读取撤销/账户状态和精确截止，避免校验后被另一writer撤销。
            let sql = crate::auth::resolve_auth_sql(&database,
                "SELECT p.user_id, p.abilities, p.expires_at FROM tb_personal_access_tokens p JOIN tb_users u ON u.id=p.user_id WHERE p.token_hash=? AND p.revoked_at IS NULL AND u.disabled_at IS NULL")?;
            let row = transaction.query_one_raw(Statement::from_sql_and_values(DbBackend::Sqlite, sql, [hash.clone().into()]))
                .await.map_err(Error::internal)?.ok_or(Error::Unauthorized)?;
            let expires: Option<chrono::DateTime<chrono::Utc>> = row.try_get("", "expires_at").map_err(Error::internal)?;
            let now = chrono::Utc::now();
            if expires.is_some_and(|expiry| expiry <= now) { return Err(Error::Unauthorized); }
            let user_id: i64 = row.try_get("", "user_id").map_err(Error::internal)?;
            if user_id <= 0 { return Err(Error::Unauthorized); }
            let encoded: String = row.try_get("", "abilities").map_err(Error::internal)?;
            let abilities: serde_json::Value = serde_json::from_str(&encoded).map_err(|_| Error::internal(InvalidAbilities))?;
            let abilities = abilities.as_array().ok_or_else(|| Error::internal(InvalidAbilities))?
                .iter().map(|ability| ability.as_str().map(str::to_owned).ok_or_else(|| Error::internal(InvalidAbilities)))
                .collect::<Result<Vec<_>>>()?;
            let update = crate::auth::resolve_auth_sql(&database,
                "UPDATE tb_personal_access_tokens SET last_used_at=? WHERE token_hash=? AND revoked_at IS NULL")?;
            let updated = transaction.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, update,
                [crate::db::dialect::utc_value(DbBackend::Sqlite, now), hash.into()]))
                .await.map_err(Error::internal)?;
            if updated.rows_affected() != 1 { return Err(Error::Unauthorized); }
            Ok(AuthPrincipal::new(user_id, Some(abilities)))
        }) }).await
    }
}

impl Authenticator for DatabaseAuthenticator {
    fn authenticate<'a>(
        &'a self,
        credential: AuthCredential<'a>,
    ) -> crate::core::BoxFuture<'a, Result<AuthPrincipal>> {
        Box::pin(async move {
            let db = self
                .db
                .get()
                .ok_or_else(|| Error::internal(DatabaseNotReady))?;
            let connection = db.connection()?;
            let backend = db.backend()?;
            match credential.source() {
                AuthSource::Jwt => Err(Error::internal(UnexpectedJwtCredential)),
                AuthSource::Session => {
                    self.authenticate_session(db, &connection, backend, &credential)
                        .await
                }
                AuthSource::PersonalAccessToken => {
                    self.authenticate_token(db, &connection, backend, &credential)
                        .await
                }
            }
        })
    }
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

fn is_safe_method(method: &str) -> bool {
    matches!(method, "GET" | "HEAD" | "OPTIONS" | "TRACE")
}

pub(crate) const AUTH_USER_ID_KEY: &str = "openrust.auth.user_id";
pub(crate) const AUTH_CSRF_HASH_KEY: &str = "openrust.auth.csrf_hash";

fn unsupported_auth_database() -> Error {
    Error::custom(
        "unsupported_auth_database",
        "authentication requires PostgreSQL, MySQL or SQLite",
    )
}

#[derive(Debug)]
struct MissingSession;

impl std::fmt::Display for MissingSession {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Cookie authentication requires a shared Session")
    }
}

impl std::error::Error for MissingSession {}

#[derive(Debug)]
struct MissingToken;

impl std::fmt::Display for MissingToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("personal access token authentication requires a token value")
    }
}

impl std::error::Error for MissingToken {}

#[derive(Debug)]
struct DatabaseNotReady;

impl std::fmt::Display for DatabaseNotReady {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("authenticator database has not been registered")
    }
}

impl std::error::Error for DatabaseNotReady {}

#[derive(Debug)]
struct InvalidAbilities;

impl std::fmt::Display for InvalidAbilities {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("personal token abilities must be a JSON array of strings")
    }
}

impl std::error::Error for InvalidAbilities {}

#[derive(Debug)]
struct UnexpectedJwtCredential;

impl std::fmt::Display for UnexpectedJwtCredential {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("JWT credentials must be validated by JwtTokens")
    }
}

impl std::error::Error for UnexpectedJwtCredential {}
