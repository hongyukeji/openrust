//! 应用代码配置清单；按声明顺序单次求值，每个区段可读取已完成依赖。

pub mod api;
pub mod app;
pub mod auth;
pub mod cache;
pub mod database;
{{grpc_module}}pub mod hashing;
pub mod i18n;
pub mod translation_sources;
pub mod log;
pub mod mail;
pub mod modules;
{{queue_module}}pub mod security;
pub mod server;
pub mod session;
pub mod sms;
pub mod storage;
pub mod view;

// realtime.rs 是尚未声明的可选配置源码；精简项目同样不声明 queue.rs。
// 启用领域须同步依赖、mod/section 与 Provider；generate job 保留旧项目的显式接入。

/// 登记默认配置；显式文件、环境及前置覆写通过同一仓储参与依赖读取。
pub fn boot() -> openrust::config::ConfigManifest {
    let manifest = openrust::config::ConfigManifest::new()
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
{{queue_registration}}    // 只控制默认区段声明；显式来源仍可让 OptionalViewServiceProvider 启用视图。
    let manifest = if view::ENABLED {
        manifest.section("view", view::config)
    } else {
        manifest
    };
{{grpc_registration}}    manifest
}
