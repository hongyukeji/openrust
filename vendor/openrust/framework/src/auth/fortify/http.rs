//! 官方登录、密码确认及双因素HTTP入口；身份始终来自同一请求Guard。

use axum::{
    Json,
    http::{HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};

use super::{
    Fortify, FortifyConfig, FortifyLoginOutcome, TwoFactorOptions,
    responses::{
        ConfirmPasswordViewResponse, FailedPasswordConfirmationResponse,
        FailedTwoFactorLoginResponse, LoginResponse, LoginViewResponse, LogoutResponse,
        PasswordConfirmedResponse, RecoveryCodesGeneratedResponse, TwoFactorChallengeViewResponse,
        TwoFactorConfirmedResponse, TwoFactorDisabledResponse, TwoFactorEnabledResponse,
        TwoFactorLoginResponse,
    },
};
use crate::Error;
use crate::Result;
use crate::auth::AuthUser;
use crate::auth::Credentials;
use crate::core::error::ValidationErrors;
use crate::http::Inject;
use crate::http::Request;
use crate::routing::Redirector;
use crate::routing::RouteGroup;
use crate::routing::Router;

pub(super) fn route_builder(
    config: FortifyConfig,
    options: &TwoFactorOptions,
    require_authority: bool,
) -> Result<impl Fn(Router) -> Router + Send + Sync + use<>> {
    let enabled = config.two_factor_enabled()?;
    let confirm_password = options.confirm_password;
    let login = config.route_path("login", "/login")?;
    let logout = config.route_path("logout", "/logout")?;
    let confirm = config.route_path("password.confirm", "/user/confirm-password")?;
    let status = config.route_path("password.confirmation", "/user/confirmed-password-status")?;
    let challenge = config.route_path("two-factor.login", "/two-factor-challenge")?;
    let enable = config.route_path("two-factor.enable", "/user/two-factor-authentication")?;
    let disable = config.route_path("two-factor.disable", "/user/two-factor-authentication")?;
    let confirm_two_factor = config.route_path(
        "two-factor.confirm",
        "/user/confirmed-two-factor-authentication",
    )?;
    let qr = config.route_path("two-factor.qr-code", "/user/two-factor-qr-code")?;
    let secret = config.route_path("two-factor.secret-key", "/user/two-factor-secret-key")?;
    let recovery = config.route_path(
        "two-factor.recovery-codes",
        "/user/two-factor-recovery-codes",
    )?;
    Ok(move |routes: Router| {
        let mut group = RouteGroup::new().prefix(&config.prefix);
        if let Some(domain) = &config.domain {
            group = group.domain(domain);
        }
        for middleware in &config.middleware {
            group = group.middleware_alias(middleware);
        }
        routes.group(group, |routes| {
            let routes = routes.group(
                RouteGroup::new().middleware_alias(format!("guest:{}", config.guard)),
                |routes| {
                    let mut routes = routes.post(&login, login_store).name("login.store");
                    if config.views {
                        routes = routes.get(&login, login_view).name("login");
                    }
                    if enabled || require_authority {
                        routes = routes
                            .post(&challenge, challenge_store)
                            .name("two-factor.login.store");
                        if config.views {
                            routes = routes
                                .get(&challenge, challenge_view)
                                .name("two-factor.login");
                        }
                    }
                    routes
                },
            );
            routes.group(
                RouteGroup::new()
                    .middleware_alias(format!("{}:{}", config.auth_middleware, config.guard)),
                |routes| {
                    let mut routes = routes
                        .post(&logout, logout_store)
                        .name("logout")
                        .get(&status, password_status)
                        .name("password.confirmation")
                        .post(&confirm, password_confirm)
                        .name("password.confirm.store");
                    if config.views {
                        routes = routes.get(&confirm, password_view).name("password.confirm");
                    }
                    if enabled {
                        let mut management = RouteGroup::new();
                        if confirm_password {
                            management = management.middleware_alias("password.confirm");
                        }
                        routes = routes.group(management, |routes| {
                            routes
                                .post(&enable, enable_two_factor)
                                .name("two-factor.enable")
                                .post(&confirm_two_factor, confirm_factor)
                                .name("two-factor.confirm")
                                .delete(&disable, disable_two_factor)
                                .name("two-factor.disable")
                                .get(&qr, qr_code)
                                .name("two-factor.qr-code")
                                .get(&secret, secret_key)
                                .name("two-factor.secret-key")
                                .get(&recovery, recovery_codes)
                                .name("two-factor.recovery-codes")
                                .post(&recovery, regenerate_recovery_codes)
                                .name("two-factor.regenerate-recovery-codes")
                        });
                    }
                    routes
                },
            )
        })
    })
}

macro_rules! respond {
    ($fortify:expr, $request:expr, $contract:ident) => {
        $fortify
            .resolver
            .resolve::<dyn $contract>()
            .await?
            .to_response($request)
            .await
    };
}

async fn login_view(Inject(fortify): Inject<Fortify>, request: Request) -> Result<Response> {
    respond!(fortify, &request, LoginViewResponse)
}
async fn password_view(Inject(fortify): Inject<Fortify>, request: Request) -> Result<Response> {
    respond!(fortify, &request, ConfirmPasswordViewResponse)
}
async fn challenge_view(Inject(fortify): Inject<Fortify>, request: Request) -> Result<Response> {
    if fortify
        .login(&request)
        .await?
        .challenged_user()
        .await?
        .is_none()
    {
        return Ok(Redirector::from_request(&request)
            .route("login", &std::collections::BTreeMap::new())?
            .into_response());
    }
    respond!(fortify, &request, TwoFactorChallengeViewResponse)
}

fn string_input(request: &Request, key: &str, required: bool) -> Result<Option<String>> {
    match request.input(key)? {
        Some(Value::String(value)) if !required || !value.trim().is_empty() => Ok(Some(value)),
        None | Some(Value::Null) if !required => Ok(None),
        _ => {
            let mut errors = ValidationErrors::new();
            errors.add(
                key,
                if required {
                    format!("The {key} field is required and must be a string.")
                } else {
                    format!("The {key} field must be a string.")
                },
            );
            Err(Error::Validation(errors))
        }
    }
}

// 对应Request::boolean的FILTER_VALIDATE_BOOLEAN，不将任意非空字符串视为true。
fn boolean(value: Option<Value>) -> bool {
    match value {
        Some(Value::Bool(value)) => value,
        Some(Value::Number(value)) => value.as_f64() == Some(1.0),
        Some(Value::String(value)) => ["1", "true", "on", "yes"]
            .iter()
            .any(|word| value.trim().eq_ignore_ascii_case(word)),
        _ => false,
    }
}

async fn login_store(Inject(fortify): Inject<Fortify>, request: Request) -> Result<Response> {
    if fortify.config.limiters.login.is_some()
        && let Some(response) = super::throttle::before_login(&fortify, &request).await?
    {
        return Ok(response);
    }
    let username =
        string_input(&request, &fortify.config.username, true)?.ok_or(Error::Unauthorized)?;
    let password = string_input(&request, "password", true)?.ok_or(Error::Unauthorized)?;
    if fortify.config.limiters.login.is_none()
        && let Some(response) = super::throttle::before_login(&fortify, &request).await?
    {
        return Ok(response);
    }
    let mut attributes = request.input_all().as_object().cloned().unwrap_or_default();
    attributes.insert(
        fortify.config.username.clone(),
        Value::String(if fortify.config.lowercase_usernames {
            username.to_lowercase()
        } else {
            username
        }),
    );
    attributes.insert("password".into(), Value::String(password));
    let credentials = Credentials::new(attributes);
    match fortify
        .login(&request)
        .await?
        .begin(
            request.info(),
            &credentials,
            boolean(request.input("remember")?),
        )
        .await
    {
        Ok(FortifyLoginOutcome::Authenticated) => {
            super::throttle::login_succeeded(&fortify, &request).await?;
            respond!(fortify, &request, LoginResponse)
        }
        Ok(FortifyLoginOutcome::TwoFactorChallenge) => {
            if request.wants_json() {
                Ok(Json(json!({"two_factor":true})).into_response())
            } else {
                Ok(Redirector::from_request(&request)
                    .route("two-factor.login", &std::collections::BTreeMap::new())?
                    .into_response())
            }
        }
        Err(error @ (Error::Unauthorized | Error::Validation(_))) => {
            super::throttle::login_failed(&fortify, &request).await?;
            Err(error)
        }
        Err(error) => Err(error),
    }
}

async fn challenge_store(Inject(fortify): Inject<Fortify>, request: Request) -> Result<Response> {
    let code = string_input(&request, "code", false)?;
    let recovery = string_input(&request, "recovery_code", false)?;
    super::throttle::before_challenge(&fortify, &request).await?;
    match fortify
        .login(&request)
        .await?
        .complete(code.as_deref(), recovery.as_deref())
        .await
    {
        Ok(()) => respond!(fortify, &request, TwoFactorLoginResponse),
        Err(Error::Unauthorized | Error::Validation(_)) => {
            respond!(fortify, &request, FailedTwoFactorLoginResponse)
        }
        Err(error) => Err(error),
    }
}

async fn user(fortify: &Fortify) -> Result<AuthUser> {
    fortify.management().await.map(|(account, _)| account)
}

async fn logout_store(Inject(fortify): Inject<Fortify>, request: Request) -> Result<Response> {
    let guard = fortify.guard().await?;
    guard
        .as_stateful()
        .ok_or_else(|| {
            Error::custom_with_status(
                StatusCode::INTERNAL_SERVER_ERROR,
                "fortify_stateful_guard_required",
                "Fortify需要StatefulGuard",
            )
        })?
        .logout()
        .await?;
    let session = request.session()?;
    session.invalidate().await?;
    session.regenerate_token()?;
    respond!(fortify, &request, LogoutResponse)
}

async fn password_confirm(Inject(fortify): Inject<Fortify>, request: Request) -> Result<Response> {
    let account = user(&fortify).await?;
    if fortify
        .confirm_password(account, string_input(&request, "password", false)?)
        .await?
    {
        request
            .session()?
            .put_typed("auth.password_confirmed_at", chrono::Utc::now().timestamp())?;
        respond!(fortify, &request, PasswordConfirmedResponse)
    } else {
        respond!(fortify, &request, FailedPasswordConfirmationResponse)
    }
}

// PHP对秒数执行标量整数转换；保留负数和字符串数字前缀，不作正值夹取。
#[allow(
    clippy::cast_possible_truncation,
    reason = "对应PHP标量整数转换，浮点值按语言规则向零截断；保留既有边界向量"
)]
fn php_integer(value: &Value) -> i64 {
    match value {
        Value::Null => 0,
        Value::Bool(value) => i64::from(*value),
        Value::Number(value) => value
            .as_i64()
            .unwrap_or_else(|| value.as_f64().unwrap_or(0.0) as i64),
        Value::String(value) => {
            let value = value.trim_start();
            let numeric = value.trim_start_matches(['+', '-']);
            if !(numeric.as_bytes().first().is_some_and(u8::is_ascii_digit)
                || (numeric.starts_with('.')
                    && numeric.as_bytes().get(1).is_some_and(u8::is_ascii_digit)))
            {
                return 0;
            }
            let mut longest = None;
            for (end, _) in value
                .char_indices()
                .skip(1)
                .chain(std::iter::once((value.len(), ' ')))
            {
                if let Ok(number) = value[..end].parse::<f64>() {
                    longest = Some(number);
                }
            }
            longest.unwrap_or(0.0) as i64
        }
        Value::Array(values) => i64::from(!values.is_empty()),
        Value::Object(_) => 1,
    }
}

async fn password_status(Inject(fortify): Inject<Fortify>, request: Request) -> Result<Response> {
    let confirmed_at = request
        .session()?
        .get_json("auth.password_confirmed_at")?
        .as_ref()
        .map_or(0, php_integer);
    let elapsed = chrono::Utc::now().timestamp().saturating_sub(confirmed_at);
    let seconds = request.input("seconds")?.as_ref().map_or_else(
        || php_integer(&Value::from(fortify.password_timeout)),
        php_integer,
    );
    let confirmed = elapsed < seconds;
    let mut response = Json(json!({"confirmed":confirmed})).into_response();
    if confirmed && elapsed > 0 {
        response.headers_mut().insert(
            "X-Retry-After",
            HeaderValue::from_str(&elapsed.to_string()).map_err(Error::internal)?,
        );
    }
    Ok(response)
}

async fn enable_two_factor(Inject(fortify): Inject<Fortify>, request: Request) -> Result<Response> {
    let (account, actions) = fortify.management().await?;
    actions
        .enable(&account, boolean(request.input("force")?))
        .await?;
    if actions.confirmation_required()? {
        let state = super::actions::capability(&account)?
            .two_factor_state()
            .await?;
        if state.secret.is_some() && state.confirmed_at.is_none() {
            request.session()?.remove("two_factor_confirming_at")?;
        }
    }
    respond!(fortify, &request, TwoFactorEnabledResponse)
}
async fn confirm_factor(Inject(fortify): Inject<Fortify>, request: Request) -> Result<Response> {
    let (account, actions) = fortify.management().await?;
    // Action负责官方named error bag与验证码失败语义。
    let code = match string_input(&request, "code", false) {
        Ok(code) => code.unwrap_or_default(),
        Err(Error::Validation(errors)) => {
            return Err(Error::validation_in_bag(
                "confirmTwoFactorAuthentication",
                errors,
            ));
        }
        Err(error) => return Err(error),
    };
    let guard = fortify.guard().await?;
    let stateful = guard.as_stateful().ok_or(Error::Unauthorized)?;
    if let Some(receipt) = actions
        .confirm_with_receipt(
            &account,
            &code,
            &fortify.config.guard,
            guard.provider_name(),
            stateful.provider().as_ref(),
        )
        .await?
    {
        stateful.attest_factor_receipt(receipt).await?;
    }
    respond!(fortify, &request, TwoFactorConfirmedResponse)
}
async fn disable_two_factor(
    Inject(fortify): Inject<Fortify>,
    request: Request,
) -> Result<Response> {
    let (account, actions) = fortify.management().await?;
    actions.disable(&account).await?;
    respond!(fortify, &request, TwoFactorDisabledResponse)
}
async fn qr_code(Inject(fortify): Inject<Fortify>, _request: Request) -> Result<Response> {
    let (account, actions) = fortify.management().await?;
    match actions
        .qr_code_url(&account, &fortify.app_name, &fortify.config.username)
        .await?
    {
        Some(url) => Ok(
            Json(json!({"svg":super::two_factor_qr_code_svg(&url)?, "url":&*url})).into_response(),
        ),
        None => Ok(Json(Vec::<String>::new()).into_response()),
    }
}
async fn secret_key(Inject(fortify): Inject<Fortify>, _request: Request) -> Result<Response> {
    let (account, actions) = fortify.management().await?;
    let secret = actions.secret_key(&account).await?;
    Ok(Json(json!({"secretKey":&*secret})).into_response())
}
async fn recovery_codes(Inject(fortify): Inject<Fortify>, _request: Request) -> Result<Response> {
    let (account, actions) = fortify.management().await?;
    let codes = actions.recovery_codes(&account).await?;
    Ok(Json(&*codes).into_response())
}
async fn regenerate_recovery_codes(
    Inject(fortify): Inject<Fortify>,
    request: Request,
) -> Result<Response> {
    let (account, actions) = fortify.management().await?;
    actions.generate_new_recovery_codes(&account).await?;
    respond!(fortify, &request, RecoveryCodesGeneratedResponse)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod http_contract_tests;
