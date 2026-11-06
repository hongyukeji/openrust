//! 固定Fortify具名响应契约；默认直接使用HTTP JSON与同一重定向/Session机制。

use super::config::FortifyConfig;
use crate::Error;
use crate::Result;
use crate::core::error::ValidationErrors;
use crate::foundation::RegisterContext;
use crate::http::Request;
use crate::http::redirect_response::RedirectErrors;
use crate::routing::Redirector;
use crate::support::ArrayKey;
use crate::support::BoxFuture;
use crate::support::MessageInput;
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc};

macro_rules! response_contract {
    ($($name:ident),+ $(,)?) => {$(
        #[doc = concat!("Fortify 的可替换 ", stringify!($name), " 响应；不记录请求输入。")]
        pub trait $name: Send + Sync {
            /// 根据真实请求协商返回JSON或HTML响应。
            fn to_response<'a>(&'a self, request: &'a Request) -> BoxFuture<'a, Result<Response>>;
        }
    )+};
}
response_contract!(
    LoginResponse,
    LogoutResponse,
    TwoFactorLoginResponse,
    TwoFactorEnabledResponse,
    TwoFactorConfirmedResponse,
    TwoFactorDisabledResponse,
    RecoveryCodesGeneratedResponse,
    PasswordConfirmedResponse,
    FailedPasswordConfirmationResponse,
    FailedTwoFactorLoginResponse,
    LoginViewResponse,
    ConfirmPasswordViewResponse,
    TwoFactorChallengeViewResponse
);

/// 固定官方响应的默认实现；视图由应用绑定，不生成默认页面。
#[derive(Clone, Debug)]
pub struct DefaultFortifyResponses {
    config: FortifyConfig,
}
impl DefaultFortifyResponses {
    /// 使用同一配置仓储已经求值的Fortify配置。
    pub fn new(config: FortifyConfig) -> Self {
        Self { config }
    }
    fn intended(&self, request: &Request, key: &str) -> Result<Response> {
        Ok(Redirector::from_request(request)
            .intended_or(self.config.redirect_for(key, &self.config.home))?
            .into_response())
    }
    fn management(request: &Request, status: &str) -> Result<Response> {
        if request.wants_json() {
            return Ok(Json("").into_response());
        }
        request.session()?.flash_typed("status", status)?;
        Ok(Redirector::from_request(request)
            .back(None)?
            .into_response())
    }
}
impl LoginResponse for DefaultFortifyResponses {
    fn to_response<'a>(&'a self, request: &'a Request) -> BoxFuture<'a, Result<Response>> {
        Box::pin(async move {
            if request.wants_json() {
                Ok(Json(json!({"two_factor": false})).into_response())
            } else {
                self.intended(request, "login")
            }
        })
    }
}
impl LogoutResponse for DefaultFortifyResponses {
    fn to_response<'a>(&'a self, request: &'a Request) -> BoxFuture<'a, Result<Response>> {
        Box::pin(async move {
            if request.wants_json() {
                Ok(StatusCode::NO_CONTENT.into_response())
            } else {
                Ok(Redirector::from_request(request)
                    .to(self.config.redirect_for("logout", "/"))?
                    .into_response())
            }
        })
    }
}
impl TwoFactorLoginResponse for DefaultFortifyResponses {
    fn to_response<'a>(&'a self, request: &'a Request) -> BoxFuture<'a, Result<Response>> {
        Box::pin(async move {
            if request.wants_json() {
                Ok(StatusCode::NO_CONTENT.into_response())
            } else {
                self.intended(request, "login")
            }
        })
    }
}
impl PasswordConfirmedResponse for DefaultFortifyResponses {
    fn to_response<'a>(&'a self, request: &'a Request) -> BoxFuture<'a, Result<Response>> {
        Box::pin(async move {
            if request.wants_json() {
                Ok((StatusCode::CREATED, Json("")).into_response())
            } else {
                self.intended(request, "password-confirmation")
            }
        })
    }
}
macro_rules! management_response {
    ($($name:ident => $status:literal),+ $(,)?) => {$(
        impl $name for DefaultFortifyResponses {
            fn to_response<'a>(&'a self, request: &'a Request) -> BoxFuture<'a, Result<Response>> {
                Box::pin(async move { Self::management(request, $status) })
            }
        }
    )+};
}
management_response!(TwoFactorEnabledResponse => "two-factor-authentication-enabled",
    TwoFactorConfirmedResponse => "two-factor-authentication-confirmed",
    TwoFactorDisabledResponse => "two-factor-authentication-disabled",
    RecoveryCodesGeneratedResponse => "recovery-codes-generated");

fn validation(field: &str, message: &str) -> Error {
    let mut errors = ValidationErrors::new();
    errors.add(field, message);
    Error::Validation(errors)
}
fn failed_html(request: &Request, field: &str, message: &str, challenge: bool) -> Result<Response> {
    let redirector = Redirector::from_request(request);
    let mut response = if challenge {
        redirector.route("two-factor.login", &BTreeMap::new())?
    } else {
        redirector.back(None)?
    };
    response.with_errors(
        RedirectErrors::Entries(&[(ArrayKey::from(field), MessageInput::One(message.to_owned()))]),
        "default",
    )?;
    Ok(response.into_response())
}
impl FailedPasswordConfirmationResponse for DefaultFortifyResponses {
    fn to_response<'a>(&'a self, request: &'a Request) -> BoxFuture<'a, Result<Response>> {
        Box::pin(async move {
            let message = "The provided password was incorrect.";
            if request.wants_json() {
                Err(validation("password", message))
            } else {
                failed_html(request, "password", message, false)
            }
        })
    }
}
impl FailedTwoFactorLoginResponse for DefaultFortifyResponses {
    fn to_response<'a>(&'a self, request: &'a Request) -> BoxFuture<'a, Result<Response>> {
        Box::pin(async move {
            let (field, message) = if request.filled(&["recovery_code"]) {
                (
                    "recovery_code",
                    "The provided two factor recovery code was invalid.",
                )
            } else {
                (
                    "code",
                    "The provided two factor authentication code was invalid.",
                )
            };
            if request.wants_json() {
                Err(validation(field, message))
            } else {
                failed_html(request, field, message, true)
            }
        })
    }
}

/// 包内失败计数触线的可替换响应，不与具名路由Throttle响应混同。
pub trait LockoutResponse: Send + Sync {
    /// 默认返回带username字段错误的429校验异常。
    fn to_response<'a>(&'a self, request: &'a Request) -> BoxFuture<'a, Result<Response>>;
}
struct DefaultLockoutResponse {
    config: FortifyConfig,
    limiter: crate::core::RateLimiter,
}
impl LockoutResponse for DefaultLockoutResponse {
    fn to_response<'a>(&'a self, request: &'a Request) -> BoxFuture<'a, Result<Response>> {
        Box::pin(async move {
            let key = super::throttle::counter_key(
                "fortify.login",
                &super::throttle::login_key(&self.config, request)?,
            );
            let seconds = self.limiter.available_in(&key).await?;
            let mut errors = ValidationErrors::new();
            errors.add(
                &self.config.username,
                format!("Too many login attempts. Please try again in {seconds} seconds."),
            );
            Err(Error::validation_with_status(
                StatusCode::TOO_MANY_REQUESTS,
                errors,
            ))
        })
    }
}

/// 仅登记未覆写的默认响应，使用与Guard相同的请求scope；视图保持应用绑定。
pub(crate) async fn register(app: &mut RegisterContext<'_>) -> Result<()> {
    macro_rules! bind {
        ($($name:ident),+ $(,)?) => {$(
            if !app.is_bound::<dyn $name>() {
                app.scoped::<dyn $name, _>(|resolver| Box::pin(async move {
                    let config = resolver.section::<FortifyConfig>("fortify")?;
                    Ok(Arc::new(DefaultFortifyResponses::new(config)) as Arc<dyn $name>)
                })).await?;
            }
        )+};
    }
    bind!(
        LoginResponse,
        LogoutResponse,
        TwoFactorLoginResponse,
        TwoFactorEnabledResponse,
        TwoFactorConfirmedResponse,
        TwoFactorDisabledResponse,
        RecoveryCodesGeneratedResponse,
        PasswordConfirmedResponse,
        FailedPasswordConfirmationResponse,
        FailedTwoFactorLoginResponse
    );
    if !app.is_bound::<dyn LockoutResponse>() {
        app.scoped::<dyn LockoutResponse, _>(|resolver| {
            Box::pin(async move {
                let config = resolver.section::<FortifyConfig>("fortify")?;
                let limiter = resolver.resolve::<crate::core::RateLimiter>().await?;
                Ok(Arc::new(DefaultLockoutResponse {
                    config,
                    limiter: (*limiter).clone(),
                }) as Arc<dyn LockoutResponse>)
            })
        })
        .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
