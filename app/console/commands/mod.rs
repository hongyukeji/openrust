//! Application command types registered through bootstrap.

use openrust::console::{Application as Artisan, ConsoleError};

pub(crate) fn register(_artisan: &mut Artisan) -> Result<(), ConsoleError> {
    Ok(())
}
