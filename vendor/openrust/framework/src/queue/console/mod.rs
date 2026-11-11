//! Artisan commands for queue workers.

mod work_command;

pub use work_command::WorkCommand;

mod failed_commands;

pub(crate) fn register(
    artisan: &mut crate::console::Application,
) -> Result<(), crate::console::ConsoleError> {
    artisan.add_command(Box::new(WorkCommand::new()))?;
    failed_commands::register(artisan)
}
