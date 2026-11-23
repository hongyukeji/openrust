use std::{env, error::Error, ffi::OsStr, path::Path, process::ExitCode, sync::Arc};

use crate::{
    console::ConsoleError,
    contracts::{container::Container as _, foundation::Application as _},
};

use super::{
    Application,
    console::config_cache_command::{ExecutableEntry, WRITE_COMMAND, write_fresh},
    console::serve_command,
};

/// Selects the process entry point before creating the application.
pub async fn run_application(
    create: impl FnOnce() -> Result<Application, Box<dyn Error + Send + Sync>>,
) -> ExitCode {
    let mut arguments = env::args_os().peekable();
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
    #[cfg(windows)]
    if artisan
        && arguments
            .peek()
            .is_some_and(|arg| arg == super::console::dev_command::WINDOWS_CHILD)
    {
        arguments.next();
        if let Err(error) = crate::process::windows_job::wait_for_assignment() {
            eprintln!("Development child startup failed: {error}");
            return ExitCode::FAILURE;
        }
        match arguments.next() {
            Some(kind) if kind == "artisan" => {}
            Some(kind) if kind == "shell" => {
                if let Some(script) = arguments.next()
                    && arguments.next().is_none()
                {
                    match super::console::dev_command::run_shell_child(script).await {
                        Ok(code) => std::process::exit(code),
                        Err(error) => eprintln!("Development shell failed: {error}"),
                    }
                } else {
                    eprintln!("Invalid development shell arguments");
                }
                return ExitCode::FAILURE;
            }
            _ => {
                eprintln!("Invalid development child kind");
                return ExitCode::FAILURE;
            }
        }
    }
    let isolated_job = artisan
        && arguments
            .peek()
            .is_some_and(|arg| arg == crate::queue::isolated_job::COMMAND);
    #[cfg(windows)]
    if isolated_job && let Err(error) = crate::queue::isolated_job::wait_for_assignment() {
        eprintln!("Queue job execution process failed: {error}");
        return ExitCode::FAILURE;
    }
    let app = match create() {
        Ok(app) => app,
        Err(error) => {
            eprintln!("Application startup failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    app.container()
        .instance(Arc::new(ExecutableEntry))
        .expect("compiled entry is initially unbound");
    let (result, error_context) = if artisan {
        let arguments: Vec<_> = arguments.collect();
        if isolated_job {
            let result =
                crate::queue::isolated_job::serve(app, arguments.get(1).map(Path::new)).await;
            (result.map(|()| 0), "Queue job execution process failed")
        } else if arguments
            .first()
            .is_some_and(|argument| argument == serve_command::CHILD_COMMAND)
        {
            let result = serve_command::child(app, &arguments[1..]).await;
            (result.map(|()| 0), "Development HTTP server failed")
        } else if arguments
            .first()
            .is_some_and(|argument| argument == WRITE_COMMAND)
        {
            let result = async {
                let destination = arguments.get(1).map(Path::new).ok_or_else(|| {
                    ConsoleError::new("Missing configuration cache staging path.")
                })?;
                let selected = arguments
                    .get(2)
                    .map(|argument| {
                        argument
                            .to_str()
                            .ok_or_else(|| ConsoleError::new("Invalid environment name."))
                    })
                    .transpose()?;
                write_fresh(app, destination, selected).await
            }
            .await;
            (result.map(|()| 0), "Configuration cache build failed")
        } else {
            (app.handle_command(&arguments).await, "Artisan failed")
        }
    } else {
        (app.start().await.map(|()| 0), "Application startup failed")
    };
    match result {
        Ok(status) => ExitCode::from(u8::try_from(status).unwrap_or(1)),
        Err(error) => {
            eprintln!("{error_context}: {error}");
            let status = super::runner::exit_status(error.as_ref()).unwrap_or(1);
            ExitCode::from(status)
        }
    }
}
