//! 应用程序入口。

#[path = "../app/mod.rs"]
mod app;
#[path = "../bootstrap/app.rs"]
mod bootstrap;
#[path = "../config/mod.rs"]
mod config;
{{grpc_module}}#[path = "../modules/mod.rs"]
mod modules;
#[path = "../routes/mod.rs"]
mod routes;
{{database_module}}
fn main() -> std::process::ExitCode {
    #[cfg(feature = "source-artisan")]
    if std::env::var_os("OPENRUST_SOURCE_ARTISAN").is_some_and(|value| value == "1") {
        return openrust::run_application_with_source_command::<bootstrap::{{type_name}}>(
            cli::release::SourceBuild,
        );
    }
    openrust::run_application::<bootstrap::{{type_name}}>()
}
