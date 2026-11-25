//! 显式安装的用户套件；不改变独立 Fortify 应用的响应与功能默认值。

#[cfg(feature = "sanctum")]
pub mod account;
#[cfg(feature = "sanctum")]
pub mod api;
#[cfg(feature = "sanctum")]
mod challenges;
#[cfg(feature = "sanctum")]
pub mod client;
#[cfg(feature = "sanctum")]
mod client_middleware;
mod configuration;
#[cfg(feature = "sanctum")]
pub mod confirmation;
mod mail_queue;
mod messages;
#[cfg(feature = "sanctum")]
mod token_web;
pub use mail_queue::configure_mail_queue;
mod password_tokens;
#[cfg(feature = "sanctum")]
pub(crate) use challenges::prune_for as prune_challenges_for;
pub use jiff::Timestamp as TokenTimestamp;
pub use password_tokens::bind_password_tokens;
#[cfg(feature = "sanctum")]
pub mod credentials;
#[cfg(feature = "sanctum")]
mod device_api;
#[cfg(feature = "sanctum")]
mod device_web;
#[cfg(feature = "sanctum")]
pub mod devices;
pub(crate) mod install;
mod responses;
#[cfg(feature = "sanctum")]
pub mod updates;
mod user_resource;
mod views;
#[cfg(feature = "sanctum")]
pub(crate) mod web;

pub use configuration::configure;
pub use user_resource::UserResource;
pub use views::register_views;

use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};

/// 放在 Fortify Provider 之后注册，为套件选择业务 JSON 响应。
/// 普通 HTML 请求继续使用固定 Fortify 的重定向、错误包和闪存。
pub struct StarterKitServiceProvider;
impl ServiceProvider for StarterKitServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            use crate::contracts;
            use std::sync::Arc;
            #[cfg(feature = "sanctum")]
            app.container()
                .singleton::<api::PasswordResetLinkController, _>(|_| {
                    Ok(Arc::new(api::PasswordResetLinkController))
                })?;
            macro_rules! response {
                ($name:ident) => {
                    app.container().bind::<dyn contracts::$name, _>(|_| {
                        Ok(Arc::new(responses::$name) as Arc<dyn contracts::$name>)
                    })?;
                };
            }
            response!(PasswordResetResponse);
            response!(SuccessfulPasswordResetLinkRequestResponse);
            response!(LoginResponse);
            response!(RegisterResponse);
            response!(ProfileInformationUpdatedResponse);
            response!(PasswordUpdateResponse);
            response!(PasswordConfirmedResponse);
            response!(EmailVerificationNotificationSentResponse);
            response!(TwoFactorLoginResponse);
            response!(TwoFactorEnabledResponse);
            response!(TwoFactorConfirmedResponse);
            response!(TwoFactorDisabledResponse);
            response!(RecoveryCodesGeneratedResponse);
            // 退出及邮箱验证保留原生204，错误继续走现有验证异常体系。
            Ok(())
        })
    }
}
