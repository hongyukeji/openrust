use clap::{Arg, ArgAction, Command as Syntax};

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output, confirm_production,
    },
    contracts::container::Container as _,
    foundation::Application,
    queue::{QueueError, QueueManager, QueueResult, queue_name},
    support::facades::{Facade, Queue},
};

use super::work_command::error;

pub struct ClearCommand(CommandDefinition);

impl ClearCommand {
    /// Controls queue clearing for the application in the current Facade scope.
    pub fn prohibit(prohibited: bool) -> QueueResult<()> {
        Queue::getFacadeRoot()
            .map_err(QueueError::Unavailable)?
            .set_clear_prohibited(prohibited);
        Ok(())
    }
}

impl Command for ClearCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self,
        app: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let container = app.container_handle();
            let manager = container.make::<QueueManager>().map_err(error)?;
            if manager.clear_prohibited() {
                return Err(ConsoleError::new("Queue clearing is prohibited."));
            }
            let args = input.matches();
            let environment = container.get::<str>("env").map_err(error)?;
            if !confirm_production(environment.as_ref() == "production", args)? {
                return Err(ConsoleError::new("Command cancelled."));
            }

            let connection = args.get_one::<String>("connection").map(String::as_str);
            let queue = manager.connection(connection).map_err(error)?;
            let clearable = queue.clearable().ok_or_else(|| {
                ConsoleError::new(format!(
                    "Clearing queues is not supported on [{}].",
                    queue.get_connection_name()
                ))
            })?;
            let selected = args
                .get_one::<String>("queue")
                .filter(|name| !name.is_empty())
                .map_or_else(|| queue.default_queue(), String::as_str);
            let mut names = Vec::new();
            for name in selected
                .split(',')
                .map(str::trim)
                .filter(|name| !name.is_empty())
            {
                queue_name(Some(name), queue.default_queue()).map_err(error)?;
                if !names.contains(&name) {
                    names.push(name);
                }
            }
            let mut count = 0_u64;
            for name in &names {
                count = count
                    .checked_add(clearable.clear(name).await.map_err(error)?)
                    .ok_or_else(|| ConsoleError::new("The cleared job count is out of range."))?;
            }
            let jobs = if count == 1 { "job" } else { "jobs" };
            let queues = if names.len() == 1 { "queue" } else { "queues" };
            output.info(&format!(
                "Cleared {count} {jobs} from the [{}] {queues}",
                names.join(", ")
            ));
            Ok(0)
        })
    }
}

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    let syntax = Syntax::new("queue:clear")
        .about("Delete all of the jobs from the specified queues")
        .arg(Arg::new("connection").help("The name of the queue connection to clear"))
        .arg(
            Arg::new("queue")
                .long("queue")
                .help("The names of the queues to clear"),
        )
        .arg(
            Arg::new("force")
                .long("force")
                .action(ArgAction::SetTrue)
                .help("Force the operation to run when in production"),
        );
    artisan.add_command(Box::new(ClearCommand(CommandDefinition::new(syntax))))
}
