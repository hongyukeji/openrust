//! OpenRust 框架集成宿主。
#![forbid(unsafe_code)]

#[path = "../apps/mod.rs"]
mod apps;

use apps::Server;

fn main() -> std::process::ExitCode {
    openrust::run::<Server>()
}
