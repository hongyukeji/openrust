use std::ffi::OsString;

use serde::{Deserialize, Serialize};

use crate::{
    bus::Bus,
    console::Output,
    contracts::queue::JobHandler,
    queue::{
        DispatchOptions, DispatchResult, JobContext, Payload, QueueError, QueueFuture, QueueResult,
    },
};

/// A command and its queue destination, ready for explicit asynchronous dispatch.
#[must_use = "dispatch the command to enqueue it"]
pub struct PendingQueuedCommand {
    command: String,
    arguments: Vec<OsString>,
    options: DispatchOptions,
}

impl PendingQueuedCommand {
    pub(crate) fn new(command: &str, arguments: &[OsString]) -> Self {
        Self {
            command: command.into(),
            arguments: arguments.to_vec(),
            options: DispatchOptions::default(),
        }
    }

    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    pub fn onConnection(mut self, name: impl Into<String>) -> Self {
        self.options.connection = Some(name.into());
        self
    }

    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    pub fn onQueue(mut self, name: impl Into<String>) -> Self {
        self.options.queue = Some(name.into());
        self
    }

    pub async fn dispatch(self) -> QueueResult<DispatchResult> {
        let arguments = self
            .arguments
            .into_iter()
            .map(|argument| {
                argument
                    .into_string()
                    .map_err(|_| QueueError::InvalidPayload)
            })
            .collect::<QueueResult<_>>()?;
        let data = CommandInvocation {
            command: self.command,
            arguments,
        };
        let payload = Payload::new(QueuedCommand::NAME, &data)?.with_display_name(data.command);
        Bus::dispatch(payload, self.options, None).await
    }
}

#[derive(Deserialize, Serialize)]
struct CommandInvocation {
    command: String,
    arguments: Vec<String>,
}

pub(crate) struct QueuedCommand;

impl QueuedCommand {
    pub(crate) const NAME: &str = "artisan.queued_command";
}

impl JobHandler for QueuedCommand {
    fn handle<'a>(
        &'a self,
        _: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            let data: CommandInvocation = serde_json::from_value(payload.data().clone())
                .map_err(|_| QueueError::InvalidPayload)?;
            let arguments: Vec<OsString> = data.arguments.into_iter().map(OsString::from).collect();
            // the framework acknowledges a returned status and retries only on an execution error.
            super::call_in_application(&data.command, &arguments, &mut Output::default())
                .await
                .map_err(|source| QueueError::Execution {
                    source: Box::new(source),
                })?;
            Ok(())
        })
    }
}
