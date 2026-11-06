//! 凭据登录和当前 Session 的账户操作。含凭据的响应禁止缓存。

#[cfg(feature = "auth-fortify")]
use crate::modules::example::http::requests::account_security::ChallengeInput;
use crate::modules::example::http::requests::account_security::{
    CredentialsInput, Density, PreferenceInput,
};
use crate::modules::example::http::resources::credentials::CredentialsResponse;
use crate::modules::example::services::account_security::AccountSecurity;
use openrust::auth::{AuthCredentials, CurrentUser, LoginAttempt, Session};
use openrust::http::axum::{
    http::{HeaderMap, HeaderValue, header},
    response::{IntoResponse, Response},
};
use openrust::http::{ClientIp, Inject, Json, Validated};
use openrust::{Error, Result};

fn private<T: serde::Serialize>(value: T) -> Response {
    let mut response = Json(value).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
        .headers_mut()
        .insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
    response
}
fn credentials(value: &AuthCredentials) -> CredentialsResponse {
    CredentialsResponse {
        user_id: value.user_id(),
        token_type: "Bearer",
        access_token: value.access_token().into(),
        refresh_token: value.refresh_token().expose().into(),
        refresh_expires_at: value.refresh_expires_at().to_rfc3339(),
    }
}
#[derive(serde::Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum LoginResponse {
    Authenticated {
        credentials: CredentialsResponse,
    },
    #[cfg(feature = "auth-fortify")]
    TwoFactorRequired {
        challenge: String,
        expires_at: String,
    },
}
pub(crate) async fn login(
    Inject(service): Inject<AccountSecurity>,
    headers: HeaderMap,
    source: ClientIp,
    Validated(Json(input)): Validated<Json<CredentialsInput>>,
) -> Result<Response> {
    service.require_same_origin(&headers)?;
    let value = match service.login(input, &source.ip().to_string()).await? {
        LoginAttempt::Authenticated(value) => LoginResponse::Authenticated {
            credentials: credentials(&value),
        },
        #[cfg(feature = "auth-fortify")]
        LoginAttempt::TwoFactorRequired(challenge) => LoginResponse::TwoFactorRequired {
            challenge: challenge.expose().into(),
            expires_at: challenge.expires_at().to_rfc3339(),
        },
    };
    Ok(private(value))
}
pub(crate) async fn session_login(
    Inject(service): Inject<AccountSecurity>,
    session: Session,
    headers: HeaderMap,
    source: ClientIp,
    Validated(Json(input)): Validated<Json<CredentialsInput>>,
) -> Result<Response> {
    service.require_same_origin(&headers)?;
    let value = service
        .session_login(input, &source.ip().to_string(), &session)
        .await?;
    Ok(private(
        serde_json::json!({"user_id":value.user_id(),"csrf_token":value.csrf_token()}),
    ))
}
pub(crate) async fn session_logout(
    _user: CurrentUser,
    Inject(service): Inject<AccountSecurity>,
    session: Session,
    source: ClientIp,
) -> Result<Response> {
    service
        .session_logout(&source.ip().to_string(), &session)
        .await?;
    Ok(private(serde_json::json!({"status":"logged_out"})))
}
/// 该业务模块的整数账户域固定为同一数据库 users 表，不以其他 Provider 同值 ID 授权。
pub(crate) async fn require_session_account(
    user: CurrentUser,
    openrust::http::axum::Extension(scope): openrust::http::axum::Extension<
        std::sync::Arc<openrust::container::Scope>,
    >,
    request: openrust::http::axum::extract::Request,
    next: openrust::http::axum::middleware::Next,
) -> Result<Response> {
    let db = scope.resolve::<openrust::database::Db>().await?;
    let auth = scope.resolve::<openrust::auth::AuthManager>().await?;
    let selected = auth.guard(user.guard_name()).await?;
    let guard = selected.as_stateful().ok_or(Error::Unauthorized)?;
    if !guard.provider().uses_database_accounts(&db, "users")? {
        return Err(Error::Unauthorized);
    }
    Ok(next.run(request).await)
}

/// 受保护资源按当前账户策略校验 Session 的双因素证明。
#[cfg(feature = "auth-fortify")]
pub(crate) async fn require_session_factor(
    user: CurrentUser,
    openrust::http::axum::Extension(scope): openrust::http::axum::Extension<
        std::sync::Arc<openrust::container::Scope>,
    >,
    request: openrust::http::axum::extract::Request,
    next: openrust::http::axum::middleware::Next,
) -> Result<Response> {
    let name = user.guard_name().ok_or(Error::Unauthorized)?;
    let manager = scope.resolve::<openrust::auth::AuthManager>().await?;
    let guard = manager.guard(Some(name)).await?;
    let fortify = scope.resolve::<openrust::auth::fortify::Fortify>().await?;
    let confirm = fortify.confirms_two_factor_authentication()?;
    openrust::auth::fortify::authorize_session_factor(guard.as_ref(), name, confirm).await?;
    Ok(next.run(request).await)
}

const DENSITY_KEY: &str = "example.account.density";
pub(crate) async fn preference(_user: CurrentUser, session: Session) -> Result<Response> {
    let density = session
        .get_typed::<Density>(DENSITY_KEY)
        .map_err(Error::internal)?
        .unwrap_or_default();
    Ok(private(serde_json::json!({"density":density})))
}
pub(crate) async fn set_preference(
    _user: CurrentUser,
    session: Session,
    Json(input): Json<PreferenceInput>,
) -> Result<Response> {
    session
        .put_typed(DENSITY_KEY, input.density)
        .map_err(Error::internal)?;
    Ok(private(serde_json::json!({"density":input.density})))
}
#[cfg(feature = "auth-fortify")]
pub(crate) async fn complete(
    Inject(service): Inject<AccountSecurity>,
    headers: HeaderMap,
    source: ClientIp,
    Validated(Json(input)): Validated<Json<ChallengeInput>>,
) -> Result<Response> {
    service.require_same_origin(&headers)?;
    let value = service.complete(input, &source.ip().to_string()).await?;
    Ok(private(credentials(&value)))
}
/// 从同一已验证Session交换原始因子来源不变的JWT/refresh家族。
#[cfg(feature = "auth-fortify")]
pub(crate) async fn session_credentials(
    user: CurrentUser,
    Inject(service): Inject<AccountSecurity>,
    Inject(flows): Inject<openrust::auth::AuthFlows>,
    Inject(auth): Inject<openrust::auth::AuthManager>,
    headers: HeaderMap,
) -> Result<Response> {
    service.require_same_origin(&headers)?;
    let name = user.guard_name().ok_or(Error::Unauthorized)?;
    let selected = auth.guard(Some(name)).await?;
    let guard = selected.as_stateful().ok_or(Error::Unauthorized)?;
    let issued = flows.issue_credentials_from_session(guard, name).await?;
    Ok(private(credentials(&issued)))
}

/// 唯一示例只授予文章读取权限；框架不会把因子验证升级成任意业务授权。
#[cfg(feature = "auth-fortify")]
pub(crate) async fn session_read_token(
    user: CurrentUser,
    Inject(service): Inject<AccountSecurity>,
    Inject(tokens): Inject<openrust::auth::PersonalAccessTokens>,
    Inject(authority): Inject<openrust::auth::fortify::CredentialAuthority>,
    Inject(auth): Inject<openrust::auth::AuthManager>,
    headers: HeaderMap,
) -> Result<Response> {
    service.require_same_origin(&headers)?;
    let name = user.guard_name().ok_or(Error::Unauthorized)?;
    let selected = auth.guard(Some(name)).await?;
    let verified = authority
        .prepare_session(selected.as_stateful().ok_or(Error::Unauthorized)?, name)
        .await?;
    let maximum = chrono::Utc::now() + chrono::Duration::hours(24);
    let expires_at = verified
        .expires_at()
        .map_or(maximum, |parent| parent.min(maximum));
    let token = tokens
        .issue_from_verified(
            verified,
            "example-read",
            &["articles.read".into()],
            Some(expires_at),
        )
        .await?;
    Ok(private(
        serde_json::json!({"token":token.expose(),"abilities":["articles.read"]}),
    ))
}
