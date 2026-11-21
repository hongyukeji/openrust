//! Application assembly entry point.

mod app;
pub(crate) mod packages;
mod providers;

pub(crate) use app::create_application;
