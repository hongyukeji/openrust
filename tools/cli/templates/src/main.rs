//! 应用程序入口。

#[path = "../bootstrap/app.rs"]
mod bootstrap;
#[path = "../modules/mod.rs"]
mod modules;
#[path = "../app/mod.rs"]
mod app;
#[path = "../routes/web.rs"]
mod routes;
{{database_module}}
{{grpc_module}}

fn main() -> std::process::ExitCode {
    openrust::run::<bootstrap::{{type_name}}>()
}
