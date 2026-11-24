//! 显式安装的用户套件；不改变独立 Fortify 应用的响应与功能默认值。

mod responses;
mod user_resource;

pub use user_resource::UserResource;

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
