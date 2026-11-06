//! 应用代码配置清单；按声明顺序单次求值，每个区段可读取已完成依赖。

pub mod api;
pub mod app;
pub mod auth;
pub mod cache;
pub mod database;
#[cfg(feature = "auth-fortify")]
pub mod fortify;
#[cfg(feature = "grpc")]
pub mod grpc;
pub mod hashing;
pub mod i18n;
pub mod log;
pub mod mail;
pub mod modules;
#[cfg(feature = "queue")]
pub mod queue;
#[cfg(feature = "realtime")]
pub mod realtime;
pub mod security;
pub mod server;
pub mod session;
pub mod sms;
pub mod storage;
pub mod translation_sources;
#[cfg(feature = "view")]
pub mod view;

/// 登记默认配置；显式文件、环境及前置覆写通过同一仓储参与依赖读取。
pub fn boot() -> openrust::config::ConfigManifest {
    let manifest = openrust::config::ConfigManifest::new();
    #[cfg(feature = "auth-fortify")]
    let manifest = manifest.section("auth-profile", fortify::profile);
    let manifest = manifest
        .section("server", server::config)
        .section("app", app::config)
        .section("api", api::config)
        .section("log", log::config)
        .section("cache", cache::config)
        .section("database", database::config)
        .section("i18n", i18n::config)
        .section("translation_sources", translation_sources::config)
        .section("mail", mail::config)
        .section("modules", modules::config)
        .section("session", session::config)
        .section("sms", sms::config)
        .section("storage", storage::config)
        .section("auth", auth::config)
        .section("hashing", hashing::config)
        .section("security", security::config);
    #[cfg(feature = "auth-fortify")]
    let manifest = manifest
        .section("fortify", fortify::config)
        .section("fortify-options", fortify::options);
    #[cfg(feature = "queue")]
    let manifest = manifest.section("queue", queue::config);
    #[cfg(feature = "realtime")]
    let manifest = manifest.section("realtime", realtime::config);
    #[cfg(feature = "grpc")]
    let manifest = manifest.section("grpc", grpc::config);
    #[cfg(feature = "view")]
    let manifest = if view::ENABLED {
        manifest.section("view", view::config)
    } else {
        manifest
    };
    manifest
}
