//! 应用自身的 Provider；由 bootstrap/providers.rs 登记。

use openrust::foundation::BootContext;
use openrust::support::BoxFuture;
use openrust::support::ServiceProvider;
use openrust::prelude::*;

/// 默认应用路由与异常呈现的启动钩子。
pub struct AppServiceProvider;

impl ServiceProvider for AppServiceProvider {
{{auth_register}}    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let api = app.section::<openrust::config::ApiConfig>("api")?;
            let prefix = api.normalized_prefix()?;
            app.exceptions(|handler| {
                handler.api_envelope_when(move |request, _| api.matches_path(request.path()));
            })?;
            app.routes(move |routes| crate::bootstrap::routes(routes, &prefix))?;
            Ok(())
        })
    }
}
