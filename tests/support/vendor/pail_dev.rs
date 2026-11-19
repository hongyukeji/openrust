use openrust::{console::ConsoleError, foundation::DevCommands};

pub(super) fn register() -> Result<(), ConsoleError> {
    DevCommands::register("echo package logs", Some("logs"))?.green();
    Ok(())
}
