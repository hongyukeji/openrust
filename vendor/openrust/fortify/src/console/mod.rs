pub mod install_command;

pub(crate) fn register() -> Result<(), openrust::console::ConsoleError> {
    openrust::support::facades::Artisan::add(Box::new(install_command::InstallCommand::default()))
}

#[cfg(feature = "sanctum")]
pub(crate) mod prune_auth_challenges;
