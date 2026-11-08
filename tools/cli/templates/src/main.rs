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
    openrust::run_application::<bootstrap::{{type_name}}>()
}
