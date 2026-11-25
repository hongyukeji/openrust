use crate::{DatabaseWebhookStore, Error, jobs::QueuedWebhookJob, models::WebhookCall};
use clap::{Arg, ArgAction, Command as Syntax, value_parser};
use openrust::{
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    database::DatabaseManager,
    foundation::Application,
    queue::QueueManager,
    support::facades::{Artisan, Config},
};

pub(crate) fn register() -> Result<(), ConsoleError> {
    let syntax = Syntax::new("webhook-client:prune")
        .about("清理当前范围内的过期 Webhook 记录或附件暂存对象")
        .arg(Arg::new("scope").long("scope").required(true))
        .arg(Arg::new("database").long("database"))
        .arg(
            Arg::new("limit")
                .long("limit")
                .default_value("100")
                .value_parser(value_parser!(u32).range(1..=500)),
        )
        .arg(
            Arg::new("verification-audit")
                .long("verification-audit")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("attachment-staging")
                .long("attachment-staging")
                .conflicts_with("verification-audit")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("pretend")
                .long("pretend")
                .action(ArgAction::SetTrue),
        );
    Artisan::add(Box::new(Prune(CommandDefinition::new(syntax))))?;
    let recover = Syntax::new("webhook-client:recover")
        .about("派发当前范围内待恢复的 Webhook 处理任务")
        .arg(Arg::new("scope").long("scope").required(true))
        .arg(Arg::new("database").long("database"))
        .arg(
            Arg::new("limit")
                .long("limit")
                .default_value("100")
                .value_parser(value_parser!(u32).range(1..=500)),
        )
        .arg(
            Arg::new("pretend")
                .long("pretend")
                .action(ArgAction::SetTrue),
        );
    Artisan::add(Box::new(Recover(CommandDefinition::new(recover))))?;
    let exhausted = Syntax::new("webhook-client:exhausted")
        .about("列出当前范围内已耗尽自动派发预算的接收记录")
        .arg(Arg::new("scope").long("scope").required(true))
        .arg(Arg::new("database").long("database"))
        .arg(
            Arg::new("limit")
                .long("limit")
                .default_value("100")
                .value_parser(value_parser!(u32).range(1..=500)),
        );
    Artisan::add(Box::new(Exhausted(CommandDefinition::new(exhausted))))?;
    let replay = Syntax::new("webhook-client:replay")
        .about("申请一个受应用授权的接收重放代次")
        .arg(Arg::new("id").required(true))
        .arg(Arg::new("scope").long("scope").required(true))
        .arg(Arg::new("database").long("database"))
        .arg(Arg::new("actor").long("actor").required(true))
        .arg(Arg::new("reason").long("reason").required(true));
    Artisan::add(Box::new(Replay(CommandDefinition::new(replay))))
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
            let matches = input.matches();
            let staging = matches.get_flag("attachment-staging");
            let days = if staging {
                0
            } else {
                let value = Config::get("webhook-client.delete_after_days")
                    .map_err(ConsoleError::from_source)?;
                match value {
                    None => 30,
                    Some(serde_json::Value::Null) => {
                        output.info("Webhook 接收记录保留期清理已停用");
                        return Ok(0);
                    }
                    Some(value) => value
                        .as_u64()
                        .filter(|days| *days <= 36_500)
                        .and_then(|days| u32::try_from(days).ok())
                        .ok_or_else(|| {
                            ConsoleError::from_source(Error::InvalidConfig("delete_after_days"))
                        })?,
                }
            };
            let scope = matches.get_one::<String>("scope").expect("必填范围");
            let connection = matches.get_one::<String>("database").map(String::as_str);
            let limit = *matches.get_one::<u32>("limit").expect("默认批次上限");
            let manager = app
                .make::<DatabaseManager>()
                .await
                .map_err(ConsoleError::from_source)?;
            let connection = manager
                .connection(connection)
                .map_err(ConsoleError::from_source)?;
            if matches.get_flag("verification-audit") {
                let audit = crate::verification::DatabaseVerificationAudit::new(connection, scope)
                    .map_err(ConsoleError::from_source)?;
                let pretend = matches.get_flag("pretend");
                let rows = audit
                    .prune(days, limit, pretend)
                    .await
                    .map_err(ConsoleError::from_source)?;
                let action = if pretend { "可清理" } else { "已清理" };
                output.info(&format!("{action} {rows} 条 Webhook 验签判定记录"));
                return Ok(0);
            }
            let store =
                DatabaseWebhookStore::new(connection, scope).map_err(ConsoleError::from_source)?;
            if staging {
                let pretend = matches.get_flag("pretend");
                let report = store
                    .prune_attachment_staging(limit, pretend)
                    .await
                    .map_err(ConsoleError::from_source)?;
                if pretend {
                    output.info(&format!("可清理 {} 个 Webhook 附件对象", report.candidates));
                } else {
                    output.info(&format!(
                        "已清理 {} 个 Webhook 附件对象，{} 个待重试",
                        report.removed, report.failed
                    ));
                }
                return Ok(i32::from(report.failed > 0));
            }
            if matches.get_flag("pretend") {
                let ids = WebhookCall::prunable(&store, days, limit)
                    .await
                    .map_err(ConsoleError::from_source)?;
                output.info(&format!("可清理 {} 条 Webhook 接收记录", ids.len()));
                return Ok(0);
            }
            let removed = store
                .prune(days, limit)
                .await
                .map_err(ConsoleError::from_source)?;
            output.info(&format!("已清理 {removed} 条 Webhook 接收记录"));
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
            let matches = input.matches();
            let scope = matches.get_one::<String>("scope").expect("必填范围");
            let connection = matches.get_one::<String>("database").map(String::as_str);
            let limit = *matches.get_one::<u32>("limit").expect("默认批次上限");
            let pretend = matches.get_flag("pretend");
            let manager = app
                .make::<DatabaseManager>()
                .await
                .map_err(ConsoleError::from_source)?;
            let connection = manager
                .connection(connection)
                .map_err(ConsoleError::from_source)?;
            let store =
                DatabaseWebhookStore::new(connection, scope).map_err(ConsoleError::from_source)?;
            let queue = app
                .make::<QueueManager>()
                .await
                .map_err(ConsoleError::from_source)?;
            let report = QueuedWebhookJob::recover_dispatches(&store, &queue, limit, pretend)
                .await
                .map_err(ConsoleError::from_source)?;
            if pretend {
                output.info(&format!("可派发 {} 条 Webhook 接收记录", report.candidates));
            } else {
                output.info(&format!(
                    "已派发 {} 条 Webhook 接收记录，{} 条待重试",
                    report.dispatched, report.failed
                ));
            }
            Ok(i32::from(report.failed > 0))
        })
    }
}

struct Exhausted(CommandDefinition);

impl Command for Exhausted {
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
            let store = operations_store(app, input).await?;
            let limit = *input
                .matches()
                .get_one::<u32>("limit")
                .expect("默认批次上限");
            let rows = store
                .exhausted_receipts(limit)
                .await
                .map_err(ConsoleError::from_source)?;
            let json = serde_json::to_string(&rows).map_err(ConsoleError::from_source)?;
            output
                .write_raw(&format!("{json}\n"))
                .map_err(ConsoleError::from_source)?;
            Ok(0)
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
            let store = operations_store(app, input).await?;
            let matches = input.matches();
            let id = matches.get_one::<String>("id").expect("必填接收 ID");
            let actor = matches.get_one::<String>("actor").expect("必填操作身份");
            let reason = matches.get_one::<String>("reason").expect("必填操作原因");
            let generation = crate::replay::request(&store, id, actor, reason)
                .await
                .map_err(ConsoleError::from_source)?;
            output.info(&format!("已登记 Webhook 重放代次 {generation}"));
            Ok(0)
        })
    }
}

async fn operations_store(
    app: &Application,
    input: &Input,
) -> Result<DatabaseWebhookStore, ConsoleError> {
    let matches = input.matches();
    let scope = matches.get_one::<String>("scope").expect("必填范围");
    let connection = matches.get_one::<String>("database").map(String::as_str);
    let manager = app
        .make::<DatabaseManager>()
        .await
        .map_err(ConsoleError::from_source)?;
    let connection = manager
        .connection(connection)
        .map_err(ConsoleError::from_source)?;
    DatabaseWebhookStore::new(connection, scope).map_err(ConsoleError::from_source)
}
