#![doc = include_str!("README.md")]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::{convert::Infallible, sync::Arc};

use crate::config::ConfigError;
use crate::core::Error;
use crate::core::Result;
use crate::database::{Db, DbBackend};
use crate::foundation::ApplicationState;
use crate::support::BoxFuture;
use axum::extract::{Extension, FromRequestParts, Request, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::middleware::{self as axum_middleware, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::Route;
use http::request::Parts;
use tower::{Layer, Service};

mod credential;
mod database;
pub use credential::{CredentialDomain, FactorOrigin, VerifiedCredential};
pub(crate) use database::AUTH_USER_ID_KEY;
#[cfg(feature = "auth-fortify")]
mod factor_gate;
mod flows;
#[cfg(feature = "auth-fortify")]
pub mod fortify;
#[cfg(feature = "auth-fortify")]
pub use fortify::Lockout;
mod guard;
pub(crate) use guard::{AuthRequestContext, install_request, session_user_id};
mod jwt;
pub(crate) mod middleware;
mod migration;
#[cfg(feature = "auth-fortify")]
mod totp;
pub use middleware::AuthenticationRedirects;
#[cfg(feature = "auth-oidc")]
mod oidc;
mod password;
mod pat;
mod policy;
mod query;
mod refresh;
mod session;
mod verification;

use database::DatabaseAuthenticator;

pub use crate::config::JwtConfig;
#[cfg(feature = "auth-fortify")]
pub use credential::TwoFactorLoginChallenge;
#[cfg(feature = "auth-fortify")]
pub use flows::ApiLoginAuditSink;
pub use flows::{
    AuthCredentials, AuthFlows, AuthFlowsServiceProvider, LoginAttempt, LoginFailed,
    LoginSucceeded, LoginValidating, RegistrationSucceeded, RegistrationValidating,
    SessionCredentials,
};
pub use guard::{Attempting, Authenticated, Failed, Login, Logout, SessionGuard, Validated};
pub use guard::{
    AuthDefaults, AuthDrivers, AuthManager, AuthUser, Authenticatable, Credentials,
    DatabaseUserProvider, FactorManagementContext, FactorReceipt, FactorVerificationMethod,
    GenericUser, Guard, GuardConfig, GuardContext, RememberTokenMutation, RememberTokenUpdate,
    SessionFactorProof, StatefulGuard, UserProvider, UserProviderConfig,
};
pub use jwt::{JwtServiceProvider, JwtTokens};
pub use migration::{
    AuthExternalIdentityMigration, AuthExternalSessionMigration, AuthMigration,
    AuthPolicyMigration, AuthRefreshTokenMigration, AuthRememberTokenMigration,
    AuthVerificationCodeMigration,
};
#[cfg(feature = "auth-oidc")]
pub use oidc::{
    BackchannelLogoutOutcome, ExternalIdentityAuthorization, ExternalIdentityFlows,
    ExternalIdentityLink, ExternalIdentityProvider, ExternalLoginHandle, ExternalLoginResult,
    ExternalLogoutRedirect, OidcAuthorization, OidcProvider, VerifiedExternalIdentity,
    VerifiedLogoutToken,
};
pub use policy::Policy;
pub use query::{
    AuthorizedSelect, MAX_AUTHORIZED_PAGE_SIZE, QueryRule, QueryScope, TenantQueryRule,
};
#[cfg(feature = "auth-fortify")]
pub use session::LiveSessionCredential;

pub(crate) fn resolve_auth_sql(db: &crate::database::Db, sql: &str) -> Result<String> {
    let backend = db.backend()?;
    let mut resolved = sql.to_owned();
    for logical in [
        "users",
        "sessions",
        "personal_access_tokens",
        "auth_refresh_tokens",
        "roles",
        "permissions",
        "user_roles",
        "role_permissions",
    ] {
        let physical = db.quoted_table_name(logical)?;
        resolved = resolved.replace(&format!("tb_{logical}"), &physical);
    }
    if !matches!(
        backend,
        crate::database::sea_orm::DbBackend::Postgres
            | crate::database::sea_orm::DbBackend::MySql
            | crate::database::sea_orm::DbBackend::Sqlite
    ) {
        return Err(Error::custom(
            "unsupported_auth_database",
            "authentication requires PostgreSQL, MySQL or SQLite",
        ));
    }
    Ok(resolved)
}
pub use crate::config::SessionConfig;
pub use crate::session::Session;
pub use password::EncodedPasswordHash;
pub use pat::{IssuedPersonalAccessToken, PersonalAccessTokenInfo, PersonalAccessTokens};
pub use refresh::{IssuedRefreshToken, RefreshRotation, RefreshTokens};
#[cfg(feature = "session-redis")]
pub use session::RedisSessionStore;
pub use session::{DatabaseSessionStore, SessionServiceProvider, SessionStoreHandle};
pub use verification::{
    DatabaseVerificationCodeStore, IssuedVerificationCode, VerificationChallenge,
    VerificationCodeFuture, VerificationCodePolicy, VerificationCodeStore, VerificationCodes,
};

/// 路由选择的认证来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthSource {
    /// 使用签名 JWT access token 进行认证。
    Jwt,
    /// 使用 Cookie Session 进行认证。
    Session,
    /// 使用个人访问令牌进行认证。
    PersonalAccessToken,
    /// 由具名 Guard 及其 Provider 确定的请求身份。
    Guard,
}

/// 认证实际JWT及其声明期限；已选择的策略还核主库因子与活动家族。
///
/// 实时握手等应用入口应复用本函数，不能把纯签名校验当作当前授权。
pub async fn authenticate_access_credential(
    state: &ApplicationState,
    value: &str,
) -> Result<(i64, std::time::SystemTime)> {
    authenticate_access_scoped(state, None, value).await
}

async fn authenticate_access_scoped(
    state: &ApplicationState,
    scope: Option<&Arc<crate::container::Scope>>,
    value: &str,
) -> Result<(i64, std::time::SystemTime)> {
    let tokens = resolve_request_service::<JwtTokens>(state, scope).await?;
    #[cfg(feature = "auth-fortify")]
    if state
        .container()
        .contains::<fortify::RequireFactorAuthority>()
        || tokens.verify_credential(value)?.grant.is_some()
    {
        let authority =
            resolve_request_service::<fortify::CredentialAuthority>(state, scope).await?;
        let credential = authority.authenticate_access_token(&tokens, value).await?;
        return Ok((
            credential.user_id(),
            credential.expires_at().ok_or(Error::Unauthorized)?.into(),
        ));
    }
    tokens.verify_access_token_with_expiry(value)
}

#[cfg(feature = "auth-fortify")]
async fn authenticate_selected_pat(
    state: &ApplicationState,
    scope: Option<&Arc<crate::container::Scope>>,
    value: &str,
) -> Result<AuthPrincipal> {
    let tokens = resolve_request_service::<PersonalAccessTokens>(state, scope).await?;
    let authority = resolve_request_service::<fortify::CredentialAuthority>(state, scope).await?;
    let tokens = (*tokens)
        .clone()
        .with_factor_authority((*authority).clone())?;
    let credential = tokens.authenticate_verified(value).await?;
    Ok(AuthPrincipal::new(
        credential.user_id(),
        Some(
            credential
                .token_abilities()
                .ok_or(Error::Unauthorized)?
                .to_vec(),
        ),
    ))
}

async fn authenticate_stored_credential(
    state: &ApplicationState,
    scope: Option<&Arc<crate::container::Scope>>,
    service: &AuthService,
    credential: AuthCredential<'_>,
) -> Result<AuthPrincipal> {
    #[cfg(feature = "auth-fortify")]
    let native = (credential.source() == AuthSource::PersonalAccessToken)
        .then(|| credential.value().map(str::to_owned))
        .flatten();
    #[cfg(not(feature = "auth-fortify"))]
    let _ = (state, scope);
    let result = service.authenticator.authenticate(credential).await;
    #[cfg(feature = "auth-fortify")]
    if matches!(&result, Err(error) if error.code() == "auth_managed_credential_validator_required")
    {
        return authenticate_selected_pat(
            state,
            scope,
            native.as_deref().ok_or(Error::Unauthorized)?,
        )
        .await;
    }
    result
}

/// 复用既有 JWT 与 Authenticator 校验 RPC Bearer；gRPC 不接受 Cookie Session。
#[cfg(feature = "grpc")]
pub(crate) async fn authenticate_grpc_bearer(
    state: &ApplicationState,
    value: &str,
) -> Result<CurrentUser> {
    if value.trim().is_empty() {
        return Err(Error::Unauthorized);
    }
    let (source, principal) = if value.starts_with("openrust_pat_") {
        let service = state
            .resolve::<AuthService>()
            .await
            .map_err(|_| Error::Unauthorized)?;
        let method = Method::POST;
        let credential = AuthCredential {
            source: AuthSource::PersonalAccessToken,
            value: Some(value),
            session: None,
            csrf_token: None,
            csrf_verified: false,
            method: &method,
        };
        #[cfg(feature = "auth-fortify")]
        let principal = if state
            .container()
            .contains::<fortify::RequireFactorAuthority>()
        {
            authenticate_selected_pat(state, None, value).await?
        } else {
            authenticate_stored_credential(state, None, service.as_ref(), credential).await?
        };
        #[cfg(not(feature = "auth-fortify"))]
        let principal =
            authenticate_stored_credential(state, None, service.as_ref(), credential).await?;
        (AuthSource::PersonalAccessToken, principal)
    } else {
        let (user_id, _) = authenticate_access_credential(state, value).await?;
        (AuthSource::Jwt, AuthPrincipal::new(user_id, None))
    };
    if principal.user_id <= 0 {
        return Err(Error::internal(InvalidPrincipal));
    }
    let token_abilities = match (source, principal.token_abilities) {
        (AuthSource::Jwt, None) => None,
        (AuthSource::PersonalAccessToken, Some(abilities)) => Some(Arc::from(abilities)),
        _ => return Err(Error::internal(InvalidPrincipal)),
    };
    Ok(CurrentUser {
        id: principal.user_id.into(),
        source,
        token_abilities,
        guard: None,
        provider: None,
    })
}

/// 单个路由的凭据选择策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    /// 仅接受 OpenRust JWT access token。
    Jwt,
    /// 仅接受 `openrust_session` Cookie。
    Session,
    /// 仅接受 `Authorization: Bearer` 个人访问令牌。
    PersonalAccessToken,
    /// 接受 Session Cookie 或 Bearer 个人访问令牌。
    SessionOrToken,
}

/// 提供给请求处理器的已验证身份。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentUser {
    id: serde_json::Value,
    source: AuthSource,
    token_abilities: Option<Arc<[String]>>,
    guard: Option<String>,
    provider: Option<String>,
}

impl CurrentUser {
    /// 返回已认证账户 ID。
    pub fn id(&self) -> serde_json::Value {
        self.id.clone()
    }

    /// 将显式采用整数主键的业务模型标识转换为i64；字符串/UUID不静默截断。
    pub fn integer_id(&self) -> Result<i64> {
        self.id.as_i64().filter(|id| *id > 0).ok_or_else(|| {
            Error::custom(
                "auth_integer_identifier_required",
                "当前业务模型需要正整数账户标识",
            )
        })
    }

    /// 返回当前具名Guard；旧显式凭据适配路径可能尚无具名归属。
    pub fn guard_name(&self) -> Option<&str> {
        self.guard.as_deref()
    }

    /// 返回当前具名账户Provider。
    pub fn provider_name(&self) -> Option<&str> {
        self.provider.as_deref()
    }

    pub(crate) fn rate_limit_key(&self) -> String {
        if self.guard.is_none() && self.provider.is_none() {
            format!("user:{}", self.id)
        } else {
            format!(
                "user:{}",
                serde_json::json!([self.guard, self.provider, self.id])
            )
        }
    }

    /// 返回此请求使用的凭据来源。
    pub const fn source(&self) -> AuthSource {
        self.source
    }

    /// 返回个人访问令牌的 abilities；Session 认证时返回 `None`。
    pub fn token_abilities(&self) -> Option<&[String]> {
        self.token_abilities.as_deref()
    }

    /// 返回此个人访问令牌是否包含完全匹配的指定 ability。
    pub fn has_ability(&self, ability: &str) -> bool {
        self.token_abilities()
            .is_some_and(|abilities| abilities.iter().any(|item| item == ability))
    }

    #[cfg(all(test, feature = "realtime"))]
    pub(crate) fn test_user(id: i64) -> Self {
        Self {
            id: id.into(),
            source: AuthSource::PersonalAccessToken,
            token_abilities: Some(Arc::from(Vec::<String>::new())),
            guard: None,
            provider: None,
        }
    }
}

/// 可选的已认证身份。`None` 表示所选凭据不存在。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionalUser(pub Option<CurrentUser>);

impl<S> FromRequestParts<S> for CurrentUser
where
    S: Send + Sync,
{
    type Rejection = Error;

    // Axum 将此 extractor trait 方法定义为 async，即使当前 extractor 只读取请求本地扩展。
    #[allow(clippy::unused_async_trait_impl)]
    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self> {
        match parts.extensions.get::<AuthOutcome>() {
            Some(AuthOutcome(Some(user))) => Ok(user.clone()),
            Some(AuthOutcome(None)) => Err(Error::Unauthorized),
            None => Err(Error::internal(MissingAuthLayer)),
        }
    }
}

impl<S> FromRequestParts<S> for OptionalUser
where
    S: Send + Sync,
{
    type Rejection = Error;

    // Axum 将此 extractor trait 方法定义为 async，即使当前 extractor 只读取请求本地扩展。
    #[allow(clippy::unused_async_trait_impl)]
    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self> {
        match parts.extensions.get::<AuthOutcome>() {
            Some(AuthOutcome(user)) => Ok(Self(user.clone())),
            None => Err(Error::internal(MissingAuthLayer)),
        }
    }
}

#[derive(Clone)]
struct AuthOutcome(Option<CurrentUser>);

/// 传递给 [`Authenticator`] 的已选请求凭据。
///
/// 该值包含原始 Session 或 bearer 密钥，因此有意不实现 `Debug` 或序列化。
pub struct AuthCredential<'a> {
    source: AuthSource,
    value: Option<&'a str>,
    session: Option<Session>,
    csrf_token: Option<&'a str>,
    csrf_verified: bool,
    method: &'a Method,
}

impl AuthCredential<'_> {
    /// 返回选中的凭据类型。
    pub const fn source(&self) -> AuthSource {
        self.source
    }

    /// 返回选中的个人访问令牌。Cookie Session ID 不会通过此方法暴露。
    pub const fn value(&self) -> Option<&str> {
        self.value
    }

    /// 如果此请求选择了 Cookie 认证，则返回共享 Session。
    pub const fn session(&self) -> Option<&Session> {
        self.session.as_ref()
    }

    /// 如果存在 session 凭据，则返回请求中的 CSRF header。
    pub const fn csrf_token(&self) -> Option<&str> {
        self.csrf_token
    }

    /// 当前请求是否已由默认 Web 中间件验证 Session CSRF token。
    pub const fn csrf_verified(&self) -> bool {
        self.csrf_verified
    }

    /// 返回与此凭据关联的 HTTP method。
    pub const fn method(&self) -> &Method {
        self.method
    }
}

/// [`Authenticator`] 返回的、凭据验证通过的账户身份。
pub struct AuthPrincipal {
    user_id: i64,
    token_abilities: Option<Vec<String>>,
}

impl AuthPrincipal {
    /// 在 authenticator 验证凭据与账户后创建 principal。
    ///
    /// Session 认证使用 `None`；个人访问令牌使用 `Some`。即使令牌没有受限 abilities，也会返回空向量。
    pub fn new(user_id: i64, token_abilities: Option<Vec<String>>) -> Self {
        Self {
            user_id,
            token_abilities,
        }
    }
}

/// 验证一个已选定的 Session 或个人访问令牌。
pub trait Authenticator: Send + Sync + 'static {
    /// 校验凭据并返回账户 ID 和 token abilities。
    fn authenticate<'a>(
        &'a self,
        credential: AuthCredential<'a>,
    ) -> BoxFuture<'a, Result<AuthPrincipal>>;
}

pub(crate) struct AuthService {
    authenticator: Arc<dyn Authenticator>,
}

/// 注册认证、密码与数据库授权服务的统一 Provider。
pub struct AuthServiceProvider {
    authenticator: Arc<dyn Authenticator>,
    database_authenticator: Option<Arc<DatabaseAuthenticator>>,
}

impl AuthServiceProvider {
    /// 创建默认的数据库认证 Provider。
    pub fn new() -> Self {
        let authenticator = Arc::new(DatabaseAuthenticator::default());
        Self {
            authenticator: authenticator.clone(),
            database_authenticator: Some(authenticator),
        }
    }

    /// 使用应用提供的认证器，不要求数据库认证器。
    pub fn with_authenticator(authenticator: impl Authenticator) -> Self {
        Self {
            authenticator: Arc::new(authenticator),
            database_authenticator: None,
        }
    }
}

impl Default for AuthServiceProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::support::ServiceProvider for AuthServiceProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut crate::foundation::RegisterContext<'_>,
    ) -> crate::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.register_provider(crate::hashing::HashServiceProvider::new())
                .await?;
            let authenticator = Arc::clone(&self.authenticator);
            let database_authenticator = self.database_authenticator.clone();
            app.singleton::<AuthService, _>(move |resolver| {
                let authenticator = Arc::clone(&authenticator);
                let database_authenticator = database_authenticator.clone();
                Box::pin(async move {
                    if let Some(database_authenticator) = database_authenticator {
                        let db = resolver.resolve::<Db>().await?;
                        if !matches!(
                            db.backend()?,
                            DbBackend::Postgres | DbBackend::MySql | DbBackend::Sqlite
                        ) {
                            return Err(Error::Config(ConfigError::Invalid(
                                "默认认证器支持PostgreSQL、MySQL与SQLite".into(),
                            )));
                        }
                        database_authenticator.initialize((*db).clone())?;
                    }
                    Ok(Arc::new(AuthService { authenticator }))
                })
            })
            .await?;
            app.singleton::<RefreshTokens, _>(|resolver| {
                Box::pin(async move {
                    let db = resolver.resolve::<Db>().await?;
                    Ok(Arc::new(RefreshTokens::new((*db).clone())))
                })
            })
            .await?;
            app.singleton::<PersonalAccessTokens, _>(|resolver| {
                Box::pin(async move {
                    let db = resolver.resolve::<Db>().await?;
                    let tokens = PersonalAccessTokens::new((*db).clone());
                    #[cfg(feature = "auth-fortify")]
                    let tokens = if resolver.is_bound::<fortify::RequireFactorAuthority>()? {
                        tokens.with_factor_authority(
                            (*resolver.resolve::<fortify::CredentialAuthority>().await?).clone(),
                        )?
                    } else {
                        tokens
                    };
                    Ok(Arc::new(tokens))
                })
            })
            .await?;
            app.singleton::<Policy, _>(|resolver| {
                Box::pin(async move {
                    let db = resolver.resolve::<Db>().await?;
                    Ok(Arc::new(Policy::new((*db).clone())))
                })
            })
            .await?;
            app.singleton::<VerificationCodes, _>(|resolver| {
                Box::pin(async move {
                    let db = resolver.resolve::<Db>().await?;
                    let passwords = resolver.resolve::<crate::hashing::HashManager>().await?;
                    Ok(Arc::new(VerificationCodes::new(
                        DatabaseVerificationCodeStore::new((*db).clone()),
                        passwords as Arc<dyn crate::hashing::Hasher>,
                    )))
                })
            })
            .await?;
            guard::register(app).await?;
            app.require::<AuthService>();
            app.require::<crate::hashing::HashManager>();
            Ok(())
        })
    }

    fn boot<'a>(
        &'a self,
        app: &'a mut crate::foundation::BootContext<'_>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let _ = app.resolve::<AuthService>().await?;
            let _ = app.resolve::<crate::hashing::HashManager>().await?;
            if self.database_authenticator.is_some() {
                let _ = app.resolve::<RefreshTokens>().await?;
                let _ = app.resolve::<PersonalAccessTokens>().await?;
                let _ = app.resolve::<Policy>().await?;
                let _ = app.resolve::<VerificationCodes>().await?;
            }
            Ok(())
        })
    }
}

/// 使用现有 Tower layer API 为路由应用必需或可选认证。
pub struct AuthLayer;

impl AuthLayer {
    /// 使用同一AuthManager的具名Guard；空名称使用当前默认Guard。
    pub fn guard(
        name: impl Into<String>,
    ) -> impl Layer<
        Route,
        Service: Service<
            Request,
            Response = Response,
            Error = Infallible,
            Future: Send + 'static,
        > + Clone
                     + Send
                     + Sync
                     + 'static,
    > + Clone
    + Send
    + Sync
    + 'static {
        axum_middleware::from_fn_with_state::<
            _,
            _,
            (
                State<NamedAuthLayerConfig>,
                Extension<ApplicationState>,
                Request,
            ),
        >(
            NamedAuthLayerConfig {
                name: name.into(),
                optional: false,
            },
            authenticate_named,
        )
    }

    /// 投影具名Guard的可选身份；Provider/存储失败仍返回错误，不当作访客。
    pub fn optional_guard(
        name: impl Into<String>,
    ) -> impl Layer<
        Route,
        Service: Service<
            Request,
            Response = Response,
            Error = Infallible,
            Future: Send + 'static,
        > + Clone
                     + Send
                     + Sync
                     + 'static,
    > + Clone
    + Send
    + Sync
    + 'static {
        axum_middleware::from_fn_with_state::<
            _,
            _,
            (
                State<NamedAuthLayerConfig>,
                Extension<ApplicationState>,
                Request,
            ),
        >(
            NamedAuthLayerConfig {
                name: name.into(),
                optional: true,
            },
            authenticate_named,
        )
    }

    /// 要求使用路由选定的凭据模式。
    pub fn required(
        mode: AuthMode,
    ) -> impl Layer<
        Route,
        Service: Service<
            Request,
            Response = Response,
            Error = Infallible,
            Future: Send + 'static,
        > + Clone
                     + Send
                     + Sync
                     + 'static,
    > + Clone
    + Send
    + Sync
    + 'static {
        axum_middleware::from_fn_with_state::<
            _,
            _,
            (State<AuthLayerConfig>, Extension<ApplicationState>, Request),
        >(
            AuthLayerConfig {
                mode,
                optional: false,
            },
            authenticate,
        )
    }

    /// 允许所选凭据缺失，但会拒绝格式错误或无效的凭据。
    pub fn optional(
        mode: AuthMode,
    ) -> impl Layer<
        Route,
        Service: Service<
            Request,
            Response = Response,
            Error = Infallible,
            Future: Send + 'static,
        > + Clone
                     + Send
                     + Sync
                     + 'static,
    > + Clone
    + Send
    + Sync
    + 'static {
        axum_middleware::from_fn_with_state::<
            _,
            _,
            (State<AuthLayerConfig>, Extension<ApplicationState>, Request),
        >(
            AuthLayerConfig {
                mode,
                optional: true,
            },
            authenticate,
        )
    }
}

#[derive(Clone)]
struct NamedAuthLayerConfig {
    name: String,
    optional: bool,
}

async fn authenticate_named(
    State(config): State<NamedAuthLayerConfig>,
    Extension(state): Extension<ApplicationState>,
    request: Request,
    next: Next,
) -> Response {
    match project_named_user(&config, &state, request).await {
        Ok(request) => next.run(request).await,
        Err(error) => error.into_response(),
    }
}

async fn prepare_named_request(
    parts: &mut axum::http::request::Parts,
    state: &ApplicationState,
) -> Result<(Arc<crate::container::Scope>, crate::http::RequestInfo)> {
    let scope = parts
        .extensions
        .get::<Arc<crate::container::Scope>>()
        .cloned()
        .ok_or_else(|| Error::custom("auth_request_scope_missing", "具名认证需要当前请求scope"))?;
    let info = crate::http::RequestInfo::from_request_parts(parts, state).await?;
    guard::install_request(
        &scope,
        guard::AuthRequestContext {
            cookies: info.cookies(),
            info: Arc::new(info.clone()),
            protection: parts
                .extensions
                .get::<crate::http::cookie::CookieProtection>()
                .cloned(),
        },
    )
    .await?;
    Ok((scope, info))
}

async fn project_named_user(
    config: &NamedAuthLayerConfig,
    state: &ApplicationState,
    request: Request,
) -> Result<Request> {
    let (mut parts, body) = request.into_parts();
    let (scope, _info) = prepare_named_request(&mut parts, state).await?;
    let manager = scope.resolve::<AuthManager>().await?;
    let name = if config.name.is_empty() {
        manager.default_driver()?
    } else {
        config.name.clone()
    };
    let selected = manager.guard(Some(&name)).await?;
    manager.should_use(name.clone())?;
    let user = match selected.user().await? {
        Some(account) => {
            let id = account.auth_identifier()?;
            if !match &id {
                serde_json::Value::Number(_) => true,
                serde_json::Value::String(value) => !value.is_empty(),
                _ => false,
            } {
                return Err(Error::internal(InvalidPrincipal));
            }
            let provider = selected.provider_name().map(str::to_owned);
            Some(CurrentUser {
                id,
                source: if selected.as_stateful().is_some() {
                    AuthSource::Session
                } else {
                    AuthSource::Guard
                },
                token_abilities: None,
                guard: Some(name),
                provider,
            })
        }
        None if config.optional => None,
        None => return Err(Error::Unauthorized),
    };
    parts.extensions.remove::<CurrentUser>();
    if let Some(user) = &user {
        parts.extensions.insert(user.clone());
    }
    parts.extensions.insert(AuthOutcome(user));
    Ok(Request::from_parts(parts, body))
}

#[derive(Clone, Copy)]
struct AuthLayerConfig {
    mode: AuthMode,
    optional: bool,
}

async fn resolve_request_service<T: Send + Sync + 'static>(
    state: &ApplicationState,
    scope: Option<&Arc<crate::container::Scope>>,
) -> Result<Arc<T>> {
    match scope {
        Some(scope) => scope.resolve::<T>().await,
        None => state.resolve::<T>().await,
    }
}

async fn configured_session_cookie_name(
    state: &ApplicationState,
    mode: AuthMode,
) -> Result<String> {
    if matches!(mode, AuthMode::Session | AuthMode::SessionOrToken)
        && state.container().contains::<session::SessionSettings>()
    {
        Ok(
            resolve_request_service::<session::SessionSettings>(state, None)
                .await?
                .cookie_name
                .clone(),
        )
    } else {
        Ok("openrust_session".to_owned())
    }
}

fn original_cookie_is_ambiguous(request: &Request, mode: AuthMode, name: &str) -> bool {
    // 解密后的唯一视图不能掩盖原始 wire 中重复的认证凭据。
    matches!(mode, AuthMode::Session | AuthMode::SessionOrToken)
        && request
            .extensions()
            .get::<crate::http::cookie::OriginalCookies>()
            .is_some_and(|original| {
                original
                    .root_names
                    .iter()
                    .filter(|root| root.as_str() == name)
                    .count()
                    > 1
            })
}

#[allow(
    clippy::too_many_lines,
    reason = "认证顺序在同一入口处理 token、session、撤销与请求身份扩展"
)]
async fn authenticate(
    State(config): State<AuthLayerConfig>,
    Extension(state): Extension<ApplicationState>,
    request: Request,
    next: Next,
) -> Response {
    let cookie_name = match configured_session_cookie_name(&state, config.mode).await {
        Ok(name) => name,
        Err(error) => return error.into_response(),
    };
    if original_cookie_is_ambiguous(&request, config.mode, &cookie_name) {
        return ambiguous_credentials().into_response();
    }
    let selected = match select_credential_named(
        config.mode,
        request.headers(),
        request.method(),
        &cookie_name,
        request
            .extensions()
            .get::<crate::http::forgery::CsrfVerified>()
            .is_some(),
    ) {
        Ok(selected) => selected,
        Err(error) => return error.into_response(),
    };
    let Some((source, value, csrf_token)) = selected else {
        if !config.optional {
            return Error::Unauthorized.into_response();
        }
        let mut request = request;
        request.extensions_mut().remove::<CurrentUser>();
        request.extensions_mut().insert(AuthOutcome(None));
        return next.run(request).await;
    };
    let scope = request.extensions().get::<Arc<crate::container::Scope>>();
    let service = match resolve_request_service::<AuthService>(&state, scope).await {
        Ok(service) => service,
        Err(error) => return error.into_response(),
    };
    let credential = AuthCredential {
        source,
        value: (source != AuthSource::Session).then_some(value),
        session: if source == AuthSource::Session {
            let Some(session) = request.extensions().get::<Session>().cloned() else {
                return Error::internal(MissingSessionLayer).into_response();
            };
            if let Err(error) = session.require_authenticated_storage() {
                return error.into_response();
            }
            if session.id().map(|id| id.to_string()).ok().as_deref() != Some(value) {
                return Error::Unauthorized.into_response();
            }
            Some(session)
        } else {
            None
        },
        csrf_token,
        csrf_verified: request
            .extensions()
            .get::<crate::http::forgery::CsrfVerified>()
            .is_some(),
        method: request.method(),
    };
    let principal = if source == AuthSource::Jwt {
        match authenticate_access_scoped(&state, scope, value).await {
            Ok((user_id, _)) => AuthPrincipal::new(user_id, None),
            Err(error) => return error.into_response(),
        }
    } else {
        #[cfg(feature = "auth-fortify")]
        if source == AuthSource::PersonalAccessToken
            && state
                .container()
                .contains::<fortify::RequireFactorAuthority>()
        {
            match authenticate_selected_pat(&state, scope, value).await {
                Ok(principal) => principal,
                Err(error) => return error.into_response(),
            }
        } else {
            match authenticate_stored_credential(&state, scope, service.as_ref(), credential).await
            {
                Ok(principal) => principal,
                Err(error) => return error.into_response(),
            }
        }
        #[cfg(not(feature = "auth-fortify"))]
        match authenticate_stored_credential(&state, scope, service.as_ref(), credential).await {
            Ok(principal) => principal,
            Err(error) => return error.into_response(),
        }
    };
    let token_abilities = match (source, principal.token_abilities) {
        (AuthSource::Jwt | AuthSource::Session, None) => None,
        (AuthSource::PersonalAccessToken, Some(abilities)) => Some(Arc::from(abilities)),
        _ => return Error::internal(InvalidPrincipal).into_response(),
    };
    if principal.user_id <= 0 {
        return Error::internal(InvalidPrincipal).into_response();
    }
    #[cfg(feature = "auth-fortify")]
    let unified_session = if source == AuthSource::Session {
        let Some(session) = request.extensions().get::<Session>() else {
            return Error::Unauthorized.into_response();
        };
        let selected = state
            .container()
            .contains::<fortify::RequireFactorAuthority>()
            || match session.get_json(credential::SESSION_FACTOR_CONTEXT_KEY) {
                Ok(value) => value.is_some(),
                Err(error) => return error.into_response(),
            };
        if selected {
            let authority = match resolve_request_service::<fortify::CredentialAuthority>(
                &state, scope,
            )
            .await
            {
                Ok(value) => value,
                Err(error) => return error.into_response(),
            };
            if let Err(error) = authority
                .authorize_session_context(principal.user_id, session)
                .await
            {
                return error.into_response();
            }
        }
        selected
    } else {
        false
    };
    #[cfg(feature = "auth-fortify")]
    let _ = unified_session;
    #[cfg(not(feature = "auth-fortify"))]
    if source == AuthSource::Session
        && let Some(session) = request.extensions().get::<Session>()
    {
        match session.get_json(credential::SESSION_FACTOR_CONTEXT_KEY) {
            Ok(None) => {}
            Ok(Some(_)) => return Error::Unauthorized.into_response(),
            Err(error) => return error.into_response(),
        }
    }
    let user = CurrentUser {
        id: principal.user_id.into(),
        source,
        token_abilities,
        guard: None,
        provider: None,
    };
    let mut request = request;
    request.extensions_mut().insert(user.clone());
    request.extensions_mut().insert(AuthOutcome(Some(user)));
    next.run(request).await
}

#[cfg(test)]
fn select_credential<'a>(
    mode: AuthMode,
    headers: &'a HeaderMap,
    method: &'a Method,
) -> Result<Option<(AuthSource, &'a str, Option<&'a str>)>> {
    select_credential_named(mode, headers, method, "openrust_session", false)
}

fn select_credential_named<'a>(
    mode: AuthMode,
    headers: &'a HeaderMap,
    method: &'a Method,
    cookie_name: &str,
    csrf_verified: bool,
) -> Result<Option<(AuthSource, &'a str, Option<&'a str>)>> {
    let session = if matches!(mode, AuthMode::Session | AuthMode::SessionOrToken) {
        session_cookie(headers, cookie_name)?
    } else {
        None
    };
    let selects_token = matches!(
        mode,
        AuthMode::Jwt | AuthMode::PersonalAccessToken | AuthMode::SessionOrToken
    );
    let authorization_present = selects_token
        && headers
            .get_all(header::AUTHORIZATION)
            .iter()
            .next()
            .is_some();
    if mode == AuthMode::SessionOrToken && session.is_some() && authorization_present {
        return Err(ambiguous_credentials());
    }
    let authorization = if selects_token {
        authorization_header(headers)?
    } else {
        None
    };
    if let Some(session) = session {
        if session.is_empty() {
            return Err(Error::Unauthorized);
        }
        let csrf = if is_safe_method(method) || csrf_verified {
            None
        } else {
            Some(
                csrf_header(headers)?
                    .ok_or_else(|| Error::Forbidden("missing or invalid CSRF token".into()))?,
            )
        };
        return Ok(Some((AuthSource::Session, session, csrf)));
    }
    if let Some(header) = authorization {
        let mut segments = header.split_ascii_whitespace();
        let scheme = segments.next().unwrap_or_default();
        let token = segments.next().unwrap_or_default();
        if !scheme.eq_ignore_ascii_case("bearer") || token.is_empty() || segments.next().is_some() {
            return Err(Error::Unauthorized);
        }
        let source = match mode {
            AuthMode::Jwt => AuthSource::Jwt,
            AuthMode::PersonalAccessToken | AuthMode::SessionOrToken => {
                AuthSource::PersonalAccessToken
            }
            AuthMode::Session => return Err(Error::Unauthorized),
        };
        return Ok(Some((source, token, None)));
    }
    Ok(None)
}

fn is_safe_method(method: &Method) -> bool {
    method == Method::GET
        || method == Method::HEAD
        || method == Method::OPTIONS
        || method == Method::TRACE
}

fn session_cookie<'a>(headers: &'a HeaderMap, cookie_name: &str) -> Result<Option<&'a str>> {
    let mut found = None;
    for value in headers.get_all(header::COOKIE) {
        let value = value.to_str().map_err(|_| Error::Unauthorized)?;
        for part in value.split(';') {
            let Some((name, cookie_value)) = part.trim().split_once('=') else {
                continue;
            };
            if name == cookie_name {
                if found.is_some() {
                    return Err(ambiguous_credentials());
                }
                found = Some(cookie_value.trim());
            }
        }
    }
    Ok(found)
}

fn authorization_header(headers: &HeaderMap) -> Result<Option<&str>> {
    let values = headers.get_all(header::AUTHORIZATION);
    let mut iter = values.iter();
    let Some(value) = iter.next() else {
        return Ok(None);
    };
    if iter.next().is_some() {
        return Err(ambiguous_credentials());
    }
    value.to_str().map(Some).map_err(|_| Error::Unauthorized)
}

fn csrf_header(headers: &HeaderMap) -> Result<Option<&str>> {
    let values = headers.get_all("x-csrf-token");
    let mut iter = values.iter();
    let Some(value) = iter.next() else {
        return Ok(None);
    };
    if iter.next().is_some() {
        return Err(Error::Forbidden("invalid CSRF token".into()));
    }
    value
        .to_str()
        .map(Some)
        .map_err(|_| Error::Forbidden("invalid CSRF token".into()))
}

fn ambiguous_credentials() -> Error {
    Error::custom_with_status(
        StatusCode::BAD_REQUEST,
        "ambiguous_credentials",
        "request contains ambiguous authentication credentials",
    )
}

#[derive(Debug)]
struct MissingAuthLayer;

impl std::fmt::Display for MissingAuthLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CurrentUser or OptionalUser requires an AuthLayer on the route")
    }
}

impl std::error::Error for MissingAuthLayer {}

#[derive(Debug)]
struct MissingSessionLayer;

impl std::fmt::Display for MissingSessionLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Cookie authentication requires the shared Session middleware")
    }
}

impl std::error::Error for MissingSessionLayer {}

#[derive(Debug)]
struct InvalidPrincipal;

impl std::fmt::Display for InvalidPrincipal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("authenticator returned principal metadata incompatible with the credential")
    }
}

impl std::error::Error for InvalidPrincipal {}

#[cfg(test)]
mod provider_tests {
    use super::*;
    use crate::foundation::BootContext;
    use crate::foundation::ProviderBuilder;
    use crate::support::ServiceProvider;
    use figment::Figment;
    use serde_json::json;

    #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
    struct RealCustomDbProvider {
        url: String,
        db: Arc<Db>,
    }

    #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
    impl ServiceProvider for RealCustomDbProvider {
        fn register<'a>(
            &'a self,
            app: &'a mut crate::foundation::RegisterContext<'_>,
        ) -> crate::support::BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                app.instance(Arc::clone(&self.db)).await?;
                Ok(())
            })
        }

        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                let pool = crate::database::sea_orm::Database::connect(self.url.as_str())
                    .await
                    .map_err(Error::internal)?;
                pool.ping().await.map_err(Error::internal)?;
                let closing = pool.clone();
                app.resources().on_shutdown(move || {
                    Box::pin(async move { closing.close_by_ref().await.map_err(Error::internal) })
                })?;
                self.db.initialize(pool)
            })
        }
    }

    #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
    #[tokio::test]
    async fn default_auth_boots_with_real_custom_database_provider() -> Result<()> {
        let Ok(url) = crate::test_database_environment::database_url() else {
            eprintln!("skipping real custom auth database: OPENRUST_TEST_DATABASE_URL is unset");
            return Ok(());
        };
        let db = Arc::new(Db::default());
        let mut providers = ProviderBuilder::new();
        providers
            .provider(RealCustomDbProvider {
                url,
                db: Arc::clone(&db),
            })
            .provider(AuthServiceProvider::new());
        let runtime = providers.start(Figment::new()).await?;
        assert!(runtime.container().resolve::<AuthService>().await.is_ok());
        let pool = db.connection()?;
        pool.ping().await.map_err(Error::internal)?;
        runtime
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
            .await?;
        assert!(
            pool.ping().await.is_err(),
            "custom pool must close at shutdown"
        );
        Ok(())
    }

    #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
    struct EarlyAuthResolve;

    #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
    impl ServiceProvider for EarlyAuthResolve {
        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                let _auth = app.resolve::<AuthService>().await?;
                let db = app.resolve::<Db>().await?;
                db.connection()?.ping().await.map_err(Error::internal)?;
                Ok(())
            })
        }
    }

    #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
    #[tokio::test]
    async fn early_boot_resolves_connected_database_and_initialized_auth() -> Result<()> {
        let Ok(url) = crate::test_database_environment::database_url() else {
            eprintln!("skipping early provider resolve: OPENRUST_TEST_DATABASE_URL is unset");
            return Ok(());
        };
        let mut providers = ProviderBuilder::new();
        providers
            .provider(EarlyAuthResolve)
            .provider(crate::database::DatabaseServiceProvider)
            .provider(AuthServiceProvider::new());
        let runtime = providers
            .start(Figment::from(figment::providers::Serialized::defaults(
                json!({"database": {"url": url}}),
            )))
            .await?;
        runtime
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
            .await
    }

    struct RejectAuthenticator;
    impl Authenticator for RejectAuthenticator {
        fn authenticate<'a>(
            &'a self,
            _: AuthCredential<'a>,
        ) -> BoxFuture<'a, Result<AuthPrincipal>> {
            Box::pin(async { Err(Error::Unauthorized) })
        }
    }
    struct InvalidPassword;
    impl ServiceProvider for InvalidPassword {
        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move { app.overlay("hashing.argon", json!({"memory": "invalid-type"})) })
        }
    }

    struct CustomDbProvider;
    impl ServiceProvider for CustomDbProvider {
        fn register<'a>(
            &'a self,
            app: &'a mut crate::foundation::RegisterContext<'_>,
        ) -> crate::support::BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                app.instance(Arc::new(Db::default())).await?;
                Ok(())
            })
        }
    }
    struct CleanupProbe(Arc<std::sync::atomic::AtomicBool>);
    impl ServiceProvider for CleanupProbe {
        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            let closed = Arc::clone(&self.0);
            Box::pin(async move {
                app.resources().on_shutdown(move || {
                    Box::pin(async move {
                        closed.store(true, std::sync::atomic::Ordering::SeqCst);
                        Ok(())
                    })
                })?;
                app.overlay("hashing.argon", json!({"memory":"invalid-type"}))
            })
        }
    }

    #[tokio::test]
    async fn custom_db_binding_is_accepted_and_uninitialized_db_fails_closed() {
        let mut builder = ProviderBuilder::new();
        builder
            .provider(CustomDbProvider)
            .provider(AuthServiceProvider::new());
        let result = builder.start(Figment::new()).await;
        assert!(result.is_err());
        let Err(error) = result else { return };
        assert_ne!(error.code(), "provider_dependency_missing");
        assert_ne!(error.code(), "service_not_bound");
    }

    #[tokio::test]
    async fn later_invalid_password_overlay_fails_and_cleans_resources() {
        let closed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut builder = ProviderBuilder::new();
        builder
            .provider(AuthServiceProvider::with_authenticator(RejectAuthenticator))
            .provider(CleanupProbe(Arc::clone(&closed)));
        assert!(builder.start(Figment::new()).await.is_err());
        assert!(closed.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[tokio::test]
    async fn jwt_middleware_and_inject_use_same_request_scope() -> Result<()> {
        use crate::foundation::http::build_router;
        use crate::http::HttpServiceProvider;
        use crate::http::Inject;
        use base64::Engine as _;
        use tower::ServiceExt;
        fn make_tokens(key: u8) -> Result<JwtTokens> {
            JwtTokens::new(JwtConfig::new(
                "issuer",
                "audience",
                "main",
                std::collections::BTreeMap::from([(
                    "main".into(),
                    base64::engine::general_purpose::STANDARD.encode([key; 32]),
                )]),
            ))
        }
        async fn handler(Inject(tokens): Inject<JwtTokens>) -> String {
            tokens.issue_access_token(42).unwrap_or_default()
        }
        struct ScopedTokens(Arc<std::sync::atomic::AtomicUsize>);
        impl ServiceProvider for ScopedTokens {
            fn register<'a>(
                &'a self,
                app: &'a mut crate::foundation::RegisterContext<'_>,
            ) -> crate::support::BoxFuture<'a, Result<()>> {
                Box::pin(async move {
                    app.instance(Arc::new(AuthService {
                        authenticator: Arc::new(RejectAuthenticator),
                    }))
                    .await?;
                    let counter = Arc::clone(&self.0);
                    app.scoped::<JwtTokens, _>(move |_| {
                        let index = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        Box::pin(async move {
                            Ok(Arc::new(make_tokens(if index == 0 { 1 } else { 2 })?))
                        })
                    })
                    .await?;
                    Ok(())
                })
            }
            fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
                Box::pin(async move {
                    app.routes(|routes| {
                        routes
                            .get("/scope-auth", handler)
                            .layer(AuthLayer::required(AuthMode::Jwt))
                    })?;
                    Ok(())
                })
            }
        }
        let token = make_tokens(2)?.issue_access_token(42)?;
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut builder = ProviderBuilder::new();
        builder
            .provider(HttpServiceProvider)
            .provider(ScopedTokens(Arc::clone(&calls)));
        let source = Figment::from(figment::providers::Serialized::defaults(
            crate::config::FrameworkConfig::default(),
        ));
        let app =
            crate::foundation::Application::from_provider_runtime(builder.start(source).await?)?;
        let _ = app.state().resolve::<JwtTokens>().await?;
        let response = build_router(&app)?
            .oneshot(
                Request::builder()
                    .uri("/scope-auth")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(axum::body::Body::empty())
                    .map_err(Error::internal)?,
            )
            .await
            .map_err(Error::internal)?;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .map_err(Error::internal)?;
        let issued = std::str::from_utf8(&body).map_err(Error::internal)?;
        assert_eq!(make_tokens(2)?.verify_access_token(issued)?, 42);
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
        Ok(())
    }

    #[tokio::test]
    async fn custom_auth_provider_shares_password_service_without_database() -> Result<()> {
        let mut builder = ProviderBuilder::new();
        builder.provider(AuthServiceProvider::with_authenticator(RejectAuthenticator));
        let runtime = builder.start(Figment::new()).await?;
        let first = runtime
            .container()
            .resolve::<crate::hashing::HashManager>()
            .await?;
        let second = runtime
            .container()
            .resolve::<crate::hashing::HashManager>()
            .await?;
        assert!(Arc::ptr_eq(&first, &second));
        runtime
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
            .await
    }

    #[tokio::test]
    async fn provider_rejects_missing_database_and_invalid_password_overlay() {
        let mut missing = ProviderBuilder::new();
        missing.provider(AuthServiceProvider::new());
        assert!(missing.start(Figment::new()).await.is_err());
        let mut invalid = ProviderBuilder::new();
        invalid
            .provider(InvalidPassword)
            .provider(AuthServiceProvider::with_authenticator(RejectAuthenticator));
        assert!(invalid.start(Figment::new()).await.is_err());
    }
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderValue, Method, header};

    use super::{AuthMode, AuthSource, select_credential, select_credential_named};

    fn code(error: crate::core::Error) -> String {
        match error {
            crate::core::Error::Custom { code, .. } => code,
            crate::core::Error::Unauthorized => "unauthorized".into(),
            crate::core::Error::Forbidden(_) => "forbidden".into(),
            other => format!("unexpected: {other}"),
        }
    }

    fn error_code<T>(result: crate::core::Result<T>) -> String {
        match result {
            Err(error) => code(error),
            Ok(_) => "unexpected_success".into(),
        }
    }

    #[test]
    fn route_mode_ignores_unselected_credentials_and_selects_one_source() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("openrust_session=session-secret"),
        );
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Basic ignored"),
        );

        let session = select_credential(AuthMode::Session, &headers, &Method::GET);
        assert!(matches!(
            session,
            Ok(Some((AuthSource::Session, "session-secret", _)))
        ));

        assert_eq!(
            error_code(select_credential(
                AuthMode::PersonalAccessToken,
                &headers,
                &Method::GET
            )),
            "unauthorized"
        );
    }

    #[test]
    fn optional_missing_credentials_are_distinct_from_malformed_credentials() {
        let headers = axum::http::HeaderMap::new();
        assert!(matches!(
            select_credential(AuthMode::SessionOrToken, &headers, &Method::GET),
            Ok(None)
        ));

        let mut headers = axum::http::HeaderMap::new();
        headers.insert(header::AUTHORIZATION, HeaderValue::from_static("Bearer   "));
        assert_eq!(
            error_code(select_credential(
                AuthMode::PersonalAccessToken,
                &headers,
                &Method::GET
            )),
            "unauthorized"
        );
    }

    #[test]
    fn combined_mode_rejects_any_two_sources_and_repeated_credentials() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("openrust_session=a"),
        );
        headers.insert(header::AUTHORIZATION, HeaderValue::from_static("malformed"));
        assert_eq!(
            error_code(select_credential(
                AuthMode::SessionOrToken,
                &headers,
                &Method::GET
            )),
            "ambiguous_credentials"
        );

        let mut headers = axum::http::HeaderMap::new();
        headers.append(header::AUTHORIZATION, HeaderValue::from_static("Bearer a"));
        headers.append(header::AUTHORIZATION, HeaderValue::from_static("Bearer b"));
        assert_eq!(
            error_code(select_credential(
                AuthMode::PersonalAccessToken,
                &headers,
                &Method::GET
            )),
            "ambiguous_credentials"
        );

        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("openrust_session=a; openrust_session=b"),
        );
        assert_eq!(
            error_code(select_credential(AuthMode::Session, &headers, &Method::GET)),
            "ambiguous_credentials"
        );
    }

    #[test]
    fn configured_cookie_name_is_selected_and_duplicates_are_rejected() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("openrust_session=other; custom_session=chosen"),
        );
        assert!(matches!(
            select_credential_named(
                AuthMode::Session,
                &headers,
                &Method::GET,
                "custom_session",
                false
            ),
            Ok(Some((AuthSource::Session, "chosen", None)))
        ));
        headers.append(
            header::COOKIE,
            HeaderValue::from_static("custom_session=duplicate"),
        );
        assert_eq!(
            error_code(select_credential_named(
                AuthMode::Session,
                &headers,
                &Method::GET,
                "custom_session",
                false,
            )),
            "ambiguous_credentials"
        );
    }

    #[test]
    fn unsafe_session_requests_require_one_csrf_header_before_authentication() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("openrust_session=session-secret"),
        );
        assert_eq!(
            error_code(select_credential(
                AuthMode::Session,
                &headers,
                &Method::POST
            )),
            "forbidden"
        );
        assert!(matches!(
            select_credential_named(
                AuthMode::Session,
                &headers,
                &Method::POST,
                "openrust_session",
                true,
            ),
            Ok(Some((AuthSource::Session, "session-secret", None)))
        ));
        headers.insert("x-csrf-token", HeaderValue::from_static("csrf-secret"));
        let selected = select_credential(AuthMode::Session, &headers, &Method::POST);
        assert!(matches!(
            selected,
            Ok(Some((AuthSource::Session, _, Some("csrf-secret"))))
        ));
    }
    #[test]
    fn named_user_rate_limit_keys_isolate_sources_and_preserve_identifier_types() {
        use super::{AuthSource, CurrentUser};
        use serde_json::{Value, json};
        let make = |id: Value, guard: Option<&str>, provider: Option<&str>| CurrentUser {
            id,
            source: AuthSource::Session,
            token_abilities: None,
            guard: guard.map(str::to_owned),
            provider: provider.map(str::to_owned),
        };
        let identities = [
            make(json!(42), Some("web"), Some("users")),
            make(json!(42), Some("admin"), Some("users")),
            make(json!(42), Some("web"), Some("admins")),
            make(json!("42"), Some("web"), Some("users")),
            make(json!(42), Some("web"), None),
            make(json!(42), None, Some("users")),
            make(json!(42), None, None),
            make(json!("42"), None, None),
            // 分隔符可出现在自定义来源中，不能用朴素字符串拼接导致边界碰撞。
            make(json!(42), Some("web|tenant"), Some("users")),
            make(json!(42), Some("web"), Some("tenant|users")),
        ];
        let keys = identities
            .iter()
            .map(CurrentUser::rate_limit_key)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            keys.len(),
            identities.len(),
            "相同ID不同来源或不同JSON类型共享了限流桶"
        );
        let same = make(json!(42), Some("web"), Some("users"));
        assert_eq!(
            same.rate_limit_key(),
            identities[0].rate_limit_key(),
            "同一来源身份没有稳定限流桶"
        );
    }
}
