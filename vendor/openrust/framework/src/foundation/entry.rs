use std::{env, error::Error, ffi::OsStr, path::Path, process::ExitCode};

use super::Application;

/// Selects the process entry point before creating the application.
#[allow(non_snake_case, reason = "Matches the public application entry naming")]
pub async fn runApplication(
    create: impl FnOnce() -> Result<Application, Box<dyn Error + Send + Sync>>,
) -> ExitCode {
    let mut arguments = env::args_os();
    let executable = arguments.next();
    let artisan = match arguments.next() {
        None => false,
        Some(mode) if mode == "artisan" => true,
        Some(_) => {
            let name = executable
                .as_deref()
                .and_then(|path| Path::new(path).file_name());
            eprintln!(
                "Usage: {} [artisan [COMMAND]]",
                name.unwrap_or_else(|| OsStr::new("app")).to_string_lossy()
            );
            return ExitCode::from(2);
        }
    };
    let app = match create() {
        Ok(app) => app,
        Err(error) => {
            eprintln!("Application startup failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    let (result, error_context) = if artisan {
        let arguments: Vec<_> = arguments.collect();
        if arguments
            .first()
            .is_some_and(|arg| arg == crate::queue::isolated_job::COMMAND)
        {
            let result =
                crate::queue::isolated_job::serve(app, arguments.get(1).map(Path::new)).await;
            (result.map(|()| 0), "Queue job execution process failed")
        } else {
            (app.handleCommand(&arguments).await, "Artisan failed")
        }
    } else {
        (app.start().await.map(|()| 0), "Application startup failed")
    };
    match result {
        Ok(status) => ExitCode::from(u8::try_from(status).unwrap_or(1)),
        Err(error) => {
            eprintln!("{error_context}: {error}");
            ExitCode::FAILURE
        }
    }
}
