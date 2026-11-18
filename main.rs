use std::process::ExitCode;

use openrust::foundation::run_application;

mod app;
mod bootstrap;
mod config;
mod database;
mod routes;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    run_application(bootstrap::create_application).await
}
