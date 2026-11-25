//! Supervisor 结构化命令的线协议名称。
use crate::{Error, QueuedCommand, Result};
use serde_json::Value;

pub const PAUSE: &str = "Laravel\\Horizon\\SupervisorCommands\\Pause";
pub const CONTINUE_WORKING: &str = "Laravel\\Horizon\\SupervisorCommands\\ContinueWorking";
pub const RESTART: &str = "Laravel\\Horizon\\SupervisorCommands\\Restart";
pub const TERMINATE: &str = "Laravel\\Horizon\\SupervisorCommands\\Terminate";
pub const SCALE: &str = "Laravel\\Horizon\\SupervisorCommands\\Scale";
pub const BALANCE: &str = "Laravel\\Horizon\\SupervisorCommands\\Balance";

pub(crate) enum Command {
    Pause,
    Continue,
    Restart,
    Terminate(i32),
    Scale(usize),
    Balance(Vec<(String, usize)>),
}

impl Command {
    pub(crate) fn legacy(name: &str) -> Result<Self> {
        match name {
            "pause" => Ok(Self::Pause),
            "continue" => Ok(Self::Continue),
            "restart" => Ok(Self::Restart),
            "terminate" => Ok(Self::Terminate(0)),
            _ => Err(Error::InvalidRecord),
        }
    }

    pub(crate) fn queued(item: QueuedCommand) -> Result<Self> {
        match item.command.as_str() {
            PAUSE => Ok(Self::Pause),
            CONTINUE_WORKING => Ok(Self::Continue),
            RESTART => Ok(Self::Restart),
            TERMINATE => {
                let status = &item.options["status"];
                let status = if status.is_null() {
                    0
                } else {
                    u8::try_from(integer(status)?).map_err(|_| Error::InvalidRecord)?
                };
                Ok(Self::Terminate(i32::from(status)))
            }
            SCALE => Ok(Self::Scale(count(&item.options["scale"])?)),
            BALANCE => {
                let balance = match item.options {
                    Value::Object(entries) => entries
                        .into_iter()
                        .map(|(queue, value)| Ok((queue, count(&value)?)))
                        .collect::<Result<Vec<_>>>(),
                    Value::Array(entries) => entries
                        .into_iter()
                        .enumerate()
                        .map(|(index, value)| Ok((index.to_string(), count(&value)?)))
                        .collect::<Result<Vec<_>>>(),
                    _ => return Err(Error::InvalidRecord),
                }?;
                Ok(Self::Balance(balance))
            }
            _ => Err(Error::InvalidRecord),
        }
    }
}

fn integer(value: &Value) -> Result<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str()?.parse().ok())
        .ok_or(Error::InvalidRecord)
}

fn count(value: &Value) -> Result<usize> {
    usize::try_from(integer(value)?.max(0)).map_err(|_| Error::InvalidRecord)
}
