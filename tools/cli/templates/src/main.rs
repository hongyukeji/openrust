//! 应用程序入口。

mod app;
mod routes;
{{database_module}}
{{grpc_module}}

fn main() -> std::process::ExitCode {
    openrust::run::<app::{{type_name}}>()
}
