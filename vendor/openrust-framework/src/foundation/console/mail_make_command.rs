use crate::console::{Application as Artisan, ConsoleError};

use super::message_make_command::{self, Kind};

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    message_make_command::register(artisan, Kind::Mailable)
}
