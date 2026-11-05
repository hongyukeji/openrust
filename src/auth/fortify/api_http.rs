//! JWT API HTTP 入口；挑战和管理授权都不升级为资源凭据。

use super::{FortifyJwtApi, JwtApiFactorManagement};
use crate::http::axum::{
    Json,
    http::{HeaderValue, header},
    response::{IntoResponse, Response},
};
use crate::{
    Error, Result,
    auth::{AuthCredentials, Credentials, LoginAttempt},
    http::{Inject, Request, RouteGroup, Routes},
};
use serde_json::{Value, json};

pub(super) fn routes(prefix: String, guard: String) -> impl Fn(Routes) -> Routes + Send + Sync {
    move |routes| {
        routes.group(
            RouteGroup::new()
                .prefix(&prefix)
                .middleware(crate::http::axum::middleware::from_fn(no_store))
                .middleware_group("api"),
            |routes| {
                let routes = routes.group(RouteGroup::new(), |routes| {
                    routes
                        .get("/user", current_user)
                        .layer(crate::auth::AuthLayer::guard(guard.clone()))
                });
                routes
                    .post("/login", login)
                    .post("/two-factor-challenge", complete)
                    .post("/refresh", refresh)
                    .post("/logout", logout)
                    .post("/two-factor/authorize", prepare_management)
                    .post("/two-factor/restart", restart_pending)
                    .post("/two-factor", enable)
                    .post("/two-factor/confirm", confirm)
                    .delete("/two-factor", disable)
                    .get("/two-factor/secret", secret)
                    .get("/two-factor/qr-code", qr)
                    .get("/two-factor/recovery-codes", recovery_codes)
                    .post("/two-factor/recovery-codes", regenerate)
            },
        )
    }
}
async fn no_store(
    request: crate::http::axum::extract::Request,
    next: crate::http::axum::middleware::Next,
) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
        .headers_mut()
        .insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
    response
}
fn response(result: Result<Value>) -> Response {
    let mut response = match result {
        Ok(value) => Json(value).into_response(),
        Err(error) => error.into_response(),
    };
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
        .headers_mut()
        .insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
    response
}
fn text(request: &Request, key: &str) -> Result<String> {
    // 凭据只从请求体读取，不接收 URL 中的 token/password。
    request
        .deserialize_json::<Value>()?
        .get(key)
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty() && v.len() <= 16_384)
        .map(str::to_owned)
        .ok_or_else(|| Error::BadRequest(format!("{key} 必须为非空字符串")))
}
fn optional_text(request: &Request, key: &str) -> Result<Option<String>> {
    match request.deserialize_json::<Value>()?.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if value.len() <= 128 => Ok(Some(value.clone())),
        _ => Err(Error::BadRequest(format!("{key} 必须为字符串或null"))),
    }
}
fn credentials(value: &AuthCredentials) -> Value {
    json!({"user_id":value.user_id(),"access_token":value.access_token(),"token_type":"Bearer","refresh_token":value.refresh_token().expose(),"refresh_expires_at":value.refresh_expires_at()})
}
async fn login(Inject(api): Inject<FortifyJwtApi>, request: Request) -> Response {
    response(async {
        let mut values = request.deserialize_json::<Value>()?.as_object().cloned().ok_or_else(|| Error::BadRequest("登录请求必须为JSON对象".into()))?;
        let username = text(&request, &api.config.username)?;
        text(&request, "password")?;
        values.insert(api.config.username.clone(), Value::String(if api.config.lowercase_usernames { username.to_lowercase() } else { username }));
        match api.begin_login(request.info(), &Credentials::new(values)).await? {
            LoginAttempt::Authenticated(value) => Ok(credentials(&value)),
            LoginAttempt::TwoFactorRequired(challenge) => Ok(json!({"two_factor":true,"challenge":challenge.expose(),"expires_at":challenge.expires_at()})),
        }
    }.await)
}
async fn complete(Inject(api): Inject<FortifyJwtApi>, request: Request) -> Response {
    response(
        async {
            Ok(credentials(
                &api.complete_login(
                    request.info(),
                    &text(&request, "challenge")?,
                    optional_text(&request, "code")?.as_deref(),
                    optional_text(&request, "recovery_code")?.as_deref(),
                )
                .await?,
            ))
        }
        .await,
    )
}
async fn refresh(Inject(api): Inject<FortifyJwtApi>, request: Request) -> Response {
    response(
        async {
            Ok(credentials(
                &api.refresh(request.info(), &text(&request, "refresh_token")?)
                    .await?,
            ))
        }
        .await,
    )
}
async fn logout(Inject(api): Inject<FortifyJwtApi>, request: Request) -> Response {
    response(
        async {
            api.logout(request.info(), &text(&request, "refresh_token")?)
                .await?;
            Ok(json!({"logged_out":true}))
        }
        .await,
    )
}
async fn current_user(
    Inject(api): Inject<FortifyJwtApi>,
    Inject(manager): Inject<crate::auth::AuthManager>,
) -> Response {
    response(
        async {
            let guard = manager.guard(Some(&api.scope.guard)).await?;
            let user = guard.user().await?.ok_or(Error::Unauthorized)?;
            Ok(json!({"id":user.auth_identifier()?}))
        }
        .await,
    )
}

async fn prepare_management(
    Inject(api): Inject<FortifyJwtApi>,
    Inject(management): Inject<JwtApiFactorManagement>,
    request: Request,
) -> Response {
    response(
        async {
            let raw = super::api::bearer(request.info())?.ok_or(Error::Unauthorized)?;
            let (_, credential) = api.authenticate(raw).await?;
            api.flows
                .limit_api(
                    "jwt_api_factor_authorize",
                    &super::api::request_source(request.info())?,
                    &credential.user_id().to_string(),
                )
                .await?;
            let token = management
                .prepare(raw, &text(&request, "password")?)
                .await?;
            Ok(json!({"management_token":token}))
        }
        .await,
    )
}
async fn restart_pending(
    Inject(api): Inject<FortifyJwtApi>,
    Inject(management): Inject<JwtApiFactorManagement>,
    request: Request,
) -> Response {
    response(async {
        let raw = super::api::bearer(request.info())?.ok_or(Error::Unauthorized)?;
        let (_, credential) = api.authenticate(raw).await?;
        api.flows.limit_api("jwt_api_factor_restart", &super::api::request_source(request.info())?, &credential.user_id().to_string()).await?;
        Ok(json!({"management_token":management.restart_pending(raw, &text(&request, "password")?).await?}))
    }.await)
}
async fn management_actions(
    api: &FortifyJwtApi,
    management: &JwtApiFactorManagement,
    request: &Request,
) -> Result<(crate::auth::AuthUser, super::TwoFactorActions)> {
    let headers = request.info().headers();
    if headers.get_all("x-fortify-management").iter().count() != 1 {
        return Err(Error::Unauthorized);
    }
    let raw = request
        .info()
        .header("x-fortify-management")?
        .ok_or(Error::Unauthorized)?;
    api.flows
        .limit_api(
            &format!(
                "jwt_api_factor_management:{}:{}",
                request.info().method(),
                request.info().path()
            ),
            &super::api::request_source(request.info())?,
            raw,
        )
        .await?;
    management.actions(raw).await
}
async fn enable(
    Inject(api): Inject<FortifyJwtApi>,
    Inject(management): Inject<JwtApiFactorManagement>,
    request: Request,
) -> Response {
    response(async { let (user,actions)=management_actions(&api,&management,&request).await?; actions.enable(&user,false).await?; Ok(json!({"enabled":true,"confirmation_required":actions.confirms_two_factor_authentication()?})) }.await)
}
async fn confirm(
    Inject(api): Inject<FortifyJwtApi>,
    Inject(management): Inject<JwtApiFactorManagement>,
    request: Request,
) -> Response {
    response(
        async {
            let (user, actions) = management_actions(&api, &management, &request).await?;
            actions.confirm(&user, &text(&request, "code")?).await?;
            Ok(json!({"confirmed":true,"login_required":true}))
        }
        .await,
    )
}
async fn disable(
    Inject(api): Inject<FortifyJwtApi>,
    Inject(management): Inject<JwtApiFactorManagement>,
    request: Request,
) -> Response {
    response(
        async {
            let (user, actions) = management_actions(&api, &management, &request).await?;
            actions.disable(&user).await?;
            Ok(json!({"disabled":true,"login_required":true}))
        }
        .await,
    )
}
async fn secret(
    Inject(api): Inject<FortifyJwtApi>,
    Inject(management): Inject<JwtApiFactorManagement>,
    request: Request,
) -> Response {
    response(
        async {
            let (user, actions) = management_actions(&api, &management, &request).await?;
            Ok(json!({"secret_key":actions.secret_key(&user).await?.as_str()}))
        }
        .await,
    )
}
async fn qr(
    Inject(api): Inject<FortifyJwtApi>,
    Inject(management): Inject<JwtApiFactorManagement>,
    request: Request,
) -> Response {
    response(
        async {
            let (user, actions) = management_actions(&api, &management, &request).await?;
            let url = actions
                .qr_code_url(&user, &api.app_name, &api.config.username)
                .await?
                .ok_or(Error::Unauthorized)?;
            Ok(json!({"url":url.as_str(),"svg":super::two_factor_qr_code_svg(&url)?}))
        }
        .await,
    )
}
async fn recovery_codes(
    Inject(api): Inject<FortifyJwtApi>,
    Inject(management): Inject<JwtApiFactorManagement>,
    request: Request,
) -> Response {
    response(
        async {
            let (user, actions) = management_actions(&api, &management, &request).await?;
            Ok(json!({"recovery_codes":&*actions.recovery_codes(&user).await?}))
        }
        .await,
    )
}
async fn regenerate(
    Inject(api): Inject<FortifyJwtApi>,
    Inject(management): Inject<JwtApiFactorManagement>,
    request: Request,
) -> Response {
    response(async { let (user,actions)=management_actions(&api,&management,&request).await?; actions.generate_new_recovery_codes(&user).await?; Ok(json!({"recovery_codes":&*actions.recovery_codes(&user).await?,"login_required":true})) }.await)
}
