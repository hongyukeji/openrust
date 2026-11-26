mod seed_command;
mod seeder_make_command;

use crate::console::{Application, ConsoleError};

pub(crate) fn register(artisan: &mut Application) -> Result<(), ConsoleError> {
    seed_command::register(artisan)?;
    seeder_make_command::register(artisan)
}
