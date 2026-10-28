//! 使用现有公开 Plugin 接口装配本模块业务。

use openrust::http::AppBuilderHttpExt;
use openrust::{AppBuilder, Plugin, Result};

/// 仅装配本模块业务，复用根应用已经注册的框架服务。
pub(crate) struct ExamplePlugin;

impl Plugin for ExamplePlugin {
    fn build(&self, app: &mut AppBuilder) -> Result<()> {
        let database_enabled = app.get::<openrust::db::Db>().is_some();
        #[cfg(feature = "view")]
        let view_enabled = app.get::<openrust::view::Views>().is_some();
        #[cfg(not(feature = "view"))]
        let view_enabled = false;
        if database_enabled {
            #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
            app.plugin(openrust::database::MigrationPlugin::<
                crate::app::example::database::migrations::Migrator,
            >::new())
                .plugin(openrust::database::SeedPlugin::new(
                    crate::app::example::database::seeders::run,
                ));
            #[cfg(feature = "queue")]
            app.plugin(crate::app::example::jobs::JobsPlugin);
        }
        app.routes(move |routes| {
            crate::app::example::routes::register(routes, database_enabled, view_enabled)
        });
        Ok(())
    }
}
