use clap::{Arg, ArgAction, Command as Syntax, value_parser};
use openrust::{
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    database::DatabaseManager,
    foundation::Application,
    queue::QueueManager,
    support::facades::Artisan,
};

use crate::{DeliveryService, DeliveryStore, Error, delivery::RegisteredDeliveryPolicy};

pub(super) fn register() -> Result<(), ConsoleError> {
    let syntax = Syntax::new("webhook-server:send")
        .about("投递指定 Webhook 或恢复当前范围内到期的投递")
        .arg(Arg::new("id"))
        .arg(Arg::new("scope").long("scope").required(true))
        .arg(Arg::new("database").long("database"))
        .arg(
            Arg::new("queue")
                .long("queue")
                .requires("id")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("limit")
                .long("limit")
                .conflicts_with("id")
                .default_value("100")
                .value_parser(value_parser!(u32).range(1..=500)),
        );
    Artisan::add(Box::new(Send(CommandDefinition::new(syntax))))
}

struct Send(CommandDefinition);

impl Command for Send {
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
            let args = input.matches();
            let scope = args.get_one::<String>("scope").expect("必填范围");
            let database = args.get_one::<String>("database").map(String::as_str);
            let manager = app
                .make::<DatabaseManager>()
                .await
                .map_err(ConsoleError::from_source)?;
            let connection = manager
                .connection(database)
                .map_err(ConsoleError::from_source)?;
            let policy = app
                .make::<RegisteredDeliveryPolicy>()
                .await
                .map_err(|_| ConsoleError::from_source(Error::InvalidConfig("delivery.policy")))?;
            let store = DeliveryStore::new(connection, scope).map_err(ConsoleError::from_source)?;
            let service = DeliveryService::new(store.clone(), policy.0.clone());
            if let Some(id) = args.get_one::<String>("id") {
                if args.get_flag("queue") {
                    let queue = app
                        .make::<QueueManager>()
                        .await
                        .map_err(ConsoleError::from_source)?;
                    service
                        .dispatch(&queue, id)
                        .await
                        .map_err(ConsoleError::from_source)?;
                    output.info("投递已入队");
                } else {
                    let status = service
                        .dispatch_sync(id)
                        .await
                        .map_err(ConsoleError::from_source)?;
                    output
                        .info(&serde_json::to_string(&status).map_err(ConsoleError::from_source)?);
                }
            } else {
                let limit = *args.get_one::<u32>("limit").expect("默认批次大小");
                let ids = store
                    .due_ids(limit)
                    .await
                    .map_err(ConsoleError::from_source)?;
                for id in ids {
                    let status = service
                        .dispatch_sync(&id)
                        .await
                        .map_err(ConsoleError::from_source)?;
                    let status =
                        serde_json::to_string(&status).map_err(ConsoleError::from_source)?;
                    output.info(&format!("{id} {status}"));
                }
            }
            Ok(0)
        })
    }
}
