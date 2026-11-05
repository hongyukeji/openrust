//! 基于数据库的账户与凭据生命周期编排。

use std::{sync::Arc, time::Duration};

use crate::core::{BoxFuture, Error, RateLimiter, Result};
use crate::db::Db;
#[cfg(feature = "auth")]
use crate::db::sea_orm::DatabaseTransaction;
use crate::db::sea_orm::{ConnectionTrait, DbBackend, SqlErr, Statement};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
#[cfg(feature = "auth-fortify")]
use chrono::SubsecRound as _;
use chrono::{Duration as ChronoDuration, Utc};

#[cfg(feature = "auth-fortify")]
use crate::auth::credential::TwoFactorLoginChallenge;
use crate::auth::{
    EncodedPasswordHash, IssuedRefreshToken, IssuedVerificationCode, JwtTokens, RefreshTokens,
    Session, VerificationCodePolicy, VerificationCodes,
    database::{AUTH_CSRF_HASH_KEY, AUTH_USER_ID_KEY},
};

/// 登录流程返回已签发凭据，或要求客户端完成一次性双因素挑战。
pub enum LoginAttempt {
    /// 登录已通过全部启用的因子并签发完整凭据。
    Authenticated(AuthCredentials),
    /// 密码或外部身份验证通过，但完整凭据须等待第二因子。
    #[cfg(feature = "auth-fortify")]
    TwoFactorRequired(TwoFactorLoginChallenge),
}

impl LoginAttempt {
    /// 返回本次成功密码或外部身份验证对应的账户 ID。
    pub const fn user_id(&self) -> i64 {
        match self {
            Self::Authenticated(credentials) => credentials.user_id(),
            #[cfg(feature = "auth-fortify")]
            Self::TwoFactorRequired(challenge) => challenge.user_id(),
        }
    }
}

impl std::fmt::Debug for LoginAttempt {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Authenticated(_) => formatter.write_str("Authenticated([REDACTED])"),
            #[cfg(feature = "auth-fortify")]
            Self::TwoFactorRequired(challenge) => formatter
                .debug_tuple("TwoFactorRequired")
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

/// API双因素登录提交前的显式事务审计扩展；不包含验证码、密钥或凭据。
///
/// 实现须使用传入事务写入；错误会回滚本次因子消费和凭据签发。
#[cfg(feature = "auth-fortify")]
pub trait ApiLoginAuditSink: Send + Sync + 'static {
    /// 核验审计目标与凭据流程的数据域；非数据库sink可保留默认实现。
    fn require_store(&self, _db: &Db) -> Result<()> {
        Ok(())
    }
    /// 在当前账户事务中记录已经实际通过双因素校验的登录。
    fn record_in_transaction<'a>(
        &'a self,
        transaction: &'a DatabaseTransaction,
        user_id: i64,
    ) -> BoxFuture<'a, Result<()>>;
}

/// 共享账户凭据流程。HTTP 路由与消息投递仍由应用负责。
#[derive(Clone)]
pub struct AuthFlows {
    db: Db,
    passwords: std::sync::Arc<dyn crate::hashing::Hasher>,
    jwt: JwtTokens,
    refresh: RefreshTokens,
    codes: VerificationCodes,
    limiter: RateLimiter,
    refresh_lifetime: ChronoDuration,
    code_policy: VerificationCodePolicy,
    #[cfg(feature = "auth-fortify")]
    factor_authority: Option<super::fortify::CredentialAuthority>,
    #[cfg(feature = "auth-fortify")]
    factor_actions: Option<super::fortify::TwoFactorActions>,
    #[cfg(feature = "auth-fortify")]
    api_login_audit: Option<Arc<dyn ApiLoginAuditSink>>,
}

impl std::fmt::Debug for AuthFlows {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AuthFlows")
    }
}

#[cfg(feature = "auth-fortify")]
struct IssuedCredentialCheck {
    grant: super::credential::JwtCredentialGrant,
    leaf_hash: String,
    access_deadline: chrono::DateTime<Utc>,
    refresh_deadline: chrono::DateTime<Utc>,
}

#[cfg(feature = "auth-fortify")]
impl IssuedCredentialCheck {
    fn capture(flow: &AuthFlows, credentials: &AuthCredentials) -> Result<Self> {
        let claims = flow.jwt.verify_credential(credentials.access_token())?;
        if claims.user_id != credentials.user_id {
            return Err(Error::Unauthorized);
        }
        Ok(Self {
            grant: claims.grant.ok_or(Error::Unauthorized)?,
            leaf_hash: digest(credentials.refresh_token.expose()),
            access_deadline: claims.expires_at.into(),
            refresh_deadline: credentials.refresh_expires_at,
        })
    }

    fn require_current(&self) -> Result<()> {
        let now = Utc::now();
        if self.access_deadline <= now || self.refresh_deadline <= now {
            return Err(Error::Unauthorized);
        }
        Ok(())
    }

    async fn validate(
        &self,
        flow: &AuthFlows,
        tx: &DatabaseTransaction,
        user_id: i64,
    ) -> Result<()> {
        let authority = flow.factor_authority.as_ref().ok_or(Error::Unauthorized)?;
        authority
            .check_context_in_transaction(tx, user_id, &self.grant.context)
            .await?;
        let deadline = flow
            .refresh
            .validate_family_in_transaction(tx, user_id, &self.grant.family_id, &self.grant.context)
            .await?;
        let row = flow
            .db
            .table("auth_refresh_tokens")?
            .using_transaction(tx)
            .select(&["token_hash", "family_id", "user_id"])?
            .where_eq("token_hash", self.leaf_hash.clone())?
            .where_null("consumed_at")?
            .where_null("revoked_at")?
            .first()
            .await?
            .ok_or(Error::Unauthorized)?;
        if row
            .try_get::<String>("", "token_hash")
            .map_err(Error::internal)?
            != self.leaf_hash
            || row
                .try_get::<String>("", "family_id")
                .map_err(Error::internal)?
                != self.grant.family_id
            || row.try_get::<i64>("", "user_id").map_err(Error::internal)? != user_id
            || deadline != self.refresh_deadline
        {
            return Err(Error::Unauthorized);
        }
        self.require_current()
    }
}

#[cfg(feature = "auth-fortify")]
#[derive(Default)]
struct LoginCommitCheck {
    user_id: i64,
    password_digest: Option<String>,
    session_parent: Option<super::VerifiedCredential>,
    issued: Option<IssuedCredentialCheck>,
    challenge: Option<super::fortify::ChallengeCommitCheck>,
    #[cfg(feature = "auth-oidc")]
    external: Option<(super::oidc::ExternalChallengeBinding, chrono::DateTime<Utc>)>,
}

#[cfg(feature = "auth-fortify")]
impl LoginCommitCheck {
    async fn validate(&self, flow: &AuthFlows, tx: &DatabaseTransaction) -> Result<()> {
        let authority = flow.factor_authority.as_ref().ok_or(Error::Unauthorized)?;
        authority
            .lock_active_user_in_transaction(tx, self.user_id)
            .await?;
        if let Some(password) = &self.password_digest {
            authority
                .require_password_for_login(tx, self.user_id, password)
                .await?;
        }
        #[cfg(feature = "auth-oidc")]
        if let Some((external, deadline)) = &self.external {
            let issued = self.issued.as_ref().ok_or(Error::Unauthorized)?;
            if *deadline <= Utc::now() {
                return Err(Error::Unauthorized);
            }
            external
                .validate_bound(
                    tx,
                    &flow.db,
                    &issued.grant.family_id,
                    issued.refresh_deadline,
                )
                .await?;
        }
        if let Some(challenge) = &self.challenge {
            challenge.validate(authority, tx).await?;
        }
        if let Some(issued) = &self.issued {
            issued.validate(flow, tx, self.user_id).await?;
        }
        if let Some(parent) = &self.session_parent {
            authority.validate_parent_in_transaction(tx, parent).await?;
            self.issued
                .as_ref()
                .ok_or(Error::Unauthorized)?
                .require_current()?;
        }
        if let Some(challenge) = &self.challenge {
            challenge.require_current()?;
        }
        #[cfg(feature = "auth-oidc")]
        if self
            .external
            .as_ref()
            .is_some_and(|(_, deadline)| *deadline <= Utc::now())
        {
            return Err(Error::Unauthorized);
        }
        Ok(())
    }
}

impl AuthFlows {
    /// 使用应用所需的共享服务创建流程服务。
    pub fn new(
        db: Db,
        passwords: std::sync::Arc<dyn crate::hashing::Hasher>,
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
            #[cfg(feature = "auth-fortify")]
            factor_authority: None,
            #[cfg(feature = "auth-fortify")]
            factor_actions: None,
            #[cfg(feature = "auth-fortify")]
            api_login_audit: None,
        }
    }

    /// 显式选择同一数据库凭据因子策略，拒绝跨账户域的签发。
    #[cfg(feature = "auth-fortify")]
    pub fn with_factor_authority(
        mut self,
        authority: super::fortify::CredentialAuthority,
    ) -> Result<Self> {
        authority.require_store(&self.db)?;
        self.refresh.require_store(&self.db)?;
        self.factor_authority = Some(authority);
        Ok(self)
    }

    /// 为直接因子挑战选择应用实际注册的验证与恢复码扩展。
    #[cfg(feature = "auth-fortify")]
    #[must_use]
    pub fn with_factor_actions(mut self, actions: super::fortify::TwoFactorActions) -> Self {
        self.factor_actions = Some(actions);
        self
    }

    /// 显式安装API双因素登录的同事务审计；未选择时不会查询审计表。
    #[cfg(feature = "auth-fortify")]
    pub fn with_api_login_audit(mut self, audit: Arc<dyn ApiLoginAuditSink>) -> Result<Self> {
        audit.require_store(&self.db)?;
        self.api_login_audit = Some(audit);
        Ok(self)
    }

    #[cfg(feature = "auth-oidc")]
    pub(crate) fn require_store(&self, db: &Db) -> Result<()> {
        if !self.db.same_handle(db) {
            return Err(Error::Unauthorized);
        }
        for table in [
            "users",
            "auth_refresh_tokens",
            "personal_access_tokens",
            "external_identities",
            "auth_external_sessions",
        ] {
            if self.db.table_name(table)? != db.table_name(table)? {
                return Err(Error::Unauthorized);
            }
        }
        self.refresh.require_store(db)?;
        #[cfg(feature = "auth-fortify")]
        if let Some(authority) = &self.factor_authority {
            authority.require_store(db)?;
        }
        Ok(())
    }

    #[cfg(all(feature = "auth-oidc", feature = "auth-fortify"))]
    pub(crate) fn external_challenge_table(&self) -> Option<&'static str> {
        #[cfg(feature = "auth-fortify")]
        if self.factor_authority.is_some() {
            return Some("auth_factor_challenges");
        }
        None
    }

    #[cfg(all(feature = "auth-oidc", feature = "auth-fortify"))]
    pub(crate) fn attachment_authority(&self) -> Option<&super::fortify::CredentialAuthority> {
        self.factor_authority.as_ref()
    }

    #[cfg(all(feature = "auth-oidc", feature = "auth-fortify"))]
    pub(crate) async fn authorize_refresh_attachment(
        &self,
        transaction: &DatabaseTransaction,
        user_id: i64,
        raw: &str,
    ) -> Result<(String, chrono::DateTime<Utc>)> {
        let authority = self.factor_authority.as_ref().ok_or(Error::Unauthorized)?;
        let (family, expiry, context) = self
            .refresh
            .attach_context_in_transaction(transaction, user_id, raw)
            .await?;
        authority
            .check_context_in_transaction(transaction, user_id, &context)
            .await?;
        if expiry <= Utc::now() {
            return Err(Error::Unauthorized);
        }
        Ok((family, expiry))
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
        let password_hash =
            crate::auth::EncodedPasswordHash::make(self.passwords.as_ref(), password).await?;
        let connection = self.db.connection()?;
        let backend = connection.get_database_backend();
        let sql = match backend {
            DbBackend::Postgres => {
                "INSERT INTO tb_users (email, password_hash, created_at, updated_at) VALUES ($1, $2, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP) ON CONFLICT (email) DO NOTHING RETURNING id"
            }
            DbBackend::MySql => {
                "INSERT INTO tb_users (email, password_hash, created_at, updated_at) VALUES (?, ?, CURRENT_TIMESTAMP(6), CURRENT_TIMESTAMP(6))"
            }
            DbBackend::Sqlite => {
                "INSERT INTO tb_users (email,password_hash,created_at,updated_at) VALUES (?,?,?,?) ON CONFLICT (email) DO NOTHING"
            }
            _ => return Err(unsupported_backend()),
        };
        let db = self.db.clone();
        let events = self.db.events();
        let insert_email = email.clone();
        let password_hash = password_hash.as_str().to_owned();
        let created = self
            .db
            .transaction_for_update(|transaction| {
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
                    let inserted = insert_registered_user(
                        transaction,
                        &db,
                        backend,
                        sql,
                        &email,
                        &password_hash,
                    )
                    .await?;
                    let Some(user_id) = inserted else {
                        return Ok(false);
                    };
                    if user_id <= 0 {
                        return Err(Error::internal(std::io::Error::other(
                            "inserted user ID is invalid",
                        )));
                    }
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
            DbBackend::Sqlite => {
                "UPDATE tb_users SET email_verified_at=?,updated_at=? WHERE email=? AND email_verified_at IS NULL AND disabled_at IS NULL"
            }
            _ => return Err(unsupported_backend()),
        };
        let result = connection
            .execute_raw(Statement::from_sql_and_values(
                backend,
                &crate::auth::resolve_auth_sql(&self.db, sql)?,
                if backend == DbBackend::Sqlite {
                    let now = Utc::now();
                    vec![
                        crate::db::dialect::utc_value(backend, now),
                        crate::db::dialect::utc_value(backend, now),
                        email.into(),
                    ]
                } else {
                    vec![email.into()]
                },
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
            DbBackend::MySql | DbBackend::Sqlite => {
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
            #[cfg(feature = "auth-fortify")]
            LoginAttempt::TwoFactorRequired(_) => Err(Error::custom(
                "auth_two_factor_required",
                "请完成双因素登录挑战后再获取凭据",
            )),
        }
    }

    /// 验证密码并返回登录成功凭据或现代短时双因素挑战。
    pub async fn begin_login(
        &self,
        email: &str,
        password: &str,
        request_source: &str,
    ) -> Result<LoginAttempt> {
        let (user_id, hash) = self
            .authenticate_password_version(email, password, request_source)
            .await?;
        #[cfg(feature = "auth-fortify")]
        if self.factor_authority.is_some() {
            return self
                .begin_password_in_scope(
                    user_id,
                    digest(hash.as_str()),
                    request_source,
                    "password",
                    None,
                )
                .await;
        }
        #[cfg(not(feature = "auth-fortify"))]
        drop(hash);
        Ok(LoginAttempt::Authenticated(
            self.issue_credentials(user_id).await?,
        ))
    }

    #[cfg(feature = "auth-fortify")]
    pub(crate) async fn begin_verified_api_login(
        &self,
        user: &super::AuthUser,
        provider: &dyn super::UserProvider,
        scope: super::credential::CredentialGuardBinding,
        request_source: &str,
    ) -> Result<LoginAttempt> {
        let id = user.auth_identifier()?;
        let user_id = id
            .as_i64()
            .filter(|id| *id > 0)
            .ok_or(Error::Unauthorized)?;
        let policy = self.factor_authority.as_ref().ok_or(Error::Unauthorized)?;
        let authority = user
            .as_two_factor()
            .and_then(super::fortify::TwoFactorAuthenticatable::two_factor_authority)
            .ok_or(Error::Unauthorized)?;
        authority.validate_account(user, provider).await?;
        policy.require_store(&self.db)?;
        let password = user.auth_password()?.ok_or(Error::Unauthorized)?;
        self.begin_password_in_scope(
            user_id,
            digest(&password),
            request_source,
            "jwt-api-password",
            Some(scope),
        )
        .await
    }

    #[cfg(feature = "auth-fortify")]
    async fn checked_login_transaction<T, F>(&self, callback: F) -> Result<T>
    where
        T: Send + 'static,
        F: for<'a> FnOnce(
                &'a DatabaseTransaction,
            ) -> BoxFuture<'a, Result<(T, Option<LoginCommitCheck>)>>
            + Send,
    {
        let checks = Arc::new(std::sync::OnceLock::new());
        let captured = checks.clone();
        let flow = self.clone();
        self.db
            .transaction_for_update_checked(
                move |tx| {
                    let result = callback(tx);
                    Box::pin(async move {
                        let (value, check) = result.await?;
                        captured.set(check).map_err(|_| Error::Unauthorized)?;
                        Ok(value)
                    })
                },
                move |tx| {
                    Box::pin(async move {
                        if let Some(check) = checks.get().ok_or(Error::Unauthorized)? {
                            check.validate(&flow, tx).await?;
                        }
                        Ok(())
                    })
                },
            )
            .await
    }

    #[cfg(feature = "auth-fortify")]
    async fn begin_password_in_scope(
        &self,
        user_id: i64,
        password_digest: String,
        request_source: &str,
        purpose: &'static str,
        scope: Option<super::credential::CredentialGuardBinding>,
    ) -> Result<LoginAttempt> {
        let this = self.clone();
        let source = request_source.to_owned();
        self.checked_login_transaction(move |tx| {
            Box::pin(async move {
                let policy = this.factor_authority.as_ref().ok_or(Error::Unauthorized)?;
                policy
                    .require_password_for_login(tx, user_id, &password_digest)
                    .await?;
                let decision = policy
                    .begin_login_in_scope_in_transaction(
                        tx,
                        user_id,
                        &source,
                        purpose,
                        scope.clone(),
                    )
                    .await?;
                let mut check = LoginCommitCheck {
                    user_id,
                    password_digest: Some(password_digest),
                    ..Default::default()
                };
                let result = match decision {
                    super::fortify::CredentialLoginDecision::Ready(context) => {
                        let issued = this
                            .issue_context_in_transaction(
                                tx,
                                user_id,
                                (Utc::now() + this.refresh_lifetime).trunc_subsecs(6),
                                *context,
                            )
                            .await?;
                        check.issued = Some(IssuedCredentialCheck::capture(&this, &issued)?);
                        LoginAttempt::Authenticated(issued)
                    }
                    super::fortify::CredentialLoginDecision::Challenge(challenge) => {
                        check.challenge = Some(
                            policy
                                .capture_new_challenge(
                                    tx,
                                    &challenge,
                                    &source,
                                    purpose,
                                    scope.as_ref(),
                                )
                                .await?,
                        );
                        LoginAttempt::TwoFactorRequired(challenge)
                    }
                };
                Ok((result, Some(check)))
            })
        })
        .await
    }

    async fn issue_credentials(&self, user_id: i64) -> Result<AuthCredentials> {
        let auth = self.clone();
        self.db
            .transaction_for_update(|transaction| {
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
        let expires_at = Utc::now() + self.refresh_lifetime;
        #[cfg(feature = "auth-fortify")]
        if let Some(authority) = &self.factor_authority {
            let context = authority
                .context_without_factor_in_transaction(transaction, user_id)
                .await?;
            return self
                .issue_context_in_transaction(transaction, user_id, expires_at, context)
                .await;
        }
        #[cfg(feature = "auth-fortify")]
        super::factor_gate::require_unbound_fortify_account(&self.db, transaction, user_id).await?;
        let access_token = self.jwt.issue_access_token(user_id)?;
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

    #[cfg(all(feature = "auth-fortify", feature = "auth-oidc"))]
    async fn begin_factor_login_in_transaction(
        &self,
        transaction: &DatabaseTransaction,
        user_id: i64,
        request_source: &str,
        purpose: &str,
    ) -> Result<LoginAttempt> {
        let authority = self.factor_authority.as_ref().ok_or(Error::Unauthorized)?;
        match authority
            .begin_login_in_transaction(transaction, user_id, request_source, purpose)
            .await?
        {
            super::fortify::CredentialLoginDecision::Ready(context) => {
                Ok(LoginAttempt::Authenticated(
                    self.issue_context_in_transaction(
                        transaction,
                        user_id,
                        Utc::now() + self.refresh_lifetime,
                        *context,
                    )
                    .await?,
                ))
            }
            super::fortify::CredentialLoginDecision::Challenge(challenge) => {
                Ok(LoginAttempt::TwoFactorRequired(challenge))
            }
        }
    }

    /// 完成直接登录的一次性统一因子挑战；不将第一因素结果升级为第二因素证明。
    #[cfg(feature = "auth-fortify")]
    pub async fn complete_two_factor_login(
        &self,
        challenge: &str,
        code: Option<&str>,
        recovery_code: Option<&str>,
        request_source: &str,
    ) -> Result<AuthCredentials> {
        self.complete_two_factor_login_in_scope(
            challenge,
            code,
            recovery_code,
            request_source,
            None,
            None,
        )
        .await
    }

    #[cfg(feature = "auth-fortify")]
    pub(crate) async fn complete_two_factor_login_in_scope(
        &self,
        challenge: &str,
        code: Option<&str>,
        recovery_code: Option<&str>,
        request_source: &str,
        scope: Option<&super::credential::CredentialGuardBinding>,
        provider: Option<&dyn super::UserProvider>,
    ) -> Result<AuthCredentials> {
        self.limit("factor_challenge", request_source, &digest(challenge))
            .await?;
        let authority = self.factor_authority.as_ref().ok_or(Error::Unauthorized)?;
        let actions = self.factor_actions.as_ref().ok_or_else(|| {
            Error::custom(
                "auth_factor_actions_missing",
                "直接登录挑战必须选择实际因子验证扩展",
            )
        })?;
        let prepared = authority
            .prepare_challenge_in_scope(challenge, request_source, scope)
            .await?;
        prepared.require_scope(scope)?;
        let expected_user = prepared.user_id();
        let expected_expiry = prepared.expires_at();
        if expected_expiry <= Utc::now() {
            return Err(Error::Unauthorized);
        }
        let actions = if let Some(provider) = provider {
            let identifier = serde_json::Value::from(expected_user);
            let user = authority
                .db()
                .with_primary(provider.retrieve_by_id(&identifier))
                .await??
                .ok_or(Error::Unauthorized)?;
            if user.auth_identifier()? != identifier {
                return Err(Error::Unauthorized);
            }
            let model = user.as_two_factor().ok_or(Error::Unauthorized)?;
            model
                .two_factor_authority()
                .ok_or(Error::Unauthorized)?
                .validate_account(&user, provider)
                .await?;
            actions.for_account(model)
        } else {
            actions.clone()
        };
        let verification = prepared.verify(&actions, code, recovery_code).await?;
        let this = self.clone();
        #[cfg(feature = "auth-oidc")]
        let challenge = challenge.to_owned();
        self.checked_login_transaction(move |tx| {
            Box::pin(async move {
                this.finish_login_in_transaction(
                    tx,
                    prepared,
                    verification,
                    #[cfg(feature = "auth-oidc")]
                    &challenge,
                )
                .await
            })
        })
        .await?
        .ok_or(Error::Unauthorized)
    }

    #[cfg(feature = "auth-fortify")]
    async fn finish_login_in_transaction(
        &self,
        transaction: &DatabaseTransaction,
        prepared: super::fortify::PreparedCredentialChallenge,
        verification: super::fortify::CredentialChallengeVerification,
        #[cfg(feature = "auth-oidc")] challenge: &str,
    ) -> Result<(Option<AuthCredentials>, Option<LoginCommitCheck>)> {
        let authority = self.factor_authority.as_ref().ok_or(Error::Unauthorized)?;
        let expected_user = prepared.user_id();
        let expected_expiry = prepared.expires_at();
        // 与后通道注销一致：users→association→challenge/refresh。
        authority
            .lock_active_user_in_transaction(transaction, expected_user)
            .await?;
        #[cfg(feature = "auth-oidc")]
        let association =
            crate::auth::oidc::lock_external_two_factor_challenge(transaction, &self.db, challenge)
                .await?;
        #[cfg(feature = "auth-oidc")]
        if association
            .as_ref()
            .is_some_and(|(_, owner)| *owner != expected_user)
        {
            return Err(Error::Unauthorized);
        }
        let Some(completed) = authority
            .finish_challenge_in_transaction(transaction, prepared, verification)
            .await?
        else {
            return Ok((None, None));
        };
        let user_id = completed
            .context
            .identifier
            .as_i64()
            .ok_or(Error::Unauthorized)?;
        if user_id != expected_user || completed.expires_at != expected_expiry {
            return Err(Error::Unauthorized);
        }
        #[cfg(feature = "auth-oidc")]
        let external = match &association {
            Some(association) => Some(
                super::oidc::ExternalChallengeBinding::capture(
                    transaction,
                    &self.db,
                    association,
                    challenge,
                    completed.check.purpose(),
                )
                .await?,
            ),
            None => None,
        };
        let credentials = self
            .issue_context_in_transaction(
                transaction,
                user_id,
                (Utc::now() + self.refresh_lifetime).trunc_subsecs(6),
                completed.context,
            )
            .await?;
        #[cfg(feature = "auth-oidc")]
        let external_expiry = crate::auth::oidc::bind_external_login_credentials(
            transaction,
            &self.db,
            association.as_ref(),
            challenge,
            &credentials,
        )
        .await?;

        if let Some(audit) = &self.api_login_audit {
            audit.record_in_transaction(transaction, user_id).await?;
        }
        #[cfg(feature = "auth-oidc")]
        if external_expiry.is_some_and(|expiry| expiry <= Utc::now()) {
            return Err(Error::Unauthorized);
        }
        if completed.expires_at <= Utc::now() {
            return Err(Error::Unauthorized);
        }
        #[cfg(feature = "auth-oidc")]
        if external.as_ref().map(|(_, deadline)| *deadline) != external_expiry {
            return Err(Error::Unauthorized);
        }
        let check = LoginCommitCheck {
            user_id,
            issued: Some(IssuedCredentialCheck::capture(self, &credentials)?),
            challenge: Some(completed.check),
            #[cfg(feature = "auth-oidc")]
            external,
            ..Default::default()
        };
        Ok((Some(credentials), Some(check)))
    }

    #[cfg(feature = "auth-fortify")]
    async fn issue_context_in_transaction(
        &self,
        transaction: &DatabaseTransaction,
        user_id: i64,
        expires_at: chrono::DateTime<Utc>,
        context: super::credential::CredentialFactorContext,
    ) -> Result<AuthCredentials> {
        self.factor_authority
            .as_ref()
            .ok_or(Error::Unauthorized)?
            .check_context_in_transaction(transaction, user_id, &context)
            .await?;
        let issued = self
            .refresh
            .issue_context_in_transaction(transaction, user_id, expires_at, &context)
            .await?;
        let access_token = self.jwt.issue_access_token_with_grant(
            user_id,
            super::credential::JwtCredentialGrant {
                family_id: issued.family_id,
                context,
            },
        )?;
        if let Some(events) = self.db.events() {
            events
                .dispatch_in_transaction(transaction, &LoginSucceeded { user_id })
                .await?;
        }
        Ok(AuthCredentials {
            user_id,
            access_token,
            refresh_token: issued.token,
            refresh_expires_at: expires_at,
        })
    }

    /// 从当前真实持久Guard准备并签发JWT/refresh，不重新要求已经完成的因子。
    #[cfg(feature = "auth-fortify")]
    pub async fn issue_credentials_from_session(
        &self,
        guard: &dyn super::StatefulGuard,
        guard_name: &str,
    ) -> Result<AuthCredentials> {
        let authority = self.factor_authority.as_ref().ok_or(Error::Unauthorized)?;
        self.issue_credentials_from(authority.prepare_session(guard, guard_name).await?)
            .await
    }

    /// 消耗已核验的真实Session身份创建独立凭据家族；PAT或JWT不升级为全账户登录。
    #[cfg(feature = "auth-fortify")]
    pub async fn issue_credentials_from(
        &self,
        credential: super::VerifiedCredential,
    ) -> Result<AuthCredentials> {
        if !matches!(&credential.domain, super::CredentialDomain::Session { .. }) {
            return Err(Error::Unauthorized);
        }
        let authority = self.factor_authority.clone().ok_or(Error::Unauthorized)?;
        let this = self.clone();
        self.checked_login_transaction(move |tx| {
            Box::pin(async move {
                authority
                    .validate_parent_in_transaction(tx, &credential)
                    .await?;
                let context = credential
                    .context
                    .as_ref()
                    .ok_or(Error::Unauthorized)?
                    .clone();
                let user_id = credential.user_id;
                let expires_at = (Utc::now() + this.refresh_lifetime).trunc_subsecs(6);
                let issued = this
                    .issue_context_in_transaction(tx, user_id, expires_at, context)
                    .await?;
                let check = LoginCommitCheck {
                    user_id,
                    issued: Some(IssuedCredentialCheck::capture(&this, &issued)?),
                    session_parent: Some(credential),
                    ..Default::default()
                };
                Ok((issued, Some(check)))
            })
        })
        .await
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
        crate::auth::EncodedPasswordHash::make(self.passwords.as_ref(), &secret).await
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
        #[cfg(not(feature = "auth-fortify"))]
        let _ = request_source;
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

        #[cfg(feature = "auth-fortify")]
        if self.factor_authority.is_some() {
            return self
                .begin_factor_login_in_transaction(
                    transaction,
                    user_id,
                    request_source,
                    &format!("external:{provider}"),
                )
                .await;
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
        session.require_authenticated_storage()?;
        let user_id = self
            .authenticate_password(email, password, request_source)
            .await?;
        #[cfg(feature = "auth-fortify")]
        let context = if let Some(authority) = &self.factor_authority {
            let authority = authority.clone();
            Some(
                self.db
                    .transaction_for_update(move |transaction| {
                        Box::pin(async move {
                            authority
                                .context_without_factor_in_transaction(transaction, user_id)
                                .await
                        })
                    })
                    .await?,
            )
        } else {
            None
        };
        let credentials = bind_session(session, user_id).await?;
        #[cfg(feature = "auth-fortify")]
        if let Some(context) = context
            && let Err(error) =
                super::fortify::CredentialAuthority::store_session_context(session, context)
        {
            let _ = remove_auth_values(session);
            return Err(error);
        }
        Ok(credentials)
    }

    /// 核对 Guard 的真实账户来源与本流程的数据库账户域一致。
    pub fn require_account_provider(&self, guard: &dyn crate::auth::StatefulGuard) -> Result<()> {
        super::session::require_account_provider(&self.db, guard)
    }

    /// 现有已验证邮箱应用流程通过指定的具名 Guard 建立 Cookie 身份。
    ///
    /// 此适配保留既有邮箱、禁用账户及双因素门禁；通用账户 `Provider` 不要求邮箱。
    /// 路由应在同一请求先安装具名 Guard 上下文，后续写请求使用标准 CSRF middleware。
    pub async fn login_session_with_guard(
        &self,
        email: &str,
        password: &str,
        request_source: &str,
        session: &Session,
        guard: &dyn crate::auth::StatefulGuard,
    ) -> Result<SessionCredentials> {
        self.require_account_provider(guard)?;
        session.require_authenticated_storage()?;
        let user_id = self
            .authenticate_password(email, password, request_source)
            .await?;
        #[cfg(feature = "auth-fortify")]
        let context = if let Some(authority) = &self.factor_authority {
            let model = guard
                .provider()
                .retrieve_by_id(&serde_json::Value::from(user_id))
                .await?
                .ok_or(Error::Unauthorized)?;
            let selected = model
                .as_two_factor()
                .and_then(super::fortify::TwoFactorAuthenticatable::two_factor_authority)
                .ok_or(Error::Unauthorized)?;
            selected
                .validate_account(&model, guard.provider().as_ref())
                .await?;
            let authority = authority.clone();
            Some(
                self.db
                    .transaction_for_update(move |transaction| {
                        Box::pin(async move {
                            authority
                                .context_without_factor_in_transaction(transaction, user_id)
                                .await
                        })
                    })
                    .await?,
            )
        } else {
            None
        };
        // 旧显式 Session 凭据不与新的具名身份并存。
        remove_auth_values(session)?;
        guard
            .login_using_id(&serde_json::Value::from(user_id), false)
            .await?
            .ok_or(Error::Unauthorized)?;
        #[cfg(feature = "auth-fortify")]
        if let Some(context) = context {
            let authority = self.factor_authority.clone().ok_or(Error::Unauthorized)?;
            if let Err(error) = self
                .db
                .transaction_for_update(move |transaction| {
                    Box::pin(async move {
                        authority
                            .check_context_in_transaction(transaction, user_id, &context)
                            .await
                    })
                })
                .await
            {
                let _ = guard.logout().await;
                return Err(error);
            }
        }
        let csrf_token = session
            .token()?
            .and_then(|value| value.as_str().map(str::to_owned))
            .ok_or_else(|| {
                Error::custom(
                    "auth_csrf_token_missing",
                    "登录Guard未生成Session CSRF token",
                )
            })?;
        Ok(SessionCredentials {
            user_id,
            csrf_token,
        })
    }

    /// 注销指定具名 Guard 并轮换 Session ID/token，保留应用自己的偏好属性。
    pub async fn logout_session_with_guard(
        &self,
        request_source: &str,
        session: &Session,
        guard: &dyn crate::auth::StatefulGuard,
    ) -> Result<()> {
        self.require_account_provider(guard)?;
        session.require_authenticated_storage()?;
        let user_id = guard
            .id()
            .await?
            .and_then(|value| value.as_i64())
            .filter(|id| *id > 0)
            .ok_or(Error::Unauthorized)?;
        self.limit("logout_session", request_source, &user_id.to_string())
            .await?;
        guard.logout().await?;
        remove_auth_values(session)?;
        session.regenerate(true).await?;
        Ok(())
    }

    /// 从 Cookie Session 中移除已认证身份并轮换 Session ID。
    ///
    /// 其他由应用持有的 Session 值会保留。路由应先要求 `AuthMode::Session`，再调用此方法。
    pub async fn logout_session(&self, request_source: &str, session: &Session) -> Result<()> {
        session.require_authenticated_storage()?;
        let user_id = session
            .get_typed::<i64>(AUTH_USER_ID_KEY)
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
        self.authenticate_password_version(email, password, request_source)
            .await
            .map(|(user_id, _)| user_id)
    }

    async fn authenticate_password_version(
        &self,
        email: &str,
        password: &str,
        request_source: &str,
    ) -> Result<(i64, EncodedPasswordHash)> {
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

    async fn verify_password(
        &self,
        email: &str,
        password: &str,
    ) -> Result<(i64, EncodedPasswordHash)> {
        EncodedPasswordHash::validate_input(self.passwords.as_ref(), password)?;
        let connection = self.db.connection()?;
        let backend = connection.get_database_backend();
        let sql = match backend {
            DbBackend::Postgres => {
                "SELECT id, password_hash FROM tb_users WHERE email = $1 AND email_verified_at IS NOT NULL AND disabled_at IS NULL"
            }
            DbBackend::MySql | DbBackend::Sqlite => {
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
            self.perform_dummy_password_work().await?;
            return Err(Error::Unauthorized);
        };
        let user_id = row.try_get::<i64>("", "id").map_err(Error::internal)?;
        let encoded = row
            .try_get::<String>("", "password_hash")
            .map_err(Error::internal)?;
        let hash = EncodedPasswordHash::from_encoded(encoded);
        match hash.verify(self.passwords.as_ref(), password).await {
            Ok(true) => Ok((user_id, hash)),
            Ok(false) => Err(Error::Unauthorized),
            Err(error)
                if matches!(
                    error.code(),
                    "password_bcrypt_input_ambiguous" | "hashing_algorithm_mismatch"
                ) =>
            {
                // 与缺失账户采用相同的当前驱动工作，避免将旧存储策略暴露为账户存在信号。
                self.perform_dummy_password_work().await?;
                Err(Error::Unauthorized)
            }
            Err(error) => Err(error),
        }
    }

    async fn perform_dummy_password_work(&self) -> Result<()> {
        // make-only 字节上限不应暴露账户存在性；正上限至少允许一字节，0 与上游一样禁用限制。
        EncodedPasswordHash::make(self.passwords.as_ref(), "x").await?;
        Ok(())
    }

    /// 原子轮换 refresh 凭据并返回后继凭据对。
    pub async fn refresh(&self, token: &str, request_source: &str) -> Result<AuthCredentials> {
        self.refresh_in_scope(
            token,
            request_source,
            #[cfg(feature = "auth-fortify")]
            None,
        )
        .await
    }

    pub(crate) async fn refresh_in_scope(
        &self,
        token: &str,
        request_source: &str,
        #[cfg(feature = "auth-fortify")] scope: Option<&super::credential::CredentialGuardBinding>,
    ) -> Result<AuthCredentials> {
        self.limit("refresh", request_source, &digest(token))
            .await?;
        #[cfg(feature = "auth-fortify")]
        if let Some(authority) = &self.factor_authority {
            let authority = authority.clone();
            let this = self.clone();
            let token = token.to_owned();
            let scope = scope.cloned();
            return self
                .checked_login_transaction(move |transaction| {
                    Box::pin(async move {
                        let Some(locked) = this
                            .refresh
                            .lock_context_in_scope_in_transaction(
                                transaction,
                                &token,
                                scope.as_ref(),
                            )
                            .await?
                        else {
                            return Ok((None, None));
                        };
                        if locked.context().scope != scope {
                            return Err(Error::Unauthorized);
                        }
                        authority
                            .check_context_in_transaction(
                                transaction,
                                locked.user_id(),
                                locked.context(),
                            )
                            .await?;
                        let parent_family = locked.family_id().to_owned();
                        let parent_expiry = locked.expires_at();
                        let rotated = this.refresh.rotate_context_in_transaction(locked).await?;
                        if rotated.family_id != parent_family || rotated.expires_at != parent_expiry
                        {
                            return Err(Error::Unauthorized);
                        }
                        let access_token = this.jwt.issue_access_token_with_grant(
                            rotated.user_id,
                            super::credential::JwtCredentialGrant {
                                family_id: rotated.family_id,
                                context: rotated.context,
                            },
                        )?;
                        let credentials = AuthCredentials {
                            user_id: rotated.user_id,
                            access_token,
                            refresh_token: rotated.token,
                            refresh_expires_at: rotated.expires_at,
                        };
                        let check = LoginCommitCheck {
                            user_id: credentials.user_id,
                            issued: Some(IssuedCredentialCheck::capture(&this, &credentials)?),
                            ..Default::default()
                        };
                        Ok((Some(credentials), Some(check)))
                    })
                })
                .await?
                .ok_or(Error::Unauthorized);
        }
        let user_id = self.refresh.owner_id_for_rotation(token).await?;
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
        let password_hash =
            crate::auth::EncodedPasswordHash::make(self.passwords.as_ref(), new_password).await?;
        if !self
            .codes
            .verify(PASSWORD_RESET_PURPOSE, &email, code)
            .await?
        {
            return Ok(false);
        }
        let db = self.db.clone();
        self.db
            .transaction_for_update(|transaction| {
                let db = db.clone();
                let email = email.clone();
                let password_hash = password_hash.as_str().to_owned();
                Box::pin(async move {
                    let backend = transaction.get_database_backend();
                    let select = match backend {
                        DbBackend::Postgres => "SELECT id FROM tb_users WHERE email = $1 AND disabled_at IS NULL FOR UPDATE",
                        DbBackend::MySql => "SELECT id FROM tb_users WHERE email = ? AND disabled_at IS NULL FOR UPDATE",
                        DbBackend::Sqlite => "SELECT id FROM tb_users WHERE email = ? AND disabled_at IS NULL",
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
                        DbBackend::Sqlite => "UPDATE tb_users SET password_hash=?,updated_at=? WHERE id=? AND disabled_at IS NULL",
                        _ => return Err(unsupported_backend()),
                    };
                    let result = transaction.execute_raw(Statement::from_sql_and_values(
                        backend,
                        &crate::auth::resolve_auth_sql(&db, update)?,
                        if backend == DbBackend::Sqlite {
                            vec![password_hash.into(), crate::db::dialect::utc_value(backend, Utc::now()), user_id.into()]
                        } else { vec![password_hash.into(), user_id.into()] },
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
            DbBackend::MySql | DbBackend::Sqlite => {
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

    #[cfg(feature = "auth-fortify")]
    pub(crate) async fn limit_api(
        &self,
        endpoint: &str,
        source: &str,
        identity: &str,
    ) -> Result<()> {
        self.limit(endpoint, source, identity).await
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

/// 通过同一容器解析认证流程依赖；请求 scope 各自获取自己的依赖。
///
pub struct AuthFlowsServiceProvider {
    refresh_lifetime: ChronoDuration,
    code_policy: VerificationCodePolicy,
}

impl AuthFlowsServiceProvider {
    /// 创建默认 30 天 refresh lifetime 的流程 Provider。
    pub fn new() -> Self {
        Self {
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
    /// 设置验证码策略。
    #[must_use]
    pub fn with_code_policy(mut self, policy: VerificationCodePolicy) -> Self {
        self.code_policy = policy;
        self
    }
}

impl Default for AuthFlowsServiceProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::core::ServiceProvider for AuthFlowsServiceProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut crate::core::RegisterContext<'_>,
    ) -> crate::core::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let lifetime = self.refresh_lifetime;
            let policy = self.code_policy;
            if !(1..=31_536_000).contains(&lifetime.num_seconds()) {
                return Err(Error::Config(crate::config::ConfigError::Invalid(
                    "auth flow refresh lifetime must be between 1 second and 365 days".into(),
                )));
            }
            app.scoped::<AuthFlows, _>(move |resolver| {
                Box::pin(async move {
                    let db = resolver.resolve::<Db>().await?;
                    let passwords: std::sync::Arc<dyn crate::hashing::Hasher> =
                        resolver.resolve::<crate::hashing::HashManager>().await?;
                    let jwt = resolver.resolve::<JwtTokens>().await?;
                    let refresh = resolver.resolve::<RefreshTokens>().await?;
                    let codes = resolver.resolve::<VerificationCodes>().await?;
                    let limiter = resolver.resolve::<RateLimiter>().await?;
                    let flows = AuthFlows::new(
                        (*db).clone(),
                        passwords.clone(),
                        (*jwt).clone(),
                        (*refresh).clone(),
                        (*codes).clone(),
                        (*limiter).clone(),
                    )
                    .with_refresh_lifetime(lifetime)
                    .with_code_policy(policy);
                    #[cfg(feature = "auth-fortify")]
                    let flows = if resolver.is_bound::<super::fortify::RequireFactorAuthority>()? {
                        flows
                            .with_factor_authority(
                                (*resolver
                                    .resolve::<super::fortify::CredentialAuthority>()
                                    .await?)
                                    .clone(),
                            )?
                            .with_factor_actions(
                                (*resolver
                                    .resolve::<super::fortify::TwoFactorActions>()
                                    .await?)
                                    .clone(),
                            )
                    } else {
                        flows
                    };
                    #[cfg(feature = "auth-fortify")]
                    let flows = if resolver.is_bound::<dyn ApiLoginAuditSink>()? {
                        flows.with_api_login_audit(
                            resolver.resolve::<dyn ApiLoginAuditSink>().await?,
                        )?
                    } else {
                        flows
                    };
                    Ok(Arc::new(flows))
                })
            })
            .await?;
            app.require::<AuthFlows>();
            Ok(())
        })
    }

    fn boot<'a>(&'a self, app: &'a mut crate::core::BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let _ = app.resolve::<AuthFlows>().await?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod provider_tests {
    use super::*;
    use crate::core::ProviderBuilder;

    struct Dependencies(Arc<std::sync::atomic::AtomicUsize>);
    impl crate::core::ServiceProvider for Dependencies {
        fn register<'a>(
            &'a self,
            app: &'a mut crate::core::RegisterContext<'_>,
        ) -> crate::core::BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                use base64::Engine as _;
                let db = Db::default();
                let manager = Arc::new(crate::hashing::HashManager::new(
                    crate::hashing::HashingConfig {
                        bcrypt: crate::hashing::BcryptConfig {
                            rounds: 4,
                            verify: true,
                            limit: None,
                        },
                        ..Default::default()
                    },
                )?);
                app.instance(Arc::clone(&manager)).await?;
                let passwords: std::sync::Arc<dyn crate::hashing::Hasher> = manager;
                app.instance(Arc::new(db.clone())).await?;
                app.instance(Arc::new(RefreshTokens::new(db.clone())))
                    .await?;
                app.instance(Arc::new(VerificationCodes::new(
                    crate::auth::DatabaseVerificationCodeStore::new(db.clone()),
                    passwords,
                )))
                .await?;
                app.instance(Arc::new(RateLimiter::new(
                    crate::db::DatabaseRateLimitStore::new(db),
                )))
                .await?;
                let counter = Arc::clone(&self.0);
                app.scoped::<JwtTokens, _>(move |_| {
                    let index = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Box::pin(async move {
                        let secret = base64::engine::general_purpose::STANDARD
                            .encode([if index == 0 { 1 } else { 2 }; 32]);
                        let config = crate::config::JwtConfig::new(
                            "i",
                            "a",
                            "key",
                            std::collections::BTreeMap::from([("key".into(), secret)]),
                        );
                        Ok(Arc::new(JwtTokens::new(config)?))
                    })
                })
                .await?;
                Ok(())
            })
        }
    }

    #[tokio::test]
    async fn later_custom_dependencies_and_scoped_jwt_are_used() -> Result<()> {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut builder = ProviderBuilder::new();
        builder
            .provider(AuthFlowsServiceProvider::new())
            .provider(Dependencies(Arc::clone(&calls)));
        let runtime = builder.start(figment::Figment::new()).await?;
        let scope = runtime.container().scope()?;
        let flows = scope.resolve::<AuthFlows>().await?;
        let jwt = scope.resolve::<JwtTokens>().await?;
        assert_eq!(
            flows
                .jwt
                .verify_access_token(&jwt.issue_access_token(42)?)?,
            42
        );
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
        scope.close();
        runtime
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(1))
            .await
    }

    #[tokio::test]
    async fn provider_rejects_missing_dependencies_and_invalid_lifetime() {
        let mut missing = ProviderBuilder::new();
        missing.provider(AuthFlowsServiceProvider::new());
        assert!(missing.start(figment::Figment::new()).await.is_err());
        let mut invalid = ProviderBuilder::new();
        invalid.provider(
            AuthFlowsServiceProvider::new().with_refresh_lifetime(ChronoDuration::seconds(0)),
        );
        assert!(invalid.start(figment::Figment::new()).await.is_err());
    }
}

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
    session.require_authenticated_storage()?;
    let mut random = [0_u8; 32];
    getrandom::fill(&mut random).map_err(Error::internal)?;
    let csrf_token = URL_SAFE_NO_PAD.encode(random);
    let csrf_hash = digest(&csrf_token);

    session.migrate(true).await?;
    remove_auth_values(session)?;
    if let Err(error) = session.put_typed(AUTH_CSRF_HASH_KEY, csrf_hash) {
        let _ = remove_auth_values(session);
        return Err(Error::internal(error));
    }
    if let Err(error) = session.put_typed(AUTH_USER_ID_KEY, user_id) {
        let _ = remove_auth_values(session);
        return Err(Error::internal(error));
    }

    Ok(SessionCredentials {
        user_id,
        csrf_token,
    })
}

fn remove_auth_values(session: &Session) -> Result<()> {
    session.forget(&[
        AUTH_USER_ID_KEY,
        AUTH_CSRF_HASH_KEY,
        super::credential::SESSION_FACTOR_CONTEXT_KEY,
    ])?;
    Ok(())
}

async fn clear_session(session: &Session) -> Result<()> {
    remove_auth_values(session)?;
    session.migrate(true).await.map(|_| ())
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

// 注册INSERT的返回ID策略属于方言差异；事件及提交仍由同一调用方事务管理。
async fn insert_registered_user(
    transaction: &DatabaseTransaction,
    db: &Db,
    backend: DbBackend,
    sql: &str,
    email: &str,
    password_hash: &str,
) -> Result<Option<i64>> {
    let sql = crate::auth::resolve_auth_sql(db, sql)?;
    let inserted = if backend == DbBackend::Postgres {
        transaction
            .query_one_raw(Statement::from_sql_and_values(
                backend,
                &sql,
                [email.into(), password_hash.into()],
            ))
            .await
            .map_err(Error::internal)?
            .map(|row| row.try_get::<i64>("", "id").map_err(Error::internal))
            .transpose()?
    } else {
        let mut values = vec![email.into(), password_hash.into()];
        if backend == DbBackend::Sqlite {
            let now = Utc::now();
            values.push(crate::db::dialect::utc_value(backend, now));
            values.push(crate::db::dialect::utc_value(backend, now));
        }
        match transaction
            .execute_raw(Statement::from_sql_and_values(backend, &sql, values))
            .await
        {
            Ok(result) if result.rows_affected() == 1 => {
                Some(i64::try_from(result.last_insert_id()).map_err(Error::internal)?)
            }
            Ok(_) => None,
            Err(error)
                if backend == DbBackend::MySql
                    && matches!(error.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) =>
            {
                None
            }
            Err(error) => return Err(Error::internal(error)),
        }
    };
    Ok(inserted)
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

    use crate::session::{MemorySessionStore as MemoryStore, Session, SessionStore};

    use super::{AUTH_CSRF_HASH_KEY, AUTH_USER_ID_KEY, bind_session, clear_session, digest};

    #[tokio::test]
    async fn session_login_rotates_id_preserves_app_data_and_redacts_csrf()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let store = Arc::new(MemoryStore::default());
        let session = Session::new(None, store.clone(), None)?;
        session.start().await?;
        session.put_typed("app.preference", "compact")?;
        session.save().await?;
        let old_id = session.id()?;
        let credentials = bind_session(&session, 42).await?;
        let csrf_token = credentials.csrf_token().to_owned();
        assert_eq!(credentials.user_id(), 42);
        assert!(!format!("{credentials:?}").contains(&csrf_token));
        assert_ne!(session.id()?, old_id);
        session.save().await?;
        let new_id = session.id()?;
        assert_ne!(new_id, old_id);
        assert_eq!(session.get_typed::<i64>(AUTH_USER_ID_KEY)?, Some(42));
        let csrf_hash = session
            .get_typed::<String>(AUTH_CSRF_HASH_KEY)?
            .ok_or_else(|| crate::Error::internal(std::io::Error::other("CSRF hash missing")))?;
        assert_eq!(csrf_hash, digest(&csrf_token));
        assert_ne!(csrf_hash, csrf_token);
        assert_eq!(
            session.get_typed::<String>("app.preference")?,
            Some("compact".to_owned())
        );

        assert!(
            store
                .load(
                    &old_id,
                    crate::session::DEFAULT_MAX_BYTES,
                    session.expiry_date()?
                )
                .await
                .map_err(crate::Error::internal)?
                .is_none()
        );

        clear_session(&session).await?;
        assert_ne!(session.id()?, new_id);
        session.save().await?;
        assert!(session.get_typed::<i64>(AUTH_USER_ID_KEY)?.is_none());
        assert!(session.get_typed::<String>(AUTH_CSRF_HASH_KEY)?.is_none());
        assert_eq!(
            session.get_typed::<String>("app.preference")?,
            Some("compact".to_owned())
        );
        assert!(
            store
                .load(
                    &new_id,
                    crate::session::DEFAULT_MAX_BYTES,
                    session.expiry_date()?
                )
                .await
                .map_err(crate::Error::internal)?
                .is_none()
        );
        Ok(())
    }
}

#[cfg(all(test, feature = "db-sqlite"))]
mod exact_input_login_tests {
    use super::*;
    use crate::db::sea_orm_migration::{MigrationTrait, MigratorTrait};

    struct Migrations;
    impl MigratorTrait for Migrations {
        fn migrations() -> Vec<Box<dyn MigrationTrait>> {
            vec![
                Box::new(crate::auth::AuthMigration),
                Box::new(crate::auth::AuthRefreshTokenMigration),
                Box::new(crate::auth::AuthVerificationCodeMigration),
                Box::new(crate::db::DatabaseRateLimitMigration),
            ]
        }
    }

    #[tokio::test]
    async fn public_login_rejects_bcrypt_aliases_identically_for_present_and_missing_accounts()
    -> Result<()> {
        let database = crate::testing::TestDatabase::sqlite_memory::<Migrations>().await?;
        let outcome = async {
            let db = Db::default();
            db.initialize(database.connection()?)?;
            let hasher: Arc<dyn crate::hashing::Hasher> = Arc::new(
                crate::hashing::HashManager::new(crate::hashing::HashingConfig {
                    bcrypt: crate::hashing::BcryptConfig {
                        rounds: 4, verify: true, limit: None,
                    },
                    ..Default::default()
                })?,
            );
            let hash = EncodedPasswordHash::make(hasher.as_ref(), "safe-password").await?;
            let now = crate::db::dialect::utc_value(DbBackend::Sqlite, Utc::now());
            db.table("users")?.insert(vec![
                ("id", 42_i64.into()),
                ("email", "present@example.test".into()),
                ("password_hash", hash.as_str().into()),
                ("email_verified_at", now.clone()),
                ("created_at", now.clone()),
                ("updated_at", now),
            ]).await?;
            let jwt = JwtTokens::new(crate::config::JwtConfig::new(
                "issuer", "audience", "test",
                std::collections::BTreeMap::from([(
                    "test".into(),
                    "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=".into(),
                )]),
            ))?;
            let flows = AuthFlows::new(
                db.clone(), hasher.clone(), jwt, RefreshTokens::new(db.clone()),
                VerificationCodes::new(
                    crate::auth::DatabaseVerificationCodeStore::new(db.clone()), hasher,
                ),
                RateLimiter::new(crate::db::DatabaseRateLimitStore::new(db.clone())),
            );
            for email in ["present@example.test", "missing@example.test"] {
                for password in ["a\0b".to_owned(), "x".repeat(73)] {
                    let result = flows.login(email, &password, "exact-input-test").await;
                    assert!(matches!(result, Err(error) if error.code() == "password_bcrypt_input_ambiguous"));
                }
                assert!(matches!(
                    flows.login(email, "wrong-short-password", "exact-input-test").await,
                    Err(Error::Unauthorized)
                ));
            }
            assert_eq!(db.table("auth_refresh_tokens")?.count().await?, 0);
            Ok(())
        }.await;
        database.close().await?;
        outcome
    }

    #[tokio::test]
    async fn public_login_hides_mixed_bcrypt_storage_policy_under_strict_argon() -> Result<()> {
        let database = crate::testing::TestDatabase::sqlite_memory::<Migrations>().await?;
        let outcome = async {
            let db = Db::default();
            db.initialize(database.connection()?)?;
            let hasher: Arc<dyn crate::hashing::Hasher> = Arc::new(
                crate::hashing::HashManager::new(crate::hashing::HashingConfig {
                    bcrypt: crate::hashing::BcryptConfig {
                        rounds: 4,
                        verify: true,
                        limit: None,
                    },
                    ..Default::default()
                })?,
            );
            let hash = EncodedPasswordHash::make(hasher.as_ref(), "safe-password").await?;
            let now = crate::db::dialect::utc_value(DbBackend::Sqlite, Utc::now());
            db.table("users")?
                .insert(vec![
                    ("id", 42_i64.into()),
                    ("email", "present@example.test".into()),
                    ("password_hash", hash.as_str().into()),
                    ("email_verified_at", now.clone()),
                    ("created_at", now.clone()),
                    ("updated_at", now),
                ])
                .await?;
            let jwt = JwtTokens::new(crate::config::JwtConfig::new(
                "issuer",
                "audience",
                "test",
                std::collections::BTreeMap::from([(
                    "test".into(),
                    "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=".into(),
                )]),
            ))?;
            let hasher: Arc<dyn crate::hashing::Hasher> = Arc::new(
                crate::hashing::HashManager::new(crate::hashing::HashingConfig {
                    driver: "argon2id".into(),
                    argon: crate::hashing::ArgonConfig {
                        memory: 1024,
                        time: 2,
                        threads: 2,
                        verify: true,
                    },
                    ..Default::default()
                })?,
            );
            let flows = AuthFlows::new(
                db.clone(),
                hasher.clone(),
                jwt,
                RefreshTokens::new(db.clone()),
                VerificationCodes::new(
                    crate::auth::DatabaseVerificationCodeStore::new(db.clone()),
                    hasher,
                ),
                RateLimiter::new(crate::db::DatabaseRateLimitStore::new(db.clone())),
            );
            for email in ["present@example.test", "missing@example.test"] {
                for password in [
                    "a\0b".to_owned(),
                    "x".repeat(73),
                    "wrong-short-password".into(),
                ] {
                    assert!(matches!(
                        flows.login(email, &password, "mixed-policy-test").await,
                        Err(Error::Unauthorized)
                    ));
                }
            }
            assert_eq!(db.table("auth_refresh_tokens")?.count().await?, 0);
            Ok(())
        }
        .await;
        database.close().await?;
        outcome
    }
    #[tokio::test]
    async fn public_login_make_only_limit_does_not_expose_account_presence() -> Result<()> {
        let database = crate::testing::TestDatabase::sqlite_memory::<Migrations>().await?;
        let outcome = async {
            let db = Db::default();
            db.initialize(database.connection()?)?;
            let hasher: Arc<dyn crate::hashing::Hasher> = Arc::new(
                crate::hashing::HashManager::new(crate::hashing::HashingConfig {
                    bcrypt: crate::hashing::BcryptConfig {
                        rounds: 4,
                        verify: true,
                        limit: Some(8),
                    },
                    ..Default::default()
                })?,
            );
            let hash = EncodedPasswordHash::make(hasher.as_ref(), "safe").await?;
            let now = crate::db::dialect::utc_value(DbBackend::Sqlite, Utc::now());
            db.table("users")?
                .insert(vec![
                    ("id", 42_i64.into()),
                    ("email", "present@example.test".into()),
                    ("password_hash", hash.as_str().into()),
                    ("email_verified_at", now.clone()),
                    ("created_at", now.clone()),
                    ("updated_at", now),
                ])
                .await?;
            let jwt = JwtTokens::new(crate::config::JwtConfig::new(
                "issuer",
                "audience",
                "test",
                std::collections::BTreeMap::from([(
                    "test".into(),
                    "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=".into(),
                )]),
            ))?;
            let flows = AuthFlows::new(
                db.clone(),
                hasher.clone(),
                jwt,
                RefreshTokens::new(db.clone()),
                VerificationCodes::new(
                    crate::auth::DatabaseVerificationCodeStore::new(db.clone()),
                    hasher,
                ),
                RateLimiter::new(crate::db::DatabaseRateLimitStore::new(db.clone())),
            );
            for email in ["present@example.test", "missing@example.test"] {
                assert!(matches!(
                    flows.login(email, "123456789", "make-limit-test").await,
                    Err(Error::Unauthorized)
                ));
            }
            assert_eq!(db.table("auth_refresh_tokens")?.count().await?, 0);
            Ok(())
        }
        .await;
        database.close().await?;
        outcome
    }
}
