//! 应用程序入口。

#[path = "../app/mod.rs"]
mod app;
#[path = "../bootstrap/app.rs"]
mod bootstrap;
{{grpc_module}}#[path = "../modules/mod.rs"]
mod modules;
#[path = "../routes/web.rs"]
mod routes;
{{database_module}}
fn main() -> std::process::ExitCode {
    openrust::run::<bootstrap::{{type_name}}>()
}
