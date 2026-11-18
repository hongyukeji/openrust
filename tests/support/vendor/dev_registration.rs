use openrust::{console::ConsoleError, foundation::DevCommands};

pub(super) fn register() -> Result<(), ConsoleError> {
    DevCommands::register("echo vendor", Some("package"))?;
    DevCommands::register("echo vendor queue", Some("queue"))?;
    Ok(())
}
