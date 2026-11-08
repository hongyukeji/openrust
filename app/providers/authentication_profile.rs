//! 根应用显式选择认证传输；缺省保留官方 Fortify Web 模式。
use openrust::Result;
use openrust::auth::fortify::{FortifyJwtApiServiceProvider, FortifyServiceProvider};
use openrust::foundation::BootContext;
use openrust::foundation::RegisterContext;
use openrust::support::{BoxFuture, ServiceProvider};
use std::sync::atomic::{AtomicBool, Ordering};

/// 按应用编译配置选择同一 Fortify 能力的官方 Web 或 JWT API 装配。
#[derive(Default)]
pub struct AuthenticationProfileProvider {
    jwt_api: AtomicBool,
}
impl ServiceProvider for AuthenticationProfileProvider {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let profile = app.config_repository().section::<String>("auth-profile")?;
            let jwt = selected(&profile)?;
            self.jwt_api.store(jwt, Ordering::Release);
            if jwt {
                FortifyJwtApiServiceProvider::new()
                    .with_database_audit()
                    .register(app)
                    .await
            } else {
                FortifyServiceProvider::new().register(app).await
            }
        })
    }
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let jwt = selected(&app.section::<String>("auth-profile")?)?;
            if jwt != self.jwt_api.load(Ordering::Acquire) {
                return Err(openrust::Error::custom(
                    "application_auth_profile_changed",
                    "认证profile在register后变化，请重新启动应用",
                ));
            }
            if jwt {
                FortifyJwtApiServiceProvider::new()
                    .with_database_audit()
                    .boot(app)
                    .await
            } else {
                FortifyServiceProvider::new().boot(app).await
            }
        })
    }
}
fn selected(profile: &str) -> Result<bool> {
    match profile {
        "web" => Ok(false),
        "jwt-api" => Ok(true),
        _ => Err(openrust::Error::custom(
            "application_auth_profile_invalid",
            "认证profile必须为web或jwt-api",
        )),
    }
}
