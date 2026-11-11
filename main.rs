use std::process::ExitCode;

use openrust::foundation::runApplication;

mod app;
mod bootstrap;
mod config;
mod database;
mod routes;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    runApplication(bootstrap::createApplication).await
}
