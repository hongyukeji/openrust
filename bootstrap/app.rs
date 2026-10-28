//! 默认应用装配；后继内核阶段迁移为 `ServiceProvider`。

use openrust::http::HttpPlugin;
use openrust::prelude::*;

/// 应用使用既有配置源中的数据库区段决定是否装配数据库基础设施。
#[derive(Default, serde::Deserialize)]
#[serde(default)]
pub(crate) struct Configuration {
    database: openrust::config::DatabaseConfig,
    #[cfg(feature = "view")]
    view: Option<openrust::config::ViewConfig>,
}

impl Configuration {
    fn uses_database(&self) -> bool {
        self.database.url.is_some()
            || !self.database.instances.is_empty()
            || !self.database.read_replicas.is_empty()
    }
}

/// 框架与综合示例共用的默认应用宿主。
pub(crate) struct Server;

impl Application for Server {
    type Config = Configuration;

    fn build(app: &mut AppBuilder) {
        let database_enabled = app
            .get::<Configuration>()
            .is_some_and(|config| config.uses_database());
        #[cfg(feature = "view")]
        let view_enabled = app
            .get::<Configuration>()
            .is_some_and(|config| config.view.is_some());
        app.plugin(HttpPlugin)
            .plugin(openrust::i18n::I18nPlugin::default())
            .plugin(openrust::mail::MailerPlugin)
            .plugin(openrust::sms::SmsPlugin);
        #[cfg(feature = "cache-redis")]
        app.plugin(openrust::cache::CachePlugin);
        #[cfg(feature = "encryption")]
        app.plugin(openrust::security::EncryptionPlugin);
        #[cfg(feature = "storage-local")]
        app.plugin(openrust::storage::StoragePlugin);
        #[cfg(feature = "view")]
        if view_enabled {
            app.plugin(openrust::view::ViewPlugin);
        }
        if database_enabled {
            app.plugin(openrust::db::DatabasePlugin);
            #[cfg(feature = "auth")]
            app.plugin(openrust::auth::JwtPlugin::new())
                .plugin(openrust::auth::AuthPlugin::new())
                .plugin(openrust::audit::AuditPlugin::database())
                .plugin(openrust::auth::AuthFlowsPlugin::new());
            #[cfg(feature = "signing")]
            app.plugin(
                openrust::signing::SignedLinksPlugin::new().with_database_revocations_from_app(),
            );
        }
        crate::app::register(app);
        app.routes(crate::routes::register);
    }
}
