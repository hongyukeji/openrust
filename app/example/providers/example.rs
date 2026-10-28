//! 通过 `ServiceProvider` 装配本模块业务。

use openrust::Result;
use openrust::core::{BootContext, BoxFuture, RegisterContext, ServiceProvider};
use std::sync::Arc;

use crate::app::example::services::ExampleMessage;
/// 将综合示例路由接入 Provider 引导阶段。
pub(crate) struct ExampleServiceProvider;

impl ServiceProvider for ExampleServiceProvider {
    fn register(&self, app: &mut RegisterContext<'_>) -> Result<()> {
        app.instance(Arc::new(ExampleMessage::default()));
        Ok(())
    }

    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let config = app.section::<openrust::config::DatabaseConfig>("database")?;
            let database_enabled = config.url.is_some()
                || !config.instances.is_empty()
                || !config.read_replicas.is_empty();
            #[cfg(feature = "view")]
            let view_enabled = app.is_bound::<openrust::view::Views>();
            #[cfg(not(feature = "view"))]
            let view_enabled = false;
            app.routes(move |routes| {
                crate::app::example::routes::register(routes, database_enabled, view_enabled)
            })?;
            Ok(())
        })
    }
}
