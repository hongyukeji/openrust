//! 通过 `ServiceProvider` 装配本模块业务。

use openrust::Result;
use openrust::core::{BootContext, BoxFuture, ModuleConfigFiles, RegisterContext, ServiceProvider};
use std::sync::Arc;

use crate::modules::example::services::{ExampleConfig, ExampleMessage};
/// 将综合示例路由接入 Provider 引导阶段。
pub(crate) struct ExampleServiceProvider {
    root: std::path::PathBuf,
}

impl ExampleServiceProvider {
    pub(crate) fn new(root: std::path::PathBuf) -> Self {
        Self { root }
    }
}

impl ServiceProvider for ExampleServiceProvider {
    fn register(&self, app: &mut RegisterContext<'_>) -> Result<()> {
        app.config_section(
            "example",
            ExampleConfig::default(),
            openrust::config::repository::SectionSchema::object([(
                "greeting",
                openrust::config::repository::SectionSchema::String,
            )]),
            |_| Ok(()),
        )?;
        app.singleton::<ExampleMessage, _>(|resolver| {
            Box::pin(async move {
                let config = resolver.section::<ExampleConfig>("example")?;
                Ok(Arc::new(ExampleMessage::new(config.greeting)))
            })
        });
        Ok(())
    }

    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let root = self.root.join("modules/example");
            let files = ModuleConfigFiles::new("example", root.clone())?;
            files.overlay(app)?;
            if app.is_bound::<openrust::i18n::I18n>() {
                let source = root.join("lang");
                let published = self.root.join("resources/lang/vendor/example");
                app.resolve::<openrust::i18n::I18n>()
                    .await?
                    .add_namespace("example", source, published)?;
            }
            #[cfg(feature = "view")]
            if app.is_bound::<openrust::view::Views>() {
                let source = files.resource("resources/views")?;
                let published = self.root.join("resources/views/vendor/example");
                app.resolve::<openrust::view::Views>()
                    .await?
                    .add_namespace("example", source, published)?;
            }
            let config = app.section::<openrust::config::DatabaseConfig>("database")?;
            let database_enabled = config.url.is_some()
                || !config.instances.is_empty()
                || !config.read_replicas.is_empty();
            #[cfg(feature = "view")]
            let view_enabled = app.is_bound::<openrust::view::Views>();
            #[cfg(not(feature = "view"))]
            let view_enabled = false;
            let i18n_enabled = app.is_bound::<openrust::i18n::I18n>();
            app.routes(move |routes| {
                crate::modules::example::routes::register(
                    routes,
                    database_enabled,
                    view_enabled,
                    i18n_enabled,
                )
            })?;
            Ok(())
        })
    }
}
