#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod app;
pub mod command;
pub mod config;
pub mod error;
pub mod lifecycle;
pub mod plugin;

pub use app::{App, AppBuilder, AppState, Application, MissingServiceError};
pub use command::Command;
pub use config::NoConfig;
pub use error::{
    ApiResponse, Error, PageMeta, ResponseContext, Result, ValidationErrors, with_response_context,
};
pub use lifecycle::{bootstrap, bootstrap_with, finalize, shutdown_signal};
pub use plugin::{BoxFuture, Plugin};
