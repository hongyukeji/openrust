use clap::{Arg, ArgAction, Command as Syntax};

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    contracts::container::Container as _,
    foundation::Application,
    queue::QueueManager,
};

use super::work_command::error;

enum Action {
    Restart,
    Pause,
    Resume,
}

struct WorkerControlCommand {
    definition: CommandDefinition,
    action: Action,
}

impl Command for WorkerControlCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.definition
    }

    fn handle<'a>(
        &'a self,
        app: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let manager = app
                .container_handle()
                .make::<QueueManager>()
                .map_err(error)?;
            let action = &self.action;
            if !manager.polls_interruptions() {
                let message = match action {
                    Action::Restart => Some("Queue restarting is currently disabled."),
                    Action::Pause => Some("Queue pausing is currently disabled."),
                    Action::Resume => None,
                };
                if let Some(message) = message {
                    return Err(ConsoleError::new(message));
                }
            }
            if matches!(action, Action::Restart) {
                manager
                    .worker_control()
                    .map_err(error)?
                    .restart()
                    .await
                    .map_err(error)?;
                output.info("Broadcasting queue restart signal.");
                return Ok(0);
            }
            let args = input.matches();
            let target = if args.get_flag("all") {
                None
            } else {
                let queue = args.get_one::<String>("queue").ok_or_else(|| {
                    ConsoleError::new("A queue name is required unless the --all option is used.")
                })?;
                Some(manager.control_target(queue).map_err(error)?)
            };
            let queue = target
                .as_ref()
                .map(|(connection, queue)| (connection.as_str(), queue.as_str()));
            let status = match (action, queue) {
                (Action::Pause, Some((connection, queue))) => {
                    manager.pause(connection, queue).await.map_err(error)?;
                    "paused"
                }
                (Action::Pause, None) => {
                    manager.pauseAll().await.map_err(error)?;
                    "paused"
                }
                (Action::Resume, Some((connection, queue))) => {
                    manager.resume(connection, queue).await.map_err(error)?;
                    "resumed"
                }
                (Action::Resume, None) => {
                    manager.resumeAll().await.map_err(error)?;
                    "resumed"
                }
                (Action::Restart, _) => unreachable!(),
            };
            if let Some((connection, queue)) = queue {
                output.info(&format!(
                    "Job processing on queue [{connection}:{queue}] has been {status}."
                ));
            } else {
                output.info(&format!(
                    "Job processing on all queues across all connections has been {status}."
                ));
            }
            Ok(0)
        })
    }
}

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    let restart = Syntax::new("queue:restart").about("Request a graceful restart of queue workers");
    let pause = Syntax::new("queue:pause").about("Pause processing on a queue");
    let resume = Syntax::new("queue:resume")
        .visible_alias("queue:continue")
        .about("Resume processing on a paused queue");
    let target = |syntax: Syntax| {
        syntax
            .arg(Arg::new("queue").help("Queue name or connection:queue"))
            .arg(Arg::new("all").long("all").action(ArgAction::SetTrue))
    };
    for (action, syntax) in [
        (Action::Restart, restart),
        (Action::Pause, target(pause)),
        (Action::Resume, target(resume)),
    ] {
        artisan.add_command(Box::new(WorkerControlCommand {
            definition: CommandDefinition::new(syntax),
            action,
        }))?;
    }
    Ok(())
}
