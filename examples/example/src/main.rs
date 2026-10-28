//! 应用程序入口。

#[path = "../bootstrap/app.rs"]
mod app;
#[path = "../app/grpc.rs"]
mod grpc;
#[path = "../routes/web.rs"]
mod routes;
#[path = "../database/seeders.rs"]
mod seed;

fn main() -> std::process::ExitCode {
    openrust::run::<app::Example>()
}

#[path = "../app/jobs/mod.rs"]
mod jobs;
