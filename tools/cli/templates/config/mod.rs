//! 应用代码配置清单；按声明顺序单次求值，每个区段可读取已完成依赖。

pub mod api;
pub mod app;
pub mod auth;
pub mod cache;
pub mod database;
{{grpc_module}}pub mod i18n;
pub mod log;
pub mod mail;
pub mod security;
pub mod server;
pub mod session;
pub mod sms;
pub mod storage;
pub mod view;

// queue.rs 与 realtime.rs 是尚未声明的可选配置源码。启用时须同步依赖 feature、
// pub mod 声明、下方 section 和实际 Provider；generate job 会接入 queue 配置。

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
        .section("mail", mail::config)
        .section("session", session::config)
        .section("sms", sms::config)
        .section("storage", storage::config)
        .section("auth", auth::config)
        .section("security", security::config);
    // 只控制默认区段声明；显式来源仍可让 OptionalViewServiceProvider 启用视图。
    let manifest = if view::ENABLED {
        manifest.section("view", view::config)
    } else {
        manifest
    };
{{grpc_registration}}    manifest
}
