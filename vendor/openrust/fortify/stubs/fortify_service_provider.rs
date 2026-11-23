use crate::app::{
    actions::fortify::{
        CreateNewUser, ResetUserPassword, UpdateUserPassword, UpdateUserProfileInformation,
    },
    models::User,
};
use openrust::{
    cache::rate_limiting::Limit,
    contracts::session::Session as _,
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider, facades::RateLimiter},
};
use openrust_fortify::{Fortify, LoginRateLimiter};
use std::sync::Arc;

/**
 * Fortify 应用服务的启动入口。
 *
 * 所有非延迟 Provider 完成 register() 后，按顺序执行 boot()。
 */
pub(crate) struct FortifyServiceProvider;

impl ServiceProvider for FortifyServiceProvider {
    /**
     * 配置用户操作、认证模型能力与登录限流规则。
     */
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let fortify = app.make::<Fortify>().await?;
            app.make::<openrust_fortify::passkeys::Passkeys>()
                .await?
                .useUserModel::<User>();
            fortify.registerTwoFactorAuthenticatable::<User>();
            fortify.registerEmailVerification::<User>();
            fortify.registerPasswordReset::<User>();
            fortify.createUsersUsing(Arc::new(CreateNewUser));
            fortify.updateUserProfileInformationUsing(Arc::new(UpdateUserProfileInformation));
            fortify.updateUserPasswordsUsing(Arc::new(UpdateUserPassword));
            fortify.resetUserPasswordsUsing(Arc::new(ResetUserPassword));
            let login = app.make::<LoginRateLimiter>().await?;
            RateLimiter::for_async("login", move |request| {
                let login = Arc::clone(&login);
                Box::pin(async move {
                    Ok(Limit::per_minute(5)
                        .by(login.throttleKey(request).await?)
                        .into())
                })
            })?;
            RateLimiter::for_async("two-factor", |request| {
                Box::pin(async move {
                    let key = request
                        .session()?
                        .get("login.id")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    let key = scalar(&key);
                    Ok(Limit::per_minute(5).by(key).into())
                })
            })?;
            RateLimiter::for_async("passkeys", |request| {
                Box::pin(async move {
                    let credential = request
                        .input(Some("credential.id"))
                        .await?
                        .unwrap_or(serde_json::Value::Null);
                    let credential = scalar(&credential);
                    let key = if credential.is_empty() || credential == "0" {
                        request.session()?.get_id().to_owned()
                    } else {
                        credential
                    };
                    Ok(Limit::per_minute(10)
                        .by(format!(
                            "{key}|{}",
                            request.ip().map(|ip| ip.to_string()).unwrap_or_default()
                        ))
                        .into())
                })
            })?;
            Ok(())
        })
    }
}
/**
 * 将限流键中的标量值转换为字符串，保持与 PHP 标量转换一致。
 */
fn scalar(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null | serde_json::Value::Bool(false) => String::new(),
        serde_json::Value::String(value) => value.clone(),
        serde_json::Value::Bool(true) => "1".into(),
        serde_json::Value::Number(value) => value.to_string(),
        _ => "Array".into(),
    }
}
