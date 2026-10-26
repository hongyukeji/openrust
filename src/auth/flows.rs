//! 基于数据库的账户与凭据生命周期编排。

use std::{
    sync::{Arc, OnceLock},
    time::Duration,
};

use crate::core::{App, AppBuilder, BoxFuture, Error, Plugin, RateLimiter, Result};
use crate::db::Db;
#[cfg(any(feature = "auth-oidc", feature = "auth-mfa"))]
use crate::db::sea_orm::DatabaseTransaction;
use crate::db::sea_orm::{ConnectionTrait, DbBackend, SqlErr, Statement};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{Duration as ChronoDuration, Utc};

#[cfg(feature = "auth-mfa")]
use crate::auth::mfa::{MfaLoginChallenge, MfaService, RecoveryCodes, TotpEnrollment};
use crate::auth::{
    EncodedPasswordHash, IssuedRefreshToken, IssuedVerificationCode, JwtTokens, Passwords,
    RefreshTokens, Session, VerificationCodePolicy, VerificationCodes,
    database::{AUTH_CSRF_HASH_KEY, AUTH_USER_ID_KEY},
};

/// 登录流程返回已签发凭据，或要求客户端完成 MFA challenge。
pub enum LoginAttempt {
    /// 登录已通过全部启用的因子并签发完整凭据。
    Authenticated(AuthCredentials),
    /// 密码或外部身份验证通过，但完整凭据须等待第二因子。
    #[cfg(feature = "auth-mfa")]
    MfaRequired(MfaLoginChallenge),
}

impl LoginAttempt {
    /// 返回本次成功密码或外部身份验证对应的账户 ID。
    pub const fn user_id(&self) -> i64 {
        match self {
            Self::Authenticated(credentials) => credentials.user_id(),
            #[cfg(feature = "auth-mfa")]
            Self::MfaRequired(challenge) => challenge.user_id(),
        }
    }
}

impl std::fmt::Debug for LoginAttempt {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Authenticated(_) => formatter.write_str("Authenticated([REDACTED])"),
            #[cfg(feature = "auth-mfa")]
            Self::MfaRequired(challenge) => formatter
                .debug_tuple("MfaRequired")
                .field(challenge)
                .finish(),
        }
    }
}

const REQUEST_LIMIT: u64 = 5;
const REQUEST_WINDOW: Duration = Duration::from_mins(15);
const EMAIL_VERIFICATION_PURPOSE: &str = "auth.email_verification";
const PASSWORD_RESET_PURPOSE: &str = "auth.password_reset";

/// 注册前置校验事件；邮箱是个人信息，监听器不得记录原值。
#[derive(Debug, Clone)]
pub struct RegistrationValidating {
    email: String,
}

impl RegistrationValidating {
    /// 返回规范化邮箱，供业务资格策略校验。
    pub fn email(&self) -> &str {
        &self.email
    }
}

/// 用户记录写入事务后、提交前派发的注册成功事实。
#[derive(Debug, Clone, Copy)]
pub struct RegistrationSucceeded {
    user_id: i64,
}

impl RegistrationSucceeded {
    /// 返回新账户 ID。
    pub const fn user_id(&self) -> i64 {
        self.user_id
    }
}

/// 登录凭据验证前的业务校验事件；不含密码、token 或验证码。
#[derive(Debug, Clone)]
pub struct LoginValidating {
    email: String,
}

impl LoginValidating {
    /// 返回规范化邮箱，供业务风控策略校验。
    pub fn email(&self) -> &str {
        &self.email
    }
}

/// 登录成功事实；refresh token 写入与此事件共用事务。
#[derive(Debug, Clone, Copy)]
pub struct LoginSucceeded {
    user_id: i64,
}

impl LoginSucceeded {
    /// 返回已认证账户 ID。
    pub const fn user_id(&self) -> i64 {
        self.user_id
    }
}

/// 登录失败通知不包含身份或失败原因，避免暴露账户状态差异。
#[derive(Debug, Clone, Copy, Default)]
pub struct LoginFailed;

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
    #[cfg(feature = "auth-mfa")]
    mfa: Option<MfaService>,
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
            #[cfg(feature = "auth-mfa")]
            mfa: None,
        }
    }

    #[cfg(feature = "auth-mfa")]
    pub(crate) fn with_mfa(mut self, mfa: Option<MfaService>) -> Self {
        self.mfa = mfa;
        self
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
                "INSERT INTO tb_users (email, password_hash, created_at, updated_at) VALUES ($1, $2, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP) ON CONFLICT (email) DO NOTHING RETURNING id"
            }
            DbBackend::MySql => {
                "INSERT INTO tb_users (email, password_hash, created_at, updated_at) VALUES (?, ?, CURRENT_TIMESTAMP(6), CURRENT_TIMESTAMP(6))"
            }
            _ => return Err(unsupported_backend()),
        };
        let db = self.db.clone();
        let events = self.db.events();
        let insert_email = email.clone();
        let password_hash = password_hash.as_str().to_owned();
        let created = self
            .db
            .transaction(|transaction| {
                let db = db.clone();
                let events = events.clone();
                let email = insert_email.clone();
                let password_hash = password_hash.clone();
                Box::pin(async move {
                    if let Some(events) = &events {
                        events
                            .dispatch_direct_in_transaction(
                                transaction,
                                &RegistrationValidating {
                                    email: email.clone(),
                                },
                            )
                            .await?;
                    }
                    let sql = crate::auth::resolve_auth_sql(&db, sql)?;
                    let inserted = if backend == DbBackend::Postgres {
                        transaction
                            .query_one_raw(Statement::from_sql_and_values(
                                backend,
                                &sql,
                                [email.clone().into(), password_hash.into()],
                            ))
                            .await
                            .map_err(Error::internal)?
                            .map(|row| row.try_get::<i64>("", "id").map_err(Error::internal))
                            .transpose()?
                    } else {
                        match transaction
                            .execute_raw(Statement::from_sql_and_values(
                                backend,
                                &sql,
                                [email.into(), password_hash.into()],
                            ))
                            .await
                        {
                            Ok(result) if result.rows_affected() == 1 => Some(
                                i64::try_from(result.last_insert_id()).map_err(Error::internal)?,
                            ),
                            Ok(_) => None,
                            Err(error)
                                if matches!(
                                    error.sql_err(),
                                    Some(SqlErr::UniqueConstraintViolation(_))
                                ) =>
                            {
                                None
                            }
                            Err(error) => return Err(Error::internal(error)),
                        }
                    };
                    let Some(user_id) = inserted else {
                        return Ok(false);
                    };
                    if let Some(events) = events {
                        events
                            .dispatch_in_transaction(
                                transaction,
                                &RegistrationSucceeded { user_id },
                            )
                            .await?;
                    }
                    Ok(true)
                })
            })
            .await?;
        if !created {
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
        match self.begin_login(email, password, request_source).await? {
            LoginAttempt::Authenticated(credentials) => Ok(credentials),
            #[cfg(feature = "auth-mfa")]
            LoginAttempt::MfaRequired(_) => Err(Error::custom(
                "auth_mfa_required",
                "complete the multi-factor login challenge before receiving credentials",
            )),
        }
    }

    /// 验证密码并返回登录成功凭据或短时 MFA challenge。
    pub async fn begin_login(
        &self,
        email: &str,
        password: &str,
        request_source: &str,
    ) -> Result<LoginAttempt> {
        let user_id = self
            .authenticate_password(email, password, request_source)
            .await?;
        #[cfg(feature = "auth-mfa")]
        if let Some(mfa) = &self.mfa {
            let auth = self.clone();
            let mfa = mfa.clone();
            let source = request_source.to_owned();
            return self
                .db
                .transaction(|transaction| {
                    let auth = auth.clone();
                    let mfa = mfa.clone();
                    let source = source.clone();
                    Box::pin(async move {
                        mfa.lock_user_in_transaction(transaction, user_id).await?;
                        if mfa.enabled_in_transaction(transaction, user_id).await? {
                            Ok(LoginAttempt::MfaRequired(
                                mfa.create_challenge_in_transaction(transaction, user_id, &source)
                                    .await?,
                            ))
                        } else {
                            Ok(LoginAttempt::Authenticated(
                                auth.issue_credentials_in_transaction(transaction, user_id)
                                    .await?,
                            ))
                        }
                    })
                })
                .await;
        }
        Ok(LoginAttempt::Authenticated(
            self.issue_credentials(user_id).await?,
        ))
    }

    /// 原子消费 MFA challenge 并在同一事务内签发完整凭据。
    #[cfg(feature = "auth-mfa")]
    pub async fn complete_mfa_login(
        &self,
        challenge: &str,
        code: &str,
        request_source: &str,
    ) -> Result<AuthCredentials> {
        let mfa = self
            .mfa
            .as_ref()
            .ok_or_else(|| {
                Error::custom(
                    "auth_mfa_unavailable",
                    "multi-factor authentication is not enabled for this application",
                )
            })?
            .clone();
        let auth = self.clone();
        let token = challenge.to_owned();
        let code = code.to_owned();
        let source = request_source.to_owned();
        self.db
            .transaction(|transaction| {
                let mfa = mfa.clone();
                let auth = auth.clone();
                let token = token.clone();
                let code = code.clone();
                let source = source.clone();
                Box::pin(async move {
                    let Some(user_id) = mfa
                        .complete_challenge_in_transaction(transaction, &token, &source, &code)
                        .await?
                    else {
                        return Ok(None);
                    };
                    let credentials = auth
                        .issue_credentials_in_transaction(transaction, user_id)
                        .await?;
                    mfa.record_login_success_in_transaction(transaction, user_id)
                        .await?;
                    Ok(Some(credentials))
                })
            })
            .await?
            .ok_or(Error::Unauthorized)
    }

    /// 重新验证本地密码并为账户创建待确认 TOTP enrollment。
    #[cfg(feature = "auth-mfa")]
    pub async fn begin_totp_enrollment(
        &self,
        user_id: i64,
        email: &str,
        password: &str,
        issuer: &str,
        request_source: &str,
    ) -> Result<TotpEnrollment> {
        let authenticated = self.reauthenticate(email, password, request_source).await?;
        if authenticated != user_id {
            return Err(Error::Unauthorized);
        }
        self.mfa
            .as_ref()
            .ok_or_else(|| {
                Error::custom(
                    "auth_mfa_unavailable",
                    "multi-factor authentication is not enabled for this application",
                )
            })?
            .begin_enrollment(user_id, email, issuer, request_source)
            .await
    }

    /// 验证待确认 seed；成功后一次性返回恢复码并启用 MFA。
    #[cfg(feature = "auth-mfa")]
    pub async fn confirm_totp_enrollment(
        &self,
        user_id: i64,
        code: &str,
        request_source: &str,
    ) -> Result<RecoveryCodes> {
        self.mfa
            .as_ref()
            .ok_or_else(|| {
                Error::custom(
                    "auth_mfa_unavailable",
                    "multi-factor authentication is not enabled for this application",
                )
            })?
            .confirm_enrollment(user_id, request_source, code)
            .await?
            .ok_or(Error::Unauthorized)
    }

    /// 重新验证密码与当前第二因子后禁用 MFA，并撤销所有 refresh token。
    #[cfg(feature = "auth-mfa")]
    pub async fn disable_mfa(
        &self,
        user_id: i64,
        email: &str,
        password: &str,
        code: &str,
        request_source: &str,
    ) -> Result<()> {
        if self.reauthenticate(email, password, request_source).await? != user_id {
            return Err(Error::Unauthorized);
        }
        let mfa = self.mfa.as_ref().ok_or_else(|| {
            Error::custom(
                "auth_mfa_unavailable",
                "multi-factor authentication is not enabled for this application",
            )
        })?;
        if mfa.disable(user_id, request_source, code).await? {
            Ok(())
        } else {
            Err(Error::Unauthorized)
        }
    }

    /// 重新验证密码与当前第二因子后轮换并一次性返回恢复码。
    #[cfg(feature = "auth-mfa")]
    pub async fn regenerate_mfa_recovery_codes(
        &self,
        user_id: i64,
        email: &str,
        password: &str,
        code: &str,
        request_source: &str,
    ) -> Result<RecoveryCodes> {
        if self.reauthenticate(email, password, request_source).await? != user_id {
            return Err(Error::Unauthorized);
        }
        self.mfa
            .as_ref()
            .ok_or_else(|| {
                Error::custom(
                    "auth_mfa_unavailable",
                    "multi-factor authentication is not enabled for this application",
                )
            })?
            .regenerate_recovery_codes(user_id, request_source, code)
            .await?
            .ok_or(Error::Unauthorized)
    }

    async fn issue_credentials(&self, user_id: i64) -> Result<AuthCredentials> {
        let auth = self.clone();
        self.db
            .transaction(|transaction| {
                let auth = auth.clone();
                Box::pin(async move {
                    auth.issue_credentials_in_transaction(transaction, user_id)
                        .await
                })
            })
            .await
    }

    pub(crate) async fn issue_credentials_in_transaction(
        &self,
        transaction: &DatabaseTransaction,
        user_id: i64,
    ) -> Result<AuthCredentials> {
        let access_token = self.jwt.issue_access_token(user_id)?;
        let expires_at = Utc::now() + self.refresh_lifetime;
        let refresh_token = self
            .refresh
            .issue_in_transaction(transaction, user_id, expires_at)
            .await?;
        if let Some(events) = self.db.events() {
            events
                .dispatch_in_transaction(transaction, &LoginSucceeded { user_id })
                .await?;
        }
        Ok(AuthCredentials {
            user_id,
            access_token,
            refresh_token,
            refresh_expires_at: expires_at,
        })
    }

    #[cfg(feature = "auth-mfa")]
    async fn mfa_enabled(&self, user_id: i64) -> Result<bool> {
        match &self.mfa {
            Some(mfa) => mfa.enabled(user_id).await,
            None => Ok(false),
        }
    }

    /// 使用当前密码重新验证账户，不签发新的 bearer 凭据。
    pub async fn reauthenticate(
        &self,
        email: &str,
        password: &str,
        request_source: &str,
    ) -> Result<i64> {
        self.authenticate_password(email, password, request_source)
            .await
    }

    #[cfg(feature = "auth-oidc")]
    pub(crate) async fn prepare_external_password_hash(&self) -> Result<EncodedPasswordHash> {
        let mut random = [0_u8; 32];
        getrandom::fill(&mut random).map_err(Error::internal)?;
        let secret = URL_SAFE_NO_PAD.encode(random);
        self.passwords.hash(&secret).await
    }

    #[cfg(feature = "auth-oidc")]
    pub(crate) async fn limit_external_identity(
        &self,
        request_source: &str,
        identity: &str,
    ) -> Result<()> {
        self.limit("external_identity", request_source, identity)
            .await
    }

    #[cfg(feature = "auth-oidc")]
    #[allow(
        clippy::too_many_lines,
        reason = "新账户、外部身份、凭据与审计必须在同一事务中完成"
    )]
    pub(crate) async fn complete_external_identity_in_transaction(
        &self,
        transaction: &DatabaseTransaction,
        provider: &str,
        identity: &crate::auth::VerifiedExternalIdentity,
        new_password_hash: Option<&EncodedPasswordHash>,
        request_source: &str,
    ) -> Result<LoginAttempt> {
        let identity_table = self.db.quoted_table_name("external_identities")?;
        let backend = self.db.backend()?;
        let lookup = match backend {
            DbBackend::Postgres => {
                format!("SELECT user_id FROM {identity_table} WHERE identity_hash = $1")
            }
            DbBackend::MySql => {
                format!("SELECT user_id FROM {identity_table} WHERE identity_hash = ?")
            }
            _ => return Err(unsupported_backend()),
        };
        let identity_hash = identity.identity_hash(provider);
        let values = [identity_hash.clone().into()];
        let user_id = if let Some(row) = transaction
            .query_one_raw(Statement::from_sql_and_values(
                backend,
                &lookup,
                values.clone(),
            ))
            .await
            .map_err(Error::internal)?
        {
            row.try_get::<i64>("", "user_id").map_err(Error::internal)?
        } else {
            let email = identity
                .email()
                .filter(|_| identity.email_verified())
                .ok_or_else(external_identity_link_required)?;
            let email = normalize_email(email)?;
            let hash = new_password_hash.ok_or_else(external_identity_link_required)?;
            let users_table = self.db.quoted_table_name("users")?;
            let insert_user = match backend {
                DbBackend::Postgres => format!(
                    "INSERT INTO {users_table} (email, password_hash, email_verified_at, created_at, updated_at) VALUES ($1, $2, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP) ON CONFLICT (email) DO NOTHING RETURNING id"
                ),
                DbBackend::MySql => format!(
                    "INSERT INTO {users_table} (email, password_hash, email_verified_at, created_at, updated_at) VALUES (?, ?, CURRENT_TIMESTAMP(6), CURRENT_TIMESTAMP(6), CURRENT_TIMESTAMP(6))"
                ),
                _ => return Err(unsupported_backend()),
            };
            let created_user_id = if backend == DbBackend::Postgres {
                transaction
                    .query_one_raw(Statement::from_sql_and_values(
                        backend,
                        &insert_user,
                        [email.clone().into(), hash.as_str().to_owned().into()],
                    ))
                    .await
                    .map_err(Error::internal)?
                    .map(|row| row.try_get::<i64>("", "id").map_err(Error::internal))
                    .transpose()?
            } else {
                match transaction
                    .execute_raw(Statement::from_sql_and_values(
                        backend,
                        &insert_user,
                        [email.clone().into(), hash.as_str().to_owned().into()],
                    ))
                    .await
                {
                    Ok(result) if result.rows_affected() == 1 => {
                        Some(i64::try_from(result.last_insert_id()).map_err(Error::internal)?)
                    }
                    Ok(_) => None,
                    Err(error)
                        if matches!(
                            error.sql_err(),
                            Some(SqlErr::UniqueConstraintViolation(_))
                        ) =>
                    {
                        None
                    }
                    Err(error) => return Err(Error::internal(error)),
                }
            };
            let Some(user_id) = created_user_id else {
                return Err(external_identity_link_required());
            };
            if let Some(events) = self.db.events() {
                events
                    .dispatch_in_transaction(transaction, &RegistrationSucceeded { user_id })
                    .await?;
            }
            user_id
        };

        let users_table = self.db.quoted_table_name("users")?;
        let active_user = match backend {
            DbBackend::Postgres => {
                format!("SELECT id FROM {users_table} WHERE id = $1 AND disabled_at IS NULL")
            }
            DbBackend::MySql => {
                format!("SELECT id FROM {users_table} WHERE id = ? AND disabled_at IS NULL")
            }
            _ => return Err(unsupported_backend()),
        };
        if transaction
            .query_one_raw(Statement::from_sql_and_values(
                backend,
                &active_user,
                [user_id.into()],
            ))
            .await
            .map_err(Error::internal)?
            .is_none()
        {
            return Err(Error::Unauthorized);
        }

        let insert_identity = match backend {
            DbBackend::Postgres => format!(
                "INSERT INTO {identity_table} (user_id, identity_hash, provider, issuer, subject, created_at) VALUES ($1, $2, $3, $4, $5, CURRENT_TIMESTAMP) ON CONFLICT (identity_hash) DO NOTHING"
            ),
            DbBackend::MySql => format!(
                "INSERT INTO {identity_table} (user_id, identity_hash, provider, issuer, subject, created_at) VALUES (?, ?, ?, ?, ?, CURRENT_TIMESTAMP(6))"
            ),
            _ => return Err(unsupported_backend()),
        };
        let inserted = transaction
            .execute_raw(Statement::from_sql_and_values(
                backend,
                &insert_identity,
                [
                    user_id.into(),
                    identity_hash.into(),
                    provider.to_owned().into(),
                    identity.issuer().to_owned().into(),
                    identity.subject().to_owned().into(),
                ],
            ))
            .await;
        if let Err(error) = inserted
            && !matches!(error.sql_err(), Some(SqlErr::UniqueConstraintViolation(_)))
        {
            return Err(Error::internal(error));
        }
        let mapped_user = transaction
            .query_one_raw(Statement::from_sql_and_values(backend, &lookup, values))
            .await
            .map_err(Error::internal)?
            .ok_or_else(external_identity_link_required)?
            .try_get::<i64>("", "user_id")
            .map_err(Error::internal)?;
        if mapped_user != user_id {
            return Err(external_identity_link_required());
        }

        #[cfg(feature = "auth-mfa")]
        if let Some(mfa) = &self.mfa {
            mfa.lock_user_in_transaction(transaction, user_id).await?;
            if mfa.enabled_in_transaction(transaction, user_id).await? {
                return Ok(LoginAttempt::MfaRequired(
                    mfa.create_challenge_in_transaction(transaction, user_id, request_source)
                        .await?,
                ));
            }
        }
        Ok(LoginAttempt::Authenticated(
            self.issue_credentials_in_transaction(transaction, user_id)
                .await?,
        ))
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
        #[cfg(feature = "auth-mfa")]
        if self.mfa_enabled(user_id).await? {
            return Err(Error::Unauthorized);
        }
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
        if let Some(events) = self.db.events() {
            events
                .dispatch_direct(&LoginValidating {
                    email: email.clone(),
                })
                .await?;
        }
        let result = self.verify_password(&email, password).await;
        if result.is_err()
            && let Some(events) = self.db.events()
        {
            events.dispatch_direct(&LoginFailed).await?;
        }
        result
    }

    async fn verify_password(&self, email: &str, password: &str) -> Result<i64> {
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
                [email.to_owned().into()],
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
        let flows = AuthFlows::new(db, passwords, jwt, refresh, codes, limiter)
            .with_refresh_lifetime(self.refresh_lifetime)
            .with_code_policy(self.code_policy);
        #[cfg(feature = "auth-mfa")]
        let flows = flows.with_mfa(app.get::<MfaService>().map(|value| (*value).clone()));
        app.insert(flows);
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

#[cfg(feature = "auth-oidc")]
fn external_identity_link_required() -> Error {
    Error::custom(
        "auth_external_identity_link_required",
        "external identity requires an explicit local account link",
    )
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
