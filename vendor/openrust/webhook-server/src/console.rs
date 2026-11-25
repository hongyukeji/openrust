use clap::{Arg, ArgAction, Command as Syntax, value_parser};
use openrust::{
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    database::DatabaseManager,
    foundation::Application,
    queue::QueueManager,
    support::facades::Artisan,
};

use crate::{
    DeliveryRetention, DeliveryService, DeliveryStore, Error, IdempotencyKind, RetentionRecord,
    delivery::RegisteredDeliveryPolicy,
};

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
    Artisan::add(Box::new(Send(CommandDefinition::new(syntax))))?;
    let recovery = Syntax::new("webhook-server:recover")
        .about("将到期的 Webhook 投递重新交给 Worker")
        .arg(Arg::new("scope").long("scope").required(true))
        .arg(Arg::new("database").long("database"))
        .arg(
            Arg::new("pretend")
                .long("pretend")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("limit")
                .long("limit")
                .default_value("100")
                .value_parser(value_parser!(u32).range(1..=500)),
        );
    Artisan::add(Box::new(Recover(CommandDefinition::new(recovery))))?;
    let replay = Syntax::new("webhook-server:replay")
        .about("登记有独立预算且可审计的 Webhook 人工重放")
        .arg(Arg::new("id").required(true))
        .arg(Arg::new("scope").long("scope").required(true))
        .arg(Arg::new("database").long("database"))
        .arg(Arg::new("actor").long("actor").required(true))
        .arg(Arg::new("reason").long("reason").required(true))
        .arg(
            Arg::new("include-succeeded")
                .long("include-succeeded")
                .action(ArgAction::SetTrue),
        );
    Artisan::add(Box::new(Replay(CommandDefinition::new(replay))))?;
    let inspect = Syntax::new("webhook-server:inspect")
        .about("查询指定范围内的脱敏投递、消息或尝试")
        .arg(Arg::new("id").required(true))
        .arg(Arg::new("scope").long("scope").required(true))
        .arg(Arg::new("database").long("database"))
        .arg(
            Arg::new("message")
                .long("message")
                .conflicts_with("attempts")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("attempts")
                .long("attempts")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("after")
                .long("after")
                .requires("attempts")
                .value_parser(value_parser!(u32)),
        )
        .arg(
            Arg::new("limit")
                .long("limit")
                .requires("attempts")
                .value_parser(value_parser!(u32).range(1..=100)),
        );
    Artisan::add(Box::new(Inspect(CommandDefinition::new(inspect))))?;
    let prune = Syntax::new("webhook-server:prune")
        .about("分批清理已过保留期的出站快照、尝试或共享正文")
        .arg(Arg::new("scope").long("scope").required(true))
        .arg(Arg::new("database").long("database"))
        .arg(
            Arg::new("attempts")
                .long("attempts")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("messages")
                .long("messages")
                .conflicts_with("attempts")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("idempotency")
                .long("idempotency")
                .conflicts_with_all(["messages", "attempts"])
                .value_parser(["delivery", "publication"]),
        )
        .arg(Arg::new("after").long("after").requires("messages"))
        .arg(
            Arg::new("pretend")
                .long("pretend")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("days")
                .long("days")
                .required(true)
                .value_parser(value_parser!(u32).range(0..=36_500)),
        )
        .arg(
            Arg::new("limit")
                .long("limit")
                .default_value("100")
                .value_parser(value_parser!(u32).range(1..=500)),
        );
    Artisan::add(Box::new(Prune(CommandDefinition::new(prune))))?;
    let hold = Syntax::new("webhook-server:hold")
        .about("设置或解除出站记录的保留标记")
        .arg(Arg::new("id").required(true))
        .arg(Arg::new("scope").long("scope").required(true))
        .arg(Arg::new("database").long("database"))
        .arg(
            Arg::new("message")
                .long("message")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("release")
                .long("release")
                .action(ArgAction::SetTrue),
        );
    Artisan::add(Box::new(Hold(CommandDefinition::new(hold))))
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
            let service = delivery_service(app, input).await?;
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
                let ids = service
                    .store
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

struct Recover(CommandDefinition);

impl Command for Recover {
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
            let service = delivery_service(app, input).await?;
            let queue = app
                .make::<QueueManager>()
                .await
                .map_err(ConsoleError::from_source)?;
            let args = input.matches();
            let limit = *args.get_one::<u32>("limit").expect("默认批次大小");
            let report = service
                .recover_due(&queue, limit, args.get_flag("pretend"))
                .await
                .map_err(ConsoleError::from_source)?;
            output.info(&serde_json::to_string(&report).map_err(ConsoleError::from_source)?);
            Ok(i32::from(report.failed != 0))
        })
    }
}

struct Replay(CommandDefinition);

impl Command for Replay {
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
            let service = delivery_service(app, input).await?;
            let args = input.matches();
            let replay = service
                .replay(
                    args.get_one::<String>("id").expect("必填投递 ID"),
                    args.get_one::<String>("actor").expect("必填操作者"),
                    args.get_one::<String>("reason").expect("必填操作原因"),
                    args.get_flag("include-succeeded"),
                )
                .await
                .map_err(ConsoleError::from_source)?;
            output.info(&serde_json::to_string(&replay).map_err(ConsoleError::from_source)?);
            Ok(0)
        })
    }
}

struct Inspect(CommandDefinition);

impl Command for Inspect {
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
            let store = delivery_store(app, input).await?;
            let args = input.matches();
            let id = args.get_one::<String>("id").expect("必填 ID");
            let json = if args.get_flag("message") {
                serde_json::to_string(
                    &store
                        .inspect_message(id)
                        .await
                        .map_err(ConsoleError::from_source)?,
                )
            } else if args.get_flag("attempts") {
                let after = args.get_one::<u32>("after").copied().unwrap_or(0);
                let limit = args.get_one::<u32>("limit").copied().unwrap_or(50);
                serde_json::to_string(
                    &store
                        .inspect_attempts(id, after, limit)
                        .await
                        .map_err(ConsoleError::from_source)?,
                )
            } else {
                serde_json::to_string(
                    &store
                        .inspect_delivery(id)
                        .await
                        .map_err(ConsoleError::from_source)?,
                )
            }
            .map_err(ConsoleError::from_source)?;
            output
                .write_raw(&format!("{json}\n"))
                .map_err(ConsoleError::from_source)?;
            Ok(0)
        })
    }
}

struct Prune(CommandDefinition);

impl Command for Prune {
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
            let store = delivery_store(app, input).await?;
            let args = input.matches();
            let artifact = if args.get_flag("attempts") {
                DeliveryRetention::Attempts
            } else {
                DeliveryRetention::Payloads
            };
            let days = *args.get_one::<u32>("days").expect("必填保留天数");
            let limit = *args.get_one::<u32>("limit").expect("默认批次大小");
            let pretend = args.get_flag("pretend");
            let json = if let Some(kind) = args.get_one::<String>("idempotency") {
                let kind = if kind == "delivery" {
                    IdempotencyKind::Delivery
                } else {
                    IdempotencyKind::Publication
                };
                serde_json::to_string(
                    &store
                        .prune_idempotency_keys(kind, days, limit, pretend)
                        .await
                        .map_err(ConsoleError::from_source)?,
                )
            } else if args.get_flag("messages") {
                let after = args.get_one::<String>("after").map(String::as_str);
                serde_json::to_string(
                    &store
                        .prune_publication_bodies(days, limit, after, pretend)
                        .await
                        .map_err(ConsoleError::from_source)?,
                )
            } else {
                serde_json::to_string(
                    &store
                        .prune(artifact, days, limit, pretend)
                        .await
                        .map_err(ConsoleError::from_source)?,
                )
            }
            .map_err(ConsoleError::from_source)?;
            output
                .write_raw(&format!("{json}\n"))
                .map_err(ConsoleError::from_source)?;
            Ok(0)
        })
    }
}

struct Hold(CommandDefinition);

impl Command for Hold {
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
            let store = delivery_store(app, input).await?;
            let args = input.matches();
            let id = args.get_one::<String>("id").expect("必填记录 ID");
            let record = if args.get_flag("message") {
                RetentionRecord::Publication
            } else {
                RetentionRecord::Delivery
            };
            let changed = store
                .set_retention_hold(record, id, !args.get_flag("release"))
                .await
                .map_err(ConsoleError::from_source)?;
            output
                .write_raw(&format!("{}\n", serde_json::json!({ "changed": changed })))
                .map_err(ConsoleError::from_source)?;
            Ok(0)
        })
    }
}

async fn delivery_service(
    app: &Application,
    input: &Input,
) -> Result<DeliveryService, ConsoleError> {
    let store = delivery_store(app, input).await?;
    let policy = app
        .make::<RegisteredDeliveryPolicy>()
        .await
        .map_err(|_| ConsoleError::from_source(Error::InvalidConfig("delivery.policy")))?;
    Ok(DeliveryService::new(store, policy.0.clone()))
}

async fn delivery_store(app: &Application, input: &Input) -> Result<DeliveryStore, ConsoleError> {
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
    DeliveryStore::new(connection, scope).map_err(ConsoleError::from_source)
}
