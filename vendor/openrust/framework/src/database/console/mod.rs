//! Database Artisan commands.

pub(crate) mod factories;
pub(crate) mod migrations;
pub(crate) mod seeds;

use crate::{console::ConsoleError, foundation::Application};

pub(super) fn confirm(app: &Application, matches: &clap::ArgMatches) -> Result<bool, ConsoleError> {
    let config = app
        .configuration_repository()
        .map_err(|error| ConsoleError::new(error.to_string()))?;
    let production = config
        .get("app.env")
        .is_some_and(|value| value.as_str() == Some("production"));
    crate::console::confirm_production(production, matches)
}
