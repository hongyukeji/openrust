use crate::contracts::events::Event;
use std::time::Duration;

/// A successful Redis command, with its original binary arguments.
#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
pub struct CommandExecuted {
    pub command: String,
    pub parameters: Vec<Vec<u8>>,
    pub time: Duration,
    pub connectionName: String,
}
impl Event for CommandExecuted {
    fn name(&self) -> &str {
        "OpenRust\\Redis\\Events\\CommandExecuted"
    }
}
