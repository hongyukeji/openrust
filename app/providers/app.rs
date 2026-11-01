//! 应用自身的 Provider；在 bootstrap 清单中显式登记。

use openrust::core::{BootContext, BoxFuture, ServiceProvider};
use openrust::prelude::*;

/// 根应用路由与异常呈现的启动钩子。
pub struct AppServiceProvider;

impl ServiceProvider for AppServiceProvider {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.exceptions(|handler| {
                handler.api_envelope_when(|request, _| {
                    request.path() == "/api" || request.path().starts_with("/api/")
                });
            })?;
            app.routes(crate::routes::register)?;
            Ok(())
        })
    }
}
