mod keys_command;
pub use keys_command::KeysCommand;
pub(crate) fn register() -> Result<(), openrust::console::ConsoleError> {
    openrust::support::facades::Artisan::add(Box::new(KeysCommand::default()))
}
