//! 默认应用入口。
#![forbid(unsafe_code)]

#[path = "../app/mod.rs"]
mod app;
#[path = "../bootstrap/app.rs"]
mod bootstrap;
#[path = "../routes/web.rs"]
mod routes;

fn main() -> std::process::ExitCode {
    let application_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    // Cargo 从 workspace 根启动默认成员时仍保留根 cwd；其他启动目录保持部署约定。
    if std::env::current_dir()
        .is_ok_and(|directory| application_dir.ancestors().nth(2) == Some(directory.as_path()))
        && let Err(error) = std::env::set_current_dir(application_dir)
    {
        eprintln!("无法进入应用目录：{error}");
        return std::process::ExitCode::FAILURE;
    }
    openrust::run::<bootstrap::Server>()
}
