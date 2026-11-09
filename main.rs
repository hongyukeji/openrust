//! 默认应用入口。
#![forbid(unsafe_code)]

#[path = "app/mod.rs"]
mod app;
#[path = "bootstrap/app.rs"]
mod bootstrap;
#[path = "config/mod.rs"]
mod config;
#[path = "database/mod.rs"]
mod database;
#[path = "modules/mod.rs"]
mod modules;
#[path = "routes/mod.rs"]
mod routes;

fn main() -> std::process::ExitCode {
    #[cfg(feature = "source-artisan")]
    if std::env::var_os("OPENRUST_SOURCE_ARTISAN").is_some_and(|value| value == "1") {
        return openrust::run_application_with_source_command::<bootstrap::Server>(
            cli::release::SourceBuild,
        );
    }
    openrust::run_application::<bootstrap::Server>()
}
