//! Artisan commands for queue workers.

mod prune_batches_command;
mod prune_chain_deliveries_command;
mod publish_batches_command;
mod retry_batch_command;
mod work_command;
mod worker_control_command;

pub use work_command::WorkCommand;

mod failed_commands;

pub(crate) fn register(
    artisan: &mut crate::console::Application,
) -> Result<(), crate::console::ConsoleError> {
    artisan.add_command(Box::new(WorkCommand::new()))?;
    worker_control_command::register(artisan)?;
    prune_batches_command::register(artisan)?;
    prune_chain_deliveries_command::register(artisan)?;
    retry_batch_command::register(artisan)?;
    publish_batches_command::register(artisan)?;
    failed_commands::register(artisan)
}
