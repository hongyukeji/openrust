use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    contracts::{container::Container as _, foundation::Application as _},
    events::Dispatcher,
    foundation::Application,
};

struct EventListCommand(CommandDefinition);

impl Command for EventListCommand {
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
            let dispatcher = app
                .container()
                .make::<Dispatcher>()
                .map_err(|source| ConsoleError::new(source.to_string()))?;
            let filter = input.matches().get_one::<String>("event");
            let mut events = dispatcher.listing();
            events.retain(|(event, _)| filter.is_none_or(|filter| event.contains(filter)));
            events.sort_unstable_by(|(a, _), (b, _)| a.cmp(b));
            if input.matches().get_flag("json") {
                let data: Vec<_> = events.iter().map(|(event, listeners)| {
                    serde_json::json!({ "event": event, "listeners": listeners })
                }).collect();
                output
                    .stdout
                    .push_str(&serde_json::Value::Array(data).to_string());
                output.stdout.push('\n');
            } else if events.is_empty() {
                output
                    .info("Your application doesn't have any events matching the given criteria.");
            } else {
                for (event, listeners) in events {
                    output.stdout.push_str(&format!("  {event}\n"));
                    for listener in listeners {
                        output.stdout.push_str(&format!("    → {listener}\n"));
                    }
                }
            }
            Ok(0)
        })
    }
}

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(EventListCommand(CommandDefinition::new(
        clap::Command::new("event:list")
            .about("Show events and their registered listeners")
            .arg(clap::Arg::new("event").long("event"))
            .arg(
                clap::Arg::new("json")
                    .long("json")
                    .action(clap::ArgAction::SetTrue),
            ),
    ))))
}
