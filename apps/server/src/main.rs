//! OpenRust framework integration host.
#![forbid(unsafe_code)]

mod app;
mod routes;

use app::Server;

fn main() -> std::process::ExitCode {
    openrust::run::<Server>()
}
