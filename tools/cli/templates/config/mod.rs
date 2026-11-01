//! 应用代码配置清单；每个区段由 Rust 函数求值。

pub mod api;
pub mod app;
pub mod auth;
pub mod cache;
pub mod database;
{{grpc_module}}pub mod i18n;
pub mod mail;
pub mod security;
pub mod server;
pub mod session;
pub mod sms;
pub mod view;

/// 登记应用默认配置；运行环境覆盖仍由统一配置仓储处理。
pub fn boot() -> openrust::config::ConfigManifest {
    openrust::config::ConfigManifest::new()
        .section("app", app::config)
        .section("server", server::config)
        .section("api", api::config)
        .section("cache", cache::config)
        .section("database", database::config)
        .section("i18n", i18n::config)
        .section("mail", mail::config)
        .section("session", session::config)
        .section("sms", sms::config)
        .section("auth", auth::config)
        .section("security", security::config)
        .section("view", view::config)
{{grpc_registration}}}
