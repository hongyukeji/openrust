#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::process::ExitCode;

pub mod config;
pub mod core;
pub mod db;
pub mod http;
pub mod mail;

#[cfg(feature = "auth")]
pub mod auth;

pub use core::{
    ApiResponse, App, AppBuilder, AppState, Application, BoxFuture, Command, Error,
    MissingServiceError, NoConfig, PageMeta, Plugin, Result, ValidationErrors, lifecycle,
};
pub use openrust_macros::resource;

#[cfg(test)]
static ENV_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
pub(crate) fn lock_test_env() -> std::sync::MutexGuard<'static, ()> {
    ENV_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Database layer, enabled by the `db-postgres` or `db-mysql` facade feature.
#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
pub use db as database;

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
pub use db::{DatabaseConfig, DatabasePlugin, Db, MigrationPlugin, SeedPlugin};

/// Service extension methods for testing an HTTP router without binding a listener.
pub use tower::ServiceExt;

/// Tokio runtime for applications depending only on the facade.
pub use tokio;

/// Everything a typical `app.rs`, controller or plugin needs.
pub mod prelude {
    #[cfg(feature = "auth")]
    pub use crate::auth::{
        AuthCredential, AuthLayer, AuthMigration, AuthMode, AuthPlugin, AuthPrincipal, AuthSource,
        Authenticator, CurrentUser, JwtConfig, JwtPlugin, JwtTokens, OptionalUser,
    };
    pub use crate::http::axum::extract::State;
    pub use crate::http::axum::http::StatusCode;
    pub use crate::http::axum::response::IntoResponse;
    pub use crate::http::{
        Action, AppBuilderHttpExt, Created, Inject, Json, NoContent, PageParams, Paginated, Path,
        Query, Resource, ResourceActions, Routes, Validated,
    };
    pub use crate::mail::{Email, MailConfig, Mailer, MailerPlugin};
    pub use crate::{
        App, AppBuilder, Application, BoxFuture, Command, Error, NoConfig, Plugin, Result,
    };
    #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
    pub use crate::{DatabaseConfig, DatabasePlugin, Db, MigrationPlugin, SeedPlugin};
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
        let app = match lifecycle::bootstrap::<A>().await {
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

#[cfg(test)]
mod tests {
    use super::prelude::*;

    #[test]
    fn prelude_json_is_a_response() {
        let response = Json("ok").into_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["content-type"], "application/json");
    }
}
