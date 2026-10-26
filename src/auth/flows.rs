//! 基于数据库的账户与凭据生命周期编排。

use std::{
    sync::{Arc, OnceLock},
    time::Duration,
};

use crate::core::{App, AppBuilder, BoxFuture, Error, Plugin, RateLimiter, Result};
use crate::db::Db;
use crate::db::sea_orm::{ConnectionTrait, DbBackend, SqlErr, Statement};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{Duration as ChronoDuration, Utc};

use crate::auth::{
    EncodedPasswordHash, IssuedRefreshToken, IssuedVerificationCode, JwtTokens, Passwords,
    RefreshTokens, Session, VerificationCodePolicy, VerificationCodes,
    database::{AUTH_CSRF_HASH_KEY, AUTH_USER_ID_KEY},
};

const REQUEST_LIMIT: u64 = 5;
const REQUEST_WINDOW: Duration = Duration::from_mins(15);
const EMAIL_VERIFICATION_PURPOSE: &str = "auth.email_verification";
const PASSWORD_RESET_PURPOSE: &str = "auth.password_reset";

/// 共享账户凭据流程。HTTP 路由与消息投递仍由应用负责。
#[derive(Clone)]
pub struct AuthFlows {
    db: Db,
    passwords: Passwords,
    jwt: JwtTokens,
    refresh: RefreshTokens,
    codes: VerificationCodes,
    limiter: RateLimiter,
    refresh_lifetime: ChronoDuration,
    code_policy: VerificationCodePolicy,
}

impl std::fmt::Debug for AuthFlows {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AuthFlows")
    }
}

impl AuthFlows {
    /// 使用应用所需的共享服务创建流程服务。
    pub fn new(
        db: Db,
        passwords: Passwords,
        jwt: JwtTokens,
        refresh: RefreshTokens,
        codes: VerificationCodes,
        limiter: RateLimiter,
    ) -> Self {
        Self {
            db,
            passwords,
            jwt,
            refresh,
            codes,
            limiter,
            refresh_lifetime: ChronoDuration::days(30),
            code_policy: VerificationCodePolicy::default(),
        }
    }

    /// 设置 refresh token 家族的最长有效期。
    #[must_use]
    pub fn with_refresh_lifetime(mut self, lifetime: ChronoDuration) -> Self {
        self.refresh_lifetime = lifetime;
        self
    }

    /// 设置邮箱验证码和密码重置码的策略。
    #[must_use]
    pub fn with_code_policy(mut self, policy: VerificationCodePolicy) -> Self {
        self.code_policy = policy;
        self
    }

    /// 创建未验证账户，并返回由调用方负责投递的验证码。
    ///
    /// `None` 表示该邮箱地址已存在；为避免账户枚举，调用方应对新注册地址和已存在地址返回相同的公开响应。
    pub async fn register(
        &self,
        email: &str,
        password: &str,
        request_source: &str,
    ) -> Result<Option<IssuedVerificationCode>> {
        let email = normalize_email(email)?;
        self.limit("register", request_source, &email).await?;
        let password_hash = self.passwords.hash(password).await?;
        let connection = self.db.connection()?;
        let backend = connection.get_database_backend();
        let sql = match backend {
            DbBackend::Postgres => {
                "INSERT INTO tb_users (email, password_hash, created_at, updated_at) VALUES ($1, $2, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)"
            }
            DbBackend::MySql => {
                "INSERT INTO tb_users (email, password_hash, created_at, updated_at) VALUES (?, ?, CURRENT_TIMESTAMP(6), CURRENT_TIMESTAMP(6))"
            }
            _ => return Err(unsupported_backend()),
        };
        let result = connection
            .execute_raw(Statement::from_sql_and_values(
                backend,
                &crate::auth::resolve_auth_sql(&self.db, sql)?,
                [
                    email.clone().into(),
                    password_hash.as_str().to_owned().into(),
                ],
            ))
            .await;
        let result = match result {
            Ok(result) => result,
            Err(error) if matches!(error.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) => {
                return Ok(None);
            }
            Err(error) => return Err(Error::internal(error)),
        };
        if result.rows_affected() == 0 {
            return Ok(None);
        }
        self.issue_code_uniform(EMAIL_VERIFICATION_PURPOSE, &email)
            .await
    }

    /// 使用一个用途绑定的验证码验证邮箱地址。
    pub async fn verify_email(
        &self,
        email: &str,
        code: &str,
        request_source: &str,
    ) -> Result<bool> {
        let email = normalize_email(email)?;
        self.limit("verify_email", request_source, &email).await?;
        if !self
            .codes
            .verify(EMAIL_VERIFICATION_PURPOSE, &email, code)
            .await?
        {
            return Ok(false);
        }
        let connection = self.db.connection()?;
        let backend = connection.get_database_backend();
        let sql = match backend {
            DbBackend::Postgres => {
                "UPDATE tb_users SET email_verified_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP WHERE email = $1 AND email_verified_at IS NULL AND disabled_at IS NULL"
            }
            DbBackend::MySql => {
                "UPDATE tb_users SET email_verified_at = CURRENT_TIMESTAMP(6), updated_at = CURRENT_TIMESTAMP(6) WHERE email = ? AND email_verified_at IS NULL AND disabled_at IS NULL"
            }
            _ => return Err(unsupported_backend()),
        };
        let result = connection
            .execute_raw(Statement::from_sql_and_values(
                backend,
                &crate::auth::resolve_auth_sql(&self.db, sql)?,
                [email.into()],
            ))
            .await
            .map_err(Error::internal)?;
        Ok(result.rows_affected() == 1)
    }

    /// 为已存在、尚未验证且未禁用的账户申请邮箱验证码。
    ///
    /// `None` 表示账户不存在、已禁用或已验证。公开 handler 对这两种结果都应返回相同响应。
    pub async fn request_email_verification(
        &self,
        email: &str,
        request_source: &str,
    ) -> Result<Option<IssuedVerificationCode>> {
        let email = normalize_email(email)?;
        self.limit("request_email_verification", request_source, &email)
            .await?;
        let connection = self.db.connection()?;
        let backend = connection.get_database_backend();
        let sql = match backend {
            DbBackend::Postgres => {
                "SELECT id FROM tb_users WHERE email = $1 AND email_verified_at IS NULL AND disabled_at IS NULL"
            }
            DbBackend::MySql => {
                "SELECT id FROM tb_users WHERE email = ? AND email_verified_at IS NULL AND disabled_at IS NULL"
            }
            _ => return Err(unsupported_backend()),
        };
        if connection
            .query_one_raw(Statement::from_sql_and_values(
                backend,
                &crate::auth::resolve_auth_sql(&self.db, sql)?,
                [email.clone().into()],
            ))
            .await
            .map_err(Error::internal)?
            .is_none()
        {
            return Ok(None);
        }
        self.issue_code_uniform(EMAIL_VERIFICATION_PURPOSE, &email)
            .await
    }

    /// 验证已确认账户的身份，并签发 access/refresh 凭据对。
    pub async fn login(
        &self,
        email: &str,
        password: &str,
        request_source: &str,
    ) -> Result<AuthCredentials> {
        let user_id = self
            .authenticate_password(email, password, request_source)
            .await?;
        let access_token = self.jwt.issue_access_token(user_id)?;
        let expires_at = Utc::now() + self.refresh_lifetime;
        let refresh_token = self.refresh.issue(user_id, expires_at).await?;
        Ok(AuthCredentials {
            user_id,
            access_token,
            refresh_token,
            refresh_expires_at: expires_at,
        })
    }

    /// 验证已确认账户的身份，轮换其 Cookie Session ID，并返回 CSRF token。
    ///
    /// 此可选 Session 登录路径不会签发 JWT 或 refresh 凭据。调用方必须通过应用响应将 CSRF token 返回给客户端。
    pub async fn login_session(
        &self,
        email: &str,
        password: &str,
        request_source: &str,
        session: &Session,
    ) -> Result<SessionCredentials> {
        let user_id = self
            .authenticate_password(email, password, request_source)
            .await?;
        bind_session(session, user_id).await
    }

    /// 从 Cookie Session 中移除已认证身份并轮换 Session ID。
    ///
    /// 其他由应用持有的 Session 值会保留。路由应先要求 `AuthMode::Session`，再调用此方法。
    pub async fn logout_session(&self, request_source: &str, session: &Session) -> Result<()> {
        let user_id = session
            .get::<i64>(AUTH_USER_ID_KEY)
            .await
            .map_err(Error::internal)?
            .ok_or(Error::Unauthorized)?;
        self.limit("logout_session", request_source, &user_id.to_string())
            .await?;
        clear_session(session).await
    }

    async fn authenticate_password(
        &self,
        email: &str,
        password: &str,
        request_source: &str,
    ) -> Result<i64> {
        let email = normalize_email(email)?;
        self.limit("login", request_source, &email).await?;
        let connection = self.db.connection()?;
        let backend = connection.get_database_backend();
        let sql = match backend {
            DbBackend::Postgres => {
                "SELECT id, password_hash FROM tb_users WHERE email = $1 AND email_verified_at IS NOT NULL AND disabled_at IS NULL"
            }
            DbBackend::MySql => {
                "SELECT id, password_hash FROM tb_users WHERE email = ? AND email_verified_at IS NOT NULL AND disabled_at IS NULL"
            }
            _ => return Err(unsupported_backend()),
        };
        let Some(row) = connection
            .query_one_raw(Statement::from_sql_and_values(
                backend,
                &crate::auth::resolve_auth_sql(&self.db, sql)?,
                [email.into()],
            ))
            .await
            .map_err(Error::internal)?
        else {
            self.passwords.hash(password).await?;
            return Err(Error::Unauthorized);
        };
        let user_id = row.try_get::<i64>("", "id").map_err(Error::internal)?;
        let encoded = row
            .try_get::<String>("", "password_hash")
            .map_err(Error::internal)?;
        let Ok(hash) = EncodedPasswordHash::parse(encoded) else {
            self.passwords.hash(password).await?;
            return Err(Error::Unauthorized);
        };
        if !self.passwords.verify(password, &hash).await? {
            return Err(Error::Unauthorized);
        }
        Ok(user_id)
    }

    /// 原子轮换 refresh 凭据并返回后继凭据对。
    pub async fn refresh(&self, token: &str, request_source: &str) -> Result<AuthCredentials> {
        self.limit("refresh", request_source, &digest(token))
            .await?;
        let user_id = self.refresh.user_id_for_token(token).await?;
        // 先签发新 token，再消费旧 refresh 凭据，避免签名失败时旧凭据已被消耗。
        let access_token = self.jwt.issue_access_token(user_id)?;
        let rotated = self.refresh.rotate(token).await?;
        let (user_id, refresh_token, refresh_expires_at) = rotated.into_parts();
        Ok(AuthCredentials {
            user_id,
            access_token,
            refresh_token,
            refresh_expires_at,
        })
    }

    /// 撤销所提交 refresh token 所属的整个 token 家族。
    pub async fn logout(&self, token: &str, request_source: &str) -> Result<()> {
        self.limit("logout", request_source, &digest(token)).await?;
        self.refresh.revoke(token).await
    }

    /// 申请密码重置码；仅当账户符合条件时返回该验证码。
    ///
    /// 无论返回 `Some` 还是 `None`，HTTP handler 都应返回相同的公开响应；只有存在验证码时才发送。
    pub async fn request_password_reset(
        &self,
        email: &str,
        request_source: &str,
    ) -> Result<Option<IssuedVerificationCode>> {
        let email = normalize_email(email)?;
        self.limit("password_reset", request_source, &email).await?;
        if !self.eligible_account(&email).await? {
            return Ok(None);
        }
        self.issue_code_uniform(PASSWORD_RESET_PURPOSE, &email)
            .await
    }

    /// 消费密码重置码、替换密码并撤销 refresh token 家族。
    pub async fn reset_password(
        &self,
        email: &str,
        code: &str,
        new_password: &str,
        request_source: &str,
    ) -> Result<bool> {
        let email = normalize_email(email)?;
        self.limit("password_reset_confirm", request_source, &email)
            .await?;
        let password_hash = self.passwords.hash(new_password).await?;
        if !self
            .codes
            .verify(PASSWORD_RESET_PURPOSE, &email, code)
            .await?
        {
            return Ok(false);
        }
        let db = self.db.clone();
        self.db
            .transaction(|transaction| {
                let db = db.clone();
                let email = email.clone();
                let password_hash = password_hash.as_str().to_owned();
                Box::pin(async move {
                    let backend = transaction.get_database_backend();
                    let select = match backend {
                        DbBackend::Postgres => "SELECT id FROM tb_users WHERE email = $1 AND disabled_at IS NULL FOR UPDATE",
                        DbBackend::MySql => "SELECT id FROM tb_users WHERE email = ? AND disabled_at IS NULL FOR UPDATE",
                        _ => return Err(unsupported_backend()),
                    };
                    let Some(row) = transaction.query_one_raw(Statement::from_sql_and_values(
                        backend,
                        &crate::auth::resolve_auth_sql(&db, select)?,
                        [email.into()],
                    )).await.map_err(Error::internal)? else {
                        return Ok(false);
                    };
                    let user_id = row.try_get::<i64>("", "id").map_err(Error::internal)?;
                    let update = match backend {
                        DbBackend::Postgres => "UPDATE tb_users SET password_hash = $1, updated_at = CURRENT_TIMESTAMP WHERE id = $2 AND disabled_at IS NULL",
                        DbBackend::MySql => "UPDATE tb_users SET password_hash = ?, updated_at = CURRENT_TIMESTAMP(6) WHERE id = ? AND disabled_at IS NULL",
                        _ => return Err(unsupported_backend()),
                    };
                    let result = transaction.execute_raw(Statement::from_sql_and_values(
                        backend,
                        &crate::auth::resolve_auth_sql(&db, update)?,
                        [password_hash.into(), user_id.into()],
                    )).await.map_err(Error::internal)?;
                    if result.rows_affected() != 1 {
                        return Ok(false);
                    }
                    crate::auth::refresh::revoke_all_for_user_in_transaction(transaction, &db, user_id).await?;
                    Ok(true)
                })
            })
            .await
    }

    async fn eligible_account(&self, email: &str) -> Result<bool> {
        let connection = self.db.connection()?;
        let backend = connection.get_database_backend();
        let sql = match backend {
            DbBackend::Postgres => {
                "SELECT id FROM tb_users WHERE email = $1 AND email_verified_at IS NOT NULL AND disabled_at IS NULL"
            }
            DbBackend::MySql => {
                "SELECT id FROM tb_users WHERE email = ? AND email_verified_at IS NOT NULL AND disabled_at IS NULL"
            }
            _ => return Err(unsupported_backend()),
        };
        Ok(connection
            .query_one_raw(Statement::from_sql_and_values(
                backend,
                &crate::auth::resolve_auth_sql(&self.db, sql)?,
                [email.to_owned().into()],
            ))
            .await
            .map_err(Error::internal)?
            .is_some())
    }

    async fn issue_code_uniform(
        &self,
        purpose: &str,
        email: &str,
    ) -> Result<Option<IssuedVerificationCode>> {
        match self.codes.issue(purpose, email, self.code_policy).await {
            Ok(code) => Ok(Some(code)),
            Err(Error::RateLimited | Error::RateLimitedWithRetryAfter(_)) => Ok(None),
            Err(error) => Err(error),
        }
    }

    async fn limit(&self, endpoint: &str, source: &str, identity: &str) -> Result<()> {
        if source.is_empty()
            || source.len() > 160
            || source.bytes().any(|byte| byte.is_ascii_control())
        {
            return Err(Error::BadRequest("invalid trusted request source".into()));
        }
        let key = format!("auth:{endpoint}:{source}:{}", digest(identity));
        self.limiter
            .check(&key, REQUEST_LIMIT, REQUEST_WINDOW)
            .await?;
        Ok(())
    }
}

/// 在 `DatabasePlugin`、`AuthPlugin` 和 `JwtPlugin` 之后注册 [`AuthFlows`]。
pub struct AuthFlowsPlugin {
    refresh_lifetime: ChronoDuration,
    code_policy: VerificationCodePolicy,
    expected: OnceLock<Arc<AuthFlows>>,
}

impl AuthFlowsPlugin {
    /// 创建应用流程服务，refresh token 有效期默认为 30 天。
    pub fn new() -> Self {
        Self {
            refresh_lifetime: ChronoDuration::days(30),
            code_policy: VerificationCodePolicy::default(),
            expected: OnceLock::new(),
        }
    }

    /// 设置 refresh token 家族的最长有效期，范围为 1 秒至 1 年。
    #[must_use]
    pub fn with_refresh_lifetime(mut self, lifetime: ChronoDuration) -> Self {
        self.refresh_lifetime = lifetime;
        self
    }

    /// 设置邮箱验证码和密码重置码的策略。
    #[must_use]
    pub fn with_code_policy(mut self, policy: VerificationCodePolicy) -> Self {
        self.code_policy = policy;
        self
    }
}

impl Default for AuthFlowsPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for AuthFlowsPlugin {
    fn name(&self) -> &'static str {
        "auth-flows"
    }

    fn build(&self, app: &mut AppBuilder) -> Result<()> {
        if app.get::<AuthFlows>().is_some() {
            return Err(Error::custom(
                "auth_flows_already_registered",
                "register only one AuthFlowsPlugin per application",
            ));
        }
        if !(1..=31_536_000).contains(&self.refresh_lifetime.num_seconds()) {
            return Err(Error::Config(crate::config::ConfigError::Invalid(
                "auth flow refresh lifetime must be between 1 second and 365 days".into(),
            )));
        }
        let required = |name: &'static str| {
            Error::custom(
                "auth_flow_dependency_missing",
                format!("AuthFlowsPlugin requires {name} to be registered first"),
            )
        };
        let db = app
            .get::<Db>()
            .map(|value| (*value).clone())
            .ok_or_else(|| required("DatabasePlugin"))?;
        let passwords = app
            .get::<Passwords>()
            .map(|value| (*value).clone())
            .ok_or_else(|| required("AuthPlugin"))?;
        let jwt = app
            .get::<JwtTokens>()
            .map(|value| (*value).clone())
            .ok_or_else(|| required("JwtPlugin"))?;
        let refresh = app
            .get::<RefreshTokens>()
            .map(|value| (*value).clone())
            .ok_or_else(|| required("AuthPlugin with DatabasePlugin"))?;
        let codes = app
            .get::<VerificationCodes>()
            .map(|value| (*value).clone())
            .ok_or_else(|| required("AuthPlugin with DatabasePlugin"))?;
        let limiter = app
            .get::<RateLimiter>()
            .map(|value| (*value).clone())
            .ok_or_else(|| required("DatabasePlugin shared rate limiter"))?;
        app.insert(
            AuthFlows::new(db, passwords, jwt, refresh, codes, limiter)
                .with_refresh_lifetime(self.refresh_lifetime)
                .with_code_policy(self.code_policy),
        );
        self.expected
            .set(
                app.get::<AuthFlows>()
                    .ok_or_else(|| Error::internal(MissingRegisteredFlows))?,
            )
            .map_err(|_| {
                Error::custom(
                    "auth_flows_plugin_reused",
                    "AuthFlowsPlugin can only build once",
                )
            })?;
        Ok(())
    }

    fn finish<'a>(&'a self, app: &'a App) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let expected = self
                .expected
                .get()
                .ok_or_else(|| Error::internal(MissingRegisteredFlows))?;
            let registered = app.state().try_get::<AuthFlows>()?;
            if !Arc::ptr_eq(expected, &registered) {
                return Err(Error::custom(
                    "auth_flows_replaced",
                    "a later plugin replaced the AuthFlows service",
                ));
            }
            Ok(())
        })
    }
}

#[derive(Debug)]
struct MissingRegisteredFlows;

impl std::fmt::Display for MissingRegisteredFlows {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AuthFlowsPlugin built without registering its service")
    }
}

impl std::error::Error for MissingRegisteredFlows {}

/// 登录或 refresh 成功后一次性返回的凭据对。
pub struct AuthCredentials {
    user_id: i64,
    access_token: String,
    refresh_token: IssuedRefreshToken,
    refresh_expires_at: chrono::DateTime<Utc>,
}

/// Cookie Session 登录成功后一次性返回的 CSRF 凭据。
pub struct SessionCredentials {
    user_id: i64,
    csrf_token: String,
}

impl SessionCredentials {
    /// 返回与 Cookie Session 关联的账户 ID。
    pub const fn user_id(&self) -> i64 {
        self.user_id
    }

    /// 返回供应用响应一次性发送的 CSRF token。
    pub fn csrf_token(&self) -> &str {
        &self.csrf_token
    }
}

impl std::fmt::Debug for SessionCredentials {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionCredentials")
            .field("user_id", &self.user_id)
            .field("csrf_token", &"[REDACTED]")
            .finish()
    }
}

impl AuthCredentials {
    /// 返回与此凭据对关联的账户 ID。
    pub const fn user_id(&self) -> i64 {
        self.user_id
    }
    /// 返回签名后的短期 bearer access token。
    pub fn access_token(&self) -> &str {
        &self.access_token
    }
    /// 返回供当前响应发送的不透明 refresh token。
    pub fn refresh_token(&self) -> &IssuedRefreshToken {
        &self.refresh_token
    }
    /// 返回 refresh 不会延长的 token 家族有效期。
    pub const fn refresh_expires_at(&self) -> chrono::DateTime<Utc> {
        self.refresh_expires_at
    }
}

impl std::fmt::Debug for AuthCredentials {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AuthCredentials")
            .field("user_id", &self.user_id)
            .field("access_token", &"[REDACTED]")
            .field("refresh_token", &self.refresh_token)
            .field("refresh_expires_at", &self.refresh_expires_at)
            .finish()
    }
}

fn normalize_email(email: &str) -> Result<String> {
    let normalized = email.trim().to_ascii_lowercase();
    if normalized.is_empty()
        || normalized.len() > 320
        || normalized.bytes().any(|byte| byte.is_ascii_control())
        || !normalized.contains('@')
    {
        return Err(Error::BadRequest("invalid email address".into()));
    }
    Ok(normalized)
}

async fn bind_session(session: &Session, user_id: i64) -> Result<SessionCredentials> {
    let mut random = [0_u8; 32];
    getrandom::fill(&mut random).map_err(Error::internal)?;
    let csrf_token = URL_SAFE_NO_PAD.encode(random);
    let csrf_hash = digest(&csrf_token);

    session.cycle_id().await.map_err(Error::internal)?;
    remove_auth_values(session).await?;
    if let Err(error) = session.insert(AUTH_CSRF_HASH_KEY, csrf_hash).await {
        let _ = remove_auth_values(session).await;
        return Err(Error::internal(error));
    }
    if let Err(error) = session.insert(AUTH_USER_ID_KEY, user_id).await {
        let _ = remove_auth_values(session).await;
        return Err(Error::internal(error));
    }

    Ok(SessionCredentials {
        user_id,
        csrf_token,
    })
}

async fn remove_auth_values(session: &Session) -> Result<()> {
    session
        .remove_value(AUTH_USER_ID_KEY)
        .await
        .map_err(Error::internal)?;
    session
        .remove_value(AUTH_CSRF_HASH_KEY)
        .await
        .map_err(Error::internal)?;
    Ok(())
}

async fn clear_session(session: &Session) -> Result<()> {
    remove_auth_values(session).await?;
    session.cycle_id().await.map_err(Error::internal)
}

fn digest(value: &str) -> String {
    use sha2::{Digest, Sha256};
    let bytes = Sha256::digest(value.as_bytes());
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn unsupported_backend() -> Error {
    Error::custom(
        "unsupported_auth_database",
        "authentication requires PostgreSQL or MySQL",
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tower_sessions::{MemoryStore, Session, SessionStore};

    use super::{AUTH_CSRF_HASH_KEY, AUTH_USER_ID_KEY, bind_session, clear_session, digest};

    #[tokio::test]
    async fn session_login_rotates_id_preserves_app_data_and_redacts_csrf()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let store = Arc::new(MemoryStore::default());
        let session = Session::new(None, store.clone(), None);
        session.insert("app.preference", "compact").await?;
        session.save().await?;
        let old_id = session.id().ok_or_else(|| {
            crate::Error::internal(std::io::Error::other("session ID missing after save"))
        })?;
        let credentials = bind_session(&session, 42).await?;
        let csrf_token = credentials.csrf_token().to_owned();
        assert_eq!(credentials.user_id(), 42);
        assert!(!format!("{credentials:?}").contains(&csrf_token));
        assert!(session.id().is_none());
        session.save().await?;
        let new_id = session.id().ok_or_else(|| {
            crate::Error::internal(std::io::Error::other(
                "rotated session ID missing after save",
            ))
        })?;
        assert_ne!(new_id, old_id);
        assert_eq!(session.get::<i64>(AUTH_USER_ID_KEY).await?, Some(42));
        let csrf_hash = session
            .get::<String>(AUTH_CSRF_HASH_KEY)
            .await?
            .ok_or_else(|| crate::Error::internal(std::io::Error::other("CSRF hash missing")))?;
        assert_eq!(csrf_hash, digest(&csrf_token));
        assert_ne!(csrf_hash, csrf_token);
        assert_eq!(
            session.get::<String>("app.preference").await?,
            Some("compact".to_owned())
        );

        assert!(
            store
                .load(&old_id)
                .await
                .map_err(crate::Error::internal)?
                .is_none()
        );

        clear_session(&session).await?;
        assert!(session.id().is_none());
        session.save().await?;
        assert!(session.get::<i64>(AUTH_USER_ID_KEY).await?.is_none());
        assert!(session.get::<String>(AUTH_CSRF_HASH_KEY).await?.is_none());
        assert_eq!(
            session.get::<String>("app.preference").await?,
            Some("compact".to_owned())
        );
        assert!(
            store
                .load(&new_id)
                .await
                .map_err(crate::Error::internal)?
                .is_none()
        );
        Ok(())
    }
}
