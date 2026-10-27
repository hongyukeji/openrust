//! 应用程序入口。

mod app;
mod grpc;
mod routes;
mod seed;

fn main() -> std::process::ExitCode {
    openrust::run::<app::Example>()
}

mod jobs;
