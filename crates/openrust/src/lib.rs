#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::process::ExitCode;

pub use openrust_core::{
    App, AppBuilder, AppState, Application, BoxFuture, Command, Error, MissingServiceError,
    NoConfig, Plugin, ProblemDetails, Result, ValidationErrors, config, lifecycle,
};

/// HTTP layer: routing, extractors, middleware, `serve`.
pub use openrust_http as http;

/// Service extension methods for testing an HTTP router without binding a listener.
pub use tower::ServiceExt;

/// Tokio runtime for applications depending only on the facade.
pub use tokio;

/// Everything a typical `app.rs`, controller or plugin needs.
pub mod prelude {
    pub use crate::http::axum::extract::{Path, Query, State};
    pub use crate::http::axum::response::IntoResponse;
    pub use crate::http::axum::{Json, http::StatusCode};
    pub use crate::http::{
        Action, AppBuilderHttpExt, Created, Inject, NoContent, PageParams, Paginated, Resource,
        ResourceActions, Routes, Validated,
    };
    pub use crate::{
        App, AppBuilder, Application, BoxFuture, Command, Error, NoConfig, Plugin, Result,
    };
}

/// Boots `A` and runs the command named by the first argument (`serve` by default).
///
/// Returns a process exit code instead of panicking so `main` stays a one-liner.
pub fn run<A: Application>() -> ExitCode {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => return fail(&format!("cannot start tokio runtime: {error}")),
    };
    runtime.block_on(async {
        let app = match lifecycle::bootstrap_with::<A>(|app| {
            app.plugin(http::HttpPlugin);
        })
        .await
        {
            Ok(app) => app,
            Err(error) => return fail(&format!("bootstrap failed: {error}")),
        };
        let mut args = std::env::args().skip(1);
        let name = args.next().unwrap_or_else(|| "serve".to_owned());
        match app.run_command(&name, args.collect()).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => fail(&format!("{name}: {error}")),
        }
    })
}

fn fail(message: &str) -> ExitCode {
    // tracing may not be installed yet (configuration failed), so always write to stderr.
    eprintln!("error: {message}");
    ExitCode::FAILURE
}
