//! 应用自身的 Provider；在 bootstrap 清单中显式登记。

use openrust::core::{BootContext, BoxFuture, ServiceProvider};
use openrust::prelude::*;

/// 根应用路由清单与异常呈现的启动钩子。
pub struct AppServiceProvider;

impl ServiceProvider for AppServiceProvider {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let api: openrust::config::ApiConfig = app.section("api")?;
            let prefix = api.normalized_prefix()?;
            app.exceptions(move |handler| {
                handler.api_envelope_when(move |request, _| api.matches_path(request.path()));
            })?;
            app.routes(move |routes| crate::bootstrap::routes(routes, &prefix))?;
            Ok(())
        })
    }
}
