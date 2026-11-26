use std::{sync::Arc, time::Instant};

use clap::ArgMatches;

use crate::{
    contracts::events::Dispatcher as _,
    events::{Dispatcher, EventContext},
    foundation::Application,
};

use super::{
    CommandDefinition, ConsoleError, Output,
    events::{CommandFinished, CommandStarting},
};

/// 区分控制台应用执行和命令处理器内直接调用子命令。
#[derive(Clone, Copy)]
pub(crate) enum CommandExecution {
    Application,
    Child,
}

pub(super) struct CommandEvents<'a> {
    dispatcher: Option<Arc<Dispatcher>>,
    definition: &'a CommandDefinition,
    stdout_start: usize,
    generation: u64,
    started: Instant,
}

impl<'a> CommandEvents<'a> {
    pub(super) async fn new(
        app: &Application,
        definition: &'a CommandDefinition,
        output: &Output,
        execution: CommandExecution,
    ) -> Result<Self, ConsoleError> {
        let dispatcher = match execution {
            CommandExecution::Application => Some(
                app.make::<Dispatcher>()
                    .await
                    .map_err(ConsoleError::from_source)?,
            ),
            CommandExecution::Child => None,
        };
        Ok(Self {
            dispatcher,
            definition,
            stdout_start: output.stdout.len(),
            generation: output.capture_generation,
            started: Instant::now(),
        })
    }

    pub(super) async fn start(&mut self, input: &ArgMatches) -> Result<(), ConsoleError> {
        if let Some(dispatcher) = &self.dispatcher
            && dispatcher.has_listeners(CommandStarting::NAME)
        {
            dispatcher
                .dispatch(
                    Arc::new(CommandStarting {
                        command: self.definition.name().to_owned(),
                        input: input.clone(),
                        definition: self.definition.syntax.clone(),
                    }),
                    &mut EventContext::new(),
                )
                .await
                .map_err(ConsoleError::from_source)?;
        }
        self.started = Instant::now();
        Ok(())
    }

    pub(super) async fn finish(
        self,
        input: &ArgMatches,
        output: &Output,
        result: Result<i32, ConsoleError>,
    ) -> Result<i32, ConsoleError> {
        if let Some(dispatcher) = &self.dispatcher
            && dispatcher.has_listeners(CommandFinished::NAME)
        {
            let reported = result
                .as_ref()
                .map_or_else(ConsoleError::command_exit_code, |status| *status);
            let body = if self.generation == output.capture_generation {
                output
                    .stdout
                    .get(self.stdout_start..)
                    .unwrap_or(&output.stdout)
            } else {
                &output.stdout
            };
            let finished = dispatcher
                .dispatch(
                    Arc::new(CommandFinished {
                        command: self.definition.name().to_owned(),
                        input: input.clone(),
                        definition: self.definition.syntax.clone(),
                        exit_code: reported,
                        duration: self.started.elapsed(),
                        output: body.to_owned(),
                    }),
                    &mut EventContext::new(),
                )
                .await
                .map_err(ConsoleError::from_source);
            result.and_then(|status| finished.map(|_| status))
        } else {
            result
        }
    }
}
