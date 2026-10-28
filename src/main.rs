//! 默认应用入口。
#![forbid(unsafe_code)]

#[path = "../app/mod.rs"]
mod app;
#[path = "../bootstrap/app.rs"]
mod bootstrap;
#[path = "../routes/web.rs"]
mod routes;

fn main() -> std::process::ExitCode {
    openrust::run::<bootstrap::Server>()
}
