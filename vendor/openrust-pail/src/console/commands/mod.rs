mod pail_command;
pub use pail_command::PailCommand;

pub(crate) fn register() -> Result<(), openrust::console::ConsoleError> {
    openrust::support::facades::Artisan::add(Box::new(PailCommand::default()))
}
