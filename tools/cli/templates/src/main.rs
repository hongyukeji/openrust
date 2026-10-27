//! 应用程序入口。

#[path = "../bootstrap/app.rs"]
mod app;
#[path = "../routes/web.rs"]
mod routes;
{{database_module}}
{{grpc_module}}

fn main() -> std::process::ExitCode {
    openrust::run::<app::{{type_name}}>()
}
