#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod app;
pub mod command;
pub mod config;
pub mod error;
pub mod lifecycle;
pub mod provider;

pub use app::{App, AppBuilder, AppState, Application, ServiceNotRegistered};
pub use command::Command;
pub use config::NoConfig;
pub use error::{Error, ProblemDetails, Result, ValidationErrors};
pub use lifecycle::{bootstrap, bootstrap_with, finalize, shutdown_signal};
pub use provider::{BoxFuture, ServiceProvider};
