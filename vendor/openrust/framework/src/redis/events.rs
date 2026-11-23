use crate::contracts::events::Event;
use std::time::Duration;

/// A successful Redis command, with its original binary arguments.
pub struct CommandExecuted {
    pub command: String,
    pub parameters: Vec<Vec<u8>>,
    pub time: Duration,
    pub connection_name: String,
}
impl Event for CommandExecuted {
    fn name(&self) -> &str {
        "OpenRust\\Redis\\Events\\CommandExecuted"
    }
}
