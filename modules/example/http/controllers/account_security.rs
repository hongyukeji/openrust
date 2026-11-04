//! JWT 管理与 Cookie Session 路由分别装配；秘密响应均禁止缓存。

#[cfg(feature = "auth-mfa")]
use crate::modules::example::http::requests::account_security::{ChallengeInput, FactorInput};
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
    #[cfg(feature = "auth-mfa")]
    MfaRequired {
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
        #[cfg(feature = "auth-mfa")]
        LoginAttempt::MfaRequired(challenge) => LoginResponse::MfaRequired {
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
        std::sync::Arc<openrust::core::container::Scope>,
    >,
    request: openrust::http::axum::extract::Request,
    next: openrust::http::axum::middleware::Next,
) -> Result<Response> {
    let db = scope.resolve::<openrust::db::Db>().await?;
    let auth = scope.resolve::<openrust::auth::AuthManager>().await?;
    let selected = auth.guard(user.guard_name()).await?;
    let guard = selected.as_stateful().ok_or(Error::Unauthorized)?;
    if !guard.provider().uses_database_accounts(&db, "users")? {
        return Err(Error::Unauthorized);
    }
    Ok(next.run(request).await)
}

/// 现有示例的增强策略：启用 MFA 后旧 Session 不得继续充当已完成第二因子的凭据。
#[cfg(feature = "auth-mfa")]
pub(crate) async fn require_session_factor(
    user: CurrentUser,
    openrust::http::axum::Extension(scope): openrust::http::axum::Extension<
        std::sync::Arc<openrust::core::container::Scope>,
    >,
    request: openrust::http::axum::extract::Request,
    next: openrust::http::axum::middleware::Next,
) -> Result<Response> {
    let flows = scope.resolve::<openrust::auth::AuthFlows>().await?;
    if flows.mfa_enabled(user.integer_id()?).await? {
        return Err(Error::Unauthorized);
    }
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
#[cfg(feature = "auth-mfa")]
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
#[cfg(feature = "auth-mfa")]
pub(crate) async fn enroll(
    user: CurrentUser,
    Inject(service): Inject<AccountSecurity>,
    source: ClientIp,
    Validated(Json(input)): Validated<Json<CredentialsInput>>,
) -> Result<Response> {
    let value = service
        .enroll(user.integer_id()?, input, &source.ip().to_string())
        .await?;
    Ok(private(
        serde_json::json!({"provisioning_uri":value.provisioning_uri(),"expires_at":value.expires_at().to_rfc3339()}),
    ))
}
#[cfg(feature = "auth-mfa")]
pub(crate) async fn confirm(
    user: CurrentUser,
    Inject(service): Inject<AccountSecurity>,
    source: ClientIp,
    Validated(Json(input)): Validated<Json<FactorInput>>,
) -> Result<Response> {
    let codes = service
        .confirm(user.integer_id()?, input, &source.ip().to_string())
        .await?;
    Ok(private(
        serde_json::json!({"recovery_codes":codes.expose()}),
    ))
}
#[cfg(feature = "auth-mfa")]
pub(crate) async fn regenerate(
    user: CurrentUser,
    Inject(service): Inject<AccountSecurity>,
    source: ClientIp,
    Validated(Json(input)): Validated<Json<FactorInput>>,
) -> Result<Response> {
    let codes = service
        .regenerate(user.integer_id()?, input, &source.ip().to_string())
        .await?;
    Ok(private(
        serde_json::json!({"recovery_codes":codes.expose()}),
    ))
}
#[cfg(feature = "auth-mfa")]
pub(crate) async fn disable(
    user: CurrentUser,
    Inject(service): Inject<AccountSecurity>,
    source: ClientIp,
    Validated(Json(input)): Validated<Json<FactorInput>>,
) -> Result<Response> {
    service
        .disable(user.integer_id()?, input, &source.ip().to_string())
        .await?;
    Ok(private(serde_json::json!({"status":"disabled"})))
}
