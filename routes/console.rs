//! Application console route registrations.

use openrust::{console::ConsoleError, foundation::Inspiring, support::facades::Artisan};

pub(crate) fn register() -> Result<(), ConsoleError> {
    Artisan::command("inspire", |command| {
        command.comment(&Inspiring::quote());
    })?
    .purpose("Display an inspiring quote")
}
