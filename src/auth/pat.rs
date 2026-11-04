//! 通过共享数据库签发和撤销个人访问令牌。

use std::fmt;

use crate::core::{Error, Result};
use crate::db::sea_orm::{ConnectionTrait, DbBackend, Statement};
use crate::db::{DatabaseConnection, Db};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};

/// 基于数据库的个人访问令牌生命周期操作。
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
    /// 基于应用共享数据库创建令牌服务。
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// 创建不透明令牌，并一次性返回 bearer 值。
    ///
    /// 仅持久化 SHA-256 摘要和较短的查询前缀。
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
        if backend == DbBackend::Sqlite {
            return self
                .issue_sqlite(user_id, name, abilities, expires_at)
                .await;
        }
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

    /// 列出账户拥有的令牌元数据，不暴露令牌哈希或 bearer 值。
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
            DbBackend::MySql | DbBackend::Sqlite => {
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
                let abilities = if backend == DbBackend::Sqlite {
                    let encoded: String = row.try_get("", "abilities").map_err(Error::internal)?;
                    serde_json::from_str::<serde_json::Value>(&encoded)
                        .map_err(|_| Error::internal(InvalidAbilities))?
                } else {
                    row.try_get::<serde_json::Value>("", "abilities")
                        .map_err(Error::internal)?
                };
                let abilities = abilities
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

    /// 仅当令牌属于 `user_id` 时撤销该令牌。
    ///
    /// 此调用将活动令牌改为已撤销时返回 `true`。
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
            DbBackend::Sqlite => {
                "UPDATE tb_personal_access_tokens SET revoked_at=? WHERE user_id=? AND id=? AND revoked_at IS NULL"
            }
            _ => return Err(unsupported_backend()),
        };
        let result = connection
            .execute_raw(Statement::from_sql_and_values(
                backend,
                &crate::auth::resolve_auth_sql(&self.db, sql)?,
                if backend == DbBackend::Sqlite {
                    vec![
                        crate::db::dialect::utc_value(backend, Utc::now()),
                        user_id.into(),
                        token_id.into(),
                    ]
                } else {
                    vec![user_id.into(), token_id.into()]
                },
            ))
            .await
            .map_err(Error::internal)?;
        Ok(result.rows_affected() == 1)
    }

    /// 撤销账户的所有活动个人访问令牌。
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
            DbBackend::Sqlite => {
                "UPDATE tb_personal_access_tokens SET revoked_at=? WHERE user_id=? AND revoked_at IS NULL"
            }
            _ => return Err(unsupported_backend()),
        };
        let result = connection
            .execute_raw(Statement::from_sql_and_values(
                backend,
                &crate::auth::resolve_auth_sql(&self.db, sql)?,
                if backend == DbBackend::Sqlite {
                    vec![
                        crate::db::dialect::utc_value(backend, Utc::now()),
                        user_id.into(),
                    ]
                } else {
                    vec![user_id.into()]
                },
            ))
            .await
            .map_err(Error::internal)?;
        Ok(result.rows_affected())
    }

    async fn issue_sqlite(
        &self,
        user_id: i64,
        name: &str,
        abilities: &[String],
        expires_at: Option<DateTime<Utc>>,
    ) -> Result<IssuedPersonalAccessToken> {
        let value = format!("openrust_pat_{}", random_token()?);
        let prefix = value.chars().take(24).collect::<String>();
        let hash = sha256_hex(&value);
        let abilities = serde_json::to_string(abilities).map_err(Error::internal)?;
        let name = name.trim().to_owned();
        let db = self.db.clone();
        self.db.transaction_for_update(move |transaction| { Box::pin(async move {
            let account = crate::auth::resolve_auth_sql(&db, "SELECT id FROM tb_users WHERE id=? AND disabled_at IS NULL")?;
            if transaction.query_one_raw(Statement::from_sql_and_values(DbBackend::Sqlite, account, [user_id.into()]))
                .await.map_err(Error::internal)?.is_none() {
                return Err(Error::BadRequest("personal access token account does not exist or is disabled".into()));
            }
            let now = Utc::now();
            if expires_at.is_some_and(|expiry| expiry <= now) {
                return Err(Error::BadRequest("personal access token expiry must be in the future".into()));
            }
            let insert = crate::auth::resolve_auth_sql(&db,
                "INSERT INTO tb_personal_access_tokens (user_id,name,token_prefix,token_hash,abilities,expires_at,created_at) VALUES (?,?,?,?,?,?,?)")?;
            transaction.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, insert,
                [user_id.into(), name.into(), prefix.into(), hash.into(), abilities.into(),
                    expires_at.map_or(sea_orm::Value::String(None), |expiry| crate::db::dialect::utc_value(DbBackend::Sqlite, expiry)),
                    crate::db::dialect::utc_value(DbBackend::Sqlite, now)]))
                .await.map_err(Error::internal)?;
            Ok(())
        }) }).await?;
        Ok(IssuedPersonalAccessToken { value })
    }

    fn connection(&self) -> Result<DatabaseConnection> {
        let connection = self.db.connection()?;
        match connection.get_database_backend() {
            DbBackend::Postgres | DbBackend::MySql | DbBackend::Sqlite => Ok(connection),
            _ => Err(unsupported_backend()),
        }
    }
}

/// 新签发的个人访问令牌，必须且只能向客户端发送一次。
pub struct IssuedPersonalAccessToken {
    value: String,
}

impl IssuedPersonalAccessToken {
    /// 暴露 bearer 密钥，供响应发送给客户端。
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

/// 按账户列出个人访问令牌时返回的非敏感元数据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonalAccessTokenInfo {
    /// 数据库记录 ID。
    pub id: i64,
    /// 用户提供的描述名称。
    pub name: String,
    /// 用于识别凭据的非敏感短前缀。
    pub token_prefix: String,
    /// 分配给此 token 的精确 abilities。
    pub abilities: Vec<String>,
    /// 可选的过期时间点。
    pub expires_at: Option<DateTime<Utc>>,
    /// 撤销时间；尚未撤销时为空。
    pub revoked_at: Option<DateTime<Utc>>,
    /// 最近一次成功认证的时间点。
    pub last_used_at: Option<DateTime<Utc>>,
    /// 创建时间点。
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
        "personal access tokens require PostgreSQL, MySQL or SQLite",
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

#[cfg(all(test, feature = "db-sqlite"))]
mod sqlite_tests {
    use super::*;
    use crate::auth::{AuthCredential, AuthSource, Authenticator, CurrentUser, Policy};
    use crate::db::sea_orm_migration::{MigrationTrait, MigratorTrait};
    use crate::testing::TestDatabase;

    struct Migrator;
    impl MigratorTrait for Migrator {
        fn migrations() -> Vec<Box<dyn MigrationTrait>> {
            vec![
                Box::new(crate::auth::AuthMigration),
                Box::new(crate::auth::AuthPolicyMigration),
            ]
        }
    }

    async fn fixture() -> Result<(TestDatabase, Db)> {
        let database = TestDatabase::sqlite_file::<Migrator>().await?;
        let db = Db::default();
        db.initialize(database.connection()?)?;
        let now = crate::db::dialect::utc_value(DbBackend::Sqlite, Utc::now());
        let sql = crate::auth::resolve_auth_sql(
            &db,
            "INSERT INTO tb_users (id,email,password_hash,email_verified_at,created_at,updated_at) VALUES (42,?,?,?,?,?)",
        )?;
        db.connection()?
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                sql,
                [
                    "sqlite-pat@example.test".into(),
                    "not-used-by-pat".into(),
                    now.clone(),
                    now.clone(),
                    now,
                ],
            ))
            .await
            .map_err(Error::internal)?;
        Ok((database, db))
    }

    fn credential(token: &str) -> AuthCredential<'_> {
        AuthCredential {
            source: AuthSource::PersonalAccessToken,
            value: Some(token),
            session: None,
            csrf_token: None,
            csrf_verified: false,
            method: &http::Method::GET,
        }
    }

    async fn invalid_sqlite_abilities_are_not_wildcard(
        db: &Db,
        tokens: &PersonalAccessTokens,
        authenticator: &crate::auth::database::DatabaseAuthenticator,
        token: &str,
    ) -> Result<()> {
        let sql = crate::auth::resolve_auth_sql(
            db,
            "UPDATE tb_personal_access_tokens SET abilities=? WHERE user_id=42",
        )?;
        db.connection()?
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                &sql,
                ["{}".into()],
            ))
            .await
            .map_err(Error::internal)?;
        assert!(authenticator.authenticate(credential(token)).await.is_err());
        assert!(tokens.list_for_user(42).await.is_err());
        db.connection()?
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                sql,
                ["[\"articles.read\"]".into()],
            ))
            .await
            .map_err(Error::internal)?;
        Ok(())
    }

    #[tokio::test]
    async fn sqlite_pat_issue_authenticate_expiry_json_and_revoke_use_real_database() -> Result<()>
    {
        let (database, db) = fixture().await?;
        let outcome: Result<()> = async {
            let tokens = PersonalAccessTokens::new(db.clone());
            let expiry =
                DateTime::<Utc>::from_timestamp(Utc::now().timestamp() + 86_400, 123_456_000)
                    .ok_or_else(|| Error::BadRequest("测试截止无效".into()))?;
            let issued = tokens
                .issue(42, "SQLite test", &["articles.read".into()], Some(expiry))
                .await?;
            let list = tokens.list_for_user(42).await?;
            assert_eq!(list.len(), 1);
            let info = list
                .first()
                .ok_or_else(|| Error::BadRequest("缺少PAT元数据".into()))?;
            assert_eq!(info.expires_at, Some(expiry));
            assert_eq!(info.abilities, ["articles.read"]);
            assert!(info.last_used_at.is_none());
            let authenticator = crate::auth::database::DatabaseAuthenticator::default();
            authenticator.initialize(db.clone())?;
            let principal = authenticator
                .authenticate(credential(issued.expose()))
                .await?;
            assert_eq!(principal.user_id, 42);
            assert_eq!(
                principal.token_abilities,
                Some(vec!["articles.read".into()])
            );
            assert!(
                tokens
                    .list_for_user(42)
                    .await?
                    .first()
                    .is_some_and(|info| info.last_used_at.is_some())
            );
            invalid_sqlite_abilities_are_not_wildcard(
                &db,
                &tokens,
                &authenticator,
                issued.expose(),
            )
            .await?;
            // 外部RFC3339带offset值也必须解析后比较，不能与空格UTC按TEXT词法判断。
            let expired = (Utc::now() - chrono::Duration::microseconds(1))
                .fixed_offset()
                .with_timezone(
                    &chrono::FixedOffset::east_opt(8 * 3600)
                        .ok_or_else(|| Error::BadRequest("测试时区无效".into()))?,
                )
                .to_rfc3339();
            let sql = crate::auth::resolve_auth_sql(
                &db,
                "UPDATE tb_personal_access_tokens SET expires_at=? WHERE user_id=?",
            )?;
            db.connection()?
                .execute_raw(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    sql,
                    [expired.into(), 42_i64.into()],
                ))
                .await
                .map_err(Error::internal)?;
            assert!(matches!(
                authenticator
                    .authenticate(credential(issued.expose()))
                    .await,
                Err(Error::Unauthorized)
            ));
            assert!(tokens.revoke(42, info.id).await?);
            assert!(!tokens.revoke(42, info.id).await?);
            let permanent = tokens.issue(42, "nullable expiry", &[], None).await?;
            assert!(
                authenticator
                    .authenticate(credential(permanent.expose()))
                    .await
                    .is_ok()
            );
            assert_eq!(tokens.revoke_all_for_user(42).await?, 1);
            assert!(matches!(
                authenticator
                    .authenticate(credential(permanent.expose()))
                    .await,
                Err(Error::Unauthorized)
            ));
            assert!(tokens.issue(0, "invalid", &[], None).await.is_err());
            Ok(())
        }
        .await;
        drop(db);
        let cleanup = database.close().await;
        outcome?;
        cleanup
    }

    #[tokio::test]
    async fn sqlite_policy_targeted_upserts_preserve_exact_permissions_and_pat_abilities()
    -> Result<()> {
        let (database, db) = fixture().await?;
        let outcome: Result<()> = async {
            let policy = Policy::new(db.clone());
            let role = policy.ensure_role("editor").await?;
            assert!(role > 0);
            assert_eq!(policy.ensure_role("editor").await?, role);
            policy.assign_role(42, role).await?;
            policy.assign_role(42, role).await?;
            policy.grant_permission(role, "articles.read").await?;
            policy.grant_permission(role, "articles.read").await?;
            let user = CurrentUser {
                id: 42.into(),
                guard: None,
                provider: None,
                source: AuthSource::PersonalAccessToken,
                token_abilities: Some(vec!["articles.read".into()].into()),
            };
            assert!(policy.allows(&user, "articles.read").await?);
            assert!(!policy.allows(&user, "articles").await?);
            let unscoped = CurrentUser {
                id: 42.into(),
                guard: None,
                provider: None,
                source: AuthSource::PersonalAccessToken,
                token_abilities: Some(Vec::<String>::new().into()),
            };
            assert!(!policy.allows(&unscoped, "articles.read").await?);
            policy.revoke_permission(role, "articles.read").await?;
            assert!(!policy.allows(&user, "articles.read").await?);
            policy.grant_permission(role, "articles.read").await?;
            policy.revoke_role(42, role).await?;
            assert!(!policy.allows(&user, "articles.read").await?);
            assert!(policy.assign_role(0, role).await.is_err());
            Ok(())
        }
        .await;
        drop(db);
        let cleanup = database.close().await;
        outcome?;
        cleanup
    }
}
