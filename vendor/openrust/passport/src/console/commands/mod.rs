mod client_command;
mod keys_command;
pub use client_command::ClientCommand;
pub use keys_command::KeysCommand;
pub(crate) fn register() -> Result<(), openrust::console::ConsoleError> {
    openrust::support::facades::Artisan::add(Box::new(KeysCommand::default()))?;
    openrust::support::facades::Artisan::add(Box::new(ClientCommand::default()))
}
