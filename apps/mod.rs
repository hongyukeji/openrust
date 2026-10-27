//! 仓库根 `server` 宿主的显式应用模块注册表。
//!
//! 在这里声明 `apps/` 下的 Rust 模块，并在 [`Server::build`] 中注册其插件、路由及服务。
//! Rust 模块在编译期确定；框架不会扫描文件系统加载应用代码。

use openrust::http::HttpPlugin;
use openrust::prelude::*;

/// 仓库根目录的默认集成宿主。
pub(crate) struct Server;

impl Application for Server {
    type Config = NoConfig;

    fn build(app: &mut AppBuilder) {
        let app = app
            .plugin(HttpPlugin)
            .plugin(openrust::i18n::I18nPlugin::default())
            .plugin(openrust::mail::MailerPlugin)
            .plugin(openrust::sms::SmsPlugin);
        #[cfg(feature = "cache-redis")]
        let app = app.plugin(openrust::cache::CachePlugin);
        #[cfg(feature = "encryption")]
        let app = app.plugin(openrust::security::EncryptionPlugin);
        #[cfg(feature = "storage-local")]
        let app = app.plugin(openrust::storage::StoragePlugin);
        app.routes(openrust::routes::framework_routes);
    }
}
